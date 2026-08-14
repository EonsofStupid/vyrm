//! `SPEC.md` §8.2/§8.3 acceptance (`PLAN.md` Step 5): an induced divergence
//! halts and quarantines; a matching projection emits `grounded` with a
//! digest; a crash mid-rebuild replays the interval rather than skipping it.

use vyrm_core::{Claim, Predicate, Producer, Subject};
use vyrm_store::{
    Error, GroundingReport, ProjectionStatus, Store, CURRENT_PROJECTION,
};

fn claim(subject: &str, predicate: &str, object: &str, from: u64) -> Claim {
    Claim::new(
        Subject::new(subject).unwrap(),
        Predicate::new(predicate).unwrap(),
        object,
        from,
        from,
        Producer {
            actor: "test".into(),
            on_behalf_of: None,
            session: None,
        },
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
fn rebuild_applies_the_interval_and_advances_the_watermark_with_it() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    store.append_batch(&corpus()).unwrap();

    let outcome = store.rebuild_current().unwrap();
    assert_eq!((outcome.from, outcome.to, outcome.applied), (0, 6, 6));

    let projection = store.current_projection().unwrap();
    assert_eq!(projection.watermark, store.sequence().unwrap());
    assert_eq!(projection.len(), 4, "one entry per (subject, predicate) pair");
    let newest = projection
        .get(&Subject::new("wp3").unwrap(), &Predicate::new("status").unwrap())
        .unwrap()
        .expect("pair is projected");
    assert_eq!(newest.object, "done", "the newest version wins the fold");

    // A second rebuild finds an empty interval and changes nothing.
    let again = store.rebuild_current().unwrap();
    assert_eq!(again.applied, 0);
}

#[test]
fn a_crash_mid_rebuild_replays_the_interval_rather_than_skipping_it() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    store.append_batch(&corpus()[..3]).unwrap();
    store.rebuild_current().unwrap();
    store.append_batch(&corpus()[3..]).unwrap();

    // The crash: claims of the new interval were read and folded in memory,
    // but the process died before the projection write. Nothing was stored, so
    // the watermark MUST still name the old interval end (§8.2: the watermark
    // advances in the same write as the projection, so there is no state in
    // which it moved and the entries did not).
    let interval = store.claims_in_range(3, store.sequence().unwrap()).unwrap();
    assert_eq!(interval.len(), 3);
    drop(interval); // folded state lost with the crash

    let projection = store.current_projection().unwrap();
    assert_eq!(projection.watermark, 3, "watermark did not advance without the write");

    // Recovery is an ordinary rebuild: the same interval replays in full and
    // the result equals a from-scratch recomputation — proven by grounding,
    // not by a hand-written expectation.
    let outcome = store.rebuild_current().unwrap();
    assert_eq!((outcome.from, outcome.to, outcome.applied), (3, 6, 3));
    match store.ground_current(1_000).unwrap() {
        GroundingReport::Grounded(stamp) => assert_eq!(stamp.sequence, 6),
        GroundingReport::Divergence { differences } => {
            panic!("replayed projection diverged: {differences:?}")
        }
    }
}

#[test]
fn a_matching_projection_emits_grounded_with_a_stable_digest() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    store.append_batch(&corpus()).unwrap();
    store.rebuild_current().unwrap();

    let first = match store.ground_current(500).unwrap() {
        GroundingReport::Grounded(stamp) => stamp,
        GroundingReport::Divergence { differences } => panic!("diverged: {differences:?}"),
    };
    assert_eq!(first.at, 500);
    assert_eq!(first.sequence, 6);

    // Grounding again without new claims reproduces the digest: the digest
    // names content, not the grounding run.
    let second = match store.ground_current(900).unwrap() {
        GroundingReport::Grounded(stamp) => stamp,
        GroundingReport::Divergence { differences } => panic!("diverged: {differences:?}"),
    };
    assert_eq!(first.digest, second.digest);
    assert_eq!(
        store.current_projection().unwrap().last_grounded.unwrap().digest,
        second.digest,
        "the stamp is persisted with the projection"
    );
}

#[test]
fn an_induced_divergence_halts_and_quarantines_and_only_reset_recovers() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    store.append_batch(&corpus()).unwrap();
    store.rebuild_current().unwrap();

    // Induce the divergence §8.3 exists to catch: corrupt one entry of the
    // stored blob directly, bypassing the module's own write path.
    let bytes = store.get_projection(CURRENT_PROJECTION).unwrap().unwrap();
    let corrupted =
        String::from_utf8(bytes).unwrap().replacen("\"done\"", "\"drifted\"", 1);
    assert!(corrupted.contains("drifted"), "corruption must have landed");
    store
        .put_projection(CURRENT_PROJECTION, corrupted.as_bytes())
        .unwrap();

    let differences = match store.ground_current(700).unwrap() {
        GroundingReport::Divergence { differences } => differences,
        GroundingReport::Grounded(_) => panic!("induced divergence went undetected"),
    };
    assert_eq!(differences.len(), 1);
    assert!(differences[0].contains("wp3/status"), "the differential names the pair: {differences:?}");

    // Halted: reads, rebuilds, and further grounding all refuse.
    let projection = store.current_projection().unwrap();
    assert!(matches!(projection.status, ProjectionStatus::Quarantined { at: 700, .. }));
    let subject = Subject::new("wp3").unwrap();
    let predicate = Predicate::new("status").unwrap();
    assert!(matches!(projection.get(&subject, &predicate), Err(Error::Quarantined(_))));
    assert!(matches!(store.rebuild_current(), Err(Error::Quarantined(_))));
    assert!(matches!(store.ground_current(800), Err(Error::Quarantined(_))));

    // The quarantine is Authoritative: it survives a clean reopen with no
    // flush, unlike ordinary Buffered projection writes.
    drop(projection);
    drop(store);
    let reopened = Store::open(dir.path()).unwrap();
    assert!(
        matches!(
            reopened.current_projection().unwrap().status,
            ProjectionStatus::Quarantined { .. }
        ),
        "a detected divergence must not be forgettable"
    );

    // The only exit is the explicit operator reset: recomputation becomes the
    // projection, and grounding passes again.
    let outcome = reopened.reset_current().unwrap();
    assert_eq!(outcome.applied, 6);
    assert!(matches!(
        reopened.ground_current(900).unwrap(),
        GroundingReport::Grounded(_)
    ));
    let recovered = reopened.current_projection().unwrap();
    assert_eq!(
        recovered
            .get(&subject, &predicate)
            .unwrap()
            .expect("pair restored")
            .object,
        "done"
    );
}
