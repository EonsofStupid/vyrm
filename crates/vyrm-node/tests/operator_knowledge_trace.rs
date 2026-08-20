use std::collections::BTreeSet;
use vyrm_core::{
    digest, EmbeddingProvenance, ProjectionStamp, ProjectionState, RuntimeCommit, RuntimeMutation,
    RuntimeProperties, RuntimeRecord, RuntimeRecordSchema, RuntimeRef, RuntimeSchemaRegistry,
    RuntimeType, RuntimeValue, RuntimeVector, ScopeId, VectorNormalization, VectorValue,
    DATA_RUNTIME_CONTRACT_VERSION,
};
use vyrm_node::{execute_traced_operator_search, InstanceBinding, InstanceManifest};
use vyrm_operator::{
    OperatorAccessPath, OperatorAdapterDescriptor, OperatorKnowledgeBinding,
    OperatorSearchControls, OperatorSearchRequest, OperatorSourceRevision, OperatorSyncWork,
    ReferenceOperatorAdapter, ReferenceOperatorWriter, OPERATOR_KNOWLEDGE_CONTRACT_VERSION,
};
use vyrm_store::{Engine, MemoryEngine, NativeEngine, Store};
use vyrm_vector::{
    EmbeddingModelBinding, ScoreMetric, SearchMode, SearchRequest, VectorCandidate, VectorQuery,
    VectorRuntime,
};

fn scope() -> ScopeId {
    ScopeId::new("instance:operator-trace").unwrap()
}

fn model() -> EmbeddingModelBinding {
    EmbeddingModelBinding {
        name: "operator-fixture-v1".into(),
        digest: "11".repeat(32),
    }
}

fn fixture<E: Engine>(store: &E) -> Vec<VectorCandidate> {
    let mut registry = RuntimeSchemaRegistry::empty(1, "operator trace fixture");
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
        provenance: Some(EmbeddingProvenance {
            source_digest: format!("{:02x}", index + 32).repeat(32),
            model: model().name,
            model_digest: model().digest,
            dimensions: 3,
            normalization: VectorNormalization::None,
            generation_parameters: RuntimeProperties::new(),
        }),
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
            scope: scope(),
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
            scope: scope(),
            source_cursor: 5 + index as u64,
            vector,
        })
        .collect()
}

fn knowledge(instance: &InstanceBinding) -> OperatorKnowledgeBinding {
    OperatorKnowledgeBinding {
        contract_version: OPERATOR_KNOWLEDGE_CONTRACT_VERSION,
        adapter: "pgvector".into(),
        project_id: instance.manifest.id.clone(),
        member: instance.member.to_string_lossy().into_owned(),
        scope: scope(),
        config_digest: "22".repeat(32),
        source_identity_digest: "33".repeat(32),
        relation_digest: "44".repeat(32),
        tenant_digest: "55".repeat(32),
        model: model(),
        dimensions: 3,
        projection: ProjectionStamp {
            contract_version: DATA_RUNTIME_CONTRACT_VERSION,
            id: vyrm_core::ProjectionId::new("operator:pgvector:knowledge").unwrap(),
            generation: 1,
            source_cursor: 7,
            config_digest: "22".repeat(32),
            artifact_digest: "66".repeat(32),
            state: ProjectionState::Ready,
        },
    }
}

fn descriptor() -> OperatorAdapterDescriptor {
    OperatorAdapterDescriptor {
        contract_version: OPERATOR_KNOWLEDGE_CONTRACT_VERSION,
        adapter: "pgvector".into(),
        implementation_digest: "77".repeat(32),
        max_dimensions: 2_000,
        vector_kinds: BTreeSet::from([vyrm_operator::OperatorVectorKind::Dense]),
        search_capabilities: std::collections::BTreeMap::from([(
            OperatorAccessPath::Exact,
            BTreeSet::from([ScoreMetric::Dot]),
        )]),
        supports_tenant_filter: true,
        supports_payload_filter: false,
        supports_stable_revision: true,
    }
}

fn revision(project_id: &str, stable: &str) -> OperatorSourceRevision {
    OperatorSourceRevision {
        contract_version: OPERATOR_KNOWLEDGE_CONTRACT_VERSION,
        adapter: "pgvector".into(),
        project_id: project_id.into(),
        source_identity_digest: "33".repeat(32),
        snapshot_digest: "88".repeat(32),
        catalog_digest: "99".repeat(32),
        stable_revision: Some(stable.into()),
        wal_lsn_digest: Some("aa".repeat(32)),
    }
}

fn request<E: Engine>(store: &E, knowledge: &OperatorKnowledgeBinding) -> OperatorSearchRequest {
    OperatorSearchRequest {
        contract_version: OPERATOR_KNOWLEDGE_CONTRACT_VERSION,
        binding_digest: knowledge.digest().unwrap(),
        required_source_cursor: knowledge.projection.source_cursor,
        search: SearchRequest {
            scope: scope(),
            read: store.runtime_read_stamp(&scope()).unwrap(),
            valid_at: 10,
            field: "body".into(),
            query: VectorQuery::Dense {
                values: vec![0.314_159_27, 0.271_828_18, 0.0],
            },
            metric: ScoreMetric::Dot,
            embedding_model: Some(model()),
            top_k: 2,
            mode: SearchMode::Exact,
            filter: None,
        },
        controls: OperatorSearchControls::exact(),
        expected_stable_revision: Some("project-revision-7".into()),
    }
}

fn adapter(
    knowledge: &OperatorKnowledgeBinding,
    candidates: Vec<VectorCandidate>,
    stable: &str,
) -> ReferenceOperatorAdapter {
    ReferenceOperatorAdapter::new(
        descriptor(),
        knowledge,
        revision(&knowledge.project_id, stable),
        VectorRuntime::new(candidates).unwrap(),
    )
    .unwrap()
}

#[derive(Debug, PartialEq, Eq)]
struct TraceView {
    name: String,
    phase: String,
    outcome: String,
    parent: Option<String>,
    attributes: RuntimeProperties,
    encoded: String,
}

fn traces<E: Engine>(store: &E) -> Vec<TraceView> {
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
                let parent = event.properties.get("parent_span_id").map(|value| {
                    let RuntimeValue::String(value) = value else {
                        panic!("parent span had unexpected value {value:?}")
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
                    parent,
                    attributes: attributes.clone(),
                    encoded: serde_json::to_string(&event).unwrap(),
                })
            }
            _ => None,
        })
        .collect()
}

fn exercise<E: Engine>(
    store: &E,
    instance: &InstanceBinding,
) -> (vyrm_node::TracedOperatorSearch, Vec<TraceView>) {
    let candidates = fixture(store);
    let knowledge = knowledge(instance);
    let request = request(store, &knowledge);
    let mut adapter = adapter(&knowledge, candidates, "project-revision-7");
    let result = execute_traced_operator_search(
        store,
        instance,
        &knowledge,
        &mut adapter,
        &request,
        "operator:test",
        100,
    )
    .unwrap();
    (result, traces(store))
}

#[test]
fn operator_search_is_project_bound_private_and_equal_across_engines() {
    let instance_root = tempfile::tempdir().unwrap();
    InstanceManifest::ensure_dedicated(instance_root.path()).unwrap();
    let instance = InstanceBinding::discover(instance_root.path()).unwrap();
    let memory = MemoryEngine::new();
    let fjall_root = tempfile::tempdir().unwrap();
    let fjall = Store::open(fjall_root.path()).unwrap();
    let native_root = tempfile::tempdir().unwrap();
    let native_path = native_root.path().join("native");
    let native = NativeEngine::open(&native_path).unwrap();

    let (memory_result, memory_traces) = exercise(&memory, &instance);
    let (fjall_result, fjall_traces) = exercise(&fjall, &instance);
    let (native_result, native_traces) = exercise(&native, &instance);
    assert_eq!(memory_result, fjall_result);
    assert_eq!(memory_result, native_result);
    assert_eq!(memory_result.result.hits.len(), 2);
    assert_eq!(memory.runtime_cursor().unwrap(), 12);
    let normalize = |traces: &[TraceView]| {
        traces
            .iter()
            .map(|trace| {
                (
                    trace.name.clone(),
                    trace.phase.clone(),
                    trace.outcome.clone(),
                    trace.parent.clone(),
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
            ("operator.knowledge.search", "start", "running"),
            ("operator.knowledge.execute", "start", "running"),
            ("operator.knowledge.execute", "finish", "ok"),
            ("operator.knowledge.search", "finish", "ok"),
        ]
    );
    let root = memory_traces[0].parent.clone();
    assert!(root.is_none());
    assert!(memory_traces[1].parent.is_some());
    let encoded = memory_traces
        .iter()
        .map(|trace| trace.encoded.as_str())
        .collect::<String>();
    for secret in ["0.31415927", "0.27182818", "doc-0", "project-revision-7"] {
        assert!(!encoded.contains(secret), "trace leaked {secret}");
    }

    drop(native);
    let reopened = NativeEngine::open(&native_path).unwrap();
    assert_eq!(normalize(&traces(&reopened)), normalize(&native_traces));
}

#[test]
fn stale_projection_revision_and_foreign_project_are_durable_denials() {
    let instance_root = tempfile::tempdir().unwrap();
    InstanceManifest::ensure_dedicated(instance_root.path()).unwrap();
    let instance = InstanceBinding::discover(instance_root.path()).unwrap();

    let stale_store = MemoryEngine::new();
    let candidates = fixture(&stale_store);
    let stale_knowledge = knowledge(&instance);
    let stale_request = request(&stale_store, &stale_knowledge);
    let mut stale = adapter(&stale_knowledge, candidates, "project-revision-6");
    let error = execute_traced_operator_search(
        &stale_store,
        &instance,
        &stale_knowledge,
        &mut stale,
        &stale_request,
        "operator:test",
        100,
    )
    .unwrap_err();
    assert!(error.to_string().contains("stale"));
    assert_eq!(
        traces(&stale_store)
            .iter()
            .map(|trace| trace.outcome.as_str())
            .collect::<Vec<_>>(),
        ["running", "running", "denied", "denied"]
    );

    let projection_store = MemoryEngine::new();
    let candidates = fixture(&projection_store);
    let mut projection_knowledge = knowledge(&instance);
    projection_knowledge.projection.source_cursor = 6;
    let mut projection_request = request(&projection_store, &projection_knowledge);
    projection_request.required_source_cursor = 7;
    let mut projection_adapter = adapter(&projection_knowledge, candidates, "project-revision-7");
    let error = execute_traced_operator_search(
        &projection_store,
        &instance,
        &projection_knowledge,
        &mut projection_adapter,
        &projection_request,
        "operator:test",
        100,
    )
    .unwrap_err();
    assert!(error.to_string().contains("projection is stale"));
    assert_eq!(
        traces(&projection_store)
            .iter()
            .map(|trace| trace.outcome.as_str())
            .collect::<Vec<_>>(),
        ["running", "running", "denied", "denied"]
    );

    let foreign_store = MemoryEngine::new();
    let candidates = fixture(&foreign_store);
    let mut foreign = knowledge(&instance);
    foreign.project_id = "another-project".into();
    let request = request(&foreign_store, &foreign);
    let mut adapter = adapter(&foreign, candidates, "project-revision-7");
    let error = execute_traced_operator_search(
        &foreign_store,
        &instance,
        &foreign,
        &mut adapter,
        &request,
        "operator:test",
        100,
    )
    .unwrap_err();
    assert!(error.to_string().contains("not instance"));
    assert_eq!(traces(&foreign_store).len(), 2);
    assert_eq!(traces(&foreign_store)[1].outcome, "denied");
}

#[test]
fn raw_query_identity_is_digest_only() {
    let query = serde_json::to_vec(&VectorQuery::Dense {
        values: vec![0.314_159_27, 0.271_828_18, 0.0],
    })
    .unwrap();
    assert_eq!(digest::sha256_hex(&query).len(), 64);
}

#[test]
fn traced_outbox_retry_applies_external_payload_once() {
    let instance_root = tempfile::tempdir().unwrap();
    InstanceManifest::ensure_dedicated(instance_root.path()).unwrap();
    let instance = InstanceBinding::discover(instance_root.path()).unwrap();
    let store = MemoryEngine::new();
    let candidates = fixture(&store);
    let knowledge = knowledge(&instance);
    let source = store
        .runtime_outbox_since(0, 100)
        .unwrap()
        .into_iter()
        .find(|work| work.source_cursor == 7)
        .unwrap();
    let source_change = store
        .runtime_changes_since(6, 1, Some(&scope()))
        .unwrap()
        .changes
        .pop()
        .unwrap();
    let payload = serde_json::to_vec(&candidates[2].vector).unwrap();
    let work = OperatorSyncWork::for_vector(
        &knowledge,
        &source,
        source_change.digest,
        digest::sha256_hex(&payload),
    )
    .unwrap();
    let mut writer = ReferenceOperatorWriter::new(
        descriptor(),
        &knowledge,
        revision(&knowledge.project_id, "project-revision-8"),
    )
    .unwrap();

    let first = vyrm_node::execute_traced_operator_sync(
        &store,
        &instance,
        &knowledge,
        &mut writer,
        &work,
        &payload,
        "operator:test",
        200,
    )
    .unwrap();
    let retry = vyrm_node::execute_traced_operator_sync(
        &store,
        &instance,
        &knowledge,
        &mut writer,
        &work,
        &payload,
        "operator:test",
        300,
    )
    .unwrap();
    assert!(first.receipt.applied_now);
    assert!(retry.receipt.idempotent_replay);
    assert_eq!(writer.apply_count(), 1);
    assert_eq!(store.runtime_cursor().unwrap(), 16);
    let traces = traces(&store);
    assert_eq!(
        traces
            .iter()
            .map(|trace| (trace.name.as_str(), trace.outcome.as_str()))
            .collect::<Vec<_>>(),
        [
            ("operator.knowledge.sync", "running"),
            ("operator.knowledge.apply", "running"),
            ("operator.knowledge.apply", "ok"),
            ("operator.knowledge.sync", "ok"),
            ("operator.knowledge.sync", "running"),
            ("operator.knowledge.apply", "running"),
            ("operator.knowledge.apply", "ok"),
            ("operator.knowledge.sync", "ok"),
        ]
    );
    let encoded = traces
        .iter()
        .map(|trace| trace.encoded.as_str())
        .collect::<String>();
    assert!(!encoded.contains("doc-2"));
    assert!(traces[2].attributes.contains_key("applied_now"));
    assert_eq!(
        traces[6].attributes.get("idempotent_replay"),
        Some(&RuntimeValue::Bool(true))
    );
}
