//! Golden vectors: the cross-language storage contract (`PLAN.md` Step S).
//!
//! A parity engine in another language — the Go/bbolt engine for the LFG
//! side first — is byte-compatible with vyrm exactly when it reproduces
//! these vectors: key encodings (including the inverted-timestamp ordering
//! that makes newest-first a forward scan), prefixes and their exclusive
//! ends, and the recall content digest. The fixture is checked in; this
//! test regenerates every vector from the kernel and fails on any drift,
//! so an encoding change cannot land silently and orphan a parity
//! implementation.
//!
//! Regenerate deliberately with `GOLDEN_WRITE=1 cargo test -p vyrm-core
//! --test golden` — and treat a diff in the fixture as what it is: a wire
//! format break that every engine must follow.

use vyrm_core::reference::MemoryClaims;
use vyrm_core::{key, recall, Claim, Predicate, Producer, RecallQuery, Reader, Subject};

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn subject(s: &str) -> Subject {
    Subject::new(s).unwrap()
}

fn predicate(p: &str) -> Predicate {
    Predicate::new(p).unwrap()
}

/// Every vector, computed from the kernel. The Go implementation mirrors
/// this function; the JSON is the meeting point.
fn vectors() -> serde_json::Value {
    let wp3 = subject("wp3");
    let status = predicate("status");

    let older = key::claim_key(&wp3, &status, 1_000, 2_000);
    let newer = key::claim_key(&wp3, &status, 1_001, 2_000);

    let mut reference = MemoryClaims::new();
    let mut first = Claim::new(
        wp3.clone(),
        status.clone(),
        "planned",
        100,
        100,
        Producer { actor: "golden".into(), on_behalf_of: None, session: None },
    );
    first.valid_to = Some(200);
    let second = Claim::new(
        wp3.clone(),
        status.clone(),
        "active",
        200,
        210,
        Producer { actor: "golden".into(), on_behalf_of: None, session: None },
    );
    reference.insert(first.clone()).unwrap();
    reference.insert(second.clone()).unwrap();
    let set = recall(
        &reference,
        &RecallQuery { subjects: vec![wp3.clone()], predicates: None, as_of: 300 },
        10_000,
    )
    .unwrap();

    serde_json::json!({
        "comment": "regenerate with GOLDEN_WRITE=1; a diff here is a wire-format break",
        "claim_key": {
            "wp3/status valid_from=1000 tx=2000": hex(&older),
            "wp3/status valid_from=1001 tx=2000": hex(&newer),
            "newer_sorts_before_older": newer < older,
        },
        "prefixes": {
            "subject_prefix wp3": hex(&key::subject_prefix(&wp3)),
            "version_prefix wp3/status": hex(&key::version_prefix(&wp3, &status)),
            "seek_key wp3/status as_of=1500": hex(&key::seek_key(&wp3, &status, 1_500)),
            "prefix_end(subject_prefix wp3)": hex(&key::prefix_end(&key::subject_prefix(&wp3)).unwrap()),
        },
        "sequence_key 42": hex(&key::sequence_key(42)),
        "access_key at=1234 reader=agent:x wp3/status": hex(&key::access_key(
            1_234,
            &Reader::new("agent:x").unwrap(),
            &wp3,
            &status,
        )),
        "invert": {
            "0": key::invert(0),
            "1000": key::invert(1_000),
        },
        "claim_json": serde_json::to_value(&second).unwrap(),
        "recall wp3 as_of=300 budget=10000": {
            "claims_returned": set.claims.len(),
            "current_object": set.claims[0].object,
            "digest": set.digest,
            "token_estimate": set.token_estimate,
        },
    })
}

#[test]
fn the_wire_format_matches_the_checked_in_vectors() {
    let computed = serde_json::to_string_pretty(&vectors()).unwrap();
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/golden-vectors.json");
    if std::env::var("GOLDEN_WRITE").is_ok() {
        std::fs::write(path, &computed).unwrap();
        return;
    }
    let stored = std::fs::read_to_string(path)
        .expect("fixtures/golden-vectors.json is checked in; GOLDEN_WRITE=1 creates it");
    assert_eq!(
        computed, stored,
        "wire format drifted from the golden vectors — if intentional, \
         regenerate with GOLDEN_WRITE=1 and version the break for every parity engine"
    );
}
