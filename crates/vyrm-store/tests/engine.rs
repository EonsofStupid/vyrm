//! The storage port, proven by differential (`PLAN.md` Step S, standing
//! rule 3): the Fjall engine and the reference engine must be
//! indistinguishable through the trait — same recall, same projection, same
//! grounding stamp digest. This test is what makes "fold in storage" a
//! contract rather than a promise: a bbolt engine in Go, or a vyrm-native
//! engine in Rust, is correct exactly when this differential (and the
//! golden key vectors in vyrm-core) holds for it.

use vyrm_core::{recall, Claim, Predicate, Producer, RecallQuery, Subject};
use vyrm_store::{Engine, GroundingReport, MemoryEngine, Store};

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
fn two_engines_are_indistinguishable_through_the_port() {
    let dir = tempfile::tempdir().unwrap();
    let fjall = Store::open(dir.path()).unwrap();
    let memory = MemoryEngine::new();

    for engine in [&fjall as &dyn AnyEngine, &memory as &dyn AnyEngine] {
        engine.load(&corpus());
    }

    // Same sequence, same interval replay, same subjects.
    assert_eq!(Engine::sequence(&fjall).unwrap(), Engine::sequence(&memory).unwrap());
    assert_eq!(
        Engine::claims_in_range(&fjall, 2, 5).unwrap(),
        Engine::claims_in_range(&memory, 2, 5).unwrap()
    );
    assert_eq!(Engine::subjects(&fjall).unwrap(), Engine::subjects(&memory).unwrap());

    // Same recall set, digest included.
    let query = RecallQuery {
        subjects: vec![Subject::new("wp3").unwrap(), Subject::new("wp4").unwrap()],
        predicates: None,
        as_of: 400,
    };
    let a = recall(&fjall, &query, 10_000).unwrap();
    let b = recall(&memory, &query, 10_000).unwrap();
    assert_eq!(a.claims, b.claims);
    assert_eq!(a.digest, b.digest, "the content digest is engine-independent");

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
    assert_eq!(ga.sequence, gb.sequence);
    assert_eq!(ga.digest, gb.digest, "grounding digests agree across engines");

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
