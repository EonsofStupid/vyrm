//! The storage port, proven by differential (`PLAN.md` Step S, standing
//! rule 3): the Fjall engine and the reference engine must be
//! indistinguishable through the trait — same recall, same projection, same
//! grounding stamp digest. This test is what makes "fold in storage" a
//! contract rather than a promise: a bbolt engine in Go, or a vyrm-native
//! engine in Rust, is correct exactly when this differential (and the
//! golden key vectors in vyrm-core) holds for it.

use vyrm_core::{recall, Claim, ClaimReader, Predicate, Producer, RecallQuery, Subject};
use vyrm_store::{Engine, GroundingReport, MemoryEngine, NativeEngine, Store};

fn claim(subject: &str, predicate: &str, object: &str, from: u64) -> Claim {
    Claim::new(
        Subject::new(subject).unwrap(),
        Predicate::new(predicate).unwrap(),
        object,
        from,
        from,
        Producer { actor: "test".into(), on_behalf_of: None, session: None },
    )
}

fn corpus() -> Vec<Claim> {
    vec![
        claim("wp3", "status", "planned", 100),
        claim("wp3", "status", "active", 200),
        claim("wp3", "owner", "ada", 150),
        claim("wp4", "status", "planned", 120),
        claim("wp3", "status", "done", 300),
        claim("wp4", "owner", "lin", 180),
    ]
}

#[test]
fn compatibility_physical_maintenance_preserves_exact_semantics_across_reopen() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("fjall");
    let store = Store::open(&root).unwrap();
    Engine::append_batch(&store, &corpus()).unwrap();
    let before = Engine::claims_in_range(&store, 0, corpus().len() as u64).unwrap();
    store.compact_physical().unwrap();
    assert_eq!(
        Engine::claims_in_range(&store, 0, corpus().len() as u64).unwrap(),
        before
    );
    drop(store);

    let reopened = Store::open(&root).unwrap();
    assert_eq!(
        Engine::claims_in_range(&reopened, 0, corpus().len() as u64).unwrap(),
        before
    );
}

#[test]
fn all_engines_are_indistinguishable_through_the_port() {
    let dir = tempfile::tempdir().unwrap();
    let fjall = Store::open(dir.path()).unwrap();
    let native_dir = tempfile::tempdir().unwrap();
    let native = NativeEngine::open(&native_dir.path().join("native")).unwrap();
    let memory = MemoryEngine::new();

    for engine in [
        &fjall as &dyn AnyEngine,
        &native as &dyn AnyEngine,
        &memory as &dyn AnyEngine,
    ] {
        engine.load(&corpus());
    }

    // Same sequence, same interval replay, same subjects.
    assert_eq!(Engine::sequence(&fjall).unwrap(), Engine::sequence(&memory).unwrap());
    assert_eq!(Engine::sequence(&fjall).unwrap(), Engine::sequence(&native).unwrap());
    assert_eq!(
        Engine::claims_in_range(&fjall, 2, 5).unwrap(),
        Engine::claims_in_range(&memory, 2, 5).unwrap()
    );
    assert_eq!(
        Engine::claims_in_range(&fjall, 2, 5).unwrap(),
        Engine::claims_in_range(&native, 2, 5).unwrap()
    );
    assert_eq!(Engine::subjects(&fjall).unwrap(), Engine::subjects(&memory).unwrap());
    assert_eq!(Engine::subjects(&fjall).unwrap(), Engine::subjects(&native).unwrap());

    // Same recall set, digest included.
    let query = RecallQuery {
        subjects: vec![Subject::new("wp3").unwrap(), Subject::new("wp4").unwrap()],
        predicates: None,
        as_of: 400,
    };
    let a = recall(&fjall, &query, 10_000).unwrap();
    let b = recall(&memory, &query, 10_000).unwrap();
    let c = recall(&native, &query, 10_000).unwrap();
    assert_eq!(a.claims, b.claims);
    assert_eq!(a.claims, c.claims);
    assert_eq!(a.digest, b.digest, "the content digest is engine-independent");
    assert_eq!(a.digest, c.digest, "the native digest is engine-independent");

    // Same projection after rebuild, and the SAME grounding digest — the
    // stamp names content, not the engine that computed it.
    let ga = match (fjall.rebuild_current().unwrap(), fjall.ground_current(500).unwrap()) {
        (_, GroundingReport::Grounded(stamp)) => stamp,
        (_, GroundingReport::Divergence { differences }) => panic!("fjall diverged: {differences:?}"),
    };
    let gb = match (memory.rebuild_current().unwrap(), memory.ground_current(500).unwrap()) {
        (_, GroundingReport::Grounded(stamp)) => stamp,
        (_, GroundingReport::Divergence { differences }) => panic!("memory diverged: {differences:?}"),
    };
    let gc = match (native.rebuild_current().unwrap(), native.ground_current(500).unwrap()) {
        (_, GroundingReport::Grounded(stamp)) => stamp,
        (_, GroundingReport::Divergence { differences }) => panic!("native diverged: {differences:?}"),
    };
    assert_eq!(ga.sequence, gb.sequence);
    assert_eq!(ga.sequence, gc.sequence);
    assert_eq!(ga.digest, gb.digest, "grounding digests agree across engines");
    assert_eq!(ga.digest, gc.digest, "native grounding digest agrees");

    // The quarantine semantics ride the trait too: corrupt the reference
    // engine's stored blob and the provided ground_current halts it.
    let bytes = Engine::get_projection(&memory, vyrm_store::CURRENT_PROJECTION).unwrap().unwrap();
    let corrupted = String::from_utf8(bytes).unwrap().replacen("\"done\"", "\"drifted\"", 1);
    Engine::put_projection(&memory, vyrm_store::CURRENT_PROJECTION, corrupted.as_bytes()).unwrap();
    assert!(matches!(
        memory.ground_current(600).unwrap(),
        GroundingReport::Divergence { .. }
    ));
    assert!(matches!(
        memory.rebuild_current(),
        Err(vyrm_store::Error::Quarantined(_))
    ));
}

#[test]
fn a_rejected_batch_is_atomic_in_all_engines() {
    let dir = tempfile::tempdir().unwrap();
    let fjall = Store::open(dir.path()).unwrap();
    let native_dir = tempfile::tempdir().unwrap();
    let native = NativeEngine::open(&native_dir.path().join("native")).unwrap();
    let memory = MemoryEngine::new();
    let valid = claim("wp3", "status", "valid", 100);
    let mut invalid = claim("wp4", "status", "invalid", 200);
    invalid.valid_to = Some(200);
    assert!(Engine::append_batch(&fjall, &[valid.clone(), invalid.clone()]).is_err());
    assert!(Engine::append_batch(&memory, &[valid, invalid]).is_err());
    let valid = claim("wp3", "status", "valid", 100);
    let mut invalid = claim("wp4", "status", "invalid", 200);
    invalid.valid_to = Some(200);
    assert!(Engine::append_batch(&native, &[valid, invalid]).is_err());
    assert_eq!(Engine::sequence(&fjall).unwrap(), 0);
    assert_eq!(Engine::sequence(&memory).unwrap(), 0);
    assert_eq!(Engine::sequence(&native).unwrap(), 0);
    assert!(Engine::subjects(&fjall).unwrap().is_empty());
    assert!(Engine::subjects(&memory).unwrap().is_empty());
    assert!(Engine::subjects(&native).unwrap().is_empty());
}

#[test]
fn assert_retires_the_previous_claim_atomically() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    let first = claim("wp3", "status", "failing", 100);
    let mut second = claim("wp3", "status", "passing", 200);
    second.tx_time = 250;
    Engine::assert(&store, &first).unwrap();
    Engine::assert(&store, &second).unwrap();
    let history = store.history(&first.subject, &first.predicate).unwrap();
    let retired = history.iter()
        .find(|candidate| candidate.object == "failing" && candidate.valid_to == Some(200))
        .expect("retirement correction is retained in history");
    assert_eq!(retired.tx_time, 250);
    assert_eq!(store.as_of(&first.subject, &first.predicate, 150).unwrap().unwrap().object, "failing");
    assert_eq!(store.as_of(&first.subject, &first.predicate, 250).unwrap().unwrap().object, "passing");
}

/// Object-safe loading helper so both engines take the corpus through the
/// same call shape.
trait AnyEngine {
    fn load(&self, claims: &[Claim]);
}
impl<E: Engine> AnyEngine for E {
    fn load(&self, claims: &[Claim]) {
        Engine::append_batch(self, claims).unwrap();
    }
}
