//! Authoritative publication and restart reconstruction for vector artifacts.
//!
//! The in-process `VectorRuntime` is only a serving view. This module makes a
//! strict runtime record and a verified immutable object reference the source
//! of truth, committed together through `vyrmDS` and surrounded by a durable
//! control-plane trace.

use crate::{active_reasoning_run, DurableTraceSpan, TraceIdentity};
use std::collections::{BTreeMap, BTreeSet};
use vyrm_core::{
    DataTransaction, Millis, RuntimeCommit, RuntimeCommitOutcome, RuntimeMutation,
    RuntimeProperties, RuntimePropertySchema, RuntimeRecord, RuntimeRecordSchema, RuntimeRef,
    RuntimeSchemaRegistry, RuntimeType, RuntimeValue, RuntimeValueType, ScopeId, TraceDataClass,
    TraceDomain, TraceLink, TraceOutcome,
};
use vyrm_store::{DataRuntime, Engine, Error as StoreError, ImmutableObjectStore};
use vyrm_vector::{
    VectorArtifact, VectorArtifactCatalogEntry, VectorCandidate, VectorRuntime,
    VECTOR_ARTIFACT_RECORD_TYPE,
};

const REPLAY_PAGE: usize = 4_096;
const PUBLICATION_RETRIES: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VectorArtifactPublication {
    pub catalog_revision: u64,
    pub entry: VectorArtifactCatalogEntry,
    pub commit: RuntimeCommitOutcome,
}

/// Publishes one immutable artifact without exposing it to serving until its
/// object and catalog binding have committed atomically.
pub fn publish_traced_vector_artifact<E, O>(
    data: &DataRuntime<E, O>,
    runtime: &mut VectorRuntime,
    expected_catalog_revision: u64,
    artifact: VectorArtifact,
    actor: &str,
    at: Millis,
) -> Result<VectorArtifactPublication, Box<dyn std::error::Error>>
where
    E: Engine,
    O: ImmutableObjectStore,
{
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
pub fn reopen_vector_runtime<E, O>(
    data: &DataRuntime<E, O>,
    scope: &ScopeId,
    canonical: impl IntoIterator<Item = VectorCandidate>,
) -> Result<VectorRuntime, Box<dyn std::error::Error>>
where
    E: Engine,
    O: ImmutableObjectStore,
{
    let entries = vector_artifact_catalog_entries(data.engine(), scope)?;
    let mut runtime = VectorRuntime::new(canonical)?;
    for entry in entries {
        let expected_revision = runtime.catalog().revision;
        let bytes = data.objects().get(&entry.object)?;
        let artifact = entry.decode_artifact(&bytes)?;
        runtime.publish(expected_revision, artifact)?;
    }
    Ok(runtime)
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
    let mut records = BTreeMap::<RuntimeRef, (RuntimeRecord, String)>::new();
    let mut objects = BTreeMap::new();
    let mut cursor = 0;
    loop {
        let page = engine.runtime_changes_since(cursor, REPLAY_PAGE, Some(scope))?;
        let has_more = page.has_more();
        let through_cursor = page.through_cursor;
        for change in page.changes {
            if !change.verify_digest() {
                return Err("vector catalog replay encountered a corrupt change digest".into());
            }
            match change.mutation {
                RuntimeMutation::Record { record }
                    if record.reference.kind.as_str() == VECTOR_ARTIFACT_RECORD_TYPE =>
                {
                    records.insert(record.reference.clone(), (record, change.commit_id));
                }
                RuntimeMutation::Object { object } => {
                    if let Some(subject) = &object.subject {
                        if subject.kind.as_str() == VECTOR_ARTIFACT_RECORD_TYPE {
                            objects.insert(object.reference.clone(), (object, change.commit_id));
                        }
                    }
                }
                _ => {}
            }
        }
        if !has_more {
            break;
        }
        if through_cursor <= cursor {
            return Err("vector catalog replay did not advance its cursor".into());
        }
        cursor = through_cursor;
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

fn commit_catalog_record<E, O>(
    data: &DataRuntime<E, O>,
    scope: &ScopeId,
    actor: &str,
    at: Millis,
    record: RuntimeRecord,
    object: vyrm_core::ObjectReference,
) -> Result<RuntimeCommitOutcome, Box<dyn std::error::Error>>
where
    E: Engine,
    O: ImmutableObjectStore,
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
            RuntimeValue::Digest(vyrm_core::digest::sha256_hex(rendered.as_bytes())),
        ),
    ]);
    if let Err(trace_error) = span.finish(store, TraceOutcome::Error, Vec::new(), attributes) {
        return Err(
            format!("{rendered}; authoritative trace finish also failed: {trace_error}").into(),
        );
    }
    Err(error)
}
