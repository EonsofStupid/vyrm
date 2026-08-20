use std::collections::BTreeSet;
use tempfile::tempdir;
use vyrm_core::{
    ProjectionId, RuntimeMutation, RuntimeProperties, RuntimeRef, RuntimeVector, ScopeId,
    VectorValue,
};
use vyrm_node::{
    publish_traced_vector_artifact, reopen_vector_runtime, vector_artifact_catalog_entries,
};
use vyrm_store::{DataRuntime, Engine, LocalObjectStore, MemoryEngine, NativeEngine, Store};
use vyrm_vector::{
    HnswConfig, HnswIndex, ScoreMetric, VectorCandidate, VectorRuntime, VECTOR_ARTIFACT_RECORD_TYPE,
};

fn scope() -> ScopeId {
    ScopeId::new("instance:vector-catalog").unwrap()
}

fn candidates() -> Vec<VectorCandidate> {
    let scope = scope();
    [[1.0, 0.0], [0.0, 1.0], [0.7, 0.3]]
        .into_iter()
        .enumerate()
        .map(|(index, values)| VectorCandidate {
            scope: scope.clone(),
            source_cursor: index as u64 + 1,
            vector: RuntimeVector {
                reference: RuntimeRef::new("embedding", format!("body-{index}")).unwrap(),
                subject: RuntimeRef::new("document", format!("doc-{index}")).unwrap(),
                field: "body".into(),
                valid_from: 1,
                valid_to: None,
                value: VectorValue::Dense {
                    values: values.to_vec(),
                },
                provenance: None,
                properties: RuntimeProperties::new(),
            },
        })
        .collect()
}

fn hnsw(candidates: Vec<VectorCandidate>) -> HnswIndex {
    HnswIndex::build(
        HnswConfig {
            id: ProjectionId::new("vector:hnsw:body").unwrap(),
            scope: scope(),
            field: "body".into(),
            dimensions: 2,
            metric: ScoreMetric::Dot,
            embedding_model: None,
            m: 2,
            ef_construction: 4,
            max_level: 3,
            seed: 7,
            filter_properties: BTreeSet::new(),
        },
        1,
        3,
        candidates,
    )
    .unwrap()
}

#[test]
fn publication_atomically_binds_typed_record_object_and_serving_view() {
    let object_dir = tempdir().unwrap();
    let data = DataRuntime::new(
        MemoryEngine::new(),
        LocalObjectStore::open(object_dir.path()).unwrap(),
    );
    let canonical = candidates();
    let mut runtime = VectorRuntime::new(canonical.clone()).unwrap();
    let published = publish_traced_vector_artifact(
        &data,
        &mut runtime,
        0,
        hnsw(canonical.clone()).into(),
        "operator:catalog-test",
        100,
    )
    .unwrap();

    assert_eq!(published.catalog_revision, 1);
    assert_eq!(runtime.catalog().revision, 1);
    let entries = vector_artifact_catalog_entries(data.engine(), &scope()).unwrap();
    assert_eq!(entries, vec![published.entry.clone()]);

    let changes = data
        .engine()
        .runtime_changes_since(0, usize::MAX, Some(&scope()))
        .unwrap()
        .changes;
    let record_commit = changes.iter().find_map(|change| match &change.mutation {
        RuntimeMutation::Record { record }
            if record.reference.kind.as_str() == VECTOR_ARTIFACT_RECORD_TYPE =>
        {
            Some(change.commit_id.clone())
        }
        _ => None,
    });
    let object_commit = changes.iter().find_map(|change| match &change.mutation {
        RuntimeMutation::Object { object }
            if object.subject.as_ref()
                == Some(&published.entry.object.subject.clone().unwrap()) =>
        {
            Some(change.commit_id.clone())
        }
        _ => None,
    });
    assert_eq!(record_commit, Some(published.commit.commit_id.clone()));
    assert_eq!(object_commit, record_commit);

    let reopened = reopen_vector_runtime(&data, &scope(), canonical).unwrap();
    assert_eq!(reopened.catalog(), runtime.catalog());
}

#[test]
fn native_reopen_reconstructs_catalog_and_missing_bytes_fail_closed() {
    let root = tempdir().unwrap();
    let engine_path = root.path().join("engine");
    let object_path = root.path().join("objects");
    let canonical = candidates();
    let object_sha;
    {
        let data = DataRuntime::new(
            NativeEngine::open(&engine_path).unwrap(),
            LocalObjectStore::open(&object_path).unwrap(),
        );
        let mut runtime = VectorRuntime::new(canonical.clone()).unwrap();
        let publication = publish_traced_vector_artifact(
            &data,
            &mut runtime,
            0,
            hnsw(canonical.clone()).into(),
            "operator:catalog-test",
            100,
        )
        .unwrap();
        object_sha = publication.entry.object.sha256;
    }

    let reopened_data = DataRuntime::new(
        NativeEngine::open(&engine_path).unwrap(),
        LocalObjectStore::open(&object_path).unwrap(),
    );
    let reopened = reopen_vector_runtime(&reopened_data, &scope(), canonical.clone()).unwrap();
    assert_eq!(reopened.catalog().revision, 1);

    let key = vyrm_core::ObjectReference::canonical_key(&object_sha).unwrap();
    std::fs::remove_file(object_path.join(key)).unwrap();
    let error = reopen_vector_runtime(&reopened_data, &scope(), canonical)
        .unwrap_err()
        .to_string();
    assert!(error.contains("object") || error.contains("No such file"));
}

#[test]
fn authoritative_revision_conflict_does_not_mutate_a_fresh_serving_view() {
    let object_dir = tempdir().unwrap();
    let data = DataRuntime::new(
        MemoryEngine::new(),
        LocalObjectStore::open(object_dir.path()).unwrap(),
    );
    let canonical = candidates();
    let artifact = hnsw(canonical.clone());
    let mut first = VectorRuntime::new(canonical.clone()).unwrap();
    publish_traced_vector_artifact(
        &data,
        &mut first,
        0,
        artifact.clone().into(),
        "operator:catalog-test",
        100,
    )
    .unwrap();

    let mut stale = VectorRuntime::new(canonical).unwrap();
    let error = publish_traced_vector_artifact(
        &data,
        &mut stale,
        0,
        artifact.into(),
        "operator:catalog-test",
        101,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("authoritative revision 1"));
    assert_eq!(stale.catalog().revision, 0);
}

fn published_entry<E: Engine>(
    engine: E,
    object_path: &std::path::Path,
) -> vyrm_vector::VectorArtifactCatalogEntry {
    let data = DataRuntime::new(engine, LocalObjectStore::open(object_path).unwrap());
    let canonical = candidates();
    let mut runtime = VectorRuntime::new(canonical.clone()).unwrap();
    publish_traced_vector_artifact(
        &data,
        &mut runtime,
        0,
        hnsw(canonical).into(),
        "operator:catalog-test",
        100,
    )
    .unwrap()
    .entry
}

#[test]
fn catalog_publication_is_logically_identical_across_memory_fjall_and_native() {
    let root = tempdir().unwrap();
    let memory = published_entry(MemoryEngine::new(), &root.path().join("memory-objects"));
    let fjall = published_entry(
        Store::open(&root.path().join("fjall-engine")).unwrap(),
        &root.path().join("fjall-objects"),
    );
    let native = published_entry(
        NativeEngine::open(&root.path().join("native-engine")).unwrap(),
        &root.path().join("native-objects"),
    );
    assert_eq!(memory, fjall);
    assert_eq!(memory, native);
}
