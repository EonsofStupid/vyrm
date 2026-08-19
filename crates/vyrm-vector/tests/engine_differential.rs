use tempfile::tempdir;
use vyrm_core::{
    ReadStamp, RuntimeCommit, RuntimeMutation, RuntimeProperties, RuntimeRecord,
    RuntimeRecordSchema, RuntimeRef, RuntimeSchemaRegistry, RuntimeType, RuntimeValue,
    RuntimeVector, ScopeId, VectorValue,
};
use vyrm_store::{Engine, MemoryEngine, NativeEngine, Store};
use vyrm_vector::{search_changes_exact, ScoreMetric, SearchMode, SearchRequest, VectorQuery};

fn commit(engine: &dyn Engine, scope: &ScopeId) -> ReadStamp {
    let mut schema = RuntimeSchemaRegistry::empty(1, "vector differential");
    schema.records.insert(
        RuntimeType::new("document").unwrap(),
        RuntimeRecordSchema::default(),
    );
    let documents = [
        ("a", vec![1.0, 0.0], "red"),
        ("b", vec![0.8, 0.2], "red"),
        ("c", vec![0.0, 1.0], "blue"),
    ];
    let mut mutations = vec![RuntimeMutation::Schema { registry: schema }];
    for (id, _, _) in &documents {
        mutations.push(RuntimeMutation::Record {
            record: RuntimeRecord {
                reference: RuntimeRef::new("document", *id).unwrap(),
                valid_from: 1,
                valid_to: None,
                properties: RuntimeProperties::new(),
            },
        });
    }
    for (id, values, color) in documents {
        let mut properties = RuntimeProperties::new();
        properties.insert("color".into(), RuntimeValue::String(color.into()));
        mutations.push(RuntimeMutation::Vector {
            vector: RuntimeVector {
                reference: RuntimeRef::new("embedding", id).unwrap(),
                subject: RuntimeRef::new("document", id).unwrap(),
                field: "body".into(),
                valid_from: 1,
                valid_to: None,
                value: VectorValue::Dense { values },
                provenance: None,
                properties,
            },
        });
    }
    engine
        .commit_runtime(&RuntimeCommit {
            scope: scope.clone(),
            at: 1,
            actor: "agent:vector-differential".into(),
            expected_cursor: 0,
            mutations,
        })
        .unwrap();
    engine.runtime_read_stamp(scope).unwrap()
}

fn search(engine: &dyn Engine) -> Vec<(String, f64)> {
    let scope = ScopeId::new("instance:vector-differential").unwrap();
    let read = commit(engine, &scope);
    let page = engine.runtime_read_changes(&read, 0, usize::MAX).unwrap();
    let hits = search_changes_exact(
        &SearchRequest {
            scope,
            read,
            valid_at: 2,
            field: "body".into(),
            query: VectorQuery::Dense {
                values: vec![1.0, 0.0],
            },
            metric: ScoreMetric::Cosine,
            embedding_model: None,
            top_k: 3,
            mode: SearchMode::Exact,
            filter: None,
        },
        &page.changes,
    )
    .unwrap();
    hits.into_iter()
        .map(|hit| (hit.reference.id.as_str().to_owned(), hit.score))
        .collect()
}

#[test]
fn exact_search_is_identical_across_memory_fjall_and_native_logs() {
    let memory = search(&MemoryEngine::new());

    let fjall_directory = tempdir().unwrap();
    let fjall = search(&Store::open(fjall_directory.path()).unwrap());

    let native_directory = tempdir().unwrap();
    let native = search(&NativeEngine::open(&native_directory.path().join("native")).unwrap());

    assert_eq!(memory, fjall);
    assert_eq!(memory, native);
    assert_eq!(memory[0].0, "a");
}
