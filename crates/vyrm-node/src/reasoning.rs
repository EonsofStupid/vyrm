//! Durable runtime composition for the typed reasoning-run contract.
//!
//! New writes are append-only typed runtime commits. The former whole-ledger
//! projection remains readable and is migrated atomically on its next mutation.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use vyrm_core::{
    Millis, ReasoningEvent, ReasoningPayload, ReasoningRun, RuntimeCommit, RuntimeEvent,
    RuntimeEventSchema, RuntimeMutation, RuntimeProperties, RuntimePropertySchema, RuntimeRecord,
    RuntimeRecordSchema, RuntimeRef, RuntimeSchemaRegistry, RuntimeType, RuntimeValue,
    RuntimeValueType, ScopeId,
};
use vyrm_store::Engine;

/// Legacy projection name. Kept for a read-once migration path only.
pub const REASONING_LEDGER: &str = "reasoning-ledger-v1";
pub const REASONING_SCOPE: &str = "instance:default";
const FORMAT: u32 = 1;
const REPLAY_PAGE: usize = 1_024;
const RUN_TYPE: &str = "reasoning_run";
const EVENT_TYPE: &str = "reasoning_event";

#[derive(Debug, Default, Serialize, Deserialize)]
struct Ledger {
    format: u32,
    active: Option<String>,
    runs: BTreeMap<String, Vec<ReasoningEvent>>,
}

fn empty_ledger() -> Ledger {
    Ledger {
        format: FORMAT,
        ..Ledger::default()
    }
}

fn validate_ledger(mut ledger: Ledger) -> Result<Ledger, Box<dyn std::error::Error>> {
    if ledger.format != FORMAT {
        return Err(format!(
            "reasoning ledger format {} is unsupported (expected {})",
            ledger.format, FORMAT
        )
        .into());
    }
    let mut active = None;
    for (id, events) in &ledger.runs {
        let run = ReasoningRun::replay(events.clone())?;
        if run.id() != id {
            return Err(
                format!("reasoning ledger key {id:?} disagrees with its event run id").into(),
            );
        }
        if !run.is_complete() {
            if let Some(other) = active.replace(id.clone()) {
                return Err(format!(
                    "reasoning ledger has multiple active runs: {other:?} and {id:?}"
                )
                .into());
            }
        }
    }
    ledger.active = active;
    Ok(ledger)
}

fn load_legacy<E: Engine>(store: &E) -> Result<Ledger, Box<dyn std::error::Error>> {
    let Some(bytes) = store.get_projection(REASONING_LEDGER)? else {
        return Ok(empty_ledger());
    };
    let ledger: Ledger = serde_json::from_slice(&bytes).map_err(|error| {
        format!("reasoning ledger is unreadable and mutations must wait: {error}")
    })?;
    validate_ledger(ledger)
}

/// Returns `None` when the typed log contains no reasoning events, which is
/// how the caller distinguishes a legacy store requiring migration.
fn load_runtime<E: Engine>(store: &E) -> Result<(Option<Ledger>, u64), Box<dyn std::error::Error>> {
    let scope = ScopeId::new(REASONING_SCOPE)?;
    let observed_head = store.runtime_cursor()?;
    let mut cursor = 0;
    let mut ledger = empty_ledger();
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
            let RuntimeMutation::Event { event } = change.mutation else {
                continue;
            };
            if event.kind.as_str() != EVENT_TYPE {
                continue;
            }
            let Some(RuntimeValue::String(encoded)) = event.properties.get("event_json") else {
                return Err(format!(
                    "reasoning runtime event at cursor {} has no event_json",
                    change.cursor
                )
                .into());
            };
            let reasoning: ReasoningEvent = serde_json::from_str(encoded)?;
            if event.subject.as_ref() != Some(&RuntimeRef::new(RUN_TYPE, reasoning.run_id.clone())?)
            {
                return Err(format!(
                    "reasoning runtime event at cursor {} names the wrong run",
                    change.cursor
                )
                .into());
            }
            ledger
                .runs
                .entry(reasoning.run_id.clone())
                .or_default()
                .push(reasoning);
            found = true;
        }
        cursor = through_cursor.min(observed_head);
        if !has_more || cursor >= observed_head {
            break;
        }
    }
    Ok((
        found.then(|| validate_ledger(ledger)).transpose()?,
        observed_head,
    ))
}

fn load<E: Engine>(store: &E) -> Result<(Ledger, bool, u64), Box<dyn std::error::Error>> {
    let (runtime, observed_head) = load_runtime(store)?;
    match runtime {
        Some(ledger) => Ok((ledger, false, observed_head)),
        None => {
            let legacy = load_legacy(store)?;
            let needs_migration = !legacy.runs.is_empty();
            Ok((legacy, needs_migration, observed_head))
        }
    }
}

fn event_mutation(event: &ReasoningEvent) -> Result<RuntimeMutation, Box<dyn std::error::Error>> {
    let mut properties = RuntimeProperties::new();
    properties.insert(
        "event_json".into(),
        RuntimeValue::String(serde_json::to_string(event)?),
    );
    properties.insert(
        "event_digest".into(),
        RuntimeValue::Digest(event.digest.clone()),
    );
    properties.insert("ordinal".into(), RuntimeValue::Unsigned(event.ordinal));
    properties.insert(
        "payload_kind".into(),
        RuntimeValue::String(event.payload.name().into()),
    );
    Ok(RuntimeMutation::Event {
        event: RuntimeEvent {
            kind: RuntimeType::new(EVENT_TYPE)?,
            subject: Some(RuntimeRef::new(RUN_TYPE, event.run_id.clone())?),
            properties,
        },
    })
}

fn run_record(run: &ReasoningRun) -> Result<RuntimeMutation, Box<dyn std::error::Error>> {
    let first = run
        .events()
        .first()
        .ok_or("cannot persist an empty reasoning run")?;
    let last = run
        .events()
        .last()
        .expect("first established non-empty run");
    let mut properties = RuntimeProperties::new();
    properties.insert(
        "state".into(),
        RuntimeValue::String(format!("{:?}", run.state()).to_ascii_lowercase()),
    );
    properties.insert("complete".into(), RuntimeValue::Bool(run.is_complete()));
    properties.insert(
        "last_event_digest".into(),
        RuntimeValue::Digest(last.digest.clone()),
    );
    Ok(RuntimeMutation::Record {
        record: RuntimeRecord {
            reference: RuntimeRef::new(RUN_TYPE, run.id().to_owned())?,
            valid_from: first.at,
            valid_to: None,
            properties,
        },
    })
}

fn reasoning_schema_update<E: Engine>(
    store: &E,
) -> Result<Option<RuntimeSchemaRegistry>, Box<dyn std::error::Error>> {
    let scope = ScopeId::new(REASONING_SCOPE)?;
    let current = store.runtime_schema(&scope)?;
    let mut registry = current
        .clone()
        .unwrap_or_else(|| RuntimeSchemaRegistry::empty(1, "bootstrap reasoning runtime schema"));
    let run_schema = RuntimeRecordSchema {
        properties: BTreeMap::from([
            (
                "state".into(),
                RuntimePropertySchema::required(RuntimeValueType::String),
            ),
            (
                "complete".into(),
                RuntimePropertySchema::required(RuntimeValueType::Bool),
            ),
            (
                "last_event_digest".into(),
                RuntimePropertySchema::required(RuntimeValueType::Digest),
            ),
        ]),
        ..RuntimeRecordSchema::default()
    };
    let event_schema = RuntimeEventSchema {
        subject_required: true,
        subject_types: BTreeSet::from([RuntimeType::new(RUN_TYPE)?]),
        properties: BTreeMap::from([
            (
                "event_json".into(),
                RuntimePropertySchema::required(RuntimeValueType::String),
            ),
            (
                "event_digest".into(),
                RuntimePropertySchema::required(RuntimeValueType::Digest),
            ),
            (
                "ordinal".into(),
                RuntimePropertySchema::required(RuntimeValueType::Unsigned),
            ),
            (
                "payload_kind".into(),
                RuntimePropertySchema::required(RuntimeValueType::String),
            ),
        ]),
        ..RuntimeEventSchema::default()
    };
    let unchanged = registry.records.get(&RuntimeType::new(RUN_TYPE)?) == Some(&run_schema)
        && registry.events.get(&RuntimeType::new(EVENT_TYPE)?) == Some(&event_schema);
    if unchanged {
        return Ok(None);
    }
    registry
        .records
        .insert(RuntimeType::new(RUN_TYPE)?, run_schema);
    registry
        .events
        .insert(RuntimeType::new(EVENT_TYPE)?, event_schema);
    if let Some(current) = current {
        registry.revision = current.revision.saturating_add(1);
        registry.migration = "register reasoning runtime types".into();
    }
    Ok(Some(registry))
}

/// Records exactly one validated state transition as one optimistic,
/// authoritative runtime commit. Concurrent writers conflict instead of
/// replacing each other's ledger snapshots.
pub fn record_reasoning<E: Engine>(
    store: &E,
    run_id: &str,
    at: Millis,
    actor: &str,
    payload: ReasoningPayload,
) -> Result<ReasoningEvent, Box<dyn std::error::Error>> {
    let (mut ledger, migrate_legacy, observed_cursor) = load(store)?;
    let is_goal = matches!(payload, ReasoningPayload::Goal { .. });
    if is_goal {
        if ledger.runs.contains_key(run_id) {
            return Err(format!("reasoning run {run_id:?} already exists").into());
        }
        if let Some(active) = &ledger.active {
            return Err(format!(
                "reasoning run {active:?} is still active; complete it before starting {run_id:?}"
            )
            .into());
        }
    } else if ledger.active.as_deref() != Some(run_id) {
        return Err(format!(
            "reasoning run {run_id:?} is not active (active: {})",
            ledger.active.as_deref().unwrap_or("none")
        )
        .into());
    }

    let mut run = match ledger.runs.get(run_id) {
        Some(events) => ReasoningRun::replay(events.clone())?,
        None => ReasoningRun::empty(run_id)?,
    };
    let event = run.append(at, actor, payload)?;
    ledger.runs.insert(run_id.to_owned(), run.events().to_vec());
    ledger.active = if run.is_complete() {
        None
    } else {
        Some(run_id.to_owned())
    };

    let mut mutations = Vec::new();
    if migrate_legacy {
        // One current node version per run followed by its immutable event
        // history. The migration and the new transition are all-or-nothing.
        for events in ledger.runs.values() {
            let migrated = ReasoningRun::replay(events.clone())?;
            mutations.push(run_record(&migrated)?);
            for historical in migrated.events() {
                mutations.push(event_mutation(historical)?);
            }
        }
    } else {
        mutations.push(run_record(&run)?);
        mutations.push(event_mutation(&event)?);
    }
    if let Some(registry) = reasoning_schema_update(store)? {
        mutations.insert(0, RuntimeMutation::Schema { registry });
    }

    let commit = RuntimeCommit {
        scope: ScopeId::new(REASONING_SCOPE)?,
        at,
        actor: actor.to_owned(),
        expected_cursor: observed_cursor,
        mutations,
    };
    store.commit_runtime(&commit)?;
    Ok(event)
}

pub fn reasoning_run<E: Engine>(
    store: &E,
    run_id: &str,
) -> Result<Option<ReasoningRun>, Box<dyn std::error::Error>> {
    load(store)?
        .0
        .runs
        .remove(run_id)
        .map(ReasoningRun::replay)
        .transpose()
        .map_err(Into::into)
}

pub fn active_reasoning_run<E: Engine>(
    store: &E,
) -> Result<Option<ReasoningRun>, Box<dyn std::error::Error>> {
    let mut ledger = load(store)?.0;
    ledger
        .active
        .take()
        .and_then(|id| ledger.runs.remove(&id))
        .map(ReasoningRun::replay)
        .transpose()
        .map_err(Into::into)
}

/// Every reasoning run in stable identifier order. Replay validates every
/// hash chain before any event is returned.
pub fn reasoning_runs<E: Engine>(
    store: &E,
) -> Result<Vec<ReasoningRun>, Box<dyn std::error::Error>> {
    load(store)?
        .0
        .runs
        .into_values()
        .map(ReasoningRun::replay)
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
}
