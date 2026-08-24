use std::collections::BTreeMap;
use vyrm_core::{
    digest, ProjectionId, ProjectionState, RuntimeCommit, RuntimeMutation, RuntimePropertySchema,
    RuntimeRecordSchema, RuntimeSchemaRegistry, RuntimeType, RuntimeValueType, ScopeId,
};
use vyrm_mx::{
    bind, plan, Catalog, Error, IndexCatalogueRepository, IndexDefinition, IndexMutationContext,
    Parameters,
};
use vyrm_ql::{parse, Source};
use vyrm_store::{Engine, MemoryEngine, NativeEngine, Store};

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
            mutations: vec![RuntimeMutation::Schema { registry }],
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
    assert!(!entry.is_usable_at(1));

    let ready = repository
        .publish_ready(
            &context(3, "ready-1"),
            &entry.definition.id,
            1,
            1,
            digest::sha256_hex(b"index-artifact-v1"),
        )
        .unwrap();
    assert!(ready.entries[&entry.definition.id].is_usable_at(1));
    assert!(!ready.entries[&entry.definition.id].is_usable_at(2));
    let captured = Catalog::capture(engine, &scope()).unwrap();
    let query = parse(
        "FROM record:document AT VALID 1 KNOWN HEAD WHERE status = \"open\" PROJECT title EXPLAIN CONTRACT",
    )
    .unwrap();
    let physical = plan(&bind(&query, &Parameters::new(), &captured).unwrap()).unwrap();
    let candidate = physical
        .explanation
        .candidates
        .iter()
        .find(|candidate| candidate.name == "index:document-status-title")
        .unwrap();
    assert!(!candidate.selected);
    assert!(!candidate.exact);
    assert!(candidate.reason.contains("ready and fresh at cursor 1"));
    assert!(candidate.reason.contains("matched prefix 1/2"));

    let rebuilding = repository
        .begin_rebuild(&context(4, "rebuild"), &entry.definition.id)
        .unwrap();
    assert_eq!(rebuilding.entries[&entry.definition.id].stamp.generation, 2);
    assert!(!rebuilding.entries[&entry.definition.id].is_usable_at(1));
    assert!(matches!(
        repository.publish_ready(
            &context(5, "stale-ready"),
            &entry.definition.id,
            1,
            1,
            digest::sha256_hex(b"stale"),
        ),
        Err(Error::Catalog(_))
    ));
    let ready = repository
        .publish_ready(
            &context(6, "ready-2"),
            &entry.definition.id,
            2,
            1,
            digest::sha256_hex(b"index-artifact-v2"),
        )
        .unwrap();
    assert!(ready.entries[&entry.definition.id].is_usable_at(1));
    let quarantined = repository
        .quarantine(&context(7, "quarantine"), &entry.definition.id)
        .unwrap();
    assert_eq!(
        quarantined.entries[&entry.definition.id].stamp.state,
        ProjectionState::Quarantined
    );
    assert!(!quarantined.entries[&entry.definition.id].is_usable_at(1));
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
            .publish_ready(
                &context(4, "ready"),
                &index_id,
                1,
                1,
                digest::sha256_hex(b"reopen-artifact"),
            )
            .unwrap();
    }
    let reopened = NativeEngine::open(&path).unwrap();
    let catalogue = IndexCatalogueRepository::new(&reopened, scope())
        .load()
        .unwrap();
    assert_eq!(catalogue.revision, 2);
    assert!(catalogue.entries[&index_id].is_usable_at(1));
}
