//! Removal candidacy by query. `SPEC.md` §7 and §12; `PLAN.md` Step 2.

use vyrm_core::{Claim, Predicate, Producer, Reader, Subject};
use vyrm_store::{Store, Verdict};

fn producer() -> Producer {
    Producer { actor: "test".into(), on_behalf_of: None, session: None }
}

fn claim(subject: &str, predicate: &str, object: &str, at: u64) -> Claim {
    Claim::new(
        Subject::new(subject).unwrap(),
        Predicate::new(predicate).unwrap(),
        object,
        at,
        at,
        producer(),
    )
}

fn reader(name: &str) -> Reader {
    Reader::new(name).unwrap()
}

fn store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    (dir, store)
}

#[test]
fn a_pair_accessed_within_the_interval_is_never_a_candidate() {
    let (_dir, store) = store();
    store
        .append_batch(&[
            claim("wp3", "status", "v1", 100),
            claim("wp4", "status", "v1", 100),
        ])
        .unwrap();

    store
        .observe(
            &reader("agent:clyffy"),
            &Subject::new("wp3").unwrap(),
            &Predicate::new("status").unwrap(),
            5_000,
        )
        .unwrap();

    let report = store.removal_report(1_000, 9_000).unwrap();
    let candidates: Vec<_> = report.candidates().map(|p| p.subject.to_string()).collect();
    let retained: Vec<_> = report.retained().map(|p| p.subject.to_string()).collect();

    assert_eq!(retained, vec!["wp3"]);
    assert_eq!(candidates, vec!["wp4"]);
}

#[test]
fn a_pair_with_no_access_in_the_interval_is_always_a_candidate() {
    let (_dir, store) = store();
    store.append_batch(&[claim("wp3", "status", "v1", 100)]).unwrap();

    // Accessed, but before the interval opens.
    store
        .observe(
            &reader("agent:clyffy"),
            &Subject::new("wp3").unwrap(),
            &Predicate::new("status").unwrap(),
            500,
        )
        .unwrap();

    let report = store.removal_report(1_000, 9_000).unwrap();
    assert_eq!(report.candidates().count(), 1, "access outside the interval must not retain");

    // Widening the interval to include that access retains the pair.
    let widened = store.removal_report(0, 9_000).unwrap();
    assert_eq!(widened.candidates().count(), 0);
    assert_eq!(widened.retained().count(), 1);
}

#[test]
fn an_access_after_the_evaluation_instant_does_not_retain() {
    let (_dir, store) = store();
    store.append_batch(&[claim("wp3", "status", "v1", 100)]).unwrap();
    store
        .observe(
            &reader("agent:clyffy"),
            &Subject::new("wp3").unwrap(),
            &Predicate::new("status").unwrap(),
            50_000,
        )
        .unwrap();

    let report = store.removal_report(1_000, 9_000).unwrap();
    assert_eq!(report.candidates().count(), 1);
}

#[test]
fn every_verdict_cites_its_evidence() {
    let (_dir, store) = store();
    store
        .append_batch(&[
            claim("wp3", "status", "v1", 100),
            claim("wp3", "status", "v2", 200),
            claim("wp4", "status", "v1", 100),
        ])
        .unwrap();
    for at in [2_000u64, 3_000, 4_000] {
        store
            .observe(
                &reader("agent:clyffy"),
                &Subject::new("wp3").unwrap(),
                &Predicate::new("status").unwrap(),
                at,
            )
            .unwrap();
    }

    let report = store.removal_report(1_000, 9_000).unwrap();

    let retained = report.retained().next().unwrap();
    assert_eq!(retained.access_count, 3);
    assert_eq!(retained.last_access, Some(4_000));
    assert_eq!(retained.last_reader.as_ref().unwrap().as_str(), "agent:clyffy");
    assert!(retained.reason().contains("most recent at 4000"));

    let candidate = report.candidates().next().unwrap();
    assert_eq!(candidate.claim_count, 1);
    assert_eq!(candidate.last_access, None);
    assert!(candidate.reason().contains("no access in interval"));

    // The rendered report carries the same evidence.
    let rendered = report.render();
    assert!(rendered.contains("wp3/status"));
    assert!(rendered.contains("wp4/status"));
    assert!(rendered.contains("1 candidate(s)"));
}

#[test]
fn claim_versions_are_counted_per_pair() {
    let (_dir, store) = store();
    let claims: Vec<Claim> = (0..12)
        .map(|i| claim("wp3", "status", &format!("v{i}"), 100 + i))
        .collect();
    store.append_batch(&claims).unwrap();

    let report = store.removal_report(0, 9_000).unwrap();
    let pair = report.pairs.first().unwrap();
    assert_eq!(pair.claim_count, 12);
    assert_eq!(pair.verdict, Verdict::Candidate);
}

#[test]
fn identifiers_containing_the_printable_separator_are_attributed_correctly() {
    // `/` is legal in an identifier. Under the previous access-record encoding
    // these two pairs were indistinguishable, so an access to one would have
    // retained the other.
    let (_dir, store) = store();
    store
        .append_batch(&[
            claim("a/b", "c", "v1", 100),
            claim("a", "b/c", "v1", 100),
        ])
        .unwrap();

    store
        .observe(
            &reader("r/1"),
            &Subject::new("a/b").unwrap(),
            &Predicate::new("c").unwrap(),
            5_000,
        )
        .unwrap();

    let report = store.removal_report(0, 9_000).unwrap();
    let retained: Vec<_> = report
        .retained()
        .map(|p| format!("{}|{}", p.subject, p.predicate))
        .collect();
    let candidates: Vec<_> = report
        .candidates()
        .map(|p| format!("{}|{}", p.subject, p.predicate))
        .collect();

    assert_eq!(retained, vec!["a/b|c"]);
    assert_eq!(candidates, vec!["a|b/c"], "access to one pair retained the other");
}

#[test]
fn a_pair_with_access_but_no_stored_claim_is_not_reported() {
    let (_dir, store) = store();
    // Nothing to remove, so nothing to report.
    store
        .observe(
            &reader("agent:clyffy"),
            &Subject::new("ghost").unwrap(),
            &Predicate::new("status").unwrap(),
            5_000,
        )
        .unwrap();

    let report = store.removal_report(0, 9_000).unwrap();
    assert!(report.pairs.is_empty());
}

#[test]
fn an_empty_store_reports_nothing() {
    let (_dir, store) = store();
    let report = store.removal_report(0, 9_000).unwrap();
    assert!(report.pairs.is_empty());
    assert_eq!(report.candidates().count(), 0);
    assert!(report.render().contains("0 pair(s)"));
}
