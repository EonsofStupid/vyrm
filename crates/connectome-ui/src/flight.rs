//! Prompt-flight capture and controlled context experiments.
//!
//! A flight records only externally observable runtime/provider events. It does
//! not request, infer, or persist hidden chain-of-thought.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Read};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use vyrm_core::{
    Reader, RuntimeCommit, RuntimeEvent, RuntimeEventSchema, RuntimeMutation, RuntimeProperties,
    RuntimePropertySchema, RuntimeRecord, RuntimeRecordSchema, RuntimeRef, RuntimeSchemaRegistry,
    RuntimeTraceEvent, RuntimeType, RuntimeValue, RuntimeValueType, ScopeId, TraceDataClass,
    TraceDomain, TraceLink, TraceOutcome,
};
use vyrm_node::{
    record_runtime_trace, DurableTraceSpan, HookContext, HookEvent, InstanceBinding, TraceIdentity,
};
use vyrm_store::{Engine, PersistentEngine};

const FLIGHT_LEDGER: &str = "connectome-flight-ledger-v1";
const FLIGHT_SCOPE: &str = "instance:default";
const FLIGHT_TYPE: &str = "prompt_flight";
const FLIGHT_EVENT_TYPE: &str = "prompt_flight_event";
const REPLAY_PAGE: usize = 1_024;
const FORMAT: u32 = 1;
const MAX_PROMPT_BYTES: usize = 64 * 1024;
const MAX_EVENT_DETAIL_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextMode {
    /// A new ephemeral provider session with no Vyrm context injected.
    Fresh,
    /// Only claims matched by the submitted prompt, within the declared budget.
    Pruned,
    /// Full preflight context followed by prompt-matched recall.
    Full,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningProfile {
    /// Balanced provider default for representative baseline runs.
    #[default]
    Default,
    /// More deliberate exploration when it produces a measured quality gain.
    High,
    /// User-facing name for the provider's `xhigh` effort.
    Extreme,
    /// Quality-first profile mapped to the provider's `max` effort.
    Ultra,
}

impl ReasoningProfile {
    pub fn provider_effort(self) -> &'static str {
        match self {
            Self::Default => "medium",
            Self::High => "high",
            Self::Extreme => "xhigh",
            Self::Ultra => "max",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlightStatus {
    Preparing,
    Prepared,
    Running,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchFlight {
    pub prompt: String,
    #[serde(default = "default_provider")]
    pub provider: String,
    #[serde(default = "default_context_mode")]
    pub context_mode: ContextMode,
    #[serde(default = "default_budget")]
    pub budget: usize,
    #[serde(default)]
    pub acceptance_marker: String,
    #[serde(default)]
    pub reasoning_profile: ReasoningProfile,
}

fn default_provider() -> String {
    "observe".into()
}

fn default_context_mode() -> ContextMode {
    ContextMode::Fresh
}

fn default_budget() -> usize {
    1_500
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FlightMetrics {
    pub context_tokens: u64,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub tool_calls: u64,
    pub latency_ms: Option<u64>,
    pub acceptance_met: Option<bool>,
    #[serde(default)]
    pub cached_input_tokens: Option<u64>,
    #[serde(default)]
    pub reasoning_tokens: Option<u64>,
    #[serde(default)]
    pub provider_events: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlightEvent {
    pub ordinal: u64,
    pub at: u64,
    pub elapsed_ms: u64,
    pub stage: String,
    pub kind: String,
    pub label: String,
    pub detail: String,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub data: Value,
}

struct EventDraft<'a> {
    at: u64,
    elapsed_ms: u64,
    stage: &'a str,
    kind: &'a str,
    label: &'a str,
    detail: &'a str,
    data: Value,
}

fn event<'a>(
    at: u64,
    elapsed_ms: u64,
    stage: &'a str,
    kind: &'a str,
    label: &'a str,
    detail: &'a str,
    data: Value,
) -> EventDraft<'a> {
    EventDraft {
        at,
        elapsed_ms,
        stage,
        kind,
        label,
        detail,
        data,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Flight {
    pub id: String,
    pub cohort_id: String,
    pub prompt: String,
    pub provider: String,
    pub context_mode: ContextMode,
    pub budget: usize,
    pub acceptance_marker: String,
    pub created_at: u64,
    pub status: FlightStatus,
    pub context_preview: String,
    pub routed_files: Vec<String>,
    pub output_preview: String,
    pub metrics: FlightMetrics,
    pub events: Vec<FlightEvent>,
    #[serde(default)]
    pub reasoning_profile: ReasoningProfile,
    /// Optional deterministic demonstration grouping. Normal prompt flights do
    /// not set these fields; demo pairs use them for cross-prompt comparison.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comparison_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub demo_role: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Ledger {
    format: u32,
    flights: Vec<Flight>,
}

pub struct FlightRecorder {
    store: Arc<PersistentEngine>,
    binding: InstanceBinding,
    runners_enabled: bool,
    mutation: Mutex<()>,
}

impl FlightRecorder {
    pub fn new(
        store: Arc<PersistentEngine>,
        binding: InstanceBinding,
        runners_enabled: bool,
    ) -> Self {
        Self {
            store,
            binding,
            runners_enabled,
            mutation: Mutex::new(()),
        }
    }

    pub fn runners_enabled(&self) -> bool {
        self.runners_enabled
    }

    pub fn flights(&self) -> Result<Vec<Flight>, Box<dyn std::error::Error>> {
        let _guard = self
            .mutation
            .lock()
            .map_err(|_| "flight ledger lock poisoned")?;
        Ok(load(self.store.as_ref())?.0.flights)
    }

    pub fn launch(
        self: &Arc<Self>,
        request: LaunchFlight,
        at: u64,
    ) -> Result<Flight, Box<dyn std::error::Error>> {
        validate_launch(&request, self.runners_enabled)?;
        self.binding.require_runtime_ready()?;
        self.binding.verify_store_path(self.store.path())?;

        let cohort_id = vyrm_core::digest::sha256_hex(request.prompt.trim().as_bytes());
        let id = format!("flight-{at}-{}", &cohort_id[..10]);
        let flight = Flight {
            id: id.clone(),
            cohort_id,
            prompt: request.prompt.trim().to_owned(),
            provider: request.provider.clone(),
            context_mode: request.context_mode,
            budget: request.budget,
            acceptance_marker: request.acceptance_marker.trim().to_owned(),
            created_at: at,
            status: FlightStatus::Preparing,
            context_preview: String::new(),
            routed_files: Vec::new(),
            output_preview: String::new(),
            metrics: FlightMetrics::default(),
            events: Vec::new(),
            reasoning_profile: request.reasoning_profile,
            comparison_id: None,
            demo_role: None,
        };
        self.mutate(|ledger| {
            if ledger.flights.iter().any(|candidate| candidate.id == id) {
                return Err(format!("flight {id:?} already exists").into());
            }
            ledger.flights.push(flight);
            Ok(())
        })?;

        self.append_event(
            &id,
            event(
                at,
                0,
                "prompt",
                "prompt_received",
                "Prompt entered the runtime boundary",
                request.prompt.trim(),
                json!({"bytes": request.prompt.len()}),
            ),
        )?;

        let started = Instant::now();
        let (context, context_tokens) = self.prepare_context(&id, &request, at, &started)?;
        self.prepare_routes(&id, &request.prompt, at, &started)?;
        self.update(&id, |flight| {
            flight.context_preview = truncate(&context, MAX_EVENT_DETAIL_BYTES);
            flight.metrics.context_tokens = context_tokens;
            flight.status = if request.provider == "observe" {
                FlightStatus::Prepared
            } else {
                FlightStatus::Running
            };
        })?;

        if request.provider == "observe" {
            let elapsed_ms = started.elapsed().as_millis() as u64;
            self.append_event(&id, event(
                now(),
                elapsed_ms,
                "model",
                "handoff_ready",
                "Prompt packet prepared without launching a provider",
                "Choose an enabled frontier runner to execute this packet, or inspect the context path as-is.",
                Value::Null,
            ))?;
            self.update(&id, |flight| {
                flight.metrics.latency_ms = Some(elapsed_ms);
            })?;
        } else {
            let recorder = Arc::clone(self);
            let flight_id = id.clone();
            std::thread::Builder::new()
                .name(format!("connectome-{flight_id}"))
                .spawn(move || recorder.run_provider(&flight_id, &request, &context, started))?;
        }
        self.flight(&id)?
            .ok_or_else(|| "new flight disappeared".into())
    }

    /// Creates one deterministic weak/strong pair through the same durable
    /// mutation path as real flights. The data is explicitly synthetic and is
    /// intended to exercise playback, comparison, and temporal inspection.
    pub fn seed_prompt_demos(&self, at: u64) -> Result<Vec<Flight>, Box<dyn std::error::Error>> {
        self.binding.require_runtime_ready()?;
        self.binding.verify_store_path(self.store.path())?;
        let _guard = self
            .mutation
            .lock()
            .map_err(|_| "flight ledger lock poisoned")?;
        let (mut ledger, migrate_legacy, observed_cursor) = load(self.store.as_ref())?;
        if let Some(existing) = latest_demo_pair(&ledger.flights) {
            return Ok(existing);
        }
        let comparison_id = format!("prompt-strength-{at}");
        let flights = vec![
            demo_flight(at, &comparison_id, "weak"),
            demo_flight(at + 1, &comparison_id, "strong"),
        ];
        let before = ledger.flights.clone();
        ledger.flights.extend(flights.clone());
        persist(
            self.store.as_ref(),
            &before,
            &ledger,
            migrate_legacy,
            observed_cursor,
        )?;
        Ok(flights)
    }

    fn prepare_context(
        &self,
        id: &str,
        request: &LaunchFlight,
        at: u64,
        started: &Instant,
    ) -> Result<(String, u64), Box<dyn std::error::Error>> {
        if request.context_mode == ContextMode::Fresh {
            self.append_event(id, event(
                now(),
                started.elapsed().as_millis() as u64,
                "context",
                "fresh_baseline",
                "Baseline context purged",
                "Ephemeral provider session; zero Vyrm context is injected. Authoritative history is preserved outside the prompt.",
                json!({"context_tokens": 0}),
            ))?;
            return Ok((String::new(), 0));
        }

        let reader = Reader::new(format!("connectome:{id}"))?;
        let ctx = HookContext {
            store: self.store.as_ref(),
            root: &self.binding.project_root,
            harness: Some("connectome"),
            reader: &reader,
            now: at,
            budget: request.budget,
        };
        let mut segments = Vec::new();
        let mut tokens = 0_u64;
        if request.context_mode == ContextMode::Full {
            let preflight = vyrm_node::handle(&ctx, HookEvent::SessionStart, &json!({}))?;
            tokens = tokens.saturating_add(
                preflight
                    .effectiveness
                    .as_ref()
                    .map_or(0, |value| value.tokens_emitted),
            );
            if !preflight.stdout.is_empty() {
                segments.push(preflight.stdout);
            }
            self.append_event(
                id,
                event(
                    now(),
                    started.elapsed().as_millis() as u64,
                    "context",
                    "preflight",
                    "Full instance preflight assembled",
                    preflight
                        .detail
                        .as_deref()
                        .unwrap_or("Current claims and runtime warnings assembled within budget."),
                    json!({"tokens": tokens}),
                ),
            )?;
        }
        let recalled = vyrm_node::handle(
            &ctx,
            HookEvent::UserPromptSubmit,
            &json!({"prompt": request.prompt}),
        )?;
        let recalled_tokens = recalled
            .effectiveness
            .as_ref()
            .map_or(0, |value| value.tokens_emitted);
        tokens = tokens.saturating_add(recalled_tokens);
        if !recalled.stdout.is_empty() {
            segments.push(recalled.stdout);
        }
        self.append_event(
            id,
            event(
                now(),
                started.elapsed().as_millis() as u64,
                "recall",
                if recalled_tokens == 0 {
                    "recall_empty"
                } else {
                    "recall_match"
                },
                if recalled_tokens == 0 {
                    "Prompt matched no current claim subjects"
                } else {
                    "Prompt-matched claims injected"
                },
                recalled
                    .detail
                    .as_deref()
                    .unwrap_or("Prompt recall completed against the current bi-temporal view."),
                json!({"tokens": recalled_tokens}),
            ),
        )?;
        Ok((segments.join("\n\n"), tokens))
    }

    fn prepare_routes(
        &self,
        id: &str,
        prompt: &str,
        _at: u64,
        started: &Instant,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let ready =
            vyrm_node::ensure_routing_fresh(self.store.as_ref(), &self.binding.project_root)?;
        self.append_event(
            id,
            event(
                now(),
                started.elapsed().as_millis() as u64,
                "routing",
                "freshness_barrier",
                "Source projection freshness established",
                &ready.render(),
                Value::Null,
            ),
        )?;
        let files = vyrm_node::load_routing(self.store.as_ref(), &self.binding.project_root)?
            .map(|index| {
                index
                    .route(prompt, 6)
                    .into_iter()
                    .map(|route| route.path.display().to_string())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        self.update(id, |flight| flight.routed_files = files.clone())?;
        self.append_event(
            id,
            event(
                now(),
                started.elapsed().as_millis() as u64,
                "routing",
                "route_candidates",
                &format!("{} source candidate(s) ranked", files.len()),
                &if files.is_empty() {
                    "No related file was found.".into()
                } else {
                    files.join("\n")
                },
                json!({"files": files}),
            ),
        )?;
        Ok(())
    }

    fn run_provider(&self, id: &str, request: &LaunchFlight, context: &str, started: Instant) {
        let identity = match TraceIdentity::derive(&[
            b"connectome-provider-flight-v1",
            self.binding.manifest.id.as_bytes(),
            id.as_bytes(),
        ]) {
            Ok(identity) => identity,
            Err(error) => {
                self.fail_provider(id, &started, &format!("provider trace identity: {error}"));
                return;
            }
        };
        let links = vec![TraceLink::Provider {
            provider: request.provider.clone(),
            invocation_id: id.to_owned(),
        }];
        let attributes = RuntimeProperties::from([
            (
                "context_mode".into(),
                RuntimeValue::String(format!("{:?}", request.context_mode).to_ascii_lowercase()),
            ),
            (
                "reasoning_effort".into(),
                RuntimeValue::String(request.reasoning_profile.provider_effort().into()),
            ),
            (
                "prompt_digest".into(),
                RuntimeValue::Digest(vyrm_core::digest::sha256_hex(request.prompt.as_bytes())),
            ),
        ]);
        let span = match DurableTraceSpan::start(
            self.store.as_ref(),
            match ScopeId::new(FLIGHT_SCOPE) {
                Ok(scope) => scope,
                Err(error) => {
                    self.fail_provider(id, &started, &format!("provider trace scope: {error}"));
                    return;
                }
            },
            format!("connectome:provider:{}", request.provider),
            identity.clone(),
            None,
            TraceDomain::Model,
            "provider.invoke",
            now(),
            TraceDataClass::Control,
            links,
            attributes,
        ) {
            Ok(span) => span,
            Err(error) => {
                self.fail_provider(id, &started, &format!("provider trace start: {error}"));
                return;
            }
        };
        let result = self.run_provider_inner(id, request, context, &started, &identity);
        let observed_flight = self.flight(id).ok().flatten();
        let outcome = if result.is_ok()
            && observed_flight
                .as_ref()
                .is_some_and(|flight| flight.status == FlightStatus::Succeeded)
        {
            TraceOutcome::Ok
        } else {
            TraceOutcome::Error
        };
        let metrics = observed_flight.map(|flight| flight.metrics);
        let mut finish_attributes = RuntimeProperties::new();
        if let Some(metrics) = metrics {
            finish_attributes.insert(
                "provider_events".into(),
                RuntimeValue::Unsigned(metrics.provider_events),
            );
            finish_attributes.insert(
                "tool_calls".into(),
                RuntimeValue::Unsigned(metrics.tool_calls),
            );
            if let Some(value) = metrics.input_tokens {
                finish_attributes.insert("input_tokens".into(), RuntimeValue::Unsigned(value));
            }
            if let Some(value) = metrics.output_tokens {
                finish_attributes.insert("output_tokens".into(), RuntimeValue::Unsigned(value));
            }
            if let Some(value) = metrics.reasoning_tokens {
                finish_attributes.insert("reasoning_tokens".into(), RuntimeValue::Unsigned(value));
            }
        }
        let trace_finish = span.finish(self.store.as_ref(), outcome, Vec::new(), finish_attributes);
        if let Err(error) = result {
            self.fail_provider(id, &started, &error.to_string());
        } else if let Err(error) = trace_finish {
            self.fail_provider(id, &started, &format!("provider trace finish: {error}"));
        }
    }

    fn fail_provider(&self, id: &str, started: &Instant, detail: &str) {
        let _ = self.append_event(
            id,
            event(
                now(),
                started.elapsed().as_millis() as u64,
                "outcome",
                "provider_error",
                "Provider flight failed",
                detail,
                Value::Null,
            ),
        );
        let _ = self.update(id, |flight| {
            flight.status = FlightStatus::Failed;
            flight.metrics.latency_ms = Some(started.elapsed().as_millis() as u64);
            flight.metrics.acceptance_met = Some(false);
        });
    }

    fn run_provider_inner(
        &self,
        id: &str,
        request: &LaunchFlight,
        context: &str,
        started: &Instant,
        trace_identity: &TraceIdentity,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let packet = if context.is_empty() {
            request.prompt.clone()
        } else {
            format!(
                "[VYRM CONTEXT]\n{context}\n\n[USER PROMPT]\n{}",
                request.prompt
            )
        };
        let mut command = provider_command(
            &request.provider,
            &self.binding.project_root,
            &packet,
            request.reasoning_profile,
        )?;
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = command.spawn()?;
        let stdout = child.stdout.take().ok_or("provider stdout unavailable")?;
        let stderr = child.stderr.take().ok_or("provider stderr unavailable")?;
        let stderr_handle = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = BufReader::new(stderr)
                .take(256 * 1024)
                .read_to_end(&mut bytes);
            String::from_utf8_lossy(&bytes).into_owned()
        });
        self.append_event(
            id,
            event(
                now(),
                started.elapsed().as_millis() as u64,
                "model",
                "provider_spawned",
                &format!("{} started in read-only mode", request.provider),
                "The provider receives a new ephemeral session and cannot mutate the project. The recorded effort is the exact requested provider value, not an inferred amount of thought.",
                json!({
                    "reasoning_profile": request.reasoning_profile,
                    "requested_effort": request.reasoning_profile.provider_effort(),
                    "sandbox": "read-only",
                    "session": "ephemeral"
                }),
            ),
        )?;

        let mut output = String::new();
        let mut input_tokens = None;
        let mut output_tokens = None;
        let mut cached_input_tokens = None;
        let mut reasoning_tokens = None;
        let mut tool_calls = 0_u64;
        let mut provider_events = 0_u64;
        for line in BufReader::new(stdout).lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            output.push_str(&line);
            output.push('\n');
            let value =
                serde_json::from_str::<Value>(&line).unwrap_or_else(|_| json!({"text": line}));
            let (stage, kind, label) = classify_provider_event(&value);
            provider_events = provider_events.saturating_add(1);
            input_tokens = find_u64(&value, &["input_tokens", "inputTokens"]).or(input_tokens);
            output_tokens = find_u64(&value, &["output_tokens", "outputTokens"]).or(output_tokens);
            cached_input_tokens = find_u64(
                &value,
                &[
                    "cached_input_tokens",
                    "cachedInputTokens",
                    "cache_read_input_tokens",
                ],
            )
            .or(cached_input_tokens);
            reasoning_tokens = find_u64(
                &value,
                &[
                    "reasoning_tokens",
                    "reasoningTokens",
                    "reasoning_output_tokens",
                ],
            )
            .or(reasoning_tokens);
            if stage == "tools" {
                tool_calls = tool_calls.saturating_add(1);
            }
            if let Err(error) = self.record_provider_envelope(
                id,
                request,
                trace_identity,
                provider_events,
                stage,
                &kind,
                &line,
            ) {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("provider envelope trace: {error}").into());
            }
            let detail = provider_detail(&value);
            self.append_event(
                id,
                event(
                    now(),
                    started.elapsed().as_millis() as u64,
                    stage,
                    &kind,
                    &label,
                    &detail,
                    value,
                ),
            )?;
        }
        let status = child.wait()?;
        let stderr = stderr_handle
            .join()
            .unwrap_or_else(|_| "stderr reader panicked".into());
        let acceptance_met = status.success()
            && (request.acceptance_marker.is_empty()
                || output.contains(&request.acceptance_marker));
        let latency = started.elapsed().as_millis() as u64;
        self.update(id, |flight| {
            flight.status = if acceptance_met {
                FlightStatus::Succeeded
            } else {
                FlightStatus::Failed
            };
            flight.output_preview = truncate(&output, 64 * 1024);
            flight.metrics.input_tokens = input_tokens;
            flight.metrics.output_tokens = output_tokens;
            flight.metrics.tool_calls = tool_calls;
            flight.metrics.latency_ms = Some(latency);
            flight.metrics.acceptance_met = Some(acceptance_met);
            flight.metrics.cached_input_tokens = cached_input_tokens;
            flight.metrics.reasoning_tokens = reasoning_tokens;
            flight.metrics.provider_events = provider_events;
        })?;
        self.append_event(
            id,
            event(
                now(),
                latency,
                "outcome",
                if acceptance_met {
                    "accepted"
                } else {
                    "not_accepted"
                },
                if acceptance_met {
                    "Flight met its acceptance condition"
                } else {
                    "Flight did not meet its acceptance condition"
                },
                &if stderr.trim().is_empty() {
                    format!("provider exit: {:?}", status.code())
                } else {
                    truncate(&stderr, MAX_EVENT_DETAIL_BYTES)
                },
                json!({"exit_code": status.code(), "acceptance_marker": request.acceptance_marker}),
            ),
        )?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn record_provider_envelope(
        &self,
        id: &str,
        request: &LaunchFlight,
        trace_identity: &TraceIdentity,
        ordinal: u64,
        stage: &str,
        kind: &str,
        encoded_envelope: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let provider_link = TraceLink::Provider {
            provider: request.provider.clone(),
            invocation_id: id.to_owned(),
        };
        let envelope_attributes = RuntimeProperties::from([
            ("event_ordinal".into(), RuntimeValue::Unsigned(ordinal)),
            (
                "provider_event_kind".into(),
                RuntimeValue::String(truncate(kind, 160)),
            ),
            (
                "envelope_digest".into(),
                RuntimeValue::Digest(vyrm_core::digest::sha256_hex(encoded_envelope.as_bytes())),
            ),
        ]);
        let trace_event = if stage == "tools" {
            let ordinal_bytes = ordinal.to_be_bytes();
            let child = trace_identity.child(&[b"tool-envelope", &ordinal_bytes])?;
            RuntimeTraceEvent::finish(
                child.trace_id,
                child.span_id,
                Some(trace_identity.span_id.clone()),
                TraceDomain::Tool,
                "provider.tool_envelope",
                now(),
                0,
                TraceOutcome::Ok,
                TraceDataClass::Control,
                vec![provider_link],
                envelope_attributes,
            )?
        } else {
            RuntimeTraceEvent::annotation(
                trace_identity.trace_id.clone(),
                trace_identity.span_id.clone(),
                None,
                TraceDomain::Model,
                "provider.invoke",
                now(),
                TraceOutcome::Running,
                TraceDataClass::Control,
                vec![provider_link],
                envelope_attributes,
            )?
        };
        record_runtime_trace(
            self.store.as_ref(),
            &ScopeId::new(FLIGHT_SCOPE)?,
            &format!("connectome:provider:{}", request.provider),
            trace_event,
        )?;
        Ok(())
    }

    fn append_event(
        &self,
        id: &str,
        event: EventDraft<'_>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.update(id, |flight| {
            flight.events.push(FlightEvent {
                ordinal: flight.events.len() as u64,
                at: event.at,
                elapsed_ms: event.elapsed_ms,
                stage: event.stage.to_owned(),
                kind: event.kind.to_owned(),
                label: event.label.to_owned(),
                detail: truncate(event.detail, MAX_EVENT_DETAIL_BYTES),
                data: event.data,
            });
        })
    }

    fn update(
        &self,
        id: &str,
        change: impl FnOnce(&mut Flight),
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.mutate(|ledger| {
            let flight = ledger
                .flights
                .iter_mut()
                .find(|flight| flight.id == id)
                .ok_or_else(|| format!("flight {id:?} not found"))?;
            change(flight);
            Ok(())
        })
    }

    fn flight(&self, id: &str) -> Result<Option<Flight>, Box<dyn std::error::Error>> {
        let _guard = self
            .mutation
            .lock()
            .map_err(|_| "flight ledger lock poisoned")?;
        Ok(load(self.store.as_ref())?
            .0
            .flights
            .into_iter()
            .find(|flight| flight.id == id))
    }

    fn mutate(
        &self,
        change: impl FnOnce(&mut Ledger) -> Result<(), Box<dyn std::error::Error>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let _guard = self
            .mutation
            .lock()
            .map_err(|_| "flight ledger lock poisoned")?;
        let (mut ledger, migrate_legacy, observed_cursor) = load(self.store.as_ref())?;
        let before = ledger.flights.clone();
        change(&mut ledger)?;
        persist(
            self.store.as_ref(),
            &before,
            &ledger,
            migrate_legacy,
            observed_cursor,
        )
    }
}

fn latest_demo_pair(flights: &[Flight]) -> Option<Vec<Flight>> {
    let mut groups = BTreeMap::<&str, Vec<&Flight>>::new();
    for flight in flights.iter().filter(|flight| flight.demo_role.is_some()) {
        if let Some(comparison_id) = flight.comparison_id.as_deref() {
            groups.entry(comparison_id).or_default().push(flight);
        }
    }
    groups
        .into_values()
        .filter(|group| {
            group
                .iter()
                .any(|flight| flight.demo_role.as_deref() == Some("weak"))
                && group
                    .iter()
                    .any(|flight| flight.demo_role.as_deref() == Some("strong"))
        })
        .max_by_key(|group| {
            group
                .iter()
                .map(|flight| flight.created_at)
                .max()
                .unwrap_or_default()
        })
        .map(|mut group| {
            group.sort_by_key(|flight| flight.demo_role.as_deref() == Some("strong"));
            group.into_iter().cloned().collect()
        })
}

fn demo_flight(at: u64, comparison_id: &str, role: &str) -> Flight {
    let strong = role == "strong";
    let prompt = if strong {
        "Trace one prompt from intake through context, routing, tools, verification, and outcome. Preserve read-only execution, cite every observation by digest, stop on stale evidence, and report latency, token, and tool-call differentials."
    } else {
        "Make this better."
    };
    let cohort_id = vyrm_core::digest::sha256_hex(prompt.as_bytes());
    let specifications: &[(&str, &str, &str, &str, u64, Value)] = if strong {
        &[
            ("prompt", "prompt_received", "Bounded prompt entered", "Objective, safety boundary, evidence requirements, and measurable outcome arrived together.", 0, json!({"constraints": 5, "ambiguity": 0})),
            ("context", "constraints_parsed", "Execution contract extracted", "Read-only execution, stale-evidence stop condition, digest provenance, and requested metrics became typed constraints.", 58, json!({"goal": 1, "constraints": 5, "acceptance": 4})),
            ("recall", "recall_precise", "Relevant runtime claims recalled", "The prompt named the runtime stages directly, producing a narrow evidence packet without unrelated history.", 121, json!({"claims": 6, "context_tokens": 286, "precision": "high"})),
            ("routing", "route_focused", "Three source candidates selected", "The routing projection resolved the recorder, runtime contract, and graph view as the minimal inspection neighborhood.", 204, json!({"files": ["flight.rs", "runtime.rs", "app.js"], "fanout": 3})),
            ("model", "plan_grounded", "Plan bound to observable evidence", "Each requested conclusion was mapped to one runtime event, one verification check, and one visible metric.", 318, json!({"steps": 4, "unresolved": 0})),
            ("tools", "inspection_bounded", "Three focused inspections completed", "Only the selected implementation surfaces were inspected; no mutation or broad repository scan was required.", 612, json!({"tool_calls": 3, "files_read": 3, "wasted_reads": 0})),
            ("outcome", "verification_passed", "All acceptance checks passed", "Temporal controls, causal event data, and comparative metrics were each observed with retained evidence.", 844, json!({"checks_passed": 4, "checks_failed": 0, "evidence_digests": 4})),
            ("outcome", "accepted", "Strong prompt produced a bounded outcome", "The runtime reached a verifiable conclusion with lower tool fanout and an explicit stop condition.", 980, json!({"accepted": true, "confidence": "grounded"})),
        ]
    } else {
        &[
            ("prompt", "prompt_received", "Ambiguous prompt entered", "No target, constraints, evidence standard, or acceptance condition were supplied.", 0, json!({"constraints": 0, "ambiguity": 5})),
            ("context", "intent_uncertain", "Context cannot resolve intent", "The runtime can recover project state, but it cannot infer what “better” means without inventing the operator's objective.", 146, json!({"candidate_intents": 7, "resolved": 0})),
            ("recall", "recall_broad", "Weak terms fan out across history", "Generic language produces a broad packet with little discriminating value.", 301, json!({"claims": 18, "context_tokens": 1180, "precision": "low"})),
            ("routing", "route_scattered", "Source routing fans out", "The prompt has no named subsystem, so candidate files span UI, storage, policy, tests, and documentation.", 514, json!({"files": ["app.js", "store.rs", "policy.rs", "README.md", "runtime.rs", "flight.rs", "SPEC.md"], "fanout": 7})),
            ("model", "goal_reformulated", "Model invents a working interpretation", "A provisional visual-polish goal is chosen, increasing the chance of solving the wrong problem.", 770, json!({"assumptions": 4, "unresolved": 3})),
            ("tools", "inspection_wide", "Seven exploratory inspections emitted", "Broad reads are used to discover a target that a stronger prompt would have named directly.", 1210, json!({"tool_calls": 7, "files_read": 11, "wasted_reads": 6})),
            ("outcome", "verification_blocked", "Acceptance cannot be verified", "There is no objective definition of better, so completion can only be aesthetic or assumed.", 1652, json!({"checks_passed": 0, "checks_failed": 2, "missing_acceptance": true})),
            ("outcome", "not_accepted", "Weak prompt ends without grounded proof", "The trace is visually busy but cannot establish that the operator's actual need was met.", 1840, json!({"accepted": false, "confidence": "ungrounded"})),
        ]
    };
    let events = specifications
        .iter()
        .enumerate()
        .map(
            |(ordinal, (stage, kind, label, detail, elapsed_ms, data))| FlightEvent {
                ordinal: ordinal as u64,
                at: at + elapsed_ms,
                elapsed_ms: *elapsed_ms,
                stage: (*stage).into(),
                kind: (*kind).into(),
                label: (*label).into(),
                detail: (*detail).into(),
                data: data.clone(),
            },
        )
        .collect();
    Flight {
        id: format!("demo-{role}-{at}"),
        cohort_id,
        prompt: prompt.into(),
        provider: "simulated-observable".into(),
        context_mode: if strong {
            ContextMode::Pruned
        } else {
            ContextMode::Full
        },
        budget: 1_500,
        acceptance_marker: if strong {
            "all checks passed".into()
        } else {
            String::new()
        },
        created_at: at,
        status: if strong {
            FlightStatus::Succeeded
        } else {
            FlightStatus::Failed
        },
        context_preview: if strong {
            "6 precise claims · 286 tokens".into()
        } else {
            "18 broad claims · 1,180 tokens".into()
        },
        routed_files: if strong {
            vec!["flight.rs".into(), "runtime.rs".into(), "app.js".into()]
        } else {
            vec![
                "app.js".into(),
                "store.rs".into(),
                "policy.rs".into(),
                "README.md".into(),
                "runtime.rs".into(),
                "flight.rs".into(),
                "SPEC.md".into(),
            ]
        },
        output_preview: if strong {
            "Bounded implementation verified against four acceptance checks.".into()
        } else {
            "A possible interpretation was explored, but success is undefined.".into()
        },
        metrics: FlightMetrics {
            context_tokens: if strong { 286 } else { 1_180 },
            input_tokens: Some(if strong { 540 } else { 920 }),
            output_tokens: Some(if strong { 210 } else { 340 }),
            tool_calls: if strong { 3 } else { 7 },
            latency_ms: Some(if strong { 980 } else { 1_840 }),
            acceptance_met: Some(strong),
            cached_input_tokens: Some(if strong { 220 } else { 80 }),
            reasoning_tokens: Some(if strong { 184 } else { 390 }),
            provider_events: if strong { 12 } else { 21 },
        },
        events,
        reasoning_profile: if strong {
            ReasoningProfile::High
        } else {
            ReasoningProfile::Default
        },
        comparison_id: Some(comparison_id.into()),
        demo_role: Some(role.into()),
    }
}

fn validate_launch(
    request: &LaunchFlight,
    runners_enabled: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if request.prompt.trim().is_empty() {
        return Err("prompt must not be empty".into());
    }
    if request.prompt.len() > MAX_PROMPT_BYTES {
        return Err(format!("prompt exceeds {MAX_PROMPT_BYTES} bytes").into());
    }
    if !(128..=32_000).contains(&request.budget) {
        return Err("context budget must be between 128 and 32000 tokens".into());
    }
    if !matches!(request.provider.as_str(), "observe" | "codex" | "claude") {
        return Err("provider must be observe, codex, or claude".into());
    }
    if request.provider != "observe" && !runners_enabled {
        return Err(
            "frontier runners are disabled; restart connectome with --enable-runners".into(),
        );
    }
    Ok(())
}

fn load_legacy(store: &PersistentEngine) -> Result<Ledger, Box<dyn std::error::Error>> {
    let Some(bytes) = store.get_projection(FLIGHT_LEDGER)? else {
        return Ok(Ledger {
            format: FORMAT,
            flights: Vec::new(),
        });
    };
    let ledger: Ledger = serde_json::from_slice(&bytes)
        .map_err(|error| format!("flight ledger is unreadable: {error}"))?;
    if ledger.format != FORMAT {
        return Err(format!("flight ledger format {} is unsupported", ledger.format).into());
    }
    Ok(ledger)
}

fn load_runtime(
    store: &PersistentEngine,
) -> Result<(Option<Ledger>, u64), Box<dyn std::error::Error>> {
    let scope = ScopeId::new(FLIGHT_SCOPE)?;
    let observed_head = store.runtime_cursor()?;
    let mut cursor = 0;
    let mut flights = BTreeMap::<String, Flight>::new();
    let mut found = false;
    while cursor < observed_head {
        let page = store.runtime_changes_since(cursor, REPLAY_PAGE, Some(&scope))?;
        let through_cursor = page.through_cursor;
        let has_more = page.has_more();
        for change in page
            .changes
            .into_iter()
            .filter(|change| change.cursor <= observed_head)
        {
            let RuntimeMutation::Record { record } = change.mutation else {
                continue;
            };
            if record.reference.kind.as_str() != FLIGHT_TYPE {
                continue;
            }
            let Some(RuntimeValue::String(encoded)) = record.properties.get("flight_json") else {
                return Err(format!(
                    "prompt flight record at cursor {} has no flight_json",
                    change.cursor
                )
                .into());
            };
            let flight: Flight = serde_json::from_str(encoded)?;
            if record.reference.id.as_str() != flight.id {
                return Err(format!(
                    "prompt flight record at cursor {} disagrees with its id",
                    change.cursor
                )
                .into());
            }
            flights.insert(flight.id.clone(), flight);
            found = true;
        }
        cursor = through_cursor.min(observed_head);
        if !has_more || cursor >= observed_head {
            break;
        }
    }
    Ok((
        found.then(|| Ledger {
            format: FORMAT,
            flights: flights.into_values().collect(),
        }),
        observed_head,
    ))
}

fn load(store: &PersistentEngine) -> Result<(Ledger, bool, u64), Box<dyn std::error::Error>> {
    let (runtime, observed_head) = load_runtime(store)?;
    match runtime {
        Some(ledger) => Ok((ledger, false, observed_head)),
        None => {
            let legacy = load_legacy(store)?;
            let migrate = !legacy.flights.is_empty();
            Ok((legacy, migrate, observed_head))
        }
    }
}

pub(crate) fn stored_flights(
    store: &PersistentEngine,
) -> Result<Vec<Flight>, Box<dyn std::error::Error>> {
    Ok(load(store)?.0.flights)
}

fn flight_record(flight: &Flight) -> Result<RuntimeMutation, Box<dyn std::error::Error>> {
    let mut properties = RuntimeProperties::new();
    properties.insert(
        "flight_json".into(),
        RuntimeValue::String(serde_json::to_string(flight)?),
    );
    properties.insert(
        "cohort_id".into(),
        RuntimeValue::Digest(flight.cohort_id.clone()),
    );
    properties.insert(
        "provider".into(),
        RuntimeValue::String(flight.provider.clone()),
    );
    properties.insert(
        "status".into(),
        RuntimeValue::String(format!("{:?}", flight.status).to_ascii_lowercase()),
    );
    properties.insert(
        "reasoning_effort".into(),
        RuntimeValue::String(flight.reasoning_profile.provider_effort().into()),
    );
    Ok(RuntimeMutation::Record {
        record: RuntimeRecord {
            reference: RuntimeRef::new(FLIGHT_TYPE, flight.id.clone())?,
            valid_from: flight.created_at,
            valid_to: None,
            properties,
        },
    })
}

fn flight_event_mutation(
    flight: &Flight,
    event: &FlightEvent,
) -> Result<RuntimeMutation, Box<dyn std::error::Error>> {
    let mut properties = RuntimeProperties::new();
    properties.insert(
        "event_json".into(),
        RuntimeValue::String(serde_json::to_string(event)?),
    );
    properties.insert("ordinal".into(), RuntimeValue::Unsigned(event.ordinal));
    properties.insert("stage".into(), RuntimeValue::String(event.stage.clone()));
    properties.insert("kind".into(), RuntimeValue::String(event.kind.clone()));
    Ok(RuntimeMutation::Event {
        event: RuntimeEvent {
            kind: RuntimeType::new(FLIGHT_EVENT_TYPE)?,
            subject: Some(RuntimeRef::new(FLIGHT_TYPE, flight.id.clone())?),
            properties,
        },
    })
}

fn flight_schema_update(
    store: &PersistentEngine,
) -> Result<Option<RuntimeSchemaRegistry>, Box<dyn std::error::Error>> {
    let scope = ScopeId::new(FLIGHT_SCOPE)?;
    let current = store.runtime_schema(&scope)?;
    let mut registry = current
        .clone()
        .unwrap_or_else(|| RuntimeSchemaRegistry::empty(1, "bootstrap prompt flight schema"));
    let flight_schema = RuntimeRecordSchema {
        properties: BTreeMap::from([
            (
                "flight_json".into(),
                RuntimePropertySchema::required(RuntimeValueType::String),
            ),
            (
                "cohort_id".into(),
                RuntimePropertySchema::required(RuntimeValueType::Digest),
            ),
            (
                "provider".into(),
                RuntimePropertySchema::required(RuntimeValueType::String),
            ),
            (
                "status".into(),
                RuntimePropertySchema::required(RuntimeValueType::String),
            ),
            (
                "reasoning_effort".into(),
                RuntimePropertySchema::required(RuntimeValueType::String),
            ),
        ]),
        ..RuntimeRecordSchema::default()
    };
    let event_schema = RuntimeEventSchema {
        subject_required: true,
        subject_types: BTreeSet::from([RuntimeType::new(FLIGHT_TYPE)?]),
        properties: BTreeMap::from([
            (
                "event_json".into(),
                RuntimePropertySchema::required(RuntimeValueType::String),
            ),
            (
                "ordinal".into(),
                RuntimePropertySchema::required(RuntimeValueType::Unsigned),
            ),
            (
                "stage".into(),
                RuntimePropertySchema::required(RuntimeValueType::String),
            ),
            (
                "kind".into(),
                RuntimePropertySchema::required(RuntimeValueType::String),
            ),
        ]),
        ..RuntimeEventSchema::default()
    };
    let unchanged = registry.records.get(&RuntimeType::new(FLIGHT_TYPE)?) == Some(&flight_schema)
        && registry.events.get(&RuntimeType::new(FLIGHT_EVENT_TYPE)?) == Some(&event_schema);
    if unchanged {
        return Ok(None);
    }
    registry
        .records
        .insert(RuntimeType::new(FLIGHT_TYPE)?, flight_schema);
    registry
        .events
        .insert(RuntimeType::new(FLIGHT_EVENT_TYPE)?, event_schema);
    if let Some(current) = current {
        registry.revision = current.revision.saturating_add(1);
        registry.migration = "register prompt flight runtime types".into();
    }
    Ok(Some(registry))
}

fn persist(
    store: &PersistentEngine,
    before: &[Flight],
    ledger: &Ledger,
    migrate_legacy: bool,
    observed_cursor: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    let before = before
        .iter()
        .map(|flight| (flight.id.as_str(), flight))
        .collect::<BTreeMap<_, _>>();
    let mut mutations = Vec::new();
    for flight in &ledger.flights {
        let previous = before.get(flight.id.as_str()).copied();
        if !migrate_legacy
            && previous.is_some_and(|previous| {
                serde_json::to_vec(previous).ok() == serde_json::to_vec(flight).ok()
            })
        {
            continue;
        }
        mutations.push(flight_record(flight)?);
        let prior_events = if migrate_legacy {
            0
        } else {
            previous.map_or(0, |previous| previous.events.len())
        };
        for event in flight.events.iter().skip(prior_events) {
            mutations.push(flight_event_mutation(flight, event)?);
        }
    }
    if mutations.is_empty() {
        return Ok(());
    }
    if let Some(registry) = flight_schema_update(store)? {
        mutations.insert(0, RuntimeMutation::Schema { registry });
    }
    store.commit_runtime(&RuntimeCommit {
        scope: ScopeId::new(FLIGHT_SCOPE)?,
        at: now(),
        actor: "connectome:flight-recorder".into(),
        expected_cursor: observed_cursor,
        mutations,
    })?;
    Ok(())
}

fn provider_command(
    provider: &str,
    root: &std::path::Path,
    packet: &str,
    reasoning_profile: ReasoningProfile,
) -> Result<Command, Box<dyn std::error::Error>> {
    let command = match provider {
        "codex" => {
            let mut command = Command::new("codex");
            command
                .arg("exec")
                .arg("--config")
                .arg(format!(
                    "model_reasoning_effort=\"{}\"",
                    reasoning_profile.provider_effort()
                ))
                .args([
                    "--ephemeral",
                    "--sandbox",
                    "read-only",
                    "--json",
                    "--skip-git-repo-check",
                    "--cd",
                ])
                .arg(root)
                .arg(packet);
            command
        }
        "claude" => {
            let mut command = Command::new("claude");
            command
                .args([
                    "-p",
                    "--effort",
                    reasoning_profile.provider_effort(),
                    "--permission-mode",
                    "plan",
                    "--allowedTools",
                    "Read,Grep,Glob",
                    "--output-format",
                    "stream-json",
                    "--verbose",
                    "--no-session-persistence",
                ])
                .arg(packet)
                .current_dir(root);
            command
        }
        _ => return Err(format!("unsupported provider {provider:?}").into()),
    };
    Ok(command)
}

fn classify_provider_event(value: &Value) -> (&'static str, String, String) {
    let outer = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("provider_output");
    let item = value
        .pointer("/item/type")
        .and_then(Value::as_str)
        .unwrap_or(outer);
    if matches!(item, "command_execution" | "tool_use" | "tool_result") || outer.contains("tool") {
        return ("tools", item.to_owned(), format!("Tool event: {item}"));
    }
    if outer.contains("turn")
        || outer.contains("message")
        || item.contains("message")
        || item.contains("reasoning")
    {
        return ("model", item.to_owned(), format!("Model event: {item}"));
    }
    (
        "model",
        outer.to_owned(),
        format!("Provider event: {outer}"),
    )
}

fn provider_detail(value: &Value) -> String {
    for pointer in [
        "/item/text",
        "/item/command",
        "/message/content",
        "/result",
        "/text",
    ] {
        if let Some(text) = value.pointer(pointer).and_then(Value::as_str) {
            return truncate(text, MAX_EVENT_DETAIL_BYTES);
        }
    }
    truncate(
        &serde_json::to_string(value).unwrap_or_default(),
        MAX_EVENT_DETAIL_BYTES,
    )
}

fn find_u64(value: &Value, names: &[&str]) -> Option<u64> {
    match value {
        Value::Object(map) => {
            for name in names {
                if let Some(number) = map.get(*name).and_then(Value::as_u64) {
                    return Some(number);
                }
            }
            map.values().find_map(|child| find_u64(child, names))
        }
        Value::Array(values) => values.iter().find_map(|child| find_u64(child, names)),
        _ => None,
    }
}

fn truncate(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        return value.to_owned();
    }
    let mut end = limit;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n… truncated …", &value[..end])
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use vyrm_store::Engine;

    #[test]
    fn token_metrics_are_found_inside_provider_envelopes() {
        let value = json!({
            "type":"turn.completed",
            "response": {
                "usage": {
                    "input_tokens":41,
                    "cached_input_tokens": 13,
                    "output_tokens":7,
                    "reasoning_output_tokens": 5
                }
            }
        });
        assert_eq!(find_u64(&value, &["input_tokens"]), Some(41));
        assert_eq!(find_u64(&value, &["output_tokens"]), Some(7));
        assert_eq!(find_u64(&value, &["cached_input_tokens"]), Some(13));
        assert_eq!(
            find_u64(&value, &["reasoning_tokens", "reasoning_output_tokens"]),
            Some(5)
        );
    }

    #[test]
    fn reasoning_profiles_map_to_exact_runner_arguments() {
        let root = std::path::Path::new("/tmp/vyrm-profile-test");
        let codex = provider_command("codex", root, "inspect", ReasoningProfile::Extreme).unwrap();
        let codex_args = codex
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(codex_args.contains(&"model_reasoning_effort=\"xhigh\"".into()));
        assert!(codex_args
            .windows(2)
            .any(|args| args == ["--sandbox", "read-only"]));

        let claude = provider_command("claude", root, "inspect", ReasoningProfile::Ultra).unwrap();
        let claude_args = claude
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(claude_args
            .windows(2)
            .any(|args| args == ["--effort", "max"]));
        assert!(claude_args
            .windows(2)
            .any(|args| args == ["--permission-mode", "plan"]));
    }

    #[test]
    fn unicode_truncation_keeps_a_valid_boundary() {
        assert_eq!(truncate("a🧠b", 3), "a\n… truncated …");
    }

    #[test]
    fn provider_envelopes_emit_privacy_bounded_model_and_tool_traces() {
        let root = tempfile::tempdir().unwrap();
        vyrm_node::InstanceManifest::ensure_dedicated(root.path()).unwrap();
        let store =
            Arc::new(PersistentEngine::open(&root.path().join(vyrm_node::STORE_DIR)).unwrap());
        let binding = InstanceBinding::discover(root.path()).unwrap();
        let recorder = FlightRecorder::new(Arc::clone(&store), binding, false);
        let identity = TraceIdentity::derive(&[b"provider-envelope-test"]).unwrap();
        let request = LaunchFlight {
            prompt: "secret prompt".into(),
            provider: "codex".into(),
            context_mode: ContextMode::Fresh,
            budget: 1_500,
            acceptance_marker: String::new(),
            reasoning_profile: ReasoningProfile::High,
        };
        recorder
            .record_provider_envelope(
                "flight-test",
                &request,
                &identity,
                1,
                "model",
                "message",
                r#"{"type":"message","text":"private model output"}"#,
            )
            .unwrap();
        recorder
            .record_provider_envelope(
                "flight-test",
                &request,
                &identity,
                2,
                "tools",
                "command_execution",
                r#"{"type":"tool_use","command":"private command"}"#,
            )
            .unwrap();

        let page = store.runtime_changes_since(0, 64, None).unwrap();
        let trace_events = page
            .changes
            .iter()
            .filter_map(|change| match &change.mutation {
                RuntimeMutation::Event { event }
                    if event.kind.as_str() == vyrm_core::RUNTIME_TRACE_EVENT_TYPE =>
                {
                    Some(event)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(trace_events.len(), 2);
        assert_eq!(
            trace_events[0].properties["domain"],
            RuntimeValue::String("model".into())
        );
        assert_eq!(
            trace_events[1].properties["domain"],
            RuntimeValue::String("tool".into())
        );
        assert_eq!(
            trace_events[1].properties["parent_span_id"],
            RuntimeValue::String(identity.span_id.to_string())
        );
        let encoded = serde_json::to_string(&page).unwrap();
        assert!(!encoded.contains("secret prompt"));
        assert!(!encoded.contains("private model output"));
        assert!(!encoded.contains("private command"));
        assert!(encoded.contains("envelope_digest"));
    }

    #[test]
    fn fresh_and_pruned_arms_preserve_history_but_change_injected_context() {
        use vyrm_core::{Claim, Predicate, Producer, Subject};

        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("lib.rs"), "pub fn runtime() {}\n").unwrap();
        vyrm_node::InstanceManifest::ensure_dedicated(root.path()).unwrap();
        let store =
            Arc::new(PersistentEngine::open(&root.path().join(vyrm_node::STORE_DIR)).unwrap());
        Engine::assert(
            store.as_ref(),
            &Claim::new(
                Subject::new("runtime").unwrap(),
                Predicate::new("status").unwrap(),
                "ready",
                1,
                1,
                Producer {
                    actor: "test".into(),
                    on_behalf_of: None,
                    session: None,
                },
            ),
        )
        .unwrap();
        let before = store.sequence().unwrap();
        let binding = InstanceBinding::discover(root.path()).unwrap();
        let recorder = Arc::new(FlightRecorder::new(Arc::clone(&store), binding, false));

        let fresh = recorder
            .launch(
                LaunchFlight {
                    prompt: "inspect runtime".into(),
                    provider: "observe".into(),
                    context_mode: ContextMode::Fresh,
                    budget: 1_500,
                    acceptance_marker: String::new(),
                    reasoning_profile: ReasoningProfile::Default,
                },
                10,
            )
            .unwrap();
        let pruned = recorder
            .launch(
                LaunchFlight {
                    prompt: "inspect runtime".into(),
                    provider: "observe".into(),
                    context_mode: ContextMode::Pruned,
                    budget: 1_500,
                    acceptance_marker: String::new(),
                    reasoning_profile: ReasoningProfile::Default,
                },
                11,
            )
            .unwrap();

        assert_eq!(fresh.cohort_id, pruned.cohort_id);
        assert_eq!(fresh.metrics.context_tokens, 0);
        assert!(pruned.metrics.context_tokens > 0);
        assert_eq!(
            store.sequence().unwrap(),
            before,
            "context arms never delete claims"
        );
        assert_eq!(recorder.flights().unwrap().len(), 2);
        assert!(store.runtime_cursor().unwrap() > 0);
        assert!(
            store.get_projection(FLIGHT_LEDGER).unwrap().is_none(),
            "new flight writes must not recreate the legacy blob ledger"
        );
    }

    #[test]
    fn weak_and_strong_demos_are_one_comparable_runtime_burst() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("lib.rs"), "pub fn runtime() {}\n").unwrap();
        vyrm_node::InstanceManifest::ensure_dedicated(root.path()).unwrap();
        let store =
            Arc::new(PersistentEngine::open(&root.path().join(vyrm_node::STORE_DIR)).unwrap());
        let binding = InstanceBinding::discover(root.path()).unwrap();
        let recorder = FlightRecorder::new(Arc::clone(&store), binding, false);

        let demos = recorder.seed_prompt_demos(1_000).unwrap();

        assert_eq!(demos.len(), 2);
        let weak = demos
            .iter()
            .find(|flight| flight.demo_role.as_deref() == Some("weak"))
            .unwrap();
        let strong = demos
            .iter()
            .find(|flight| flight.demo_role.as_deref() == Some("strong"))
            .unwrap();
        assert_eq!(weak.comparison_id, strong.comparison_id);
        assert_eq!(weak.metrics.acceptance_met, Some(false));
        assert_eq!(strong.metrics.acceptance_met, Some(true));
        assert!(weak.metrics.context_tokens > strong.metrics.context_tokens);
        assert!(weak.metrics.tool_calls > strong.metrics.tool_calls);

        let scope = ScopeId::new(FLIGHT_SCOPE).unwrap();
        let page = store.runtime_changes_since(0, 64, Some(&scope)).unwrap();
        assert_eq!(
            page.changes.len(),
            19,
            "one schema, two records, and sixteen events"
        );
        assert_eq!(recorder.flights().unwrap().len(), 2);

        let cursor = store.runtime_cursor().unwrap();
        let repeated = recorder.seed_prompt_demos(2_000).unwrap();
        assert_eq!(repeated[0].comparison_id, demos[0].comparison_id);
        assert_eq!(store.runtime_cursor().unwrap(), cursor);
        assert_eq!(recorder.flights().unwrap().len(), 2);
    }

    #[test]
    fn prompt_flights_survive_store_and_recorder_restart() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("lib.rs"), "pub fn runtime() {}\n").unwrap();
        vyrm_node::InstanceManifest::ensure_dedicated(root.path()).unwrap();
        let db = root.path().join(vyrm_node::STORE_DIR);
        let binding = InstanceBinding::discover(root.path()).unwrap();
        let store = Arc::new(PersistentEngine::open(&db).unwrap());
        let recorder = FlightRecorder::new(Arc::clone(&store), binding.clone(), false);

        let mut original = recorder.seed_prompt_demos(1_000).unwrap();
        let cursor = store.runtime_cursor().unwrap();
        drop(recorder);
        drop(store);

        let reopened = Arc::new(PersistentEngine::open(&db).unwrap());
        let restarted = FlightRecorder::new(Arc::clone(&reopened), binding, false);
        let mut recovered = restarted.flights().unwrap();

        original.sort_by(|left, right| left.id.cmp(&right.id));
        recovered.sort_by(|left, right| left.id.cmp(&right.id));

        assert_eq!(reopened.runtime_cursor().unwrap(), cursor);
        assert_eq!(recovered.len(), original.len());
        assert_eq!(recovered[0].id, original[0].id);
        assert_eq!(
            serde_json::to_value(&recovered[0].events).unwrap(),
            serde_json::to_value(&original[0].events).unwrap()
        );
        assert_eq!(recovered[1].id, original[1].id);
        assert_eq!(
            serde_json::to_value(&recovered[1].events).unwrap(),
            serde_json::to_value(&original[1].events).unwrap()
        );
    }
}
