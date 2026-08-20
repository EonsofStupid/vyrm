use std::collections::{BTreeMap, BTreeSet};
use vyrm_cluster::{
    prepare_artifact_transfer, NodeId, ReplicaTransferPlan, ShardId, ShardReadStamp,
    CLUSTER_CONTRACT_VERSION,
};
use vyrm_core::{
    DataTransaction, RuntimeCommit, RuntimeMutation, RuntimeRecordSchema, RuntimeSchemaRegistry,
    RuntimeType, RuntimeValue, ScopeId,
};
use vyrm_node::execute_traced_artifact_transfer;
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

#[derive(Debug, PartialEq, Eq)]
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
