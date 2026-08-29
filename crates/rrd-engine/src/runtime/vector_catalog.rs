//! Authoritative publication and restart reconstruction for vector artifacts.
//!
//! The in-process `VectorRuntime` is only a serving view. This module makes a
//! strict runtime record and a verified immutable object reference the source
//! of truth, committed together through `RRD storage coordinator` and surrounded by a durable
//! control-plane trace.

use super::{active_reasoning_run, DurableTraceSpan, TraceIdentity};
use rrd_core::{
    DataTransaction, Millis, ProjectionId, ReadStamp, RuntimeChange, RuntimeCommit,
    RuntimeCommitOutcome, RuntimeMutation, RuntimeProperties, RuntimePropertySchema, RuntimeRecord,
    RuntimeRecordSchema, RuntimeRef, RuntimeSchemaRegistry, RuntimeType, RuntimeValue,
    RuntimeValueType, ScopeId, TraceDataClass, TraceDomain, TraceLink, TraceOutcome,
};
use rrd_store::{DataRuntimeAccess, Engine, Error as StoreError, ImmutableObjectStore};
use rrd_vector::{
    QuantizationArtifactCatalogue, QuantizationArtifactEntry, QuantizationArtifactState,
    QuantizationLifecycleAction, QuantizationLifecycleEvent, VectorArtifact,
    VectorArtifactCatalogEntry, VectorCandidate, VectorRuntime, QUANTIZATION_ARTIFACT_RECORD_TYPE,
    QUANTIZATION_LIFECYCLE_RECORD_TYPE, VECTOR_ARTIFACT_RECORD_TYPE,
};
use std::collections::{BTreeMap, BTreeSet};

const REPLAY_PAGE: usize = 4_096;
const PUBLICATION_RETRIES: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VectorArtifactPublication {
    pub catalog_revision: u64,
    pub entry: VectorArtifactCatalogEntry,
    pub commit: RuntimeCommitOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuantizationArtifactPublication {
    pub lifecycle_revision: u64,
    pub entry: QuantizationArtifactEntry,
    pub state: QuantizationArtifactState,
    pub commit: RuntimeCommitOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuantizationArtifactTransition {
    pub lifecycle_revision: u64,
    pub entry: QuantizationArtifactEntry,
    pub state: QuantizationArtifactState,
    pub commit: RuntimeCommitOutcome,
}

/// Publishes one immutable artifact without exposing it to serving until its
/// object and catalog binding have committed atomically.
pub fn publish_traced_vector_artifact<D>(
    data: &D,
    runtime: &mut VectorRuntime,
    expected_catalog_revision: u64,
    artifact: VectorArtifact,
    actor: &str,
    at: Millis,
) -> Result<VectorArtifactPublication, Box<dyn std::error::Error>>
where
    D: DataRuntimeAccess,
{
    if matches!(
        artifact.kind(),
        rrd_vector::VectorArtifactKind::ScalarQuantized
            | rrd_vector::VectorArtifactKind::ProductQuantized
            | rrd_vector::VectorArtifactKind::BinaryQuantized
            | rrd_vector::VectorArtifactKind::TurboQuant
    ) {
        return Err("quantized publication must use the quantization artifact lifecycle".into());
    }
    let descriptor = artifact.descriptor();
    descriptor.validate()?;
    let scope = descriptor.scope().clone();
    let stamp = descriptor.stamp().clone();

    // Validate the exact in-process transition on a clone. The serving view is
    // replaced only after the authoritative transaction succeeds.
    let mut next_runtime = runtime.clone();
    let next_revision = next_runtime.publish(expected_catalog_revision, artifact.clone())?;

    let revision_bytes = expected_catalog_revision.to_be_bytes();
    let generation_bytes = stamp.generation.to_be_bytes();
    let identity = TraceIdentity::derive(&[
        scope.as_str().as_bytes(),
        stamp.id.as_str().as_bytes(),
        &generation_bytes,
        &revision_bytes,
    ])?;
    let read = data.engine().runtime_read_stamp(&scope)?;
    let mut links = vec![TraceLink::Read { stamp: read }];
    if let Ok(Some(run)) = active_reasoning_run(data.engine()) {
        links.push(TraceLink::ReasoningRun {
            run_id: run.id().to_owned(),
        });
    }
    let span = DurableTraceSpan::start(
        data.engine(),
        scope.clone(),
        actor,
        identity,
        None,
        TraceDomain::Projection,
        "vector.projection.publish",
        at,
        TraceDataClass::Control,
        links,
        publication_attributes(&artifact, expected_catalog_revision),
    )?;

    let durable_entries = match vector_artifact_catalog_entries(data.engine(), &scope) {
        Ok(entries) => entries,
        Err(error) => {
            return finish_publication_error(data.engine(), span, "catalog_preflight", error)
        }
    };
    let durable_revision = durable_entries
        .last()
        .map_or(0, |entry| entry.catalog_revision);
    if durable_revision != expected_catalog_revision {
        return finish_publication_error(
            data.engine(),
            span,
            "catalog_preflight",
            format!(
                "vector catalog conflict: expected revision {expected_catalog_revision}, authoritative revision {durable_revision}"
            )
            .into(),
        );
    }

    let record_reference = VectorArtifactCatalogEntry::record_reference(&descriptor)?;
    let bytes = artifact.as_bytes();
    let object = match data.stage_object(
        format!("{}@{}:bytes", stamp.id, stamp.generation),
        Some(record_reference.clone()),
        artifact.kind().media_type(),
        bytes,
    ) {
        Ok(object) => object,
        Err(error) => {
            return finish_publication_error(data.engine(), span, "object_stage", error.into())
        }
    };
    let entry = VectorArtifactCatalogEntry::new(
        next_revision,
        artifact.kind(),
        descriptor,
        object.clone(),
        at,
    )?;
    let record = catalog_record(&entry)?;

    let commit = match commit_catalog_record(data, &scope, actor, at, record, object) {
        Ok(commit) => commit,
        Err(error) => {
            return finish_publication_error(data.engine(), span, "catalog_commit", error)
        }
    };
    *runtime = next_runtime;
    let finish_attributes = RuntimeProperties::from([
        (
            "catalog_revision".into(),
            RuntimeValue::Unsigned(next_revision),
        ),
        (
            "catalog_entry_digest".into(),
            RuntimeValue::Digest(entry.entry_digest.clone()),
        ),
        (
            "object_sha256".into(),
            RuntimeValue::Digest(entry.object.sha256.clone()),
        ),
        (
            "object_length".into(),
            RuntimeValue::Unsigned(entry.object.length),
        ),
        (
            "object_backend".into(),
            RuntimeValue::String(entry.object.receipt.backend.clone()),
        ),
        (
            "commit_id".into(),
            RuntimeValue::Digest(commit.commit_id.clone()),
        ),
    ]);
    span.finish(
        data.engine(),
        TraceOutcome::Ok,
        vec![TraceLink::Projection { stamp }],
        finish_attributes,
    )?;
    Ok(VectorArtifactPublication {
        catalog_revision: next_revision,
        entry,
        commit,
    })
}

/// Reconstructs the complete serving view from authoritative catalog records
/// and content-addressed bytes. Any revision gap or cross-record mismatch is a
/// hard error; silently dropping a damaged projection would change planning.
pub fn reopen_vector_runtime<D>(
    data: &D,
    scope: &ScopeId,
    canonical: impl IntoIterator<Item = VectorCandidate>,
) -> Result<VectorRuntime, Box<dyn std::error::Error>>
where
    D: DataRuntimeAccess,
{
    let entries = vector_artifact_catalog_entries(data.engine(), scope)?;
    let mut runtime = VectorRuntime::new(canonical)?;
    for entry in entries {
        let expected_revision = runtime.catalog().revision;
        let bytes = data.objects().get(&entry.object)?;
        let artifact = entry.decode_artifact(&bytes)?;
        runtime.publish(expected_revision, artifact)?;
    }
    runtime.suppress_legacy_turboquant();
    let quantization = quantization_artifact_catalogue(data.engine(), scope)?;
    for entry in quantization.active_entries() {
        let bytes = data.objects().get(&entry.object)?;
        let artifact = entry.decode_artifact(&bytes)?;
        runtime.restore_active(artifact)?;
    }
    Ok(runtime)
}

/// Publishes immutable quantized bytes in `ready` state without changing the
/// planner. Activation is a separate authenticated lifecycle operation.
pub fn build_traced_quantization_artifact<D>(
    data: &D,
    collection_id: ProjectionId,
    vector_name: ProjectionId,
    artifact: VectorArtifact,
    actor: &str,
    at: Millis,
) -> Result<QuantizationArtifactPublication, Box<dyn std::error::Error>>
where
    D: DataRuntimeAccess,
{
    if !matches!(
        artifact.kind(),
        rrd_vector::VectorArtifactKind::ScalarQuantized
            | rrd_vector::VectorArtifactKind::ProductQuantized
            | rrd_vector::VectorArtifactKind::BinaryQuantized
            | rrd_vector::VectorArtifactKind::TurboQuant
    ) {
        return Err("quantization lifecycle cannot build a non-quantized artifact".into());
    }
    let descriptor = artifact.descriptor();
    descriptor.validate()?;
    let scope = descriptor.scope().clone();
    let stamp = descriptor.stamp().clone();
    let generation_bytes = stamp.generation.to_be_bytes();
    let identity = TraceIdentity::derive(&[
        scope.as_str().as_bytes(),
        stamp.id.as_str().as_bytes(),
        &generation_bytes,
        b"build",
    ])?;
    let trace_read = data.engine().runtime_read_stamp(&scope)?;
    let mut links = vec![TraceLink::Read { stamp: trace_read }];
    if let Ok(Some(run)) = active_reasoning_run(data.engine()) {
        links.push(TraceLink::ReasoningRun {
            run_id: run.id().to_owned(),
        });
    }
    let span = DurableTraceSpan::start(
        data.engine(),
        scope.clone(),
        actor,
        identity,
        None,
        TraceDomain::Projection,
        "vector.quantization.build",
        at,
        TraceDataClass::Control,
        links,
        quantization_trace_attributes(&stamp, artifact.kind(), "build"),
    )?;
    let result = (|| {
        let (read, catalogue) = quantization_artifact_catalogue_at_read(data.engine(), &scope)?;
        let key = (stamp.id.clone(), stamp.generation);
        if catalogue.artifacts.contains_key(&key) {
            return Err("quantization artifact generation is already built".into());
        }
        let event = catalogue.next_event(
            stamp.id.clone(),
            stamp.generation,
            QuantizationLifecycleAction::Build,
            at,
        )?;
        let record_reference = QuantizationArtifactEntry::record_reference(&descriptor)?;
        let object = data.stage_object(
            format!("{}@{}:quantized-bytes", stamp.id, stamp.generation),
            Some(record_reference),
            artifact.kind().media_type(),
            artifact.as_bytes(),
        )?;
        let entry = QuantizationArtifactEntry::new(
            collection_id,
            vector_name,
            artifact.kind(),
            descriptor,
            object.clone(),
            at,
        )?;
        let artifact_record = quantization_artifact_record(&entry)?;
        let event_record = quantization_lifecycle_record(&event)?;
        let commit = commit_quantization_records(
            data,
            read,
            &scope,
            actor,
            at,
            vec![artifact_record, event_record],
            Some(object),
        )?;
        Ok::<_, Box<dyn std::error::Error>>(QuantizationArtifactPublication {
            lifecycle_revision: event.revision,
            entry,
            state: QuantizationArtifactState::Ready,
            commit,
        })
    })();
    match result {
        Ok(publication) => {
            span.finish(
                data.engine(),
                TraceOutcome::Ok,
                vec![TraceLink::Projection { stamp }],
                quantization_finish_attributes(
                    publication.lifecycle_revision,
                    publication.state,
                    &publication.entry,
                    &publication.commit,
                ),
            )?;
            Ok(publication)
        }
        Err(error) => finish_publication_error(data.engine(), span, "quantization_build", error),
    }
}

/// Activates or retires one built generation through an append-only event.
/// Activation verifies the immutable object before it can become planner
/// visible, so corruption fails closed at the control-plane boundary.
pub fn transition_traced_quantization_artifact<D>(
    data: &D,
    scope: &ScopeId,
    projection_id: &ProjectionId,
    generation: u64,
    action: QuantizationLifecycleAction,
    actor: &str,
    at: Millis,
) -> Result<QuantizationArtifactTransition, Box<dyn std::error::Error>>
where
    D: DataRuntimeAccess,
{
    if action == QuantizationLifecycleAction::Build {
        return Err("build requires immutable artifact bytes".into());
    }
    let action_name = match action {
        QuantizationLifecycleAction::Activate => "activate",
        QuantizationLifecycleAction::Retire => "retire",
        QuantizationLifecycleAction::Build => unreachable!("build was rejected above"),
    };
    let generation_bytes = generation.to_be_bytes();
    let identity = TraceIdentity::derive(&[
        scope.as_str().as_bytes(),
        projection_id.as_str().as_bytes(),
        &generation_bytes,
        action_name.as_bytes(),
    ])?;
    let trace_read = data.engine().runtime_read_stamp(scope)?;
    let mut links = vec![TraceLink::Read { stamp: trace_read }];
    if let Ok(Some(run)) = active_reasoning_run(data.engine()) {
        links.push(TraceLink::ReasoningRun {
            run_id: run.id().to_owned(),
        });
    }
    let span = DurableTraceSpan::start(
        data.engine(),
        scope.clone(),
        actor,
        identity,
        None,
        TraceDomain::Projection,
        format!("vector.quantization.{action_name}"),
        at,
        TraceDataClass::Control,
        links,
        RuntimeProperties::from([
            (
                "projection_id".into(),
                RuntimeValue::String(projection_id.to_string()),
            ),
            ("generation".into(), RuntimeValue::Unsigned(generation)),
            (
                "lifecycle_action".into(),
                RuntimeValue::String(action_name.into()),
            ),
        ]),
    )?;
    let result = (|| {
        let (read, catalogue) = quantization_artifact_catalogue_at_read(data.engine(), scope)?;
        let key = (projection_id.clone(), generation);
        let entry = catalogue
            .artifacts
            .get(&key)
            .ok_or("quantization lifecycle target is absent")?
            .entry
            .clone();
        if action == QuantizationLifecycleAction::Activate {
            let bytes = data.objects().get(&entry.object)?;
            entry.decode_artifact(&bytes)?;
        }
        let event = catalogue.next_event(projection_id.clone(), generation, action, at)?;
        let mut next = catalogue.clone();
        next.apply(&event)?;
        let state = next
            .artifacts
            .get(&key)
            .ok_or("quantization transition target disappeared")?
            .state;
        let commit = commit_quantization_records(
            data,
            read,
            scope,
            actor,
            at,
            vec![quantization_lifecycle_record(&event)?],
            None,
        )?;
        Ok::<_, Box<dyn std::error::Error>>(QuantizationArtifactTransition {
            lifecycle_revision: event.revision,
            entry,
            state,
            commit,
        })
    })();
    match result {
        Ok(transition) => {
            span.finish(
                data.engine(),
                TraceOutcome::Ok,
                vec![TraceLink::Projection {
                    stamp: transition.entry.descriptor.stamp().clone(),
                }],
                quantization_finish_attributes(
                    transition.lifecycle_revision,
                    transition.state,
                    &transition.entry,
                    &transition.commit,
                ),
            )?;
            Ok(transition)
        }
        Err(error) => {
            finish_publication_error(data.engine(), span, "quantization_transition", error)
        }
    }
}

pub fn quantization_artifact_catalogue<E>(
    engine: &E,
    scope: &ScopeId,
) -> Result<QuantizationArtifactCatalogue, Box<dyn std::error::Error>>
where
    E: Engine,
{
    quantization_artifact_catalogue_at_read(engine, scope).map(|(_, catalogue)| catalogue)
}

fn quantization_artifact_catalogue_at_read<E>(
    engine: &E,
    scope: &ScopeId,
) -> Result<(ReadStamp, QuantizationArtifactCatalogue), Box<dyn std::error::Error>>
where
    E: Engine,
{
    let read = engine.runtime_read_stamp(scope)?;
    let mut cursor = 0;
    let mut changes = Vec::new();
    loop {
        let page = engine.runtime_read_changes(&read, cursor, REPLAY_PAGE)?;
        let has_more = page.has_more();
        let through_cursor = page.through_cursor;
        changes.extend(page.changes);
        if !has_more {
            cursor = through_cursor;
            break;
        }
        if through_cursor <= cursor {
            return Err("quantization catalogue replay did not advance its cursor".into());
        }
        cursor = through_cursor;
    }
    if cursor != read.commit_cursor && read.commit_cursor != 0 {
        return Err("quantization catalogue replay did not reach its stamped cursor".into());
    }
    let catalogue = quantization_artifact_catalogue_from_changes(&changes, scope)?;
    Ok((read, catalogue))
}

pub(crate) fn quantization_artifact_catalogue_from_changes(
    changes: &[RuntimeChange],
    scope: &ScopeId,
) -> Result<QuantizationArtifactCatalogue, Box<dyn std::error::Error>> {
    let mut artifact_records = BTreeMap::<RuntimeRef, (QuantizationArtifactEntry, String)>::new();
    let mut event_records = BTreeMap::<RuntimeRef, (QuantizationLifecycleEvent, String)>::new();
    let mut objects = BTreeMap::new();
    for change in changes.iter().filter(|change| &change.scope == scope) {
        if !change.verify_digest() {
            return Err("quantization catalogue encountered a corrupt change digest".into());
        }
        match &change.mutation {
            RuntimeMutation::Record { record }
                if record.reference.kind.as_str() == QUANTIZATION_ARTIFACT_RECORD_TYPE =>
            {
                if artifact_records
                    .insert(
                        record.reference.clone(),
                        (
                            quantization_entry_from_record(record)?,
                            change.commit_id.clone(),
                        ),
                    )
                    .is_some()
                {
                    return Err("duplicate quantization artifact record identity".into());
                }
            }
            RuntimeMutation::Record { record }
                if record.reference.kind.as_str() == QUANTIZATION_LIFECYCLE_RECORD_TYPE =>
            {
                if event_records
                    .insert(
                        record.reference.clone(),
                        (
                            quantization_event_from_record(record)?,
                            change.commit_id.clone(),
                        ),
                    )
                    .is_some()
                {
                    return Err("duplicate quantization lifecycle record identity".into());
                }
            }
            RuntimeMutation::Object { object }
                if object.subject.as_ref().is_some_and(|subject| {
                    subject.kind.as_str() == QUANTIZATION_ARTIFACT_RECORD_TYPE
                }) =>
            {
                if objects
                    .insert(
                        object.reference.clone(),
                        (object.clone(), change.commit_id.clone()),
                    )
                    .is_some()
                {
                    return Err("duplicate quantization object mutation identity".into());
                }
            }
            _ => {}
        }
    }
    let mut entries = Vec::with_capacity(artifact_records.len());
    let mut build_commits = BTreeMap::new();
    for (reference, (entry, record_commit)) in artifact_records {
        if entry.scope() != scope
            || QuantizationArtifactEntry::record_reference(&entry.descriptor)? != reference
        {
            return Err("quantization artifact record identity or scope differs".into());
        }
        let (object, object_commit) = objects
            .get(&entry.object.reference)
            .ok_or("quantization artifact references an absent object mutation")?;
        if object != &entry.object || object_commit != &record_commit {
            return Err("quantization artifact and object were not atomically built".into());
        }
        build_commits.insert(
            (
                entry.descriptor.stamp().id.clone(),
                entry.descriptor.stamp().generation,
            ),
            record_commit,
        );
        entries.push(entry);
    }
    let mut events = event_records.into_values().collect::<Vec<_>>();
    events.sort_by_key(|(event, _)| event.revision);
    let mut event_builds = BTreeSet::new();
    for (event, commit) in &events {
        if event.action == QuantizationLifecycleAction::Build {
            let key = (event.projection_id.clone(), event.generation);
            if !event_builds.insert(key.clone()) || build_commits.get(&key) != Some(commit) {
                return Err(
                    "quantization build event was not uniquely atomic with its artifact".into(),
                );
            }
        }
    }
    if event_builds != build_commits.keys().cloned().collect() {
        return Err("quantization artifact is missing its atomic build event".into());
    }
    QuantizationArtifactCatalogue::reconstruct(
        entries,
        events.into_iter().map(|(event, _)| event).collect(),
    )
    .map_err(Into::into)
}

fn commit_quantization_records<D>(
    data: &D,
    read: ReadStamp,
    scope: &ScopeId,
    actor: &str,
    at: Millis,
    records: Vec<RuntimeRecord>,
    object: Option<rrd_core::ObjectReference>,
) -> Result<RuntimeCommitOutcome, Box<dyn std::error::Error>>
where
    D: DataRuntimeAccess,
{
    let current = data.engine().runtime_schema(scope)?;
    if current.as_ref().map(|schema| schema.revision) != read.schema_revision {
        return Err("quantization lifecycle schema changed during publication preflight".into());
    }
    let expected_cursor = read.commit_cursor;
    let mut mutations = Vec::new();
    if let Some(registry) = quantization_schema_update(current)? {
        mutations.push(RuntimeMutation::Schema { registry });
    }
    mutations.extend(
        records
            .into_iter()
            .map(|record| RuntimeMutation::Record { record }),
    );
    if let Some(object) = object {
        mutations.push(RuntimeMutation::Object { object });
    }
    let transaction = DataTransaction::new(
        read,
        RuntimeCommit {
            scope: scope.clone(),
            at,
            actor: actor.to_owned(),
            expected_cursor,
            mutations,
        },
    )?;
    data.commit(&transaction).map_err(Into::into)
}

fn quantization_schema_update(
    current: Option<RuntimeSchemaRegistry>,
) -> Result<Option<RuntimeSchemaRegistry>, Box<dyn std::error::Error>> {
    let artifact_type = RuntimeType::new(QUANTIZATION_ARTIFACT_RECORD_TYPE)?;
    let lifecycle_type = RuntimeType::new(QUANTIZATION_LIFECYCLE_RECORD_TYPE)?;
    let artifact_schema = quantization_artifact_record_schema();
    let lifecycle_schema = quantization_lifecycle_record_schema();
    if current.as_ref().is_some_and(|registry| {
        registry.records.get(&artifact_type) == Some(&artifact_schema)
            && registry.records.get(&lifecycle_type) == Some(&lifecycle_schema)
    }) {
        return Ok(None);
    }
    let mut registry = current
        .clone()
        .unwrap_or_else(|| RuntimeSchemaRegistry::empty(1, "install quantization lifecycle"));
    registry.records.insert(artifact_type, artifact_schema);
    registry.records.insert(lifecycle_type, lifecycle_schema);
    if let Some(current) = current {
        registry.revision = current
            .revision
            .checked_add(1)
            .ok_or("runtime schema revision overflow while installing quantization lifecycle")?;
        registry.migration = "install quantization artifact lifecycle".into();
    }
    Ok(Some(registry))
}

fn quantization_artifact_record_schema() -> RuntimeRecordSchema {
    let required = |value_type| RuntimePropertySchema::required(value_type);
    RuntimeRecordSchema {
        properties: BTreeMap::from([
            (
                "contract_version".into(),
                required(RuntimeValueType::Unsigned),
            ),
            ("collection_id".into(), required(RuntimeValueType::String)),
            ("vector_name".into(), required(RuntimeValueType::String)),
            ("projection_id".into(), required(RuntimeValueType::String)),
            ("generation".into(), required(RuntimeValueType::Unsigned)),
            ("artifact_kind".into(), required(RuntimeValueType::String)),
            ("config_digest".into(), required(RuntimeValueType::Digest)),
            ("artifact_digest".into(), required(RuntimeValueType::Digest)),
            ("object_id".into(), required(RuntimeValueType::String)),
            ("object_sha256".into(), required(RuntimeValueType::Digest)),
            ("object_length".into(), required(RuntimeValueType::Unsigned)),
            ("built_at".into(), required(RuntimeValueType::Unsigned)),
            ("entry_digest".into(), required(RuntimeValueType::Digest)),
            ("entry_json".into(), required(RuntimeValueType::String)),
        ]),
        allow_additional_properties: false,
        unique_properties: BTreeSet::from(["entry_digest".into()]),
    }
}

fn quantization_lifecycle_record_schema() -> RuntimeRecordSchema {
    let required = |value_type| RuntimePropertySchema::required(value_type);
    RuntimeRecordSchema {
        properties: BTreeMap::from([
            (
                "contract_version".into(),
                required(RuntimeValueType::Unsigned),
            ),
            ("revision".into(), required(RuntimeValueType::Unsigned)),
            ("projection_id".into(), required(RuntimeValueType::String)),
            ("generation".into(), required(RuntimeValueType::Unsigned)),
            ("action".into(), required(RuntimeValueType::String)),
            ("at".into(), required(RuntimeValueType::Unsigned)),
            ("event_digest".into(), required(RuntimeValueType::Digest)),
            ("event_json".into(), required(RuntimeValueType::String)),
        ]),
        allow_additional_properties: false,
        unique_properties: BTreeSet::from(["revision".into(), "event_digest".into()]),
    }
}

fn quantization_artifact_record(
    entry: &QuantizationArtifactEntry,
) -> Result<RuntimeRecord, Box<dyn std::error::Error>> {
    entry.validate()?;
    let stamp = entry.descriptor.stamp();
    Ok(RuntimeRecord {
        reference: QuantizationArtifactEntry::record_reference(&entry.descriptor)?,
        valid_from: entry.built_at,
        valid_to: None,
        properties: RuntimeProperties::from([
            (
                "contract_version".into(),
                RuntimeValue::Unsigned(entry.contract_version.into()),
            ),
            (
                "collection_id".into(),
                RuntimeValue::String(entry.collection_id.to_string()),
            ),
            (
                "vector_name".into(),
                RuntimeValue::String(entry.vector_name.to_string()),
            ),
            (
                "projection_id".into(),
                RuntimeValue::String(stamp.id.to_string()),
            ),
            (
                "generation".into(),
                RuntimeValue::Unsigned(stamp.generation),
            ),
            (
                "artifact_kind".into(),
                RuntimeValue::String(entry.kind.as_str().into()),
            ),
            (
                "config_digest".into(),
                RuntimeValue::Digest(stamp.config_digest.clone()),
            ),
            (
                "artifact_digest".into(),
                RuntimeValue::Digest(stamp.artifact_digest.clone()),
            ),
            (
                "object_id".into(),
                RuntimeValue::String(entry.object.reference.id.to_string()),
            ),
            (
                "object_sha256".into(),
                RuntimeValue::Digest(entry.object.sha256.clone()),
            ),
            (
                "object_length".into(),
                RuntimeValue::Unsigned(entry.object.length),
            ),
            ("built_at".into(), RuntimeValue::Unsigned(entry.built_at)),
            (
                "entry_digest".into(),
                RuntimeValue::Digest(entry.entry_digest.clone()),
            ),
            (
                "entry_json".into(),
                RuntimeValue::String(serde_json::to_string(entry)?),
            ),
        ]),
    })
}

fn quantization_lifecycle_record(
    event: &QuantizationLifecycleEvent,
) -> Result<RuntimeRecord, Box<dyn std::error::Error>> {
    event.validate()?;
    let action = match event.action {
        QuantizationLifecycleAction::Build => "build",
        QuantizationLifecycleAction::Activate => "activate",
        QuantizationLifecycleAction::Retire => "retire",
    };
    Ok(RuntimeRecord {
        reference: QuantizationLifecycleEvent::record_reference(event.revision)?,
        valid_from: event.at,
        valid_to: None,
        properties: RuntimeProperties::from([
            (
                "contract_version".into(),
                RuntimeValue::Unsigned(event.contract_version.into()),
            ),
            ("revision".into(), RuntimeValue::Unsigned(event.revision)),
            (
                "projection_id".into(),
                RuntimeValue::String(event.projection_id.to_string()),
            ),
            (
                "generation".into(),
                RuntimeValue::Unsigned(event.generation),
            ),
            ("action".into(), RuntimeValue::String(action.into())),
            ("at".into(), RuntimeValue::Unsigned(event.at)),
            (
                "event_digest".into(),
                RuntimeValue::Digest(event.event_digest.clone()),
            ),
            (
                "event_json".into(),
                RuntimeValue::String(serde_json::to_string(event)?),
            ),
        ]),
    })
}

fn quantization_entry_from_record(
    record: &RuntimeRecord,
) -> Result<QuantizationArtifactEntry, Box<dyn std::error::Error>> {
    if record.valid_to.is_some() {
        return Err("quantization artifact records cannot have a validity end".into());
    }
    let entry: QuantizationArtifactEntry =
        serde_json::from_str(string_property(&record.properties, "entry_json")?)?;
    entry.validate()?;
    if quantization_artifact_record(&entry)? != *record {
        return Err("quantization artifact record differs from canonical entry JSON".into());
    }
    Ok(entry)
}

fn quantization_event_from_record(
    record: &RuntimeRecord,
) -> Result<QuantizationLifecycleEvent, Box<dyn std::error::Error>> {
    if record.valid_to.is_some() {
        return Err("quantization lifecycle records cannot have a validity end".into());
    }
    let event: QuantizationLifecycleEvent =
        serde_json::from_str(string_property(&record.properties, "event_json")?)?;
    event.validate()?;
    if quantization_lifecycle_record(&event)? != *record {
        return Err("quantization lifecycle record differs from canonical event JSON".into());
    }
    Ok(event)
}

/// Returns the typed, atomically bound catalog in revision order without
/// reading potentially large artifact bodies. This is the safe inspection
/// surface for Connectome and publication preflight.
pub fn vector_artifact_catalog_entries<E>(
    engine: &E,
    scope: &ScopeId,
) -> Result<Vec<VectorArtifactCatalogEntry>, Box<dyn std::error::Error>>
where
    E: Engine,
{
    let mut cursor = 0;
    let mut changes = Vec::new();
    loop {
        let page = engine.runtime_changes_since(cursor, REPLAY_PAGE, Some(scope))?;
        let has_more = page.has_more();
        let through_cursor = page.through_cursor;
        changes.extend(page.changes);
        if !has_more {
            break;
        }
        if through_cursor <= cursor {
            return Err("vector catalog replay did not advance its cursor".into());
        }
        cursor = through_cursor;
    }

    vector_artifact_catalog_entries_from_changes(&changes, scope)
}

/// Reconstructs the exact vector-artifact catalogue from a caller-owned,
/// already bounded and stamped runtime change set.
pub(crate) fn vector_artifact_catalog_entries_from_changes(
    changes: &[RuntimeChange],
    scope: &ScopeId,
) -> Result<Vec<VectorArtifactCatalogEntry>, Box<dyn std::error::Error>> {
    let mut records = BTreeMap::<RuntimeRef, (RuntimeRecord, String)>::new();
    let mut objects = BTreeMap::new();
    for change in changes.iter().filter(|change| &change.scope == scope) {
        if !change.verify_digest() {
            return Err("vector catalog replay encountered a corrupt change digest".into());
        }
        match &change.mutation {
            RuntimeMutation::Record { record }
                if record.reference.kind.as_str() == VECTOR_ARTIFACT_RECORD_TYPE =>
            {
                records.insert(
                    record.reference.clone(),
                    (record.clone(), change.commit_id.clone()),
                );
            }
            RuntimeMutation::Object { object }
                if object.subject.as_ref().is_some_and(|subject| {
                    subject.kind.as_str() == VECTOR_ARTIFACT_RECORD_TYPE
                }) =>
            {
                objects.insert(
                    object.reference.clone(),
                    (object.clone(), change.commit_id.clone()),
                );
            }
            _ => {}
        }
    }

    let mut entries = Vec::with_capacity(records.len());
    for (reference, (record, record_commit)) in records {
        let entry = entry_from_record(&record)?;
        if entry.scope() != scope
            || VectorArtifactCatalogEntry::record_reference(&entry.descriptor)? != reference
        {
            return Err("vector catalog record identity or scope differs from its entry".into());
        }
        let (object, object_commit) = objects
            .get(&entry.object.reference)
            .ok_or("vector catalog entry references an absent object mutation")?;
        if object != &entry.object || object_commit != &record_commit {
            return Err(
                "vector catalog record and object were not atomically published together".into(),
            );
        }
        entries.push(entry);
    }
    entries.sort_by_key(|entry| entry.catalog_revision);

    for (ordinal, entry) in entries.iter().enumerate() {
        let expected_revision = ordinal as u64;
        if entry.catalog_revision != expected_revision + 1 {
            return Err(format!(
                "vector catalog revision gap: expected {}, found {}",
                expected_revision + 1,
                entry.catalog_revision
            )
            .into());
        }
    }
    Ok(entries)
}

fn commit_catalog_record<D>(
    data: &D,
    scope: &ScopeId,
    actor: &str,
    at: Millis,
    record: RuntimeRecord,
    object: rrd_core::ObjectReference,
) -> Result<RuntimeCommitOutcome, Box<dyn std::error::Error>>
where
    D: DataRuntimeAccess,
{
    for _ in 0..PUBLICATION_RETRIES {
        let read = data.engine().runtime_read_stamp(scope)?;
        let current = data.engine().runtime_schema(scope)?;
        if current.as_ref().map(|schema| schema.revision) != read.schema_revision {
            continue;
        }
        let mut mutations = Vec::new();
        if let Some(registry) = catalog_schema_update(current)? {
            mutations.push(RuntimeMutation::Schema { registry });
        }
        mutations.push(RuntimeMutation::Record {
            record: record.clone(),
        });
        mutations.push(RuntimeMutation::Object {
            object: object.clone(),
        });
        let expected_cursor = read.commit_cursor;
        let transaction = DataTransaction::new(
            read,
            RuntimeCommit {
                scope: scope.clone(),
                at,
                actor: actor.to_owned(),
                expected_cursor,
                mutations,
            },
        )?;
        match data.commit(&transaction) {
            Ok(outcome) => return Ok(outcome),
            Err(StoreError::RuntimeConflict { .. }) => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Err(format!(
        "vector catalog publication could not acquire cursor CAS after {PUBLICATION_RETRIES} conflicts"
    )
    .into())
}

fn catalog_schema_update(
    current: Option<RuntimeSchemaRegistry>,
) -> Result<Option<RuntimeSchemaRegistry>, Box<dyn std::error::Error>> {
    let record_type = RuntimeType::new(VECTOR_ARTIFACT_RECORD_TYPE)?;
    let schema = catalog_record_schema();
    if current
        .as_ref()
        .and_then(|registry| registry.records.get(&record_type))
        == Some(&schema)
    {
        return Ok(None);
    }
    let mut registry = current
        .clone()
        .unwrap_or_else(|| RuntimeSchemaRegistry::empty(1, "install vector artifact catalog"));
    registry.records.insert(record_type, schema);
    if let Some(current) = current {
        registry.revision = current
            .revision
            .checked_add(1)
            .ok_or("runtime schema revision overflow while installing vector catalog")?;
        registry.migration = "install authoritative vector artifact catalog".into();
    }
    Ok(Some(registry))
}

fn catalog_record_schema() -> RuntimeRecordSchema {
    let required = |value_type| RuntimePropertySchema::required(value_type);
    RuntimeRecordSchema {
        properties: BTreeMap::from([
            (
                "contract_version".into(),
                required(RuntimeValueType::Unsigned),
            ),
            (
                "catalog_revision".into(),
                required(RuntimeValueType::Unsigned),
            ),
            ("projection_id".into(), required(RuntimeValueType::String)),
            ("generation".into(), required(RuntimeValueType::Unsigned)),
            ("source_cursor".into(), required(RuntimeValueType::Unsigned)),
            ("config_digest".into(), required(RuntimeValueType::Digest)),
            ("artifact_digest".into(), required(RuntimeValueType::Digest)),
            ("state".into(), required(RuntimeValueType::String)),
            ("artifact_kind".into(), required(RuntimeValueType::String)),
            ("object_id".into(), required(RuntimeValueType::String)),
            ("object_sha256".into(), required(RuntimeValueType::Digest)),
            ("object_length".into(), required(RuntimeValueType::Unsigned)),
            ("entry_digest".into(), required(RuntimeValueType::Digest)),
            ("entry_json".into(), required(RuntimeValueType::String)),
            ("published_at".into(), required(RuntimeValueType::Unsigned)),
        ]),
        allow_additional_properties: false,
        unique_properties: BTreeSet::from(["catalog_revision".into(), "entry_digest".into()]),
    }
}

fn catalog_record(
    entry: &VectorArtifactCatalogEntry,
) -> Result<RuntimeRecord, Box<dyn std::error::Error>> {
    entry.validate()?;
    let stamp = entry.descriptor.stamp();
    Ok(RuntimeRecord {
        reference: VectorArtifactCatalogEntry::record_reference(&entry.descriptor)?,
        valid_from: entry.published_at,
        valid_to: None,
        properties: RuntimeProperties::from([
            (
                "contract_version".into(),
                RuntimeValue::Unsigned(entry.contract_version.into()),
            ),
            (
                "catalog_revision".into(),
                RuntimeValue::Unsigned(entry.catalog_revision),
            ),
            (
                "projection_id".into(),
                RuntimeValue::String(stamp.id.to_string()),
            ),
            (
                "generation".into(),
                RuntimeValue::Unsigned(stamp.generation),
            ),
            (
                "source_cursor".into(),
                RuntimeValue::Unsigned(stamp.source_cursor),
            ),
            (
                "config_digest".into(),
                RuntimeValue::Digest(stamp.config_digest.clone()),
            ),
            (
                "artifact_digest".into(),
                RuntimeValue::Digest(stamp.artifact_digest.clone()),
            ),
            ("state".into(), RuntimeValue::String("ready".into())),
            (
                "artifact_kind".into(),
                RuntimeValue::String(entry.kind.as_str().into()),
            ),
            (
                "object_id".into(),
                RuntimeValue::String(entry.object.reference.id.to_string()),
            ),
            (
                "object_sha256".into(),
                RuntimeValue::Digest(entry.object.sha256.clone()),
            ),
            (
                "object_length".into(),
                RuntimeValue::Unsigned(entry.object.length),
            ),
            (
                "entry_digest".into(),
                RuntimeValue::Digest(entry.entry_digest.clone()),
            ),
            (
                "entry_json".into(),
                RuntimeValue::String(serde_json::to_string(entry)?),
            ),
            (
                "published_at".into(),
                RuntimeValue::Unsigned(entry.published_at),
            ),
        ]),
    })
}

fn entry_from_record(
    record: &RuntimeRecord,
) -> Result<VectorArtifactCatalogEntry, Box<dyn std::error::Error>> {
    if record.valid_to.is_some() {
        return Err("vector artifact catalog records cannot be retired by validity window".into());
    }
    let json = string_property(&record.properties, "entry_json")?;
    let entry: VectorArtifactCatalogEntry = serde_json::from_str(json)?;
    entry.validate()?;
    let expected = catalog_record(&entry)?;
    if expected.reference != record.reference
        || expected.valid_from != record.valid_from
        || expected.properties != record.properties
    {
        return Err(
            "vector artifact catalog record fields differ from canonical entry JSON".into(),
        );
    }
    Ok(entry)
}

fn string_property<'a>(
    properties: &'a RuntimeProperties,
    name: &str,
) -> Result<&'a str, Box<dyn std::error::Error>> {
    match properties.get(name) {
        Some(RuntimeValue::String(value)) => Ok(value),
        _ => Err(
            format!("vector artifact record property {name:?} is missing or not a string").into(),
        ),
    }
}

fn publication_attributes(artifact: &VectorArtifact, expected_revision: u64) -> RuntimeProperties {
    let descriptor = artifact.descriptor();
    let stamp = descriptor.stamp();
    RuntimeProperties::from([
        (
            "projection_id".into(),
            RuntimeValue::String(stamp.id.to_string()),
        ),
        (
            "projection_kind".into(),
            RuntimeValue::String(artifact.kind().as_str().into()),
        ),
        (
            "generation".into(),
            RuntimeValue::Unsigned(stamp.generation),
        ),
        (
            "source_cursor".into(),
            RuntimeValue::Unsigned(stamp.source_cursor),
        ),
        (
            "config_digest".into(),
            RuntimeValue::Digest(stamp.config_digest.clone()),
        ),
        (
            "artifact_digest".into(),
            RuntimeValue::Digest(stamp.artifact_digest.clone()),
        ),
        (
            "expected_catalog_revision".into(),
            RuntimeValue::Unsigned(expected_revision),
        ),
    ])
}

fn quantization_trace_attributes(
    stamp: &rrd_core::ProjectionStamp,
    kind: rrd_vector::VectorArtifactKind,
    action: &str,
) -> RuntimeProperties {
    RuntimeProperties::from([
        (
            "projection_id".into(),
            RuntimeValue::String(stamp.id.to_string()),
        ),
        (
            "generation".into(),
            RuntimeValue::Unsigned(stamp.generation),
        ),
        (
            "projection_kind".into(),
            RuntimeValue::String(kind.as_str().into()),
        ),
        (
            "lifecycle_action".into(),
            RuntimeValue::String(action.into()),
        ),
        (
            "config_digest".into(),
            RuntimeValue::Digest(stamp.config_digest.clone()),
        ),
        (
            "artifact_digest".into(),
            RuntimeValue::Digest(stamp.artifact_digest.clone()),
        ),
    ])
}

fn quantization_finish_attributes(
    lifecycle_revision: u64,
    state: QuantizationArtifactState,
    entry: &QuantizationArtifactEntry,
    commit: &RuntimeCommitOutcome,
) -> RuntimeProperties {
    let state = match state {
        QuantizationArtifactState::Ready => "ready",
        QuantizationArtifactState::Active => "active",
        QuantizationArtifactState::Retired => "retired",
    };
    RuntimeProperties::from([
        (
            "lifecycle_revision".into(),
            RuntimeValue::Unsigned(lifecycle_revision),
        ),
        ("lifecycle_state".into(), RuntimeValue::String(state.into())),
        (
            "catalog_entry_digest".into(),
            RuntimeValue::Digest(entry.entry_digest.clone()),
        ),
        (
            "object_sha256".into(),
            RuntimeValue::Digest(entry.object.sha256.clone()),
        ),
        (
            "object_length".into(),
            RuntimeValue::Unsigned(entry.object.length),
        ),
        (
            "commit_id".into(),
            RuntimeValue::Digest(commit.commit_id.clone()),
        ),
    ])
}

fn finish_publication_error<E: Engine, T>(
    store: &E,
    span: DurableTraceSpan,
    stage: &str,
    error: Box<dyn std::error::Error>,
) -> Result<T, Box<dyn std::error::Error>> {
    let rendered = error.to_string();
    let attributes = RuntimeProperties::from([
        ("failed_stage".into(), RuntimeValue::String(stage.into())),
        (
            "error_digest".into(),
            RuntimeValue::Digest(rrd_core::digest::sha256_hex(rendered.as_bytes())),
        ),
    ]);
    if let Err(trace_error) = span.finish(store, TraceOutcome::Error, Vec::new(), attributes) {
        return Err(
            format!("{rendered}; authoritative trace finish also failed: {trace_error}").into(),
        );
    }
    Err(error)
}
