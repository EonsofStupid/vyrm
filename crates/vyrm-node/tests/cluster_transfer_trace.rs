use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use vyrm_cluster::{
    prepare_artifact_transfer, transfer_artifacts, ArtifactObjectProgress,
    ArtifactTransferObservation, NodeId, ReplicaTransferPlan, ShardId, ShardReadStamp,
    CLUSTER_CONTRACT_VERSION,
};
use vyrm_core::{
    DataTransaction, RuntimeCommit, RuntimeMutation, RuntimeRecordSchema, RuntimeSchemaRegistry,
    RuntimeType, RuntimeValue, ScopeId,
};
use vyrm_node::{execute_traced_artifact_transfer, DurableArtifactTransferObserver};
use vyrm_store::{DataRuntime, Engine, LocalObjectStore, MemoryEngine, NativeEngine, Store};

fn scope() -> ScopeId {
    ScopeId::new("instance:cluster-transfer-trace").unwrap()
}

fn plan() -> ReplicaTransferPlan {
    ReplicaTransferPlan {
        contract_version: CLUSTER_CONTRACT_VERSION,
        shard: ShardId(11),
        placement_epoch: 2,
        source: NodeId::new("node:source").unwrap(),
        target: NodeId::new("node:target").unwrap(),
        grounded_snapshot: ShardReadStamp {
            term: 3,
            commit_index: 19,
            placement_epoch: 2,
            state_digest: "66".repeat(32),
        },
        wal_from_exclusive: 19,
        wal_through_inclusive: 19,
        artifact_digests: BTreeSet::new(),
    }
}

#[derive(Debug, PartialEq, Eq, serde::Serialize)]
struct TraceView {
    name: String,
    phase: String,
    outcome: String,
    parent: Option<String>,
    attributes: BTreeMap<String, RuntimeValue>,
}

fn traces<E: Engine>(engine: &E) -> Vec<TraceView> {
    engine
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
                let RuntimeValue::Map(attributes) = &event.properties["attributes"] else {
                    panic!("trace attributes had the wrong shape")
                };
                Some(TraceView {
                    name: string("name"),
                    phase: string("phase"),
                    outcome: string("outcome"),
                    parent: event.properties.get("parent_span_id").map(|value| {
                        let RuntimeValue::String(value) = value else {
                            panic!("parent span had unexpected value {value:?}")
                        };
                        value.clone()
                    }),
                    attributes: attributes.clone(),
                })
            }
            _ => None,
        })
        .collect()
}

fn exercise<E: Engine>(
    engine: E,
    source_path: &std::path::Path,
    target_path: &std::path::Path,
) -> (vyrm_cluster::ArtifactTransferReceipt, Vec<TraceView>) {
    let source = LocalObjectStore::open(source_path).unwrap();
    let data = DataRuntime::new(engine, source);
    let object = data
        .stage_object(
            "vector:body@1:bytes",
            None,
            "application/vnd.vyrm.vector-hnsw+json",
            b"bounded vector artifact",
        )
        .unwrap();
    let mut schema = RuntimeSchemaRegistry::empty(1, "cluster transfer trace fixture");
    schema.records.insert(
        RuntimeType::new("fixture").unwrap(),
        RuntimeRecordSchema::default(),
    );
    let read = data.engine().runtime_read_stamp(&scope()).unwrap();
    data.commit(
        &DataTransaction::new(
            read,
            RuntimeCommit {
                scope: scope(),
                at: 10,
                actor: "cluster:test".into(),
                expected_cursor: 0,
                mutations: vec![
                    RuntimeMutation::Schema { registry: schema },
                    RuntimeMutation::Object { object },
                ],
            },
        )
        .unwrap(),
    )
    .unwrap();
    let manifest = prepare_artifact_transfer(plan(), data.engine(), &scope()).unwrap();
    let target = LocalObjectStore::open(target_path).unwrap();
    let receipt = execute_traced_artifact_transfer(
        data.engine(),
        data.objects(),
        &target,
        &manifest,
        "cluster:test",
        20,
    )
    .unwrap();
    (receipt, traces(data.engine()))
}

#[test]
fn cluster_object_transfer_is_causal_private_and_equal_across_engines() {
    let root = tempfile::tempdir().unwrap();
    let (memory_receipt, memory) = exercise(
        MemoryEngine::new(),
        &root.path().join("memory-source"),
        &root.path().join("memory-target"),
    );
    let (fjall_receipt, fjall) = exercise(
        Store::open(&root.path().join("fjall-engine")).unwrap(),
        &root.path().join("fjall-source"),
        &root.path().join("fjall-target"),
    );
    let (native_receipt, native) = exercise(
        NativeEngine::open(&root.path().join("native-engine")).unwrap(),
        &root.path().join("native-source"),
        &root.path().join("native-target"),
    );

    assert_eq!(memory_receipt, fjall_receipt);
    assert_eq!(memory_receipt, native_receipt);
    assert_eq!(memory, fjall);
    assert_eq!(memory, native);
    assert_eq!(memory.len(), 4);
    assert_eq!(memory[0].name, "cluster.artifact_transfer");
    assert_eq!(memory[1].name, "object.replicate");
    assert!(memory[1].parent.is_some());
    assert_eq!(memory[2].outcome, "ok");
    assert_eq!(memory[3].outcome, "ok");
    assert_eq!(
        memory[3].attributes["transferred_objects"],
        RuntimeValue::Unsigned(1)
    );
    let encoded = serde_json::to_string(&memory[3].attributes).unwrap();
    assert!(!encoded.contains("bounded vector artifact"));
}

#[test]
fn transport_observations_persist_as_one_causal_project_trace() {
    let root = tempfile::tempdir().unwrap();
    let source = LocalObjectStore::open(root.path().join("observed-source")).unwrap();
    let runtime = DataRuntime::new(MemoryEngine::new(), source);
    let object = runtime
        .stage_object(
            "vector:observed@1:bytes",
            None,
            "application/vnd.vyrm.vector-hnsw+json",
            b"operator-generated vector index bytes",
        )
        .unwrap();
    let mut schema = RuntimeSchemaRegistry::empty(1, "observed transfer fixture");
    schema.records.insert(
        RuntimeType::new("fixture").unwrap(),
        RuntimeRecordSchema::default(),
    );
    let read = runtime.engine().runtime_read_stamp(&scope()).unwrap();
    runtime
        .commit(
            &DataTransaction::new(
                read,
                RuntimeCommit {
                    scope: scope(),
                    at: 10,
                    actor: "cluster:test".into(),
                    expected_cursor: 0,
                    mutations: vec![
                        RuntimeMutation::Schema { registry: schema },
                        RuntimeMutation::Object { object },
                    ],
                },
            )
            .unwrap(),
        )
        .unwrap();
    let manifest = prepare_artifact_transfer(plan(), runtime.engine(), &scope()).unwrap();
    let target = LocalObjectStore::open(root.path().join("observed-target")).unwrap();
    let receipt = transfer_artifacts(runtime.objects(), &target, &manifest, 23).unwrap();
    let object = &manifest.objects[0];
    let (engine, _) = runtime.into_parts();
    let engine = Arc::new(engine);
    let observer =
        DurableArtifactTransferObserver::new(Arc::clone(&engine), "cluster:transport").unwrap();
    observer
        .observe_sync(ArtifactTransferObservation::prepared(&manifest, 1, 20).unwrap())
        .unwrap();
    observer
        .observe_sync(
            ArtifactTransferObservation::progress(
                &manifest,
                1,
                21,
                &ArtifactObjectProgress {
                    sha256: object.sha256.clone(),
                    expected_length: object.length,
                    next_offset: object.length,
                    complete: true,
                },
            )
            .unwrap(),
        )
        .unwrap();
    observer
        .observe_sync(
            ArtifactTransferObservation::completed(&manifest, 1, 22, 2_000, &receipt).unwrap(),
        )
        .unwrap();

    let trace = traces(engine.as_ref());
    let transfer = trace
        .iter()
        .filter(|event| event.name == "cluster.artifact_transfer")
        .collect::<Vec<_>>();
    assert_eq!(transfer.len(), 2);
    assert_eq!(transfer[0].phase, "start");
    assert_eq!(transfer[1].phase, "finish");
    assert_eq!(transfer[1].outcome, "ok");
    let chunk = trace
        .iter()
        .find(|event| event.name == "cluster.artifact_chunk")
        .unwrap();
    assert!(chunk.parent.is_some());
    assert_eq!(
        transfer[1].attributes["transferred_bytes"],
        RuntimeValue::Unsigned(object.length)
    );
    let encoded = serde_json::to_string(&trace).unwrap();
    assert!(!encoded.contains("operator-generated vector index bytes"));
}
