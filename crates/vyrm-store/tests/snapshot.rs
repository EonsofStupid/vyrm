use std::collections::BTreeMap;
use std::sync::{Arc, Barrier};
use std::thread;

use vyrm_core::{
    DataTransaction, ReadStamp, RuntimeCommit, RuntimeEvent, RuntimeEventSchema,
    RuntimeGraphSnapshot, RuntimeMutation, RuntimeProperties, RuntimeRecord, RuntimeRecordSchema,
    RuntimeRef, RuntimeSchemaRegistry, RuntimeType, ScopeId,
};
use vyrm_store::{Engine, Error, MemoryEngine, NativeEngine, Store};

fn schema() -> RuntimeSchemaRegistry {
    let mut registry = RuntimeSchemaRegistry::empty(1, "snapshot test schema");
    registry.events.insert(
        RuntimeType::new("pulse").unwrap(),
        RuntimeEventSchema {
            subject_required: false,
            subject_types: Default::default(),
            properties: BTreeMap::new(),
            allow_additional_properties: false,
        },
    );
    registry.records.insert(
        RuntimeType::new("item").unwrap(),
        RuntimeRecordSchema::default(),
    );
    registry
}

fn bootstrap(scope: &ScopeId, expected_cursor: u64) -> RuntimeCommit {
    RuntimeCommit {
        scope: scope.clone(),
        at: 100,
        actor: "agent:snapshot-test".into(),
        expected_cursor,
        mutations: vec![RuntimeMutation::Schema { registry: schema() }],
    }
}

fn pulse(scope: &ScopeId, expected_cursor: u64) -> RuntimeCommit {
    RuntimeCommit {
        scope: scope.clone(),
        at: 101,
        actor: "agent:snapshot-test".into(),
        expected_cursor,
        mutations: vec![RuntimeMutation::Event {
            event: RuntimeEvent {
                kind: RuntimeType::new("pulse").unwrap(),
                subject: None,
                properties: RuntimeProperties::new(),
            },
        }],
    }
}

fn item(scope: &ScopeId, expected_cursor: u64, id: &str) -> RuntimeCommit {
    RuntimeCommit {
        scope: scope.clone(),
        at: 102,
        actor: "agent:snapshot-test".into(),
        expected_cursor,
        mutations: vec![RuntimeMutation::Record {
            record: RuntimeRecord {
                reference: RuntimeRef::new("item", id).unwrap(),
                valid_from: 102,
                valid_to: None,
                properties: RuntimeProperties::new(),
            },
        }],
    }
}

fn assert_snapshot_contract(engine: &dyn Engine) {
    let scope = ScopeId::new("instance:snapshot").unwrap();
    engine.commit_runtime(&bootstrap(&scope, 0)).unwrap();

    let read = engine.runtime_read_stamp(&scope).unwrap();
    assert_eq!(read.schema_revision, Some(1));
    assert_eq!(read.commit_cursor, 1);
    assert!(read.head_digest.is_some());
    read.validate().unwrap();

    let snapshot = engine
        .open_runtime_snapshot(&scope, "agent:reader", 1_000, 100)
        .unwrap();
    assert_eq!(snapshot.read, read);
    engine.commit_runtime(&pulse(&scope, 1)).unwrap();

    let frozen = engine
        .runtime_snapshot_changes(&snapshot, 0, 10, 1_050)
        .unwrap();
    assert_eq!(frozen.head_cursor, 1);
    assert_eq!(frozen.through_cursor, 1);
    assert_eq!(frozen.changes.len(), 1);
    assert!(!frozen.has_more());

    let live = engine.runtime_changes_since(0, 10, Some(&scope)).unwrap();
    assert_eq!(live.head_cursor, 2);
    assert_eq!(live.changes.len(), 2);
    assert_eq!(
        engine.runtime_snapshots(1_050).unwrap(),
        vec![snapshot.clone()]
    );
    let pins = engine.runtime_retention_pins(1_050).unwrap();
    assert_eq!(pins.len(), 1);
    assert_eq!(pins[0].snapshot_id, snapshot.id);
    assert_eq!(pins[0].manifest_id, snapshot.read.manifest_id);
    assert_eq!(pins[0].minimum_cursor, snapshot.read.commit_cursor);
    assert!(engine.runtime_snapshots(1_100).unwrap().is_empty());
    assert!(engine.runtime_retention_pins(1_100).unwrap().is_empty());
    assert!(matches!(
        engine.runtime_snapshot_changes(&snapshot, 0, 10, 1_100),
        Err(Error::SnapshotExpired {
            expired_at: 1_100,
            ..
        })
    ));
    assert!(engine.release_runtime_snapshot(&snapshot.id).unwrap());
    assert!(!engine.release_runtime_snapshot(&snapshot.id).unwrap());
    assert!(matches!(
        engine.runtime_snapshot_changes(&snapshot, 0, 10, 1_050),
        Err(Error::SnapshotNotFound(_))
    ));
}

#[test]
fn all_engines_enforce_identical_snapshot_semantics() {
    let dir = tempfile::tempdir().unwrap();
    let fjall = Store::open(dir.path()).unwrap();
    let native_dir = tempfile::tempdir().unwrap();
    let native = NativeEngine::open(&native_dir.path().join("native")).unwrap();
    let memory = MemoryEngine::new();
    assert_snapshot_contract(&fjall);
    assert_snapshot_contract(&native);
    assert_snapshot_contract(&memory);
}

#[test]
fn fjall_snapshot_catalog_survives_restart() {
    let dir = tempfile::tempdir().unwrap();
    let scope = ScopeId::new("instance:restart").unwrap();
    let handle = {
        let store = Store::open(dir.path()).unwrap();
        store.commit_runtime(&bootstrap(&scope, 0)).unwrap();
        store
            .open_runtime_snapshot(&scope, "agent:restart", 10, 100)
            .unwrap()
    };

    let reopened = Store::open(dir.path()).unwrap();
    assert_eq!(
        reopened.runtime_snapshots(20).unwrap(),
        vec![handle.clone()]
    );
    let pins = reopened.runtime_retention_pins(20).unwrap();
    assert_eq!(pins.len(), 1);
    assert_eq!(pins[0].snapshot_id, handle.id);
    let page = reopened
        .runtime_snapshot_changes(&handle, 0, 10, 20)
        .unwrap();
    assert_eq!(page.head_cursor, 1);
    assert_eq!(page.changes.len(), 1);
}

#[test]
fn native_snapshot_catalog_survives_flush_and_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("native");
    let scope = ScopeId::new("instance:native-restart").unwrap();
    let handle = {
        let store = NativeEngine::open(&path).unwrap();
        store.commit_runtime(&bootstrap(&scope, 0)).unwrap();
        let handle = store
            .open_runtime_snapshot(&scope, "agent:restart", 10, 100)
            .unwrap();
        store.flush(15).unwrap();
        handle
    };
    let reopened = NativeEngine::open(&path).unwrap();
    assert_eq!(reopened.runtime_snapshots(20).unwrap(), vec![handle.clone()]);
    let page = reopened
        .runtime_snapshot_changes(&handle, 0, 10, 20)
        .unwrap();
    assert_eq!(page.head_cursor, 1);
    assert_eq!(page.changes.len(), 1);
}

fn assert_data_transaction_contract(engine: &dyn Engine) {
    let scope = ScopeId::new("instance:transaction").unwrap();
    let read = engine.runtime_read_stamp(&scope).unwrap();
    let transaction = DataTransaction::new(read.clone(), bootstrap(&scope, 0)).unwrap();
    let digest = transaction.digest();
    assert_eq!(digest.len(), 64);
    let outcome = engine.commit_data_transaction(&transaction).unwrap();
    assert_eq!(outcome.last_cursor, 1);

    let stale = DataTransaction::new(read, bootstrap(&scope, 0)).unwrap();
    assert!(matches!(
        engine.commit_data_transaction(&stale),
        Err(Error::RuntimeConflict {
            expected: 0,
            actual: 1
        })
    ));

    let read = engine.runtime_read_stamp(&scope).unwrap();
    let pending = DataTransaction::new(read.clone(), pulse(&scope, 1)).unwrap();
    let view = engine.preview_data_transaction(&pending, 101).unwrap();
    assert_eq!(view.read, read);
    assert_eq!(view.prospective_cursor, 2);
    assert_eq!(view.records.len(), 1);
    assert_eq!(view.records[0].reference.kind.as_str(), "pulse");
    assert_eq!(view.events().count(), 1);
    assert_eq!(engine.runtime_cursor().unwrap(), 1);

    engine.commit_data_transaction(&pending).unwrap();
    let committed = engine
        .runtime_read_changes(&engine.runtime_read_stamp(&scope).unwrap(), 0, 10)
        .unwrap();
    let graph = RuntimeGraphSnapshot::from_changes(&committed.changes, scope.clone(), 101, 2);
    assert_eq!(view.records, graph.records);
    assert_eq!(view.relations, graph.relations);

    let wrong_schema = ReadStamp::new(
        scope.clone(),
        None,
        0,
        2,
        engine.runtime_read_stamp(&scope).unwrap().head_digest,
    )
    .unwrap();
    assert!(matches!(
        engine.runtime_read_changes(&wrong_schema, 0, 10),
        Err(Error::ReadStampMismatch(_))
    ));
    let unavailable = ReadStamp::new(scope, Some(1), 0, 99, Some("66".repeat(32))).unwrap();
    assert!(matches!(
        engine.runtime_read_changes(&unavailable, 0, 10),
        Err(Error::ReadStampUnavailable(_))
    ));
}

#[test]
fn data_transactions_bind_writes_to_their_read_state_on_all_engines() {
    let dir = tempfile::tempdir().unwrap();
    assert_data_transaction_contract(&Store::open(dir.path()).unwrap());
    let native_dir = tempfile::tempdir().unwrap();
    assert_data_transaction_contract(
        &NativeEngine::open(&native_dir.path().join("native")).unwrap(),
    );
    assert_data_transaction_contract(&MemoryEngine::new());
}

fn assert_concurrent_compare_and_swap<E>(engine: Arc<E>)
where
    E: Engine + Send + Sync + 'static,
{
    let scope = ScopeId::new("instance:race").unwrap();
    engine.commit_runtime(&bootstrap(&scope, 0)).unwrap();

    let run_pair = |left: DataTransaction, right: DataTransaction| {
        let barrier = Arc::new(Barrier::new(3));
        let mut workers = Vec::new();
        for transaction in [left, right] {
            let engine = Arc::clone(&engine);
            let barrier = Arc::clone(&barrier);
            workers.push(thread::spawn(move || {
                barrier.wait();
                engine.commit_data_transaction(&transaction)
            }));
        }
        barrier.wait();
        workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>()
    };

    let read = engine.runtime_read_stamp(&scope).unwrap();
    let same = run_pair(
        DataTransaction::new(read.clone(), item(&scope, 1, "shared")).unwrap(),
        DataTransaction::new(read, item(&scope, 1, "shared")).unwrap(),
    );
    assert_eq!(same.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        same.iter()
            .filter(|result| matches!(result, Err(Error::RuntimeConflict { .. })))
            .count(),
        1
    );

    let read = engine.runtime_read_stamp(&scope).unwrap();
    let disjoint = run_pair(
        DataTransaction::new(read.clone(), item(&scope, 2, "left")).unwrap(),
        DataTransaction::new(read, item(&scope, 2, "right")).unwrap(),
    );
    assert_eq!(disjoint.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        disjoint
            .iter()
            .filter(|result| matches!(result, Err(Error::RuntimeConflict { .. })))
            .count(),
        1,
        "M1 deliberately uses global serializable CAS even for disjoint identities"
    );
    assert_eq!(engine.runtime_cursor().unwrap(), 3);
}

#[test]
fn concurrent_transactions_never_lose_an_update() {
    let dir = tempfile::tempdir().unwrap();
    assert_concurrent_compare_and_swap(Arc::new(Store::open(dir.path()).unwrap()));
    let native_dir = tempfile::tempdir().unwrap();
    assert_concurrent_compare_and_swap(Arc::new(
        NativeEngine::open(&native_dir.path().join("native")).unwrap(),
    ));
    assert_concurrent_compare_and_swap(Arc::new(MemoryEngine::new()));
}

#[test]
fn deterministic_mixed_scope_trace_is_identical_across_backends() {
    let dir = tempfile::tempdir().unwrap();
    let fjall = Store::open(dir.path()).unwrap();
    let native_dir = tempfile::tempdir().unwrap();
    let native = NativeEngine::open(&native_dir.path().join("native")).unwrap();
    let memory = MemoryEngine::new();
    let scopes = [
        ScopeId::new("instance:trace-a").unwrap(),
        ScopeId::new("instance:trace-b").unwrap(),
    ];

    for scope in &scopes {
        let cursor = fjall.runtime_cursor().unwrap();
        let commit = bootstrap(scope, cursor);
        assert_eq!(
            fjall.commit_runtime(&commit).unwrap().commit_id,
            memory.commit_runtime(&commit).unwrap().commit_id
        );
        assert_eq!(
            fjall.runtime_cursor().unwrap(),
            native.commit_runtime(&commit).unwrap().last_cursor
        );
    }

    let mut state = 0x5eed_u64;
    let mut frozen = None;
    for step in 0..64 {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        let scope = &scopes[(state >> 63) as usize];
        let cursor = fjall.runtime_cursor().unwrap();
        let commit = pulse(scope, cursor);
        let left = fjall.commit_runtime(&commit).unwrap();
        let right = memory.commit_runtime(&commit).unwrap();
        let native_outcome = native.commit_runtime(&commit).unwrap();
        assert_eq!(left.commit_id, right.commit_id);
        assert_eq!(left.commit_id, native_outcome.commit_id);
        assert_eq!(left.last_cursor, right.last_cursor);
        assert_eq!(left.last_cursor, native_outcome.last_cursor);

        if step == 15 {
            let left = fjall
                .open_runtime_snapshot(&scopes[0], "agent:trace", 10_000, 1_000)
                .unwrap();
            let right = memory
                .open_runtime_snapshot(&scopes[0], "agent:trace", 10_000, 1_000)
                .unwrap();
            let native_handle = native
                .open_runtime_snapshot(&scopes[0], "agent:trace", 10_000, 1_000)
                .unwrap();
            assert_eq!(left, right);
            assert_eq!(left, native_handle);
            frozen = Some(left);
        }
        if step % 7 == 0 {
            let after = cursor.saturating_sub(3);
            assert_eq!(
                fjall.runtime_changes_since(after, 4, Some(scope)).unwrap(),
                memory.runtime_changes_since(after, 4, Some(scope)).unwrap()
            );
            assert_eq!(
                fjall.runtime_changes_since(after, 4, Some(scope)).unwrap(),
                native.runtime_changes_since(after, 4, Some(scope)).unwrap()
            );
            assert_eq!(
                fjall.runtime_read_stamp(scope).unwrap(),
                memory.runtime_read_stamp(scope).unwrap()
            );
            assert_eq!(
                fjall.runtime_read_stamp(scope).unwrap(),
                native.runtime_read_stamp(scope).unwrap()
            );
        }
    }

    let frozen = frozen.unwrap();
    assert_eq!(
        fjall
            .runtime_snapshot_changes(&frozen, 0, 128, 10_500)
            .unwrap(),
        memory
            .runtime_snapshot_changes(&frozen, 0, 128, 10_500)
            .unwrap()
    );
    assert_eq!(
        fjall
            .runtime_snapshot_changes(&frozen, 0, 128, 10_500)
            .unwrap(),
        native
            .runtime_snapshot_changes(&frozen, 0, 128, 10_500)
            .unwrap()
    );
    let mut after = 0;
    let mut replayed = Vec::new();
    loop {
        let left = fjall
            .runtime_snapshot_changes(&frozen, after, 3, 10_500)
            .unwrap();
        let right = memory
            .runtime_snapshot_changes(&frozen, after, 3, 10_500)
            .unwrap();
        let native_page = native
            .runtime_snapshot_changes(&frozen, after, 3, 10_500)
            .unwrap();
        assert_eq!(left, right);
        assert_eq!(left, native_page);
        let has_more = left.has_more();
        let through = left.through_cursor;
        replayed.extend(left.changes);
        if !has_more {
            assert_eq!(through, frozen.read.commit_cursor);
            break;
        }
        assert!(through > after);
        after = through;
    }
    let one_page = fjall
        .runtime_snapshot_changes(&frozen, 0, 128, 10_500)
        .unwrap();
    assert_eq!(replayed, one_page.changes);
}
