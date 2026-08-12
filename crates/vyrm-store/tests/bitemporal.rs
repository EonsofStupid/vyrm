//! Both timelines must be addressable. `SPEC.md` §6.
//!
//! Regression coverage for a defect found on 2026-08-10: with valid time alone in
//! the key, a later correction at the same `valid_from` overwrote the claim it
//! corrected. The sequence watermark counted both claims while only one existed,
//! so any sequence-derived reconstruction would have been wrong.

use vyrm_core::{Claim, ClaimReader, Predicate, Producer, Subject};
use vyrm_store::Store;

fn producer() -> Producer {
    Producer { actor: "test".into(), on_behalf_of: None, session: None }
}

fn recorded(subject: &str, predicate: &str, object: &str, valid_from: u64, tx_time: u64) -> Claim {
    let mut claim = Claim::new(
        Subject::new(subject).unwrap(),
        Predicate::new(predicate).unwrap(),
        object,
        valid_from,
        tx_time,
        producer(),
    );
    claim.tx_time = tx_time;
    claim
}

#[test]
fn a_correction_at_the_same_valid_from_preserves_the_claim_it_corrects() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    let subject = Subject::new("wp3").unwrap();
    let predicate = Predicate::new("status").unwrap();

    store
        .append_batch(&[
            recorded("wp3", "status", "blocked", 100, 100),
            recorded("wp3", "status", "in_progress", 100, 200),
        ])
        .unwrap();

    let history = store.history(&subject, &predicate).unwrap();
    assert_eq!(history.len(), 2, "the corrected claim was destroyed");
    assert_eq!(
        store.sequence().unwrap(),
        2,
        "watermark and stored claim count must agree"
    );

    // Current knowledge wins: the later transaction time resolves first.
    assert_eq!(
        store.as_of(&subject, &predicate, 150).unwrap().map(|c| c.object),
        Some("in_progress".into())
    );
    // The superseded knowledge remains readable.
    assert!(
        history.iter().any(|c| c.object == "blocked"),
        "prior knowledge is no longer retrievable"
    );
}

#[test]
fn stored_claim_count_tracks_the_sequence_watermark() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();

    // Deliberately collision-prone: one subject and predicate, one valid_from,
    // many successive corrections.
    let claims: Vec<Claim> = (0..50)
        .map(|i| recorded("wp3", "status", &format!("v{i}"), 100, 100 + i))
        .collect();
    store.append_batch(&claims).unwrap();

    let history = store
        .history(&Subject::new("wp3").unwrap(), &Predicate::new("status").unwrap())
        .unwrap();
    assert_eq!(history.len(), 50);
    assert_eq!(store.sequence().unwrap(), 50);
    // Newest knowledge first.
    assert_eq!(history.first().unwrap().object, "v49");
    assert_eq!(history.last().unwrap().object, "v0");
}

#[test]
fn distinct_valid_times_are_unaffected_by_the_transaction_time_field() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    let subject = Subject::new("wp3").unwrap();
    let predicate = Predicate::new("status").unwrap();

    // Recorded out of order: the earlier valid_from is learned about last.
    store
        .append_batch(&[
            recorded("wp3", "status", "second", 200, 100),
            recorded("wp3", "status", "first", 100, 900),
        ])
        .unwrap();

    // Valid-time ordering governs resolution, not the order of recording.
    assert_eq!(
        store.as_of(&subject, &predicate, 150).unwrap().map(|c| c.object),
        Some("first".into())
    );
    assert_eq!(
        store.as_of(&subject, &predicate, 250).unwrap().map(|c| c.object),
        Some("second".into())
    );
}
