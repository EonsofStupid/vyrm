//! Adapter conformance against the grounding reference.
//!
//! `SPEC.md` §8.3 and §12: a substrate adapter is correct if and only if it
//! returns what [`vyrm_core::reference::MemoryClaims`] returns for the same
//! claims. These tests are that differential, not an independent set of
//! hand-written expectations.

use vyrm_core::reference::MemoryClaims;
use vyrm_core::{Claim, ClaimReader, Predicate, Producer, Subject};
use vyrm_store::Store;

fn producer() -> Producer {
    Producer {
        actor: "test".into(),
        on_behalf_of: None,
        session: None,
    }
}

fn claim(subject: &str, predicate: &str, object: &str, from: u64, to: Option<u64>) -> Claim {
    let mut c = Claim::new(
        Subject::new(subject).unwrap(),
        Predicate::new(predicate).unwrap(),
        object,
        from,
        from,
        producer(),
    );
    c.valid_to = to;
    c
}

/// Deterministic pseudo-random generator. Avoids a dependency and keeps the
/// corpus identical on every run, so a failure is always reproducible.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self, modulo: u64) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 33) % modulo
    }
}

/// Builds a corpus with supersession, retirement, and adversarial neighbours.
fn corpus() -> Vec<Claim> {
    let mut rng = Lcg(0x5EED);
    let mut out = Vec::new();
    for s in ["wp3", "wp3x", "wp", "wp4"] {
        for p in ["status", "statusx", "owner"] {
            let versions = 1 + rng.next(4);
            let mut from = 100 + rng.next(50);
            for v in 0..versions {
                let span = 20 + rng.next(80);
                // Retire every version except the last, so each series ends open
                // or closed depending on the draw.
                let to = if v + 1 < versions || rng.next(3) == 0 {
                    Some(from + span)
                } else {
                    None
                };
                out.push(claim(s, p, &format!("{s}/{p}/v{v}"), from, to));
                from += span;
            }
        }
    }
    out
}

fn pairs() -> Vec<(Subject, Predicate)> {
    let mut out = Vec::new();
    for s in ["wp3", "wp3x", "wp", "wp4", "absent"] {
        for p in ["status", "statusx", "owner", "absent"] {
            out.push((Subject::new(s).unwrap(), Predicate::new(p).unwrap()));
        }
    }
    out
}

#[test]
fn adapter_matches_grounding_reference_across_the_corpus() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    let mut reference = MemoryClaims::new();

    let claims = corpus();
    store.append_batch(&claims).unwrap();
    for c in &claims {
        reference.insert(c.clone()).unwrap();
    }

    let mut compared = 0usize;
    for (subject, predicate) in pairs() {
        // Resolution must agree at every instant across the corpus range.
        for at in (0..600).step_by(7) {
            let from_store = store.as_of(&subject, &predicate, at).unwrap();
            let from_reference = reference.as_of(&subject, &predicate, at).unwrap();
            assert_eq!(
                from_store.as_ref().map(|c| &c.object),
                from_reference.as_ref().map(|c| &c.object),
                "divergence at subject={subject} predicate={predicate} as_of={at}"
            );
            compared += 1;
        }

        // History must agree in content and in order.
        let store_history: Vec<_> = store
            .history(&subject, &predicate)
            .unwrap()
            .into_iter()
            .map(|c| c.object)
            .collect();
        let reference_history: Vec<_> = reference
            .history(&subject, &predicate)
            .unwrap()
            .into_iter()
            .map(|c| c.object)
            .collect();
        assert_eq!(
            store_history, reference_history,
            "history divergence at subject={subject} predicate={predicate}"
        );
    }
    assert!(compared > 1000, "corpus sweep was too small to be meaningful");
}

#[test]
fn batch_allocates_contiguous_sequences_and_advances_the_watermark() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    assert_eq!(store.sequence().unwrap(), 0);

    let first = store
        .append_batch(&[
            claim("a", "p", "1", 100, None),
            claim("b", "p", "2", 100, None),
            claim("c", "p", "3", 100, None),
        ])
        .unwrap();
    assert_eq!(first.first_sequence, 1);
    assert_eq!(first.last_sequence, 3);
    assert_eq!(store.sequence().unwrap(), 3);

    let second = store.append_batch(&[claim("d", "p", "4", 100, None)]).unwrap();
    assert_eq!(second.first_sequence, 4);
    assert_eq!(second.last_sequence, 4);
    assert_eq!(store.sequence().unwrap(), 4);
}

#[test]
fn empty_batch_is_a_no_write_and_does_not_advance_the_watermark() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    store.append_batch(&[claim("a", "p", "1", 100, None)]).unwrap();
    let outcome = store.append_batch(&[]).unwrap();
    assert_eq!(outcome.count, 0);
    assert_eq!(store.sequence().unwrap(), 1);
}

#[test]
fn claims_and_watermark_survive_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let claims = corpus();
    let expected_sequence = claims.len() as u64;

    {
        let store = Store::open(dir.path()).unwrap();
        store.append_batch(&claims).unwrap();
        assert_eq!(store.sequence().unwrap(), expected_sequence);
    }

    let reopened = Store::open(dir.path()).unwrap();
    assert_eq!(
        reopened.sequence().unwrap(),
        expected_sequence,
        "watermark did not survive reopen"
    );

    let mut reference = MemoryClaims::new();
    for c in &claims {
        reference.insert(c.clone()).unwrap();
    }
    let subject = Subject::new("wp3").unwrap();
    let predicate = Predicate::new("status").unwrap();
    for at in (0..600).step_by(11) {
        assert_eq!(
            reopened.as_of(&subject, &predicate, at).unwrap().map(|c| c.object),
            reference.as_of(&subject, &predicate, at).unwrap().map(|c| c.object),
            "post-reopen divergence at as_of={at}"
        );
    }
}

#[test]
fn invalid_claim_is_rejected_before_any_write() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    // An inverted valid-time interval must abort the batch, leaving the
    // watermark untouched.
    let bad = claim("a", "p", "x", 200, Some(100));
    assert!(store.append_batch(&[bad]).is_err());
    assert_eq!(store.sequence().unwrap(), 0);
}

#[test]
fn access_records_are_written_without_blocking_reads() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    let subject = Subject::new("wp3").unwrap();
    let predicate = Predicate::new("status").unwrap();
    store.append_batch(&[claim("wp3", "status", "v1", 100, None)]).unwrap();

    for i in 0..10 {
        store.observe(&vyrm_core::Reader::new("agent:clyffy").unwrap(), &subject, &predicate, 1000 + i).unwrap();
    }
    assert_eq!(store.access_count(), 10);
    // The claim remains readable after telemetry writes.
    assert!(store.as_of(&subject, &predicate, 150).unwrap().is_some());
}
