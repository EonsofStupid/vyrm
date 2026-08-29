use rrd_core::{
    digest, ProjectionId, ProjectionState, RuntimeCommit, RuntimeMutation, RuntimePropertySchema,
    RuntimeRecord, RuntimeRecordSchema, RuntimeRef, RuntimeSchemaRegistry, RuntimeType,
    RuntimeValue, RuntimeValueType, ScopeId,
};
use rrd_query::{
    bind, execute, plan, Catalog, Error, ExecutionBudget, IndexArtifactPublication,
    IndexCatalogueRepository, IndexDefinition, IndexMutationContext, Parameters,
};
use rrd_query::{parse, Source};
use rrd_store::{Durability, Engine, MemoryEngine, NativeEngine, Store};
use std::collections::BTreeMap;

fn scope() -> ScopeId {
    ScopeId::new("instance:index-test").unwrap()
}

fn seed<E: Engine>(engine: &E) -> Catalog {
    let mut registry = RuntimeSchemaRegistry::empty(1, "index fixture");
    registry.records.insert(
        RuntimeType::new("document").unwrap(),
        RuntimeRecordSchema {
            properties: BTreeMap::from([
                (
                    "status".into(),
                    RuntimePropertySchema::required(RuntimeValueType::String),
                ),
                (
                    "title".into(),
                    RuntimePropertySchema::required(RuntimeValueType::String),
                ),
            ]),
            ..RuntimeRecordSchema::default()
        },
    );
    engine
        .commit_runtime(&RuntimeCommit {
            scope: scope(),
            at: 1,
            actor: "test".into(),
            expected_cursor: 0,
            mutations: vec![
                RuntimeMutation::Schema { registry },
                RuntimeMutation::Record {
                    record: RuntimeRecord {
                        reference: RuntimeRef::new("document", "alpha").unwrap(),
                        valid_from: 10,
                        valid_to: None,
                        properties: BTreeMap::from([
                            ("status".into(), RuntimeValue::String("open".into())),
                            ("title".into(), RuntimeValue::String("Alpha".into())),
                        ]),
                    },
                },
                RuntimeMutation::Record {
                    record: RuntimeRecord {
                        reference: RuntimeRef::new("document", "beta").unwrap(),
                        valid_from: 10,
                        valid_to: None,
                        properties: BTreeMap::from([
                            ("status".into(), RuntimeValue::String("closed".into())),
                            ("title".into(), RuntimeValue::String("Beta".into())),
                        ]),
                    },
                },
            ],
        })
        .unwrap();
    Catalog::capture(engine, &scope()).unwrap()
}

fn context(at: u64, action: &str) -> IndexMutationContext {
    IndexMutationContext {
        at,
        actor: "operator:test".into(),
        request_id: format!("request-{action}"),
        operation_id: format!("operation-{action}"),
    }
}

fn definition() -> IndexDefinition {
    IndexDefinition {
        id: ProjectionId::new("document-status-title").unwrap(),
        source: Source::Record {
            kind: RuntimeType::new("document").unwrap(),
        },
        fields: vec!["status".into(), "title".into()],
        unique: false,
        kind: rrd_query::IndexKind::Scalar,
    }
}

fn exercise<E: Engine>(engine: &E) {
    let query_catalogue = seed(engine);
    let repository = IndexCatalogueRepository::new(engine, scope());
    let created = repository
        .create(&context(2, "create"), &query_catalogue, definition())
        .unwrap();
    let entry = &created.entries[&ProjectionId::new("document-status-title").unwrap()];
    assert_eq!(created.revision, 1);
    assert_eq!(entry.stamp.generation, 1);
    assert_eq!(entry.stamp.state, ProjectionState::Building);
    assert!(!entry.is_usable_at(3, 10));

    let ready = repository
        .build(
            &context(3, "ready-1"),
            &entry.definition.id,
            10,
            &ExecutionBudget::default(),
        )
        .unwrap();
    assert!(ready.entries[&entry.definition.id].is_usable_at(3, 10));
    assert!(!ready.entries[&entry.definition.id].is_usable_at(2, 10));
    assert!(!ready.entries[&entry.definition.id].is_usable_at(3, 11));
    let captured = Catalog::capture(engine, &scope()).unwrap();
    let query = parse(
        "FROM record:document AT VALID 10 KNOWN HEAD WHERE status = \"open\" PROJECT title EXPLAIN CONTRACT",
    )
    .unwrap();
    let physical = plan(&bind(&query, &Parameters::new(), &captured).unwrap()).unwrap();
    let candidate = physical
        .explanation
        .candidates
        .iter()
        .find(|candidate| candidate.name == "index:document-status-title")
        .unwrap();
    assert!(candidate.selected);
    assert!(candidate.exact);
    assert!(candidate.reason.contains("ready and exact at cursor 3"));
    assert!(candidate.reason.contains("matched prefix 1/2"));
    let execution = execute(engine, &physical, &ExecutionBudget::default()).unwrap();
    assert_eq!(execution.scanned_changes, 2);
    assert_eq!(execution.returned_rows, 1);
    assert_eq!(
        execution.batches[0].rows[0].identity,
        "record:document:alpha"
    );

    engine
        .commit_runtime(&RuntimeCommit {
            scope: scope(),
            at: 20,
            actor: "test".into(),
            expected_cursor: 3,
            mutations: vec![RuntimeMutation::Record {
                record: RuntimeRecord {
                    reference: RuntimeRef::new("document", "gamma").unwrap(),
                    valid_from: 20,
                    valid_to: None,
                    properties: BTreeMap::from([
                        ("status".into(), RuntimeValue::String("open".into())),
                        ("title".into(), RuntimeValue::String("Gamma".into())),
                    ]),
                },
            }],
        })
        .unwrap();
    let stale_catalogue = Catalog::capture(engine, &scope()).unwrap();
    let stale_query = parse(
        "FROM record:document AT VALID 20 KNOWN HEAD WHERE status = \"open\" PROJECT title EXPLAIN CONTRACT",
    )
    .unwrap();
    let stale_plan =
        plan(&bind(&stale_query, &Parameters::new(), &stale_catalogue).unwrap()).unwrap();
    assert_eq!(
        stale_plan
            .explanation
            .candidates
            .iter()
            .find(|candidate| candidate.selected)
            .unwrap()
            .name,
        "authoritative_log_scan"
    );
    assert_eq!(
        execute(engine, &stale_plan, &ExecutionBudget::default())
            .unwrap()
            .returned_rows,
        2
    );

    let rebuilding = repository
        .begin_rebuild(&context(4, "rebuild"), &entry.definition.id)
        .unwrap();
    assert_eq!(rebuilding.entries[&entry.definition.id].stamp.generation, 2);
    assert!(!rebuilding.entries[&entry.definition.id].is_usable_at(4, 20));
    assert!(matches!(
        repository.publish_ready(
            &context(5, "stale-ready"),
            &entry.definition.id,
            &IndexArtifactPublication {
                generation: 1,
                source_cursor: 4,
                valid_at: 20,
                artifact_rows: 3,
                artifact_digest: digest::sha256_hex(b"stale"),
            },
        ),
        Err(Error::Catalog(_))
    ));
    let ready = repository
        .build(
            &context(6, "ready-2"),
            &entry.definition.id,
            20,
            &ExecutionBudget::default(),
        )
        .unwrap();
    assert!(ready.entries[&entry.definition.id].is_usable_at(4, 20));
    let historical_catalogue = Catalog::capture(engine, &scope()).unwrap();
    let historical_query = parse(
        "FROM record:document AT VALID 10 KNOWN 3 WHERE status = \"open\" PROJECT title EXPLAIN CONTRACT",
    )
    .unwrap();
    let historical_plan =
        plan(&bind(&historical_query, &Parameters::new(), &historical_catalogue).unwrap()).unwrap();
    assert_eq!(historical_plan.explanation.contract.source_cursor, 3);
    assert_eq!(
        historical_plan
            .explanation
            .candidates
            .iter()
            .find(|candidate| candidate.selected)
            .unwrap()
            .name,
        "authoritative_log_scan"
    );
    let newer_index = historical_plan
        .explanation
        .candidates
        .iter()
        .find(|candidate| candidate.name == "index:document-status-title")
        .unwrap();
    assert!(!newer_index.selected);
    assert!(newer_index.reason.contains("newer-than-requested"));
    assert_eq!(
        execute(engine, &historical_plan, &ExecutionBudget::default())
            .unwrap()
            .returned_rows,
        1
    );
    let quarantined = repository
        .quarantine(&context(7, "quarantine"), &entry.definition.id)
        .unwrap();
    assert_eq!(
        quarantined.entries[&entry.definition.id].stamp.state,
        ProjectionState::Quarantined
    );
    assert!(!quarantined.entries[&entry.definition.id].is_usable_at(4, 20));
    let retired = repository
        .retire(&context(8, "retire"), &entry.definition.id)
        .unwrap();
    assert_eq!(retired.revision, 6);
    assert_eq!(
        retired.entries[&entry.definition.id].stamp.state,
        ProjectionState::Retiring
    );
    retired.validate().unwrap();

    let journal = engine.control_journal_since(0, 32).unwrap();
    assert_eq!(journal.len(), 6);
    assert!(journal.iter().all(|entry| entry.verify()));
    assert_eq!(journal[0].action, "index.created");
    assert_eq!(journal[5].action, "index.retiring");
}

#[test]
fn lifecycle_is_identical_on_every_engine() {
    exercise(&MemoryEngine::new());
    let fjall_root = tempfile::tempdir().unwrap();
    exercise(&Store::open(fjall_root.path()).unwrap());
    let native_root = tempfile::tempdir().unwrap();
    exercise(&NativeEngine::open(&native_root.path().join("native")).unwrap());
}

#[test]
fn native_catalogue_reopens_and_invalid_fields_fail_before_control_state_changes() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("native");
    let index_id = ProjectionId::new("document-status-title").unwrap();
    {
        let engine = NativeEngine::open(&path).unwrap();
        let query_catalogue = seed(&engine);
        let repository = IndexCatalogueRepository::new(&engine, scope());
        let mut invalid = definition();
        invalid.fields = vec!["missing".into()];
        assert!(matches!(
            repository.create(&context(2, "invalid"), &query_catalogue, invalid),
            Err(Error::Binding(_))
        ));
        assert_eq!(repository.load().unwrap().revision, 0);
        repository
            .create(&context(3, "create"), &query_catalogue, definition())
            .unwrap();
        repository
            .build(
                &context(4, "ready"),
                &index_id,
                10,
                &ExecutionBudget::default(),
            )
            .unwrap();
    }
    let reopened = NativeEngine::open(&path).unwrap();
    let catalogue = IndexCatalogueRepository::new(&reopened, scope())
        .load()
        .unwrap();
    assert_eq!(catalogue.revision, 2);
    assert!(catalogue.entries[&index_id].is_usable_at(3, 10));
    let captured = Catalog::capture(&reopened, &scope()).unwrap();
    let query = parse(
        "FROM record:document AT VALID 10 KNOWN HEAD WHERE status = \"open\" PROJECT title EXPLAIN CONTRACT",
    )
    .unwrap();
    let physical = plan(&bind(&query, &Parameters::new(), &captured).unwrap()).unwrap();
    assert_eq!(
        physical
            .explanation
            .candidates
            .iter()
            .find(|candidate| candidate.selected)
            .unwrap()
            .name,
        "index:document-status-title"
    );
    assert_eq!(
        execute(&reopened, &physical, &ExecutionBudget::default())
            .unwrap()
            .returned_rows,
        1
    );
}

#[test]
fn selected_index_fails_closed_when_artifact_bytes_are_corrupted() {
    let engine = MemoryEngine::new();
    let query_catalogue = seed(&engine);
    let index_id = ProjectionId::new("document-status-title").unwrap();
    let repository = IndexCatalogueRepository::new(&engine, scope());
    repository
        .create(&context(2, "create"), &query_catalogue, definition())
        .unwrap();
    let ready = repository
        .build(
            &context(3, "build"),
            &index_id,
            10,
            &ExecutionBudget::default(),
        )
        .unwrap();
    let entry = &ready.entries[&index_id];
    let query = parse(
        "FROM record:document AT VALID 10 KNOWN HEAD WHERE status = \"open\" PROJECT title EXPLAIN CONTRACT",
    )
    .unwrap();
    let captured = Catalog::capture(&engine, &scope()).unwrap();
    let physical = plan(&bind(&query, &Parameters::new(), &captured).unwrap()).unwrap();
    assert_eq!(
        physical
            .explanation
            .candidates
            .iter()
            .find(|candidate| candidate.selected)
            .unwrap()
            .name,
        "index:document-status-title"
    );
    let artifact_name = format!(
        "query-index/{}/{}/{}/{}",
        scope(),
        index_id,
        entry.stamp.generation,
        entry.stamp.artifact_digest
    );
    engine
        .put_projection_with(&artifact_name, b"corrupted", Durability::Authoritative)
        .unwrap();
    assert!(matches!(
        execute(&engine, &physical, &ExecutionBudget::default()),
        Err(Error::Integrity(_))
    ));
}

#[test]
fn operation_receipts_replay_and_reject_idempotency_collisions() {
    let engine = MemoryEngine::new();
    let query_catalogue = seed(&engine);
    let index_id = ProjectionId::new("document-status-title").unwrap();
    let repository = IndexCatalogueRepository::new(&engine, scope());
    repository
        .create(&context(2, "create"), &query_catalogue, definition())
        .unwrap();
    let ready = repository
        .build(
            &context(3, "build"),
            &index_id,
            10,
            &ExecutionBudget::default(),
        )
        .unwrap();
    let entry = ready.entries[&index_id].clone();
    let operation_digest = digest::sha256_hex(b"ensure-document-status-title");
    repository
        .record_operation(
            &context(4, "record"),
            "ensure-key".into(),
            operation_digest.clone(),
            entry.clone(),
        )
        .unwrap();
    assert_eq!(
        repository
            .operation_receipt("ensure-key", &operation_digest)
            .unwrap()
            .unwrap()
            .entry,
        entry
    );
    assert!(matches!(
        repository.operation_receipt("ensure-key", &digest::sha256_hex(b"different")),
        Err(Error::Catalog(_))
    ));
}
