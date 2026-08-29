use super::super::policy::tool_request_digest;
use super::super::{
    authorize_planned_lifecycle_tool, complete_planned_lifecycle_tool,
    consume_lifecycle_tool_authorization, ensure_routing_fresh,
    load_active_lifecycle_tool_authorization, load_active_work_item_plan,
    record_attunement_receipt, refresh_lifecycle_projection, require_fresh_attunement,
    InstanceBinding, LifecycleEnforcementLevelV1, LifecycleProjectionRefreshV1,
    LifecycleSupervisorContextV1, LifecycleToolAuthorizationV1, LifecycleToolCompletionV1,
    LifecycleToolRequestV1, ProjectAttunementReceipt, Registry, WorkPlanSnapshot, REASONING_SCOPE,
};
use rrd_core::digest;
use rrd_store::Engine;
use serde_json::Value;

const ADAPTER_CONTRACT_VERSION: &str = "rrflow-hook-v1";

pub(super) struct PlannedHookAuthorization {
    pub context: LifecycleSupervisorContextV1,
    pub authorization: LifecycleToolAuthorizationV1,
    pub work_item_id: String,
    pub consumed: bool,
}

pub(super) struct PlannedHookCompletion {
    pub work_item_id: String,
    pub projection_refreshed: bool,
}

pub(super) fn authorize<E: Engine>(
    store: &E,
    binding: &InstanceBinding,
    harness: Option<&str>,
    input: &Value,
    snapshot: &WorkPlanSnapshot,
    receipt: &ProjectAttunementReceipt,
    now: u64,
) -> Result<PlannedHookAuthorization, Box<dyn std::error::Error>> {
    let work_item_id = snapshot
        .active_item_id
        .clone()
        .ok_or("mutation denied: checked-in work plan has no active item")?;
    let context = supervisor_context(binding, harness, input)?;
    let request = tool_request(input)?;
    let authorization = authorize_planned_lifecycle_tool(
        store,
        &context,
        &request,
        &receipt.source_tree_sha256,
        &receipt.receipt_sha256,
        now,
    )?;
    // A native blocking hook has no later RRFlow-owned dispatch seam, so it
    // consumes before returning allow. Orchestrated and proxied adapters own
    // execution and consume immediately before their actual side effect.
    let consumed = context.enforcement_level == LifecycleEnforcementLevelV1::Intercepting;
    if consumed {
        consume_lifecycle_tool_authorization(store, &context, &authorization, now)?;
    }
    Ok(PlannedHookAuthorization {
        context,
        authorization,
        work_item_id,
        consumed,
    })
}

pub(super) fn complete<E: Engine>(
    store: &E,
    root: &std::path::Path,
    binding: &InstanceBinding,
    harness: Option<&str>,
    input: &Value,
    snapshot: &WorkPlanSnapshot,
    now: u64,
) -> Result<PlannedHookCompletion, Box<dyn std::error::Error>> {
    let context = supervisor_context(binding, harness, input)?;
    let request = tool_request(input)?;
    let authorization = load_active_lifecycle_tool_authorization(store, &context, &request)?;
    let plan = load_active_work_item_plan(store, &snapshot.plan_id)?
        .ok_or("post-tool observation has no active work-item plan")?;
    let encoded = serde_json::to_vec(input)?;
    let observation_sha256 = digest::sha256_hex(&encoded);
    let success = tool_succeeded(input);

    let refreshed = ensure_routing_fresh(store, root).and_then(|ready| {
        // Reuse an already-fresh post-mutation receipt. This makes an
        // identical PostToolUse delivery an exact replay instead of minting
        // different evidence solely because its timestamp changed.
        let receipt = match require_fresh_attunement(store, root, &ready) {
            Ok(receipt) => receipt,
            Err(_) => record_attunement_receipt(store, root, &ready, now, &context.actor, None)?,
        };
        let projection = store
            .get_projection(super::super::routing::ROUTING_PROJECTION)?
            .ok_or("routing refresh completed without a persisted projection")?;
        Ok((receipt, digest::sha256_hex(&projection)))
    });

    let (project_state_changed, refresh_evidence) = match refreshed {
        Ok((receipt, projection_sha256)) => {
            let changed = plan.source_tree_sha256 != receipt.source_tree_sha256;
            (changed, Some((receipt, projection_sha256)))
        }
        Err(error) => {
            complete_planned_lifecycle_tool(
                store,
                &context,
                &authorization,
                &LifecycleToolCompletionV1 {
                    observation_sha256,
                    success,
                    project_state_changed: true,
                },
                None,
                now,
            )?;
            return Err(format!(
                "tool ran but project-state refresh failed; lifecycle is stale and further mutation is denied: {error}"
            )
            .into());
        }
    };

    complete_planned_lifecycle_tool(
        store,
        &context,
        &authorization,
        &LifecycleToolCompletionV1 {
            observation_sha256: observation_sha256.clone(),
            success,
            project_state_changed,
        },
        refresh_evidence
            .as_ref()
            .map(|(receipt, _)| receipt.source_tree_sha256.as_str()),
        now,
    )?;
    if let Some((receipt, projection_sha256)) = refresh_evidence.filter(|_| project_state_changed) {
        let changed_paths = input
            .pointer("/tool_response/repository_after/changed_paths")
            .unwrap_or(input);
        refresh_lifecycle_projection(
            store,
            &context,
            &LifecycleProjectionRefreshV1 {
                before_tree_sha256: plan.source_tree_sha256,
                after_tree_sha256: receipt.source_tree_sha256,
                changed_paths_sha256: digest::sha256_hex(&serde_json::to_vec(changed_paths)?),
                projection_sha256,
                evidence_sha256: receipt.receipt_sha256,
            },
            now,
        )?;
    }
    Ok(PlannedHookCompletion {
        work_item_id: plan.work_item_id,
        projection_refreshed: project_state_changed,
    })
}

fn supervisor_context(
    binding: &InstanceBinding,
    harness: Option<&str>,
    input: &Value,
) -> Result<LifecycleSupervisorContextV1, Box<dyn std::error::Error>> {
    let harness = harness.unwrap_or("unknown");
    let session_id = input
        .get("session_id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or("planned mutation has no canonical session_id")?;
    let enforcement_level = match harness {
        "rrflow-exec" => LifecycleEnforcementLevelV1::Proxied,
        "rrflow-orchestrator" | "rrflow-mcp-dispatch" => LifecycleEnforcementLevelV1::Orchestrated,
        "mcp" => LifecycleEnforcementLevelV1::Cooperative,
        name => Registry::builtin().get(name).map_or(
            LifecycleEnforcementLevelV1::ObserveOnly,
            |adapter| {
                if adapter.hooks {
                    LifecycleEnforcementLevelV1::Intercepting
                } else if adapter.mcp_client {
                    LifecycleEnforcementLevelV1::Cooperative
                } else {
                    LifecycleEnforcementLevelV1::ObserveOnly
                }
            },
        ),
    };
    Ok(LifecycleSupervisorContextV1 {
        scope: REASONING_SCOPE.into(),
        project_id: binding.manifest.id.clone(),
        session_id: session_id.into(),
        actor: format!("hook:{harness}"),
        adapter_kind: harness.into(),
        adapter_version: input
            .get("adapter_version")
            .and_then(Value::as_str)
            .unwrap_or(ADAPTER_CONTRACT_VERSION)
            .into(),
        enforcement_level,
    })
}

fn tool_request(input: &Value) -> Result<LifecycleToolRequestV1, Box<dyn std::error::Error>> {
    let tool_name = input
        .get("tool_name")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or("planned mutation has no tool_name")?;
    let tool_call_id = input
        .get("tool_use_id")
        .or_else(|| input.get("tool_call_id"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| {
            // Gemini's hook contract does not expose a tool-call id. The
            // exact request digest is stable across BeforeTool/AfterTool; two
            // concurrent identical calls deliberately collide and fail closed.
            format!(
                "derived-{}",
                &tool_request_digest(input).expect("JSON request digest cannot fail")[..32]
            )
        });
    Ok(LifecycleToolRequestV1 {
        tool_name: tool_name.into(),
        tool_request_sha256: tool_request_digest(input)?,
        tool_call_id,
        mutation: true,
    })
}

fn tool_succeeded(input: &Value) -> bool {
    input
        .pointer("/tool_response/success")
        .and_then(Value::as_bool)
        .or_else(|| super::run_exit_code(input).map(|exit_code| exit_code == 0))
        .unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gemini_request_without_call_id_gets_a_stable_exact_identity() {
        let before = serde_json::json!({
            "session_id": "gemini-session",
            "tool_name": "write_file",
            "tool_input": {"file_path": "src/lib.rs", "content": "changed"}
        });
        let after = serde_json::json!({
            "session_id": "gemini-session",
            "tool_name": "write_file",
            "tool_input": {"file_path": "src/lib.rs", "content": "changed"},
            "tool_response": {"success": true}
        });
        let before = tool_request(&before).unwrap();
        let after = tool_request(&after).unwrap();
        assert_eq!(before.tool_call_id, after.tool_call_id);
        assert_eq!(before.tool_request_sha256, after.tool_request_sha256);
        assert!(before.tool_call_id.starts_with("derived-"));
    }
}
