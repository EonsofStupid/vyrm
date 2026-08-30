use super::aggregate::{is_opening_event, LifecycleAggregate};
use super::{LIFECYCLE_RUNTIME_EVENT_TYPE, LIFECYCLE_RUNTIME_SESSION_TYPE};
use rrd_contract::{LifecycleEventEnvelopeV1, LifecycleSessionSnapshotV1, LIFECYCLE_SPEC_VERSION};
use rrd_core::{
    digest, RuntimeEvent, RuntimeEventSchema, RuntimeMutation, RuntimeProperties,
    RuntimePropertySchema, RuntimeRecord, RuntimeRecordSchema, RuntimeRef, RuntimeSchemaRegistry,
    RuntimeType, RuntimeValue, RuntimeValueType, ScopeId,
};
use rrd_store::Engine;
use std::collections::{BTreeMap, BTreeSet};

const REPLAY_PAGE: usize = 1_024;

pub(super) fn load_aggregate_at<E: Engine>(
    store: &E,
    scope: &ScopeId,
    project_id: &str,
    session_id: &str,
    observed_head: u64,
) -> Result<Option<LifecycleAggregate>, Box<dyn std::error::Error>> {
    let expected_subject = session_reference(project_id, session_id)?;
    let mut cursor = 0;
    let mut aggregate = LifecycleAggregate::empty(project_id, session_id);
    while cursor < observed_head {
        let page = store.runtime_changes_since(cursor, REPLAY_PAGE, Some(scope))?;
        let through_cursor = page.through_cursor.min(observed_head);
        for change in page
            .changes
            .into_iter()
            .filter(|change| change.cursor <= observed_head)
        {
            if !change.verify_digest() {
                return Err(format!(
                    "lifecycle runtime change at cursor {} failed hash verification",
                    change.cursor
                )
                .into());
            }
            let RuntimeMutation::Event { event } = change.mutation else {
                continue;
            };
            if event.kind.as_str() != LIFECYCLE_RUNTIME_EVENT_TYPE {
                continue;
            }
            let envelope = decode_runtime_event(&event)?;
            if envelope.project_id != project_id || envelope.session_id != session_id {
                continue;
            }
            if event.subject.as_ref() != Some(&expected_subject)
                || envelope.scope != scope.as_str()
                || envelope.recorded_at_unix_ms != change.at
                || envelope.actor != change.actor
            {
                return Err(format!(
                    "lifecycle runtime binding at cursor {} is inconsistent",
                    change.cursor
                )
                .into());
            }
            if aggregate.events.is_empty() && !is_opening_event(&envelope) {
                return Err("lifecycle session history does not begin with session.opened".into());
            }
            aggregate.append_replayed(envelope)?;
        }
        if through_cursor <= cursor {
            return Err("lifecycle replay made no cursor progress".into());
        }
        cursor = through_cursor;
    }
    Ok((!aggregate.events.is_empty()).then_some(aggregate))
}

pub(super) fn event_mutation(
    event: &LifecycleEventEnvelopeV1,
) -> Result<RuntimeMutation, Box<dyn std::error::Error>> {
    let properties = RuntimeProperties::from([
        (
            "envelope_json".into(),
            RuntimeValue::String(serde_json::to_string(event)?),
        ),
        (
            "event_sha256".into(),
            RuntimeValue::Digest(event.event_sha256.clone()),
        ),
        ("sequence".into(), RuntimeValue::Unsigned(event.sequence)),
        (
            "event_type".into(),
            RuntimeValue::String(event.event_type.as_str().into()),
        ),
        (
            "project_id".into(),
            RuntimeValue::String(event.project_id.clone()),
        ),
        (
            "session_id".into(),
            RuntimeValue::String(event.session_id.clone()),
        ),
    ]);
    Ok(RuntimeMutation::Event {
        event: RuntimeEvent {
            kind: RuntimeType::new(LIFECYCLE_RUNTIME_EVENT_TYPE)?,
            subject: Some(session_reference(&event.project_id, &event.session_id)?),
            properties,
        },
    })
}

fn decode_runtime_event(
    event: &RuntimeEvent,
) -> Result<LifecycleEventEnvelopeV1, Box<dyn std::error::Error>> {
    let Some(RuntimeValue::String(encoded)) = event.properties.get("envelope_json") else {
        return Err("lifecycle runtime event has no envelope_json".into());
    };
    let envelope: LifecycleEventEnvelopeV1 = serde_json::from_str(encoded)?;
    let expected = [
        (
            "event_sha256",
            RuntimeValue::Digest(envelope.event_sha256.clone()),
        ),
        ("sequence", RuntimeValue::Unsigned(envelope.sequence)),
        (
            "event_type",
            RuntimeValue::String(envelope.event_type.as_str().into()),
        ),
        (
            "project_id",
            RuntimeValue::String(envelope.project_id.clone()),
        ),
        (
            "session_id",
            RuntimeValue::String(envelope.session_id.clone()),
        ),
    ];
    if event.properties.len() != expected.len() + 1
        || expected
            .iter()
            .any(|(name, value)| event.properties.get(*name) != Some(value))
    {
        return Err("lifecycle runtime event projection disagrees with its envelope".into());
    }
    Ok(envelope)
}

pub(super) fn session_record(
    snapshot: &LifecycleSessionSnapshotV1,
    projection_stale: bool,
    at: u64,
) -> Result<RuntimeMutation, Box<dyn std::error::Error>> {
    let mut properties = RuntimeProperties::from([
        (
            "project_id".into(),
            RuntimeValue::String(snapshot.project_id.clone()),
        ),
        (
            "session_id".into(),
            RuntimeValue::String(snapshot.session_id.clone()),
        ),
        (
            "phase".into(),
            RuntimeValue::String(snapshot.phase.as_str().into()),
        ),
        (
            "projection_stale".into(),
            RuntimeValue::Bool(projection_stale),
        ),
        (
            "event_sequence".into(),
            RuntimeValue::Unsigned(snapshot.event_count),
        ),
        (
            "state_sha256".into(),
            RuntimeValue::Digest(snapshot.state_sha256.clone()),
        ),
        (
            "last_event_sha256".into(),
            RuntimeValue::Digest(snapshot.latest_event.event_sha256.clone()),
        ),
    ]);
    if let Some(turn_id) = &snapshot.active_turn_id {
        properties.insert(
            "active_turn_id".into(),
            RuntimeValue::String(turn_id.clone()),
        );
    }
    if let Some(tool_call_id) = &snapshot.active_tool_call_id {
        properties.insert(
            "active_tool_call_id".into(),
            RuntimeValue::String(tool_call_id.clone()),
        );
    }
    Ok(RuntimeMutation::Record {
        record: RuntimeRecord {
            reference: session_reference(&snapshot.project_id, &snapshot.session_id)?,
            valid_from: at,
            valid_to: None,
            properties,
        },
    })
}

pub(super) fn lifecycle_schema_update<E: Engine>(
    store: &E,
    scope: &ScopeId,
) -> Result<Option<RuntimeSchemaRegistry>, Box<dyn std::error::Error>> {
    let current = store.runtime_schema(scope)?;
    let mut registry = current.clone().unwrap_or_else(|| {
        RuntimeSchemaRegistry::empty(1, "bootstrap provider-neutral lifecycle schema")
    });
    let session_schema = RuntimeRecordSchema {
        properties: BTreeMap::from([
            required("project_id", RuntimeValueType::String),
            required("session_id", RuntimeValueType::String),
            required("phase", RuntimeValueType::String),
            required("projection_stale", RuntimeValueType::Bool),
            required("event_sequence", RuntimeValueType::Unsigned),
            required("state_sha256", RuntimeValueType::Digest),
            required("last_event_sha256", RuntimeValueType::Digest),
            optional("active_turn_id", RuntimeValueType::String),
            optional("active_tool_call_id", RuntimeValueType::String),
        ]),
        ..RuntimeRecordSchema::default()
    };
    let event_schema = RuntimeEventSchema {
        subject_required: true,
        subject_types: BTreeSet::from([RuntimeType::new(LIFECYCLE_RUNTIME_SESSION_TYPE)?]),
        properties: BTreeMap::from([
            required("envelope_json", RuntimeValueType::String),
            required("event_sha256", RuntimeValueType::Digest),
            required("sequence", RuntimeValueType::Unsigned),
            required("event_type", RuntimeValueType::String),
            required("project_id", RuntimeValueType::String),
            required("session_id", RuntimeValueType::String),
        ]),
        ..RuntimeEventSchema::default()
    };
    let session_type = RuntimeType::new(LIFECYCLE_RUNTIME_SESSION_TYPE)?;
    let event_type = RuntimeType::new(LIFECYCLE_RUNTIME_EVENT_TYPE)?;
    if registry.records.get(&session_type) == Some(&session_schema)
        && registry.events.get(&event_type) == Some(&event_schema)
    {
        return Ok(None);
    }
    registry.records.insert(session_type, session_schema);
    registry.events.insert(event_type, event_schema);
    if let Some(current) = current {
        registry.revision = current
            .revision
            .checked_add(1)
            .ok_or("lifecycle schema revision overflow")?;
        registry.migration = "register provider-neutral lifecycle types".into();
    }
    Ok(Some(registry))
}

fn required(name: &str, value_type: RuntimeValueType) -> (String, RuntimePropertySchema) {
    (name.into(), RuntimePropertySchema::required(value_type))
}

fn optional(name: &str, value_type: RuntimeValueType) -> (String, RuntimePropertySchema) {
    (name.into(), RuntimePropertySchema::optional(value_type))
}

fn session_reference(
    project_id: &str,
    session_id: &str,
) -> Result<RuntimeRef, Box<dyn std::error::Error>> {
    let identity = digest::sha256_hex(&serde_json::to_vec(&(
        LIFECYCLE_SPEC_VERSION,
        project_id,
        session_id,
    ))?);
    Ok(RuntimeRef::new(
        LIFECYCLE_RUNTIME_SESSION_TYPE,
        format!("lifecycle-{identity}"),
    )?)
}

pub(super) fn validate_lookup_identity(
    name: &str,
    value: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        return Err(format!("invalid lifecycle {name}").into());
    }
    Ok(())
}
