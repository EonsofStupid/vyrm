//! Provider and local-runtime anti-corruption layer over one canonical lifecycle.
use super::{runtime_tool_catalogue, RuntimeToolLifecyclePolicy};
use rrd_core::digest;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub use rrd_contract::{
    AdapterCoverageEntryV1, AdapterCoveragePointV1, AdapterCoverageStateV1,
    AdapterCoverageVectorV1, AdapterKindV1, AdapterMutationClassV1, CanonicalAdapterEventV1,
    LifecycleEnforcementLevelV1, LifecycleEventTypeV1, ADAPTER_CONFORMANCE_FORMAT_VERSION,
};

const FIXTURES: &str = include_str!("fixtures/adapter-conformance-v1.json");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdapterToolClass {
    ReadOnly,
    Mutation(AdapterMutationClassV1),
    Unknown,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterFixtureDocumentV1 {
    pub schema_version: u16,
    pub fixtures: Vec<AdapterFixtureV1>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterFixtureV1 {
    pub name: String,
    pub adapter: AdapterKindV1,
    pub adapter_version: String,
    pub native_event: String,
    pub payload: Value,
    pub expected: AdapterFixtureExpectedV1,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterFixtureExpectedV1 {
    pub canonical_event: LifecycleEventTypeV1,
    pub session_id: String,
    pub tool_call_id: Option<String>,
    pub tool_name: String,
    pub mutation_class: AdapterMutationClassV1,
}

pub fn adapter_conformance_fixtures() -> Result<AdapterFixtureDocumentV1, Box<dyn std::error::Error>>
{
    let fixtures: AdapterFixtureDocumentV1 = serde_json::from_str(FIXTURES)?;
    if fixtures.schema_version != ADAPTER_CONFORMANCE_FORMAT_VERSION {
        return Err("unsupported adapter fixture schema".into());
    }
    let adapters = fixtures
        .fixtures
        .iter()
        .map(|fixture| fixture.adapter)
        .collect::<BTreeSet<_>>();
    if fixtures.fixtures.len() != AdapterKindV1::ALL.len()
        || adapters != AdapterKindV1::ALL.into_iter().collect()
    {
        return Err("adapter fixtures must contain exactly one row per canonical adapter".into());
    }
    Ok(fixtures)
}

pub fn normalize_adapter_fixture(
    fixture: &AdapterFixtureV1,
) -> Result<CanonicalAdapterEventV1, Box<dyn std::error::Error>> {
    normalize_tool_proposal(
        fixture.adapter,
        &fixture.adapter_version,
        &fixture.native_event,
        &fixture.payload,
    )
}

pub fn normalize_tool_proposal(
    adapter: AdapterKindV1,
    adapter_version: &str,
    native_event: &str,
    payload: &Value,
) -> Result<CanonicalAdapterEventV1, Box<dyn std::error::Error>> {
    let (session_id, explicit_call_id, tool_name, tool_input) = match adapter {
        AdapterKindV1::ClaudeCode | AdapterKindV1::Codex | AdapterKindV1::Gemini => {
            if native_event != "PreToolUse" && native_event != "BeforeTool" {
                return Err("native hook fixture is not a pre-tool event".into());
            }
            (
                required_string(payload, "session_id")?,
                optional_string(payload, "tool_use_id")
                    .or_else(|| optional_string(payload, "tool_call_id")),
                required_string(payload, "tool_name")?,
                payload
                    .get("tool_input")
                    .cloned()
                    .ok_or("native hook fixture has no tool_input")?,
            )
        }
        AdapterKindV1::Copilot => {
            if !matches!(native_event, "preToolUse" | "PreToolUse") {
                return Err("Copilot fixture is not a pre-tool event".into());
            }
            (
                required_string(payload, "sessionId")
                    .or_else(|_| required_string(payload, "session_id"))?,
                optional_string(payload, "toolCallId")
                    .or_else(|| optional_string(payload, "tool_use_id")),
                required_string(payload, "toolName")
                    .or_else(|_| required_string(payload, "tool_name"))?,
                parse_json_value(
                    payload
                        .get("toolArgs")
                        .or_else(|| payload.get("tool_input"))
                        .ok_or("Copilot fixture has no toolArgs")?,
                )?,
            )
        }
        AdapterKindV1::OpenAiOrchestrator => {
            if native_event != "response.output_item.function_call"
                || payload.get("type").and_then(Value::as_str) != Some("function_call")
            {
                return Err("OpenAI fixture is not a function_call output item".into());
            }
            (
                required_string(payload, "session_id")?,
                Some(required_string(payload, "call_id")?),
                required_string(payload, "name")?,
                parse_json_value(
                    payload
                        .get("arguments")
                        .ok_or("OpenAI function call has no arguments")?,
                )?,
            )
        }
        AdapterKindV1::XaiOrchestrator => {
            if native_event != "response.output_item.function_call"
                || payload.get("type").and_then(Value::as_str) != Some("function")
            {
                return Err("xAI fixture is not a function tool call".into());
            }
            let function = payload
                .get("function")
                .ok_or("xAI function call has no function object")?;
            (
                required_string(payload, "session_id")?,
                Some(required_string(payload, "id")?),
                required_string(function, "name")?,
                parse_json_value(
                    function
                        .get("arguments")
                        .ok_or("xAI function call has no arguments")?,
                )?,
            )
        }
        AdapterKindV1::Mcp => {
            if native_event != "tools/call"
                || payload.get("method").and_then(Value::as_str) != Some("tools/call")
            {
                return Err("MCP fixture is not a tools/call request".into());
            }
            let params = payload
                .get("params")
                .ok_or("MCP tools/call has no params")?;
            (
                required_string(payload, "session_id")?,
                Some(required_string(payload, "id")?),
                required_string(params, "name")?,
                params
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({})),
            )
        }
        AdapterKindV1::LocalRuntime => {
            if native_event != "exec" {
                return Err("local runtime fixture is not an exec request".into());
            }
            let argv = payload
                .get("argv")
                .and_then(Value::as_array)
                .filter(|argv| {
                    !argv.is_empty() && argv.iter().all(|argument| argument.as_str().is_some())
                })
                .ok_or("local runtime exec requires a non-empty string argv")?;
            (
                required_string(payload, "session_id")?,
                Some(required_string(payload, "request_id")?),
                "RRFlowExec".into(),
                json!({"exact_argv": argv}),
            )
        }
    };
    let mutation_class = match classify_adapter_tool(&tool_name) {
        AdapterToolClass::Mutation(class) => class,
        AdapterToolClass::ReadOnly => return Err("adapter fixture is not a mutation".into()),
        AdapterToolClass::Unknown => {
            return Err(format!("unknown adapter tool {tool_name:?} fails closed").into())
        }
    };
    let tool_input_sha256 = digest::sha256_hex(&serde_json::to_vec(&tool_input)?);
    let tool_call_id = explicit_call_id.unwrap_or_else(|| {
        digest::sha256_hex(
            &serde_json::to_vec(&json!({
                "adapter": adapter,
                "session_id": session_id,
                "tool_name": tool_name,
                "tool_input_sha256": tool_input_sha256,
            }))
            .expect("canonical adapter identity serializes"),
        )
    });
    let tool_call_id = if tool_call_id.len() == 64 {
        format!("derived-{}", &tool_call_id[..32])
    } else {
        tool_call_id
    };
    let mut event = CanonicalAdapterEventV1 {
        format: ADAPTER_CONFORMANCE_FORMAT_VERSION,
        adapter,
        adapter_version: adapter_version.into(),
        native_event: native_event.into(),
        canonical_event: LifecycleEventTypeV1::ToolProposed,
        session_id,
        tool_call_id,
        tool_name,
        tool_input_sha256,
        mutation_class,
        event_sha256: String::new(),
    };
    event.seal()?;
    Ok(event)
}

pub fn normalize_hook_input(
    harness: Option<&str>,
    event: &str,
    input: &Value,
) -> Result<Value, String> {
    let mut normalized = input.clone();
    if harness == Some("github-copilot") {
        let object = normalized
            .as_object_mut()
            .ok_or_else(|| "Copilot hook input must be an object".to_owned())?;
        copy_field(object, "sessionId", "session_id");
        if matches!(event, "pre-tool-use" | "post-tool-use") {
            copy_field(object, "toolName", "tool_name");
            copy_field(object, "toolResult", "tool_response");
            if !object.contains_key("tool_input") {
                let args = object
                    .get("toolArgs")
                    .ok_or_else(|| "Copilot tool hook has no toolArgs".to_owned())?;
                let args = parse_json_value(args).map_err(|error| error.to_string())?;
                object.insert("tool_input".into(), args);
            }
        }
    }
    if matches!(event, "pre-tool-use" | "post-tool-use") {
        let tool = normalized
            .get("tool_name")
            .and_then(Value::as_str)
            .filter(|tool| !tool.is_empty())
            .ok_or_else(|| "tool hook payload has no non-empty tool_name".to_owned())?;
        if normalized.get("tool_input").is_none() {
            return Err("tool hook payload has no tool_input".into());
        }
        if classify_adapter_tool(tool) == AdapterToolClass::Unknown {
            return Err(format!(
                "unknown tool {tool:?} has no registered read-only or mutation contract"
            ));
        }
    }
    Ok(normalized)
}

pub fn classify_adapter_tool(tool: &str) -> AdapterToolClass {
    match tool {
        "Read" | "Glob" | "Grep" | "WebFetch" | "WebSearch" | "AskUserQuestion" | "TodoWrite"
        | "view" | "glob" | "grep" | "web_fetch" | "web_search" | "ask_user" | "update_todo" => {
            return AdapterToolClass::ReadOnly
        }
        "Edit" | "Write" | "NotebookEdit" | "apply_patch" | "write_file" | "replace" | "create"
        | "edit" => return AdapterToolClass::Mutation(AdapterMutationClassV1::Patch),
        "Bash" | "bash" | "powershell" | "run_shell_command" | "RRFlowExec" => {
            return AdapterToolClass::Mutation(AdapterMutationClassV1::Shell)
        }
        "generate_code" | "codegen" | "generate_client" | "write_generated" => {
            return AdapterToolClass::Mutation(AdapterMutationClassV1::GeneratedCode)
        }
        "Agent" | "Task" | "task" | "spawn_agent" => {
            return AdapterToolClass::Mutation(AdapterMutationClassV1::Subagent)
        }
        "computer" | "computer_use" | "code_interpreter" | "hosted_shell" => {
            return AdapterToolClass::Mutation(AdapterMutationClassV1::HostedTool)
        }
        "deploy" | "publish" | "rrflow_data_commit" => {
            return AdapterToolClass::Mutation(AdapterMutationClassV1::External)
        }
        _ => {}
    }
    runtime_tool_catalogue()
        .iter()
        .find(|definition| {
            definition.name == tool
                || tool.ends_with(&format!("__{}", definition.name))
                || tool.ends_with(&format!("_{}", definition.name))
        })
        .map_or(AdapterToolClass::Unknown, |definition| {
            if definition.lifecycle == RuntimeToolLifecyclePolicy::PlannedMutation {
                AdapterToolClass::Mutation(AdapterMutationClassV1::External)
            } else {
                AdapterToolClass::ReadOnly
            }
        })
}

pub fn adapter_coverage(adapter: AdapterKindV1) -> AdapterCoverageVectorV1 {
    let mut points = AdapterCoveragePointV1::ALL
        .into_iter()
        .map(|point| {
            (
                point,
                (
                    AdapterCoverageStateV1::Uncovered,
                    "the adapter has not proven this path".to_owned(),
                ),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut set = |point, state, reason: &str| {
        points.insert(point, (state, reason.into()));
    };
    let level = match adapter {
        AdapterKindV1::ClaudeCode | AdapterKindV1::Codex | AdapterKindV1::Gemini => {
            for point in [
                AdapterCoveragePointV1::SessionStartResume,
                AdapterCoveragePointV1::PlanningGate,
                AdapterCoveragePointV1::PreToolPatch,
                AdapterCoveragePointV1::PreToolShell,
                AdapterCoveragePointV1::PreToolGeneratedCode,
                AdapterCoveragePointV1::PreToolExternal,
            ] {
                set(
                    point,
                    AdapterCoverageStateV1::Enforced,
                    "native blocking hook emits the canonical lifecycle decision",
                );
            }
            for point in [
                AdapterCoveragePointV1::PostTool,
                AdapterCoveragePointV1::ToolFailure,
                AdapterCoveragePointV1::Compaction,
                AdapterCoveragePointV1::StopSessionEnd,
                AdapterCoveragePointV1::Subagent,
            ] {
                set(
                    point,
                    AdapterCoverageStateV1::Observed,
                    "native lifecycle event is normalized but does not itself gate execution",
                );
            }
            LifecycleEnforcementLevelV1::Intercepting
        }
        AdapterKindV1::Copilot => {
            for point in [
                AdapterCoveragePointV1::PreToolPatch,
                AdapterCoveragePointV1::PreToolShell,
                AdapterCoveragePointV1::PreToolGeneratedCode,
                AdapterCoveragePointV1::PreToolExternal,
                AdapterCoveragePointV1::CrashFailure,
            ] {
                set(
                    point,
                    AdapterCoverageStateV1::Enforced,
                    "Copilot command preToolUse blocks explicit denial and hook errors",
                );
            }
            for point in [
                AdapterCoveragePointV1::SessionStartResume,
                AdapterCoveragePointV1::PlanningGate,
                AdapterCoveragePointV1::PostTool,
                AdapterCoveragePointV1::ToolFailure,
                AdapterCoveragePointV1::Compaction,
                AdapterCoveragePointV1::StopSessionEnd,
                AdapterCoveragePointV1::Subagent,
            ] {
                set(
                    point,
                    AdapterCoverageStateV1::Observed,
                    "Copilot exposes this lifecycle point without complete RRFlow gating proof",
                );
            }
            set(
                AdapterCoveragePointV1::TimeoutFailure,
                AdapterCoverageStateV1::Uncovered,
                "Copilot command-hook timeouts are documented fail-open",
            );
            set(
                AdapterCoveragePointV1::InvalidOutput,
                AdapterCoverageStateV1::Uncovered,
                "invalid hook output falls through to the normal permission flow",
            );
            LifecycleEnforcementLevelV1::Intercepting
        }
        AdapterKindV1::OpenAiOrchestrator | AdapterKindV1::XaiOrchestrator => {
            set(
                AdapterCoveragePointV1::PreToolExternal,
                AdapterCoverageStateV1::Observed,
                "the provider function-call payload normalizes to a canonical proposal",
            );
            LifecycleEnforcementLevelV1::ObserveOnly
        }
        AdapterKindV1::Mcp => {
            for point in [
                AdapterCoveragePointV1::SessionStartResume,
                AdapterCoveragePointV1::PlanningGate,
                AdapterCoveragePointV1::PreToolExternal,
                AdapterCoveragePointV1::PostTool,
                AdapterCoveragePointV1::ToolFailure,
            ] {
                set(
                    point,
                    AdapterCoverageStateV1::Observed,
                    "MCP carries canonical RRFlow calls but cannot intercept host-owned tools",
                );
            }
            LifecycleEnforcementLevelV1::Cooperative
        }
        AdapterKindV1::LocalRuntime => {
            for point in AdapterCoveragePointV1::ALL {
                set(
                    point,
                    AdapterCoverageStateV1::Enforced,
                    "the RRFlow planning permit and exact command proxy own this path",
                );
            }
            for point in [
                AdapterCoveragePointV1::Subagent,
                AdapterCoveragePointV1::HostedTool,
            ] {
                set(
                    point,
                    AdapterCoverageStateV1::NotApplicable,
                    "the proxied local runtime exposes no independent hosted or subagent tool path",
                );
            }
            LifecycleEnforcementLevelV1::Proxied
        }
    };
    let entries = AdapterCoveragePointV1::ALL
        .into_iter()
        .map(|point| {
            let (state, reason) = points.remove(&point).expect("every point initialized");
            AdapterCoverageEntryV1 {
                point,
                state,
                reason,
            }
        })
        .collect::<Vec<_>>();
    let state = |point| {
        entries
            .iter()
            .find(|entry| entry.point == point)
            .expect("complete coverage")
            .state
    };
    let planning_enforced =
        state(AdapterCoveragePointV1::PlanningGate) == AdapterCoverageStateV1::Enforced;
    let mutation_enforced = [
        AdapterCoveragePointV1::PreToolPatch,
        AdapterCoveragePointV1::PreToolShell,
        AdapterCoveragePointV1::PreToolGeneratedCode,
        AdapterCoveragePointV1::PreToolExternal,
        AdapterCoveragePointV1::Subagent,
        AdapterCoveragePointV1::HostedTool,
        AdapterCoveragePointV1::TimeoutFailure,
        AdapterCoveragePointV1::CrashFailure,
        AdapterCoveragePointV1::InvalidOutput,
        AdapterCoveragePointV1::TrustFailure,
    ]
    .into_iter()
    .all(|point| {
        matches!(
            state(point),
            AdapterCoverageStateV1::Enforced | AdapterCoverageStateV1::NotApplicable
        )
    });
    let mut coverage = AdapterCoverageVectorV1 {
        format: ADAPTER_CONFORMANCE_FORMAT_VERSION,
        adapter,
        adapter_version: "reviewed-2026-08-30".into(),
        enforcement_level: level,
        planning_enforced,
        mutation_enforced,
        entries,
        coverage_sha256: String::new(),
    };
    coverage
        .seal()
        .expect("built-in adapter coverage is complete and coherent");
    coverage
}

fn required_string(value: &Value, field: &str) -> Result<String, Box<dyn std::error::Error>> {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| format!("adapter payload has no non-empty {field}").into())
}

fn optional_string(value: &Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn parse_json_value(value: &Value) -> Result<Value, Box<dyn std::error::Error>> {
    match value {
        Value::String(encoded) => Ok(serde_json::from_str(encoded)?),
        value => Ok(value.clone()),
    }
}

fn copy_field(object: &mut serde_json::Map<String, Value>, native: &str, canonical: &str) {
    if !object.contains_key(canonical) {
        if let Some(value) = object.get(native).cloned() {
            object.insert(canonical.into(), value);
        }
    }
}
