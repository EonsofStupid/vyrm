use rrd_core::{Claim, Predicate, Producer, Subject};
use rrd_store::{Engine, Error, MemoryEngine, NativeEngine, Store};

const DIGEST_A: &str = "3c94150b4ea4f9dcb27d3b602e9f190debe656533c047b99e11367bc6a28017f";
const DIGEST_B: &str = "4c94150b4ea4f9dcb27d3b602e9f190debe656533c047b99e11367bc6a28017f";

fn claim() -> Claim {
    Claim::new(
        Subject::new("request:one").unwrap(),
        Predicate::new("status").unwrap(),
        "accepted",
        10,
        11,
        Producer {
            actor: "agent:server".into(),
            on_behalf_of: None,
            session: Some("session:one".into()),
        },
    )
}

fn assert_contract(engine: &dyn Engine) {
    let first = engine
        .append_batch_idempotent("request-key-1", DIGEST_A, &[claim()])
        .unwrap();
    assert!(!first.idempotent_replay);
    assert_eq!(first.append.first_sequence, 1);
    let replay = engine
        .append_batch_idempotent("request-key-1", DIGEST_A, &[claim()])
        .unwrap();
    assert!(replay.idempotent_replay);
    assert_eq!(replay.append, first.append);
    assert_eq!(engine.sequence().unwrap(), 1);
    assert!(matches!(
        engine.append_batch_idempotent("request-key-1", DIGEST_B, &[claim()]),
        Err(Error::IdempotencyConflict(_))
    ));
    assert_eq!(engine.sequence().unwrap(), 1);
}

#[test]
fn every_engine_enforces_the_same_idempotency_contract() {
    assert_contract(&MemoryEngine::new());
    let native_root = tempfile::tempdir().unwrap();
    assert_contract(&NativeEngine::open(&native_root.path().join("native")).unwrap());
    let fjall_root = tempfile::tempdir().unwrap();
    assert_contract(&Store::open(&fjall_root.path().join("fjall")).unwrap());
}

#[test]
fn accepted_receipt_replays_after_native_and_fjall_restart() {
    let root = tempfile::tempdir().unwrap();
    for (name, native) in [("native", true), ("fjall", false)] {
        let path = root.path().join(name);
        if native {
            let engine = NativeEngine::open(&path).unwrap();
            engine
                .append_batch_idempotent("restart-key", DIGEST_A, &[claim()])
                .unwrap();
            drop(engine);
            let replay = NativeEngine::open(&path)
                .unwrap()
                .append_batch_idempotent("restart-key", DIGEST_A, &[claim()])
                .unwrap();
            assert!(replay.idempotent_replay);
        } else {
            let engine = Store::open(&path).unwrap();
            engine
                .append_batch_idempotent("restart-key", DIGEST_A, &[claim()])
                .unwrap();
            drop(engine);
            let replay = Store::open(&path)
                .unwrap()
                .append_batch_idempotent("restart-key", DIGEST_A, &[claim()])
                .unwrap();
            assert!(replay.idempotent_replay);
        }
    }
}
