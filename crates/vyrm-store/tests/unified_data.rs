use std::collections::{BTreeMap, BTreeSet};

use tempfile::tempdir;
use vyrm_core::{
    Claim, DataTransaction, EmbeddingProvenance, GeoPoint, GeoValue, Predicate, Producer,
    RuntimeCommit, RuntimeEvent, RuntimeEventSchema, RuntimeGeo, RuntimeMutation,
    RuntimeProperties, RuntimeRecord, RuntimeRecordSchema, RuntimeRef, RuntimeRelation,
    RuntimeRelationSchema, RuntimeSchemaRegistry, RuntimeSeriesSample, RuntimeType, RuntimeVector,
    ScopeId, SeriesValue, Subject, VectorNormalization, VectorValue,
};
use vyrm_store::{
    DataRuntime, DataRuntimeStep, Engine, Error, LocalObjectStore, MemoryEngine, NativeEngine,
    Store,
};

fn schema() -> RuntimeSchemaRegistry {
    let mut registry = RuntimeSchemaRegistry::empty(1, "unified data test schema");
    registry.records.insert(
        RuntimeType::new("entity").unwrap(),
        RuntimeRecordSchema::default(),
    );
    registry.relations.insert(
        RuntimeType::new("links").unwrap(),
        RuntimeRelationSchema {
            from: BTreeSet::from([RuntimeType::new("entity").unwrap()]),
            to: BTreeSet::from([RuntimeType::new("entity").unwrap()]),
            ..RuntimeRelationSchema::default()
        },
    );
    registry.events.insert(
        RuntimeType::new("observed").unwrap(),
        RuntimeEventSchema {
            subject_required: true,
            subject_types: BTreeSet::from([RuntimeType::new("entity").unwrap()]),
            properties: BTreeMap::new(),
            allow_additional_properties: false,
        },
    );
    registry
}

fn record(id: &str) -> RuntimeRecord {
    RuntimeRecord {
        reference: RuntimeRef::new("entity", id).unwrap(),
        valid_from: 100,
        valid_to: None,
        properties: RuntimeProperties::new(),
    }
}

fn bootstrap(engine: &dyn Engine, scope: &ScopeId) -> u64 {
    engine
        .commit_runtime(&RuntimeCommit {
            scope: scope.clone(),
            at: 100,
            actor: "agent:m4-test".into(),
            expected_cursor: 0,
            mutations: vec![
                RuntimeMutation::Schema { registry: schema() },
                RuntimeMutation::Record {
                    record: record("a"),
                },
                RuntimeMutation::Record {
                    record: record("b"),
                },
            ],
        })
        .unwrap()
        .last_cursor
}

fn unified_mutations(object: vyrm_core::ObjectReference) -> Vec<RuntimeMutation> {
    let subject = RuntimeRef::new("entity", "a").unwrap();
    vec![
        RuntimeMutation::Claim {
            claim: Claim::new(
                Subject::new("entity:a").unwrap(),
                Predicate::new("status").unwrap(),
                "ready",
                101,
                101,
                Producer {
                    actor: "agent:m4-test".into(),
                    on_behalf_of: None,
                    session: Some("m4".into()),
                },
            ),
        },
        RuntimeMutation::Record {
            record: record("c"),
        },
        RuntimeMutation::Relation {
            relation: RuntimeRelation {
                reference: RuntimeRef::new("links", "a-b").unwrap(),
                from: subject.clone(),
                to: RuntimeRef::new("entity", "b").unwrap(),
                valid_from: 101,
                valid_to: None,
                properties: RuntimeProperties::new(),
            },
        },
        RuntimeMutation::Event {
            event: RuntimeEvent {
                kind: RuntimeType::new("observed").unwrap(),
                subject: Some(subject.clone()),
                properties: RuntimeProperties::new(),
            },
        },
        RuntimeMutation::Vector {
            vector: RuntimeVector {
                reference: RuntimeRef::new("embedding", "a-title").unwrap(),
                subject: subject.clone(),
                field: "title".into(),
                valid_from: 101,
                valid_to: None,
                value: VectorValue::Dense {
                    values: vec![0.0, 0.6, 0.8],
                },
                provenance: Some(EmbeddingProvenance {
                    source_digest: "11".repeat(32),
                    model: "fixture-embedder".into(),
                    model_digest: "22".repeat(32),
                    dimensions: 3,
                    normalization: VectorNormalization::UnitL2,
                    generation_parameters: RuntimeProperties::new(),
                }),
                properties: RuntimeProperties::new(),
            },
        },
        RuntimeMutation::SeriesSample {
            sample: RuntimeSeriesSample {
                reference: RuntimeRef::new("sample", "a-temperature-101").unwrap(),
                series: subject.clone(),
                observed_at: 101,
                value: SeriesValue::Decimal("21.5".into()),
                properties: RuntimeProperties::new(),
            },
        },
        RuntimeMutation::Geo {
            geo: RuntimeGeo {
                reference: RuntimeRef::new("location", "a-current").unwrap(),
                subject: subject.clone(),
                field: "position".into(),
                valid_from: 101,
                valid_to: None,
                value: GeoValue::Point {
                    point: GeoPoint {
                        longitude: -122.4194,
                        latitude: 37.7749,
                    },
                },
                properties: RuntimeProperties::new(),
            },
        },
        RuntimeMutation::Object { object },
    ]
}

fn exercise_unified_commit<E: Engine>(engine: E) {
    let object_directory = tempdir().unwrap();
    let runtime = DataRuntime::new(
        engine,
        LocalObjectStore::open(object_directory.path()).unwrap(),
    );
    let scope = ScopeId::new("instance:m4").unwrap();
    let cursor = bootstrap(runtime.engine(), &scope);
    let object = runtime
        .stage_object(
            "a-source",
            Some(RuntimeRef::new("entity", "a").unwrap()),
            "text/plain",
            b"source bytes",
        )
        .unwrap();
    let read = runtime.engine().runtime_read_stamp(&scope).unwrap();
    let transaction = DataTransaction::new(
        read,
        RuntimeCommit {
            scope: scope.clone(),
            at: 101,
            actor: "agent:m4-test".into(),
            expected_cursor: cursor,
            mutations: unified_mutations(object.clone()),
        },
    )
    .unwrap();

    let outcome = runtime.commit(&transaction).unwrap();
    assert_eq!(outcome.count, 8);
    assert_eq!(outcome.outbox_count, 7);
    assert_eq!(runtime.objects().get(&object).unwrap(), b"source bytes");
    let work = runtime.engine().runtime_outbox_since(cursor, 20).unwrap();
    assert_eq!(work.len(), 7);
    assert!(work.iter().all(|entry| entry.validate().is_ok()));
    let audit = runtime
        .engine()
        .runtime_audit(&outcome.commit_id)
        .unwrap()
        .unwrap();
    audit.validate().unwrap();
    assert_eq!(audit.outcome_cursor, Some(outcome.last_cursor));

    // An acknowledgement lost after durability is safe to retry by content id.
    let retried = runtime.commit(&transaction).unwrap();
    assert_eq!(retried, outcome);
    assert_eq!(
        runtime.engine().runtime_cursor().unwrap(),
        outcome.last_cursor
    );
}

#[test]
fn unified_transaction_and_evidence_match_across_all_engines() {
    exercise_unified_commit(MemoryEngine::new());

    let fjall_directory = tempdir().unwrap();
    exercise_unified_commit(Store::open(fjall_directory.path()).unwrap());

    let native_directory = tempdir().unwrap();
    let native_path = native_directory.path().join("native");
    exercise_unified_commit(NativeEngine::open(&native_path).unwrap());
}

#[test]
fn dangling_late_family_rolls_back_every_earlier_family() {
    let directory = tempdir().unwrap();
    let engine = NativeEngine::open(&directory.path().join("native")).unwrap();
    let scope = ScopeId::new("instance:rollback").unwrap();
    let cursor = bootstrap(&engine, &scope);
    let commit = RuntimeCommit {
        scope,
        at: 101,
        actor: "agent:m4-test".into(),
        expected_cursor: cursor,
        mutations: vec![
            RuntimeMutation::Record {
                record: record("would-leak"),
            },
            RuntimeMutation::Vector {
                vector: RuntimeVector {
                    reference: RuntimeRef::new("embedding", "dangling").unwrap(),
                    subject: RuntimeRef::new("entity", "missing").unwrap(),
                    field: "body".into(),
                    valid_from: 101,
                    valid_to: None,
                    value: VectorValue::Dense { values: vec![1.0] },
                    provenance: None,
                    properties: RuntimeProperties::new(),
                },
            },
        ],
    };
    assert!(matches!(
        engine.commit_runtime(&commit),
        Err(Error::DanglingRuntimeReference(_))
    ));
    assert_eq!(engine.runtime_cursor().unwrap(), cursor);
    assert!(engine.runtime_outbox_since(cursor, 10).unwrap().is_empty());
    assert!(engine.runtime_audit(&commit.digest()).unwrap().is_none());
}

#[test]
fn object_and_commit_failure_boundaries_are_recoverable() {
    let directory = tempdir().unwrap();
    let engine = NativeEngine::open(&directory.path().join("native")).unwrap();
    let objects = LocalObjectStore::open(directory.path().join("objects")).unwrap();
    let runtime = DataRuntime::new(engine, objects);
    let scope = ScopeId::new("instance:faults").unwrap();
    let cursor = bootstrap(runtime.engine(), &scope);
    let object = runtime
        .stage_object("orphan", None, "application/octet-stream", b"orphan")
        .unwrap();
    let transaction = DataTransaction::new(
        runtime.engine().runtime_read_stamp(&scope).unwrap(),
        RuntimeCommit {
            scope,
            at: 101,
            actor: "agent:m4-test".into(),
            expected_cursor: cursor,
            mutations: vec![RuntimeMutation::Object {
                object: object.clone(),
            }],
        },
    )
    .unwrap();

    let before = runtime.commit_with_hook(&transaction, |step| {
        if step == DataRuntimeStep::BeforeCommit {
            Err(Error::FaultInjected("before_commit"))
        } else {
            Ok(())
        }
    });
    assert!(matches!(before, Err(Error::FaultInjected("before_commit"))));
    assert_eq!(runtime.engine().runtime_cursor().unwrap(), cursor);
    assert_eq!(
        runtime
            .objects()
            .inventory(&BTreeSet::new())
            .unwrap()
            .entries[0]
            .state,
        vyrm_store::ObjectInventoryState::Orphan
    );

    let after = runtime.commit_with_hook(&transaction, |step| {
        if step == DataRuntimeStep::AfterCommit {
            Err(Error::FaultInjected("after_commit"))
        } else {
            Ok(())
        }
    });
    assert!(matches!(after, Err(Error::FaultInjected("after_commit"))));
    let retried = runtime.commit(&transaction).unwrap();
    assert_eq!(
        runtime.engine().runtime_cursor().unwrap(),
        retried.last_cursor
    );
    assert_eq!(retried.last_cursor, cursor + 1);
}

#[test]
fn native_unified_evidence_survives_reopen_and_retry() {
    let directory = tempdir().unwrap();
    let database_path = directory.path().join("native");
    let object_path = directory.path().join("objects");
    let scope = ScopeId::new("instance:reopen").unwrap();
    let transaction;
    let outcome;
    {
        let runtime = DataRuntime::new(
            NativeEngine::open(&database_path).unwrap(),
            LocalObjectStore::open(&object_path).unwrap(),
        );
        let cursor = bootstrap(runtime.engine(), &scope);
        let object = runtime
            .stage_object(
                "reopen-source",
                Some(RuntimeRef::new("entity", "a").unwrap()),
                "text/plain",
                b"persistent source",
            )
            .unwrap();
        transaction = DataTransaction::new(
            runtime.engine().runtime_read_stamp(&scope).unwrap(),
            RuntimeCommit {
                scope: scope.clone(),
                at: 101,
                actor: "agent:m4-test".into(),
                expected_cursor: cursor,
                mutations: unified_mutations(object),
            },
        )
        .unwrap();
        outcome = runtime.commit(&transaction).unwrap();
        runtime.engine().flush(102).unwrap();
    }

    let reopened = DataRuntime::new(
        NativeEngine::open(&database_path).unwrap(),
        LocalObjectStore::open(&object_path).unwrap(),
    );
    assert_eq!(reopened.commit(&transaction).unwrap(), outcome);
    assert_eq!(
        reopened
            .engine()
            .runtime_outbox_since(transaction.read.commit_cursor, 20)
            .unwrap()
            .len(),
        7
    );
    assert!(reopened
        .engine()
        .runtime_audit(&outcome.commit_id)
        .unwrap()
        .is_some());
}
