use std::collections::BTreeSet;
use vyrm_core::{
    digest, ProjectionFamily, RuntimeCommit, RuntimeId, RuntimeMutation, RuntimeProperties,
    RuntimeRecord, RuntimeRecordSchema, RuntimeRef, RuntimeSchemaRegistry, RuntimeType,
    RuntimeValue, RuntimeVector, ScopeId, VectorValue,
};
use vyrm_embed::{
    EmbeddingBackend, EmbeddingJob, EmbeddingSourceReader, EmbeddingSourceSnapshot,
    FeatureHashBackend, NetworkPolicy, EMBEDDING_CONTRACT_VERSION,
};
use vyrm_node::{
    execute_traced_embedding, execute_traced_vector_search, publish_traced_vector_artifact,
};
use vyrm_store::{Engine, MemoryEngine, NativeEngine, Store};
use vyrm_vector::{
    HnswConfig, HnswIndex, ScoreMetric, SearchMode, SearchRequest, VectorCandidate, VectorQuery,
    VectorRuntime,
};

fn scope() -> ScopeId {
    ScopeId::new("instance:data-plane-trace").unwrap()
}

fn fixture<E: Engine>(store: &E) -> Vec<VectorCandidate> {
    let scope = scope();
    let mut registry = RuntimeSchemaRegistry::empty(1, "vector trace fixture");
    registry.records.insert(
        RuntimeType::new("document").unwrap(),
        RuntimeRecordSchema {
            allow_additional_properties: true,
            ..RuntimeRecordSchema::default()
        },
    );
    let records = (0..3)
        .map(|index| RuntimeRecord {
            reference: RuntimeRef::new("document", format!("doc-{index}")).unwrap(),
            valid_from: 1,
            valid_to: None,
            properties: RuntimeProperties::new(),
        })
        .collect::<Vec<_>>();
    let vectors = [
        vec![1.0, 0.0, 0.0],
        vec![0.0, 1.0, 0.0],
        vec![0.5, 0.5, 0.0],
    ]
    .into_iter()
    .enumerate()
    .map(|(index, values)| RuntimeVector {
        reference: RuntimeRef::new("embedding", format!("doc-{index}-body")).unwrap(),
        subject: records[index].reference.clone(),
        field: "body".into(),
        valid_from: 1,
        valid_to: None,
        value: VectorValue::Dense { values },
        provenance: None,
        properties: RuntimeProperties::new(),
    })
    .collect::<Vec<_>>();
    let mut mutations = vec![RuntimeMutation::Schema { registry }];
    mutations.extend(
        records
            .iter()
            .cloned()
            .map(|record| RuntimeMutation::Record { record }),
    );
    mutations.extend(
        vectors
            .iter()
            .cloned()
            .map(|vector| RuntimeMutation::Vector { vector }),
    );
    store
        .commit_runtime(&RuntimeCommit {
            scope: scope.clone(),
            at: 10,
            actor: "fixture".into(),
            expected_cursor: 0,
            mutations,
        })
        .unwrap();
    vectors
        .into_iter()
        .enumerate()
        .map(|(index, vector)| VectorCandidate {
            scope: scope.clone(),
            source_cursor: 5 + index as u64,
            vector,
        })
        .collect()
}

fn request<E: Engine>(store: &E, mode: SearchMode) -> SearchRequest {
    SearchRequest {
        scope: scope(),
        read: store.runtime_read_stamp(&scope()).unwrap(),
        valid_at: 10,
        field: "body".into(),
        query: VectorQuery::Dense {
            values: vec![0.314_159_27, 0.271_828_18, 0.0],
        },
        metric: ScoreMetric::Dot,
        embedding_model: None,
        top_k: 1,
        mode,
        filter: None,
    }
}

#[derive(Debug, PartialEq, Eq)]
struct TraceView {
    name: String,
    phase: String,
    outcome: String,
    trace_id: String,
    span_id: String,
    parent_span_id: Option<String>,
    attributes: RuntimeProperties,
    encoded: String,
}

fn trace_views<E: Engine>(store: &E) -> Vec<TraceView> {
    store
        .runtime_changes_since(0, usize::MAX, Some(&scope()))
        .unwrap()
        .changes
        .into_iter()
        .filter_map(|change| match change.mutation {
            RuntimeMutation::Event { event } if event.kind.as_str() == "runtime_trace" => {
                let string = |name: &str| match &event.properties[name] {
                    RuntimeValue::String(value) => value.clone(),
                    value => panic!("{name} had unexpected value {value:?}"),
                };
                let parent_span_id = event.properties.get("parent_span_id").map(|value| {
                    let RuntimeValue::String(value) = value else {
                        panic!("parent_span_id had unexpected value {value:?}")
                    };
                    value.clone()
                });
                let RuntimeValue::Map(attributes) = &event.properties["attributes"] else {
                    panic!("trace attributes had the wrong shape")
                };
                Some(TraceView {
                    name: string("name"),
                    phase: string("phase"),
                    outcome: string("outcome"),
                    trace_id: string("trace_id"),
                    span_id: string("span_id"),
                    parent_span_id,
                    attributes: attributes.clone(),
                    encoded: serde_json::to_string(&event).unwrap(),
                })
            }
            _ => None,
        })
        .collect()
}

fn exercise<E: Engine>(store: &E) -> (vyrm_node::TracedVectorSearch, Vec<TraceView>) {
    let candidates = fixture(store);
    let runtime = VectorRuntime::new(candidates).unwrap();
    let result = execute_traced_vector_search(
        store,
        &runtime,
        &request(store, SearchMode::Exact),
        16,
        "operator:test",
        100,
    )
    .unwrap();
    (result, trace_views(store))
}

#[test]
fn vector_search_is_causal_private_and_equal_across_all_engines() {
    let memory = MemoryEngine::new();
    let fjall_root = tempfile::tempdir().unwrap();
    let fjall = Store::open(fjall_root.path()).unwrap();
    let native_root = tempfile::tempdir().unwrap();
    let native_path = native_root.path().join("native");
    let native = NativeEngine::open(&native_path).unwrap();

    let (memory_result, memory_traces) = exercise(&memory);
    let (fjall_result, fjall_traces) = exercise(&fjall);
    let (native_result, native_traces) = exercise(&native);
    assert_eq!(memory_result, fjall_result);
    assert_eq!(memory_result, native_result);
    assert_eq!(memory_result.prepared.plan().required_source_cursor, 7);
    assert_eq!(memory_result.execution.hits.len(), 1);
    assert_eq!(memory.runtime_cursor().unwrap(), 14);
    let normalize = |traces: &[TraceView]| {
        traces
            .iter()
            .map(|trace| {
                (
                    trace.name.clone(),
                    trace.phase.clone(),
                    trace.outcome.clone(),
                    trace.trace_id.clone(),
                    trace.span_id.clone(),
                    trace.parent_span_id.clone(),
                    trace.attributes.clone(),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(normalize(&memory_traces), normalize(&fjall_traces));
    assert_eq!(normalize(&memory_traces), normalize(&native_traces));
    assert_eq!(
        memory_traces
            .iter()
            .map(|trace| (
                trace.name.as_str(),
                trace.phase.as_str(),
                trace.outcome.as_str()
            ))
            .collect::<Vec<_>>(),
        [
            ("vector.search", "start", "running"),
            ("vector.plan", "start", "running"),
            ("vector.plan", "finish", "ok"),
            ("vector.execute", "start", "running"),
            ("vector.execute", "finish", "ok"),
            ("vector.search", "finish", "ok"),
        ]
    );
    let root_span = &memory_traces[0].span_id;
    assert!(memory_traces
        .iter()
        .filter(|trace| trace.name != "vector.search")
        .all(|trace| trace.parent_span_id.as_ref() == Some(root_span)));
    assert!(memory_traces
        .iter()
        .all(|trace| trace.trace_id == memory_traces[0].trace_id));
    assert!(memory_traces.iter().all(|trace| {
        !trace.encoded.contains("0.31415927") && !trace.encoded.contains("0.27182818")
    }));
    assert_eq!(
        memory_traces[2].attributes["required_source_cursor"],
        RuntimeValue::Unsigned(7)
    );

    drop(native);
    let reopened = NativeEngine::open(&native_path).unwrap();
    assert_eq!(trace_views(&reopened).len(), 6);
}

#[test]
fn projection_publication_and_approximate_selection_remain_fresh_across_trace_events() {
    let store = MemoryEngine::new();
    let candidates = fixture(&store);
    let mut runtime = VectorRuntime::new(candidates.clone()).unwrap();
    let hnsw = HnswIndex::build(
        HnswConfig {
            id: vyrm_core::ProjectionId::new("vector:hnsw:body").unwrap(),
            scope: scope(),
            field: "body".into(),
            dimensions: 3,
            metric: ScoreMetric::Dot,
            embedding_model: None,
            m: 4,
            ef_construction: 8,
            max_level: 4,
            seed: 9,
            filter_properties: BTreeSet::new(),
        },
        1,
        7,
        candidates,
    )
    .unwrap();
    assert_eq!(
        publish_traced_vector_artifact(&store, &mut runtime, 0, hnsw.into(), "operator:test", 100,)
            .unwrap(),
        1
    );
    let search = execute_traced_vector_search(
        &store,
        &runtime,
        &request(&store, SearchMode::RequireApproximate { exact_rerank: 2 }),
        16,
        "operator:test",
        101,
    )
    .unwrap();
    assert_eq!(search.prepared.plan().required_source_cursor, 7);
    assert_eq!(
        search.prepared.plan().selected.kind,
        vyrm_vector::AccessPathKind::Hnsw
    );
    let traces = trace_views(&store);
    assert_eq!(traces[0].name, "vector.projection.publish");
    assert_eq!(traces[1].outcome, "ok");
    assert_eq!(
        traces[4].attributes["selected_path"],
        RuntimeValue::String("hnsw".into())
    );
}

#[test]
fn required_approximate_failure_closes_the_planning_tree_as_a_denial() {
    let store = MemoryEngine::new();
    let candidates = fixture(&store);
    let runtime = VectorRuntime::new(candidates).unwrap();
    let error = execute_traced_vector_search(
        &store,
        &runtime,
        &request(&store, SearchMode::RequireApproximate { exact_rerank: 2 }),
        16,
        "operator:test",
        100,
    )
    .unwrap_err();
    assert!(error.to_string().contains("no vector access path"));
    let traces = trace_views(&store);
    assert_eq!(traces.len(), 4);
    assert_eq!(traces[2].outcome, "denied");
    assert_eq!(traces[3].outcome, "denied");
    assert_eq!(
        traces[2].attributes["error_class"],
        RuntimeValue::String("freshness_or_policy".into())
    );
}

const EMBEDDING_BYTES: &[u8] = b"super-secret-embedding-source";

fn embedding_fixture<E: Engine>(store: &E) {
    let mut registry = RuntimeSchemaRegistry::empty(1, "embedding trace fixture");
    registry.records.insert(
        RuntimeType::new("document").unwrap(),
        RuntimeRecordSchema {
            allow_additional_properties: true,
            ..RuntimeRecordSchema::default()
        },
    );
    store
        .commit_runtime(&RuntimeCommit {
            scope: scope(),
            at: 10,
            actor: "fixture".into(),
            expected_cursor: 0,
            mutations: vec![
                RuntimeMutation::Schema { registry },
                RuntimeMutation::Record {
                    record: RuntimeRecord {
                        reference: RuntimeRef::new("document", "embedding-source").unwrap(),
                        valid_from: 1,
                        valid_to: None,
                        properties: RuntimeProperties::from([(
                            "content_digest".into(),
                            RuntimeValue::Digest(digest::sha256_hex(EMBEDDING_BYTES)),
                        )]),
                    },
                },
            ],
        })
        .unwrap();
}

fn embedding_job<E: Engine>(store: &E, backend: &FeatureHashBackend) -> EmbeddingJob {
    EmbeddingJob {
        contract_version: EMBEDDING_CONTRACT_VERSION,
        id: RuntimeId::new("embedding-job-1").unwrap(),
        scope: scope(),
        read: store.runtime_read_stamp(&scope()).unwrap(),
        source: RuntimeRef::new("document", "embedding-source").unwrap(),
        expected_source_digest: digest::sha256_hex(EMBEDDING_BYTES),
        target: RuntimeRef::new("embedding", "embedding-source-body").unwrap(),
        subject: RuntimeRef::new("document", "embedding-source").unwrap(),
        field: "body".into(),
        valid_from: 10,
        valid_to: None,
        model: backend.descriptor().model.clone(),
        network_policy: NetworkPolicy::Deny,
        requested_at: 100,
        properties: RuntimeProperties::new(),
    }
}

struct SequenceReader {
    snapshots: Vec<EmbeddingSourceSnapshot>,
    reads: usize,
}

impl EmbeddingSourceReader for SequenceReader {
    fn read(&mut self, _source: &RuntimeRef) -> vyrm_core::Result<EmbeddingSourceSnapshot> {
        let snapshot = self.snapshots[self.reads.min(self.snapshots.len() - 1)].clone();
        self.reads += 1;
        Ok(snapshot)
    }
}

fn embedding_reader(job: &EmbeddingJob) -> SequenceReader {
    SequenceReader {
        snapshots: vec![EmbeddingSourceSnapshot::for_bytes(
            job.source.clone(),
            "text/plain",
            EMBEDDING_BYTES.to_vec(),
        )
        .unwrap()],
        reads: 0,
    }
}

fn exercise_embedding<E: Engine>(
    store: &E,
) -> (vyrm_node::TracedEmbeddingExecution, Vec<TraceView>) {
    embedding_fixture(store);
    let mut backend = FeatureHashBackend::new(16, 7).unwrap();
    let job = embedding_job(store, &backend);
    let mut reader = embedding_reader(&job);
    let result =
        execute_traced_embedding(store, &job, &mut reader, &mut backend, "operator:test", 101)
            .unwrap();
    assert_eq!(reader.reads, 2);
    (result, trace_views(store))
}

#[test]
fn embedding_inference_rebases_only_its_trace_events_and_commits_on_all_engines() {
    let memory = MemoryEngine::new();
    let fjall_root = tempfile::tempdir().unwrap();
    let fjall = Store::open(fjall_root.path()).unwrap();
    let native_root = tempfile::tempdir().unwrap();
    let native_path = native_root.path().join("native");
    let native = NativeEngine::open(&native_path).unwrap();

    let (memory_result, memory_traces) = exercise_embedding(&memory);
    let (fjall_result, fjall_traces) = exercise_embedding(&fjall);
    let (native_result, native_traces) = exercise_embedding(&native);
    assert_eq!(memory_result, fjall_result);
    assert_eq!(memory_result, native_result);
    assert_eq!(memory_result.commit.first_cursor, 8);
    assert_eq!(memory_result.commit.last_cursor, 8);
    assert_eq!(memory.runtime_cursor().unwrap(), 10);
    let normalize = |traces: &[TraceView]| {
        traces
            .iter()
            .map(|trace| {
                (
                    trace.name.clone(),
                    trace.phase.clone(),
                    trace.outcome.clone(),
                    trace.trace_id.clone(),
                    trace.span_id.clone(),
                    trace.parent_span_id.clone(),
                    trace.attributes.clone(),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(normalize(&memory_traces), normalize(&fjall_traces));
    assert_eq!(normalize(&memory_traces), normalize(&native_traces));
    assert_eq!(
        memory_traces
            .iter()
            .map(|trace| (
                trace.name.as_str(),
                trace.phase.as_str(),
                trace.outcome.as_str()
            ))
            .collect::<Vec<_>>(),
        [
            ("embedding.run", "start", "running"),
            ("embedding.infer", "start", "running"),
            ("embedding.infer", "finish", "ok"),
            ("embedding.commit", "start", "running"),
            ("embedding.commit", "finish", "ok"),
            ("embedding.run", "finish", "ok"),
        ]
    );
    assert_eq!(
        memory_traces[4].attributes["rebased_over_trace_events"],
        RuntimeValue::Unsigned(5)
    );
    assert!(memory_traces
        .iter()
        .all(|trace| !trace.encoded.contains("super-secret-embedding-source")));
    let vector_work = memory
        .runtime_outbox_since(0, 100)
        .unwrap()
        .into_iter()
        .filter(|work| work.family == ProjectionFamily::Vector)
        .collect::<Vec<_>>();
    assert_eq!(vector_work.len(), 1);
    assert_eq!(vector_work[0].source_cursor, 8);

    drop(native);
    let reopened = NativeEngine::open(&native_path).unwrap();
    assert_eq!(trace_views(&reopened).len(), 6);
    assert_eq!(
        reopened
            .runtime_changes_since(0, 100, Some(&scope()))
            .unwrap()
            .changes
            .into_iter()
            .filter(|change| matches!(change.mutation, RuntimeMutation::Vector { .. }))
            .count(),
        1
    );
}

#[test]
fn embedding_source_change_after_inference_denies_without_a_vector_commit() {
    let store = MemoryEngine::new();
    embedding_fixture(&store);
    let mut backend = FeatureHashBackend::new(16, 7).unwrap();
    let job = embedding_job(&store, &backend);
    let mut reader = SequenceReader {
        snapshots: vec![
            EmbeddingSourceSnapshot::for_bytes(
                job.source.clone(),
                "text/plain",
                EMBEDDING_BYTES.to_vec(),
            )
            .unwrap(),
            EmbeddingSourceSnapshot::for_bytes(
                job.source.clone(),
                "text/plain",
                b"changed-after-inference".to_vec(),
            )
            .unwrap(),
        ],
        reads: 0,
    };
    let error = execute_traced_embedding(
        &store,
        &job,
        &mut reader,
        &mut backend,
        "operator:test",
        101,
    )
    .unwrap_err();
    assert!(error.to_string().contains("changed during inference"));
    let traces = trace_views(&store);
    assert_eq!(traces.len(), 4);
    assert_eq!(traces[2].outcome, "ok");
    assert_eq!(traces[3].outcome, "denied");
    assert!(store
        .runtime_outbox_since(0, 100)
        .unwrap()
        .iter()
        .all(|work| work.family != ProjectionFamily::Vector));
}

struct MutatingReader<'a, E> {
    store: &'a E,
    snapshot: EmbeddingSourceSnapshot,
    reads: usize,
}

impl<E: Engine> EmbeddingSourceReader for MutatingReader<'_, E> {
    fn read(&mut self, _source: &RuntimeRef) -> vyrm_core::Result<EmbeddingSourceSnapshot> {
        self.reads += 1;
        if self.reads == 2 {
            let read = self.store.runtime_read_stamp(&scope()).unwrap();
            self.store
                .commit_runtime(&RuntimeCommit {
                    scope: scope(),
                    at: 100,
                    actor: "concurrent:test".into(),
                    expected_cursor: read.commit_cursor,
                    mutations: vec![RuntimeMutation::Record {
                        record: RuntimeRecord {
                            reference: RuntimeRef::new("document", "concurrent-change").unwrap(),
                            valid_from: 100,
                            valid_to: None,
                            properties: RuntimeProperties::new(),
                        },
                    }],
                })
                .unwrap();
        }
        Ok(self.snapshot.clone())
    }
}

#[test]
fn embedding_rebase_rejects_non_trace_mutations_even_when_source_bytes_match() {
    let store = MemoryEngine::new();
    embedding_fixture(&store);
    let mut backend = FeatureHashBackend::new(16, 7).unwrap();
    let job = embedding_job(&store, &backend);
    let mut reader = MutatingReader {
        store: &store,
        snapshot: EmbeddingSourceSnapshot::for_bytes(
            job.source.clone(),
            "text/plain",
            EMBEDDING_BYTES.to_vec(),
        )
        .unwrap(),
        reads: 0,
    };
    let error = execute_traced_embedding(
        &store,
        &job,
        &mut reader,
        &mut backend,
        "operator:test",
        101,
    )
    .unwrap_err();
    assert!(error.to_string().contains("source state changed"));
    let traces = trace_views(&store);
    assert_eq!(traces[4].name, "embedding.commit");
    assert_eq!(traces[4].outcome, "denied");
    assert_eq!(traces[5].name, "embedding.run");
    assert_eq!(traces[5].outcome, "denied");
    assert!(store
        .runtime_outbox_since(0, 100)
        .unwrap()
        .iter()
        .all(|work| work.family != ProjectionFamily::Vector));
}
