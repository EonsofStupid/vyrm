//! Persisted, replayable runtime tracing contract.
//!
//! `tracing` spans remain cheap process diagnostics. This contract is the
//! durable causal evidence that Connectome may freeze, replay, compare, and
//! correlate with committed runtime truth. Callers provide clocks and IDs so
//! tests and replays remain deterministic.

use crate::{
    Error, Millis, ProjectionStamp, ReadStamp, Result, RuntimeEvent, RuntimeEventSchema,
    RuntimeProperties, RuntimePropertySchema, RuntimeSchemaRegistry, RuntimeType, RuntimeValue,
    RuntimeValueType, SnapshotId,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

pub const RUNTIME_TRACE_CONTRACT_VERSION: u16 = 1;
pub const RUNTIME_TRACE_EVENT_TYPE: &str = "runtime_trace";

const MAX_TRACE_NAME_BYTES: usize = 160;
const MAX_TRACE_LINKS: usize = 32;
const MAX_TRACE_ATTRIBUTES: usize = 64;
const MAX_TRACE_VALUE_DEPTH: usize = 8;
const MAX_TRACE_VALUE_NODES: usize = 1_024;
const MAX_TRACE_STRING_BYTES: usize = 8 * 1_024;

macro_rules! hex_ident {
    ($name:ident, $bytes:literal, $label:literal) => {
        #[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self> {
                let value = value.into();
                validate_hex_identity($label, &value, $bytes)?;
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl TryFrom<String> for $name {
            type Error = Error;

            fn try_from(value: String) -> Result<Self> {
                Self::new(value)
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

// W3C Trace Context widths: 16-byte trace ID and 8-byte parent/span ID.
hex_ident!(TraceId, 32, "trace id");
hex_ident!(SpanId, 16, "span id");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TracePhase {
    Start,
    Annotation,
    Finish,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceOutcome {
    Running,
    Ok,
    Error,
    Denied,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceDataClass {
    /// Digests, counts, timings, cursors, and other kernel control evidence.
    Control,
    /// Project/operator-authored knowledge safe under the instance policy.
    Operator,
    /// Prompt, document, model, or tool content requiring explicit retention.
    Content,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceDomain {
    Lifecycle,
    Reasoning,
    Query,
    Planning,
    Storage,
    Projection,
    Search,
    Embedding,
    Model,
    Tool,
    Adapter,
    Cluster,
}

/// A typed causal coordinate. These links make a trace useful for correctness
/// and optimization rather than merely a wall-clock visualization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TraceLink {
    ReasoningRun {
        run_id: String,
    },
    RuntimeCursor {
        cursor: u64,
    },
    Read {
        stamp: ReadStamp,
    },
    Snapshot {
        snapshot_id: SnapshotId,
        cursor: u64,
    },
    Plan {
        plan_digest: String,
    },
    Projection {
        stamp: ProjectionStamp,
    },
    Workflow {
        event: String,
        manifest_digest: String,
    },
    Provider {
        provider: String,
        invocation_id: String,
    },
    /// Correlates a Vyrm operation with a project-scoped external knowledge
    /// source such as pgvector without claiming cross-system ACID semantics.
    OperatorKnowledge {
        adapter: String,
        project_id: String,
        source_revision: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeTraceEvent {
    pub contract_version: u16,
    pub trace_id: TraceId,
    pub span_id: SpanId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_span_id: Option<SpanId>,
    pub phase: TracePhase,
    pub domain: TraceDomain,
    pub name: String,
    pub at: Millis,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_micros: Option<u64>,
    pub outcome: TraceOutcome,
    pub data_class: TraceDataClass,
    #[serde(default)]
    pub links: Vec<TraceLink>,
    #[serde(default)]
    pub attributes: RuntimeProperties,
}

impl RuntimeTraceEvent {
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        trace_id: TraceId,
        span_id: SpanId,
        parent_span_id: Option<SpanId>,
        domain: TraceDomain,
        name: impl Into<String>,
        at: Millis,
        data_class: TraceDataClass,
        links: Vec<TraceLink>,
        attributes: RuntimeProperties,
    ) -> Result<Self> {
        Self::new(
            trace_id,
            span_id,
            parent_span_id,
            TracePhase::Start,
            domain,
            name,
            at,
            None,
            TraceOutcome::Running,
            data_class,
            links,
            attributes,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn annotation(
        trace_id: TraceId,
        span_id: SpanId,
        parent_span_id: Option<SpanId>,
        domain: TraceDomain,
        name: impl Into<String>,
        at: Millis,
        outcome: TraceOutcome,
        data_class: TraceDataClass,
        links: Vec<TraceLink>,
        attributes: RuntimeProperties,
    ) -> Result<Self> {
        Self::new(
            trace_id,
            span_id,
            parent_span_id,
            TracePhase::Annotation,
            domain,
            name,
            at,
            None,
            outcome,
            data_class,
            links,
            attributes,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn finish(
        trace_id: TraceId,
        span_id: SpanId,
        parent_span_id: Option<SpanId>,
        domain: TraceDomain,
        name: impl Into<String>,
        at: Millis,
        duration_micros: u64,
        outcome: TraceOutcome,
        data_class: TraceDataClass,
        links: Vec<TraceLink>,
        attributes: RuntimeProperties,
    ) -> Result<Self> {
        Self::new(
            trace_id,
            span_id,
            parent_span_id,
            TracePhase::Finish,
            domain,
            name,
            at,
            Some(duration_micros),
            outcome,
            data_class,
            links,
            attributes,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn new(
        trace_id: TraceId,
        span_id: SpanId,
        parent_span_id: Option<SpanId>,
        phase: TracePhase,
        domain: TraceDomain,
        name: impl Into<String>,
        at: Millis,
        duration_micros: Option<u64>,
        outcome: TraceOutcome,
        data_class: TraceDataClass,
        links: Vec<TraceLink>,
        attributes: RuntimeProperties,
    ) -> Result<Self> {
        let event = Self {
            contract_version: RUNTIME_TRACE_CONTRACT_VERSION,
            trace_id,
            span_id,
            parent_span_id,
            phase,
            domain,
            name: name.into(),
            at,
            duration_micros,
            outcome,
            data_class,
            links,
            attributes,
        };
        event.validate()?;
        Ok(event)
    }

    pub fn validate(&self) -> Result<()> {
        if self.contract_version != RUNTIME_TRACE_CONTRACT_VERSION {
            return invalid(format!(
                "unsupported runtime trace contract version {}",
                self.contract_version
            ));
        }
        validate_bounded_text("trace operation", &self.name, MAX_TRACE_NAME_BYTES)?;
        if self.parent_span_id.as_ref() == Some(&self.span_id) {
            return invalid("a trace span cannot parent itself");
        }
        match (self.phase, self.duration_micros, self.outcome) {
            (TracePhase::Start, None, TraceOutcome::Running) => {}
            (TracePhase::Annotation, None, _) => {}
            (TracePhase::Finish, Some(_), outcome) if outcome != TraceOutcome::Running => {}
            _ => {
                return invalid(
                    "trace phase, duration, and outcome do not form a valid lifecycle transition",
                );
            }
        }
        if self.links.len() > MAX_TRACE_LINKS {
            return invalid(format!(
                "runtime trace exceeds {MAX_TRACE_LINKS} causal links"
            ));
        }
        for link in &self.links {
            validate_link(link)?;
        }
        if self.attributes.len() > MAX_TRACE_ATTRIBUTES {
            return invalid(format!(
                "runtime trace exceeds {MAX_TRACE_ATTRIBUTES} attributes"
            ));
        }
        let mut nodes = 0;
        for (name, value) in &self.attributes {
            validate_bounded_text("trace attribute name", name, MAX_TRACE_NAME_BYTES)?;
            validate_value(value, 0, &mut nodes)?;
        }
        Ok(())
    }

    /// Converts the trace into the existing append-only runtime event model.
    /// Persistence therefore inherits the runtime cursor and hash-chain rather
    /// than creating an unrelated telemetry store.
    pub fn into_runtime_event(self) -> Result<RuntimeEvent> {
        self.validate()?;
        let mut properties = RuntimeProperties::from([
            (
                "contract_version".into(),
                RuntimeValue::Unsigned(u64::from(self.contract_version)),
            ),
            (
                "trace_id".into(),
                RuntimeValue::String(self.trace_id.to_string()),
            ),
            (
                "span_id".into(),
                RuntimeValue::String(self.span_id.to_string()),
            ),
            (
                "phase".into(),
                RuntimeValue::String(enum_name(self.phase).into()),
            ),
            (
                "domain".into(),
                RuntimeValue::String(domain_name(self.domain).into()),
            ),
            ("name".into(), RuntimeValue::String(self.name)),
            ("at".into(), RuntimeValue::Unsigned(self.at)),
            (
                "outcome".into(),
                RuntimeValue::String(outcome_name(self.outcome).into()),
            ),
            (
                "data_class".into(),
                RuntimeValue::String(data_class_name(self.data_class).into()),
            ),
            (
                "links".into(),
                RuntimeValue::List(self.links.iter().map(link_value).collect()),
            ),
            ("attributes".into(), RuntimeValue::Map(self.attributes)),
        ]);
        if let Some(parent) = self.parent_span_id {
            properties.insert(
                "parent_span_id".into(),
                RuntimeValue::String(parent.to_string()),
            );
        }
        if let Some(duration) = self.duration_micros {
            properties.insert("duration_micros".into(), RuntimeValue::Unsigned(duration));
        }
        Ok(RuntimeEvent {
            kind: Self::event_type()?,
            subject: None,
            properties,
        })
    }

    pub fn event_type() -> Result<RuntimeType> {
        RuntimeType::new(RUNTIME_TRACE_EVENT_TYPE)
    }

    /// Strict schema entry an instance adds during bootstrap or migration.
    pub fn event_schema() -> RuntimeEventSchema {
        let mut properties = BTreeMap::new();
        for name in ["contract_version", "at"] {
            properties.insert(
                name.into(),
                RuntimePropertySchema::required(RuntimeValueType::Unsigned),
            );
        }
        for name in [
            "trace_id",
            "span_id",
            "phase",
            "domain",
            "name",
            "outcome",
            "data_class",
        ] {
            properties.insert(
                name.into(),
                RuntimePropertySchema::required(RuntimeValueType::String),
            );
        }
        properties.insert(
            "parent_span_id".into(),
            RuntimePropertySchema::optional(RuntimeValueType::String),
        );
        properties.insert(
            "duration_micros".into(),
            RuntimePropertySchema::optional(RuntimeValueType::Unsigned),
        );
        properties.insert(
            "links".into(),
            RuntimePropertySchema::required(RuntimeValueType::List),
        );
        properties.insert(
            "attributes".into(),
            RuntimePropertySchema::required(RuntimeValueType::Map),
        );
        RuntimeEventSchema {
            properties,
            ..RuntimeEventSchema::default()
        }
    }

    /// Installs or repairs the canonical trace event declaration in a schema
    /// under construction. The caller owns schema revision allocation because
    /// that compare-and-swap must occur against its storage engine's exact
    /// read stamp.
    pub fn register_schema(registry: &mut RuntimeSchemaRegistry) -> Result<bool> {
        let kind = Self::event_type()?;
        let schema = Self::event_schema();
        if registry.events.get(&kind) == Some(&schema) {
            return Ok(false);
        }
        registry.events.insert(kind, schema);
        Ok(true)
    }
}

fn validate_hex_identity(kind: &'static str, value: &str, width: usize) -> Result<()> {
    if value.len() != width
        || value
            .bytes()
            .any(|byte| !byte.is_ascii_hexdigit() || byte.is_ascii_uppercase())
        || value.bytes().all(|byte| byte == b'0')
    {
        return invalid(format!(
            "{kind} must be {width} lowercase hexadecimal characters and not all zero"
        ));
    }
    Ok(())
}

fn validate_link(link: &TraceLink) -> Result<()> {
    match link {
        TraceLink::ReasoningRun { run_id } => {
            validate_bounded_text("trace reasoning run", run_id, MAX_TRACE_NAME_BYTES)
        }
        TraceLink::RuntimeCursor { .. } => Ok(()),
        TraceLink::Read { stamp } => stamp.validate(),
        TraceLink::Snapshot { snapshot_id, .. } => validate_bounded_text(
            "trace snapshot id",
            snapshot_id.as_str(),
            MAX_TRACE_STRING_BYTES,
        ),
        TraceLink::Plan { plan_digest } => validate_digest("trace plan", plan_digest),
        TraceLink::Projection { stamp } => stamp.validate(),
        TraceLink::Workflow {
            event,
            manifest_digest,
        } => {
            validate_bounded_text("trace workflow event", event, MAX_TRACE_NAME_BYTES)?;
            validate_digest("trace workflow manifest", manifest_digest)
        }
        TraceLink::Provider {
            provider,
            invocation_id,
        } => {
            validate_bounded_text("trace provider", provider, MAX_TRACE_NAME_BYTES)?;
            validate_bounded_text(
                "trace provider invocation",
                invocation_id,
                MAX_TRACE_STRING_BYTES,
            )
        }
        TraceLink::OperatorKnowledge {
            adapter,
            project_id,
            source_revision,
        } => {
            validate_bounded_text("operator knowledge adapter", adapter, MAX_TRACE_NAME_BYTES)?;
            validate_bounded_text(
                "operator knowledge project",
                project_id,
                MAX_TRACE_NAME_BYTES,
            )?;
            validate_bounded_text(
                "operator knowledge revision",
                source_revision,
                MAX_TRACE_STRING_BYTES,
            )
        }
    }
}

fn validate_value(value: &RuntimeValue, depth: usize, nodes: &mut usize) -> Result<()> {
    *nodes = nodes.saturating_add(1);
    if *nodes > MAX_TRACE_VALUE_NODES || depth > MAX_TRACE_VALUE_DEPTH {
        return invalid("runtime trace attribute tree exceeds its structural budget");
    }
    match value {
        RuntimeValue::String(value)
        | RuntimeValue::Digest(value)
        | RuntimeValue::Decimal(value) => {
            if value.len() > MAX_TRACE_STRING_BYTES {
                return invalid(format!(
                    "runtime trace string exceeds {MAX_TRACE_STRING_BYTES} bytes"
                ));
            }
        }
        RuntimeValue::List(values) => {
            for value in values {
                validate_value(value, depth + 1, nodes)?;
            }
        }
        RuntimeValue::Map(values) => {
            for (name, value) in values {
                validate_bounded_text("trace map key", name, MAX_TRACE_NAME_BYTES)?;
                validate_value(value, depth + 1, nodes)?;
            }
        }
        RuntimeValue::Null
        | RuntimeValue::Bool(_)
        | RuntimeValue::Integer(_)
        | RuntimeValue::Unsigned(_) => {}
    }
    Ok(())
}

fn link_value(link: &TraceLink) -> RuntimeValue {
    let mut value = BTreeMap::new();
    match link {
        TraceLink::ReasoningRun { run_id } => {
            value.insert("kind".into(), RuntimeValue::String("reasoning_run".into()));
            value.insert("run_id".into(), RuntimeValue::String(run_id.clone()));
        }
        TraceLink::RuntimeCursor { cursor } => {
            value.insert("kind".into(), RuntimeValue::String("runtime_cursor".into()));
            value.insert("cursor".into(), RuntimeValue::Unsigned(*cursor));
        }
        TraceLink::Read { stamp } => {
            value.insert("kind".into(), RuntimeValue::String("read".into()));
            value.insert(
                "contract_version".into(),
                RuntimeValue::Unsigned(u64::from(stamp.contract_version)),
            );
            value.insert(
                "scope".into(),
                RuntimeValue::String(stamp.scope.to_string()),
            );
            if let Some(schema_revision) = stamp.schema_revision {
                value.insert(
                    "schema_revision".into(),
                    RuntimeValue::Unsigned(schema_revision),
                );
            }
            value.insert(
                "catalog_revision".into(),
                RuntimeValue::Unsigned(stamp.catalog_revision),
            );
            value.insert(
                "commit_cursor".into(),
                RuntimeValue::Unsigned(stamp.commit_cursor),
            );
            // Retain the original v1 alias for existing consumers while the
            // exact field name makes the complete stamp reconstructable.
            value.insert("cursor".into(), RuntimeValue::Unsigned(stamp.commit_cursor));
            if let Some(head_digest) = &stamp.head_digest {
                value.insert(
                    "head_digest".into(),
                    RuntimeValue::Digest(head_digest.clone()),
                );
            }
            value.insert(
                "manifest_id".into(),
                RuntimeValue::Digest(stamp.manifest_id.clone()),
            );
        }
        TraceLink::Snapshot {
            snapshot_id,
            cursor,
        } => {
            value.insert("kind".into(), RuntimeValue::String("snapshot".into()));
            value.insert(
                "snapshot_id".into(),
                RuntimeValue::String(snapshot_id.to_string()),
            );
            value.insert("cursor".into(), RuntimeValue::Unsigned(*cursor));
        }
        TraceLink::Plan { plan_digest } => {
            value.insert("kind".into(), RuntimeValue::String("plan".into()));
            value.insert(
                "plan_digest".into(),
                RuntimeValue::Digest(plan_digest.clone()),
            );
        }
        TraceLink::Projection { stamp } => {
            value.insert("kind".into(), RuntimeValue::String("projection".into()));
            value.insert(
                "contract_version".into(),
                RuntimeValue::Unsigned(u64::from(stamp.contract_version)),
            );
            value.insert("id".into(), RuntimeValue::String(stamp.id.to_string()));
            value.insert(
                "generation".into(),
                RuntimeValue::Unsigned(stamp.generation),
            );
            value.insert(
                "source_cursor".into(),
                RuntimeValue::Unsigned(stamp.source_cursor),
            );
            value.insert(
                "config_digest".into(),
                RuntimeValue::Digest(stamp.config_digest.clone()),
            );
            value.insert(
                "artifact_digest".into(),
                RuntimeValue::Digest(stamp.artifact_digest.clone()),
            );
            value.insert(
                "state".into(),
                RuntimeValue::String(projection_state_name(stamp.state).into()),
            );
        }
        TraceLink::Workflow {
            event,
            manifest_digest,
        } => {
            value.insert("kind".into(), RuntimeValue::String("workflow".into()));
            value.insert("event".into(), RuntimeValue::String(event.clone()));
            value.insert(
                "manifest_digest".into(),
                RuntimeValue::Digest(manifest_digest.clone()),
            );
        }
        TraceLink::Provider {
            provider,
            invocation_id,
        } => {
            value.insert("kind".into(), RuntimeValue::String("provider".into()));
            value.insert("provider".into(), RuntimeValue::String(provider.clone()));
            value.insert(
                "invocation_id".into(),
                RuntimeValue::String(invocation_id.clone()),
            );
        }
        TraceLink::OperatorKnowledge {
            adapter,
            project_id,
            source_revision,
        } => {
            value.insert(
                "kind".into(),
                RuntimeValue::String("operator_knowledge".into()),
            );
            value.insert("adapter".into(), RuntimeValue::String(adapter.clone()));
            value.insert(
                "project_id".into(),
                RuntimeValue::String(project_id.clone()),
            );
            value.insert(
                "source_revision".into(),
                RuntimeValue::String(source_revision.clone()),
            );
        }
    }
    RuntimeValue::Map(value)
}

fn enum_name(value: TracePhase) -> &'static str {
    match value {
        TracePhase::Start => "start",
        TracePhase::Annotation => "annotation",
        TracePhase::Finish => "finish",
    }
}

fn outcome_name(value: TraceOutcome) -> &'static str {
    match value {
        TraceOutcome::Running => "running",
        TraceOutcome::Ok => "ok",
        TraceOutcome::Error => "error",
        TraceOutcome::Denied => "denied",
        TraceOutcome::Cancelled => "cancelled",
    }
}

fn data_class_name(value: TraceDataClass) -> &'static str {
    match value {
        TraceDataClass::Control => "control",
        TraceDataClass::Operator => "operator",
        TraceDataClass::Content => "content",
    }
}

fn domain_name(value: TraceDomain) -> &'static str {
    match value {
        TraceDomain::Lifecycle => "lifecycle",
        TraceDomain::Reasoning => "reasoning",
        TraceDomain::Query => "query",
        TraceDomain::Planning => "planning",
        TraceDomain::Storage => "storage",
        TraceDomain::Projection => "projection",
        TraceDomain::Search => "search",
        TraceDomain::Embedding => "embedding",
        TraceDomain::Model => "model",
        TraceDomain::Tool => "tool",
        TraceDomain::Adapter => "adapter",
        TraceDomain::Cluster => "cluster",
    }
}

fn projection_state_name(value: crate::ProjectionState) -> &'static str {
    match value {
        crate::ProjectionState::Building => "building",
        crate::ProjectionState::Ready => "ready",
        crate::ProjectionState::Quarantined => "quarantined",
        crate::ProjectionState::Retiring => "retiring",
    }
}

fn validate_digest(kind: &'static str, value: &str) -> Result<()> {
    if value.len() != 64 || value.bytes().any(|byte| !byte.is_ascii_hexdigit()) {
        return invalid(format!("{kind} digest must be 64 hexadecimal characters"));
    }
    Ok(())
}

fn validate_bounded_text(kind: &'static str, value: &str, max: usize) -> Result<()> {
    if value.trim().is_empty() || value.len() > max || value.as_bytes().contains(&0) {
        return invalid(format!(
            "{kind} must be non-empty, contain no NUL, and fit within {max} bytes"
        ));
    }
    Ok(())
}

fn invalid<T>(reason: impl Into<String>) -> Result<T> {
    Err(Error::InvalidRuntime {
        reason: reason.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trace_id() -> TraceId {
        TraceId::new("0123456789abcdef0123456789abcdef").unwrap()
    }

    fn span_id() -> SpanId {
        SpanId::new("0123456789abcdef").unwrap()
    }

    #[test]
    fn w3c_id_widths_and_lifecycle_transitions_fail_closed() {
        assert!(TraceId::new("0".repeat(32)).is_err());
        assert!(TraceId::new("ABCDEF0123456789abcdef0123456789").is_err());
        assert!(SpanId::new("0123").is_err());

        let mut event = RuntimeTraceEvent::start(
            trace_id(),
            span_id(),
            None,
            TraceDomain::Query,
            "vyrmql.execute",
            10,
            TraceDataClass::Control,
            Vec::new(),
            RuntimeProperties::new(),
        )
        .unwrap();
        event.duration_micros = Some(1);
        assert!(event.validate().is_err());
    }

    #[test]
    fn operator_knowledge_is_linked_without_claiming_shared_transactions() {
        let event = RuntimeTraceEvent::finish(
            trace_id(),
            span_id(),
            None,
            TraceDomain::Adapter,
            "operator.pgvector.search",
            20,
            800,
            TraceOutcome::Ok,
            TraceDataClass::Operator,
            vec![TraceLink::OperatorKnowledge {
                adapter: "pgvector".into(),
                project_id: "vyrm".into(),
                source_revision: "postgres-lsn:0/16B6C50".into(),
            }],
            RuntimeProperties::from([("result_count".into(), RuntimeValue::Unsigned(7))]),
        )
        .unwrap();

        let runtime = event.into_runtime_event().unwrap();
        assert_eq!(runtime.kind.as_str(), RUNTIME_TRACE_EVENT_TYPE);
        assert_eq!(
            runtime.properties["outcome"],
            RuntimeValue::String("ok".into())
        );
        assert_eq!(
            RuntimeTraceEvent::event_schema().properties["links"].value_type,
            RuntimeValueType::List
        );
    }

    #[test]
    fn schema_registration_is_strict_and_idempotent() {
        let mut registry = RuntimeSchemaRegistry::empty(1, "trace bootstrap");
        assert!(RuntimeTraceEvent::register_schema(&mut registry).unwrap());
        assert!(!RuntimeTraceEvent::register_schema(&mut registry).unwrap());
        assert_eq!(
            registry.events[&RuntimeTraceEvent::event_type().unwrap()],
            RuntimeTraceEvent::event_schema()
        );
    }

    #[test]
    fn attributes_are_bounded_before_they_reach_the_runtime_log() {
        let event = RuntimeTraceEvent::annotation(
            trace_id(),
            span_id(),
            None,
            TraceDomain::Model,
            "provider.envelope",
            10,
            TraceOutcome::Running,
            TraceDataClass::Content,
            Vec::new(),
            RuntimeProperties::from([(
                "payload".into(),
                RuntimeValue::String("x".repeat(MAX_TRACE_STRING_BYTES + 1)),
            )]),
        );
        assert!(event.is_err());
    }

    #[test]
    fn persisted_read_and_projection_links_retain_complete_reconstructable_stamps() {
        let read = ReadStamp::new(
            crate::ScopeId::new("instance:trace").unwrap(),
            Some(7),
            11,
            29,
            Some("a".repeat(64)),
        )
        .unwrap();
        let projection = ProjectionStamp {
            contract_version: crate::DATA_RUNTIME_CONTRACT_VERSION,
            id: crate::ProjectionId::new("vector:operator").unwrap(),
            generation: 3,
            source_cursor: 29,
            config_digest: "b".repeat(64),
            artifact_digest: "c".repeat(64),
            state: crate::ProjectionState::Ready,
        };
        projection.validate().unwrap();
        let runtime = RuntimeTraceEvent::annotation(
            trace_id(),
            span_id(),
            None,
            TraceDomain::Projection,
            "projection.publish",
            10,
            TraceOutcome::Ok,
            TraceDataClass::Control,
            vec![
                TraceLink::Read {
                    stamp: read.clone(),
                },
                TraceLink::Projection {
                    stamp: projection.clone(),
                },
            ],
            RuntimeProperties::new(),
        )
        .unwrap()
        .into_runtime_event()
        .unwrap();
        let RuntimeValue::List(links) = &runtime.properties["links"] else {
            panic!("links must remain a typed list")
        };
        let RuntimeValue::Map(read_link) = &links[0] else {
            panic!("read link must remain a typed map")
        };
        assert_eq!(
            read_link["contract_version"],
            RuntimeValue::Unsigned(u64::from(read.contract_version))
        );
        assert_eq!(
            read_link["schema_revision"],
            RuntimeValue::Unsigned(read.schema_revision.unwrap())
        );
        assert_eq!(
            read_link["catalog_revision"],
            RuntimeValue::Unsigned(read.catalog_revision)
        );
        assert_eq!(
            read_link["commit_cursor"],
            RuntimeValue::Unsigned(read.commit_cursor)
        );
        assert_eq!(
            read_link["head_digest"],
            RuntimeValue::Digest(read.head_digest.unwrap())
        );
        let RuntimeValue::Map(projection_link) = &links[1] else {
            panic!("projection link must remain a typed map")
        };
        assert_eq!(
            projection_link["config_digest"],
            RuntimeValue::Digest(projection.config_digest)
        );
        assert_eq!(
            projection_link["state"],
            RuntimeValue::String("ready".into())
        );
    }
}
