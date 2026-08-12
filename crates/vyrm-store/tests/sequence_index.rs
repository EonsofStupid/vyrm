//! Sequence-ordered scan. `SPEC.md` §8.2 and §8.4; `PLAN.md` Step 1.
//!
//! The index maps an append sequence to the claim key written at that sequence.
//! It is written in the transaction that writes the claim, so it cannot diverge
//! from the watermark.

use std::sync::Arc;
use vyrm_core::reference::MemoryClaims;
use vyrm_core::{Claim, Predicate, Producer, Subject};
use vyrm_store::{Store, Writer, WriterConfig};

fn producer() -> Producer {
    Producer { actor: "test".into(), on_behalf_of: None, session: None }
}

fn claim(i: usize) -> Claim {
    Claim::new(
        Subject::new(format!("s{}", i % 7)).unwrap(),
        Predicate::new(format!("p{}", i % 3)).unwrap(),
        format!("v{i}"),
        100 + i as u64,
        1_000 + i as u64,
        producer(),
    )
}

fn store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    (dir, store)
}

#[test]
fn a_range_returns_exactly_the_claims_appended_in_it() {
    let (_dir, store) = store();
    let first: Vec<Claim> = (0..10).map(claim).collect();
    let second: Vec<Claim> = (10..25).map(claim).collect();
    store.append_batch(&first).unwrap();
    store.append_batch(&second).unwrap();

    let objects: Vec<String> = store
        .claims_in_range(10, 25)
        .unwrap()
        .into_iter()
        .map(|c| c.object)
        .collect();
    let expected: Vec<String> = second.iter().map(|c| c.object.clone()).collect();
    assert_eq!(objects, expected, "range did not return exactly the second batch");
}

#[test]
fn the_lower_bound_is_exclusive_so_a_watermark_can_be_passed_directly() {
    let (_dir, store) = store();
    store.append_batch(&(0..5).map(claim).collect::<Vec<_>>()).unwrap();

    // (0, 5] is everything; (5, 5] is empty; (4, 5] is the last claim alone.
    assert_eq!(store.claims_in_range(0, 5).unwrap().len(), 5);
    assert_eq!(store.claims_in_range(5, 5).unwrap().len(), 0);
    let tail = store.claims_in_range(4, 5).unwrap();
    assert_eq!(tail.len(), 1);
    assert_eq!(tail[0].object, "v4");
}

#[test]
fn an_inverted_or_empty_range_yields_nothing() {
    let (_dir, store) = store();
    store.append_batch(&(0..5).map(claim).collect::<Vec<_>>()).unwrap();
    assert!(store.claims_in_range(4, 2).unwrap().is_empty());
    assert!(store.claims_in_range(9, 9).unwrap().is_empty());
    assert!(store.claims_in_range(100, 200).unwrap().is_empty());
}

#[test]
fn claims_are_returned_in_append_order() {
    let (_dir, store) = store();
    // Valid times descend while append order ascends, so a result ordered by
    // valid time rather than by sequence would be detected here.
    let claims: Vec<Claim> = (0..40)
        .map(|i| {
            Claim::new(
                Subject::new("wp3").unwrap(),
                Predicate::new("status").unwrap(),
                format!("v{i}"),
                10_000 - i as u64,
                1_000 + i as u64,
                producer(),
            )
        })
        .collect();
    store.append_batch(&claims).unwrap();

    let objects: Vec<String> = store.all_claims().unwrap().into_iter().map(|c| c.object).collect();
    let expected: Vec<String> = (0..40).map(|i| format!("v{i}")).collect();
    assert_eq!(objects, expected);
}

#[test]
fn a_full_scan_reproduces_every_stored_claim_against_the_grounding_reference() {
    let (_dir, store) = store();
    let claims: Vec<Claim> = (0..500).map(claim).collect();
    store.append_batch(&claims).unwrap();

    let mut reference = MemoryClaims::new();
    for c in &claims {
        reference.insert(c.clone()).unwrap();
    }

    let mut scanned = store.all_claims().unwrap();
    let mut expected: Vec<Claim> = reference.iter().cloned().collect();
    // Compare as sets: the index scans in append order, the reference in key
    // order. Content must agree exactly.
    let key_of = |c: &Claim| format!("{}|{}|{}|{}", c.subject, c.predicate, c.valid_from, c.tx_time);
    scanned.sort_by_key(key_of);
    expected.sort_by_key(key_of);
    assert_eq!(scanned.len(), expected.len(), "scan and reference differ in size");
    assert_eq!(scanned, expected, "scan diverged from the grounding reference");
}

#[test]
fn the_index_stays_consistent_with_the_watermark_across_batches() {
    let (_dir, store) = store();
    let mut written = 0usize;
    for round in 0..12 {
        let size = 1 + round * 3;
        let batch: Vec<Claim> = (written..written + size).map(claim).collect();
        store.append_batch(&batch).unwrap();
        written += size;
        let watermark = store.sequence().unwrap();
        assert_eq!(watermark as usize, written);
        assert_eq!(
            store.claims_in_range(0, watermark).unwrap().len(),
            written,
            "index entries and watermark disagree after round {round}"
        );
    }
}

#[test]
fn the_index_survives_reopen_and_continues() {
    let dir = tempfile::tempdir().unwrap();
    {
        let store = Store::open(dir.path()).unwrap();
        store.append_batch(&(0..30).map(claim).collect::<Vec<_>>()).unwrap();
    }
    let reopened = Store::open(dir.path()).unwrap();
    assert_eq!(reopened.sequence().unwrap(), 30);
    assert_eq!(reopened.all_claims().unwrap().len(), 30);

    reopened.append_batch(&(30..45).map(claim).collect::<Vec<_>>()).unwrap();
    assert_eq!(reopened.sequence().unwrap(), 45);
    assert_eq!(reopened.all_claims().unwrap().len(), 45);
    // The claims appended after reopen are addressable by the range that
    // excludes everything written before it.
    let tail: Vec<String> = reopened
        .claims_in_range(30, 45)
        .unwrap()
        .into_iter()
        .map(|c| c.object)
        .collect();
    assert_eq!(tail, (30..45).map(|i| format!("v{i}")).collect::<Vec<_>>());
}

#[test]
fn the_index_is_consistent_after_writer_driven_appends() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open(dir.path()).unwrap());
    let writer = Writer::spawn(Arc::clone(&store), WriterConfig::default());
    for i in 0..1_000 {
        writer.submit(claim(i)).unwrap();
    }
    writer.flush().unwrap();

    let watermark = store.sequence().unwrap();
    assert_eq!(watermark, 1_000);
    assert_eq!(store.claims_in_range(0, watermark).unwrap().len(), 1_000);
}

#[test]
fn a_rejected_batch_leaves_no_index_entries() {
    let (_dir, store) = store();
    store.append_batch(&(0..5).map(claim).collect::<Vec<_>>()).unwrap();

    let mut bad = claim(99);
    bad.valid_to = Some(1); // inverted against valid_from
    assert!(store.append_batch(&[claim(6), bad]).is_err());

    // The transaction was not committed, so neither claims nor index advanced.
    assert_eq!(store.sequence().unwrap(), 5);
    assert_eq!(store.all_claims().unwrap().len(), 5);
}
