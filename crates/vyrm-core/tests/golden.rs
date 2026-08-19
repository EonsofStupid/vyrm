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
use vyrm_core::{
    key, recall, AuditDecision, AuditEnvelope, Check, CheckStatus, Claim, DataTransaction,
    DecisionKind, Evidence, Predicate, Producer, ProjectionId, ProjectionStamp, ProjectionState,
    ReadStamp, Reader, ReasoningPayload, ReasoningRun, RecallQuery, RetentionPin, RunOutcome,
    RuntimeCommit, RuntimeEvent, RuntimeGraphSnapshot, RuntimeMutation, RuntimeProperties,
    RuntimeType, ScopeId, SnapshotHandle, Subject, DATA_RUNTIME_CONTRACT_VERSION,
};

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
        Producer {
            actor: "golden".into(),
            on_behalf_of: None,
            session: None,
        },
    );
    first.valid_to = Some(200);
    let second = Claim::new(
        wp3.clone(),
        status.clone(),
        "active",
        200,
        210,
        Producer {
            actor: "golden".into(),
            on_behalf_of: None,
            session: None,
        },
    );
    reference.insert(first.clone()).unwrap();
    reference.insert(second.clone()).unwrap();
    let set = recall(
        &reference,
        &RecallQuery {
            subjects: vec![wp3.clone()],
            predicates: None,
            as_of: 300,
        },
        10_000,
    )
    .unwrap();

    let runtime_scope = ScopeId::new("instance:golden").unwrap();
    let read_stamp =
        ReadStamp::new(runtime_scope.clone(), Some(3), 2, 7, Some("11".repeat(32))).unwrap();
    let snapshot = SnapshotHandle::new(read_stamp.clone(), "agent:golden", 1_000, 5_000).unwrap();
    let data_transaction = DataTransaction::new(
        read_stamp.clone(),
        RuntimeCommit {
            scope: runtime_scope.clone(),
            at: 1_100,
            actor: "agent:golden".into(),
            expected_cursor: 7,
            mutations: vec![RuntimeMutation::Event {
                event: RuntimeEvent {
                    kind: RuntimeType::new("golden_event").unwrap(),
                    subject: None,
                    properties: RuntimeProperties::new(),
                },
            }],
        },
    )
    .unwrap();
    let data_transaction_digest = data_transaction.digest();
    let retention_pin = RetentionPin::from_snapshot(&snapshot).unwrap();
    let transaction_view = data_transaction
        .preview(&RuntimeGraphSnapshot {
            scope: runtime_scope.clone(),
            valid_at: 1_200,
            known_at_cursor: 7,
            records: Vec::new(),
            relations: Vec::new(),
        })
        .unwrap();
    let projection = ProjectionStamp {
        contract_version: DATA_RUNTIME_CONTRACT_VERSION,
        id: ProjectionId::new("vector:documents").unwrap(),
        generation: 1,
        source_cursor: 7,
        config_digest: "22".repeat(32),
        artifact_digest: "33".repeat(32),
        state: ProjectionState::Ready,
    };
    projection.validate().unwrap();
    let audit = AuditEnvelope {
        contract_version: DATA_RUNTIME_CONTRACT_VERSION,
        request_id: "request:golden".into(),
        parent_request_id: Some("request:parent".into()),
        at: 1_200,
        actor: "agent:golden".into(),
        scope: runtime_scope,
        operation: "runtime.commit".into(),
        resource: "transaction:golden".into(),
        read: Some(read_stamp.clone()),
        decision: AuditDecision::Allow,
        outcome_cursor: Some(8),
        duration_ms: 12,
        previous_digest: Some("44".repeat(32)),
        digest: String::new(),
    }
    .seal()
    .unwrap();
    audit.validate().unwrap();
    let proof = Evidence {
        source: "cargo test --workspace".into(),
        digest: "55".repeat(32),
        summary: "workspace passed".into(),
    };
    let mut reasoning = ReasoningRun::empty("run:golden").unwrap();
    reasoning
        .append(
            1_300,
            "agent:golden",
            ReasoningPayload::Goal {
                statement: "freeze the portable runtime contract".into(),
                acceptance: vec!["golden vectors round-trip".into()],
            },
        )
        .unwrap();
    reasoning
        .append(
            1_301,
            "agent:golden",
            ReasoningPayload::Plan {
                hypothesis: "canonical event bytes prevent adapter drift".into(),
                steps: vec!["generate".into(), "compare".into()],
            },
        )
        .unwrap();
    reasoning
        .append(
            1_302,
            "agent:golden",
            ReasoningPayload::Attempt {
                summary: "generated the vectors".into(),
                actions: vec!["cargo test -p vyrm-core --test golden".into()],
            },
        )
        .unwrap();
    reasoning
        .append(
            1_303,
            "tool:test",
            ReasoningPayload::Observation {
                summary: "the fixture matched".into(),
                evidence: vec![proof.clone()],
            },
        )
        .unwrap();
    reasoning
        .append(
            1_304,
            "agent:golden",
            ReasoningPayload::Decision {
                decision: DecisionKind::Verify,
                rationale: "the candidate is ready for the contract gate".into(),
            },
        )
        .unwrap();
    reasoning
        .append(
            1_305,
            "tool:test",
            ReasoningPayload::Verification {
                checks: vec![Check {
                    name: "golden round-trip".into(),
                    status: CheckStatus::Passed,
                    evidence: vec![proof],
                }],
            },
        )
        .unwrap();
    reasoning
        .append(
            1_306,
            "agent:golden",
            ReasoningPayload::Outcome {
                outcome: RunOutcome::Succeeded,
                summary: "the portable contract is frozen".into(),
            },
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
        "data_runtime_v1": {
            "read_stamp": read_stamp,
            "snapshot_handle": snapshot,
            "retention_pin": retention_pin,
            "data_transaction": {
                "envelope": data_transaction,
                "digest": data_transaction_digest,
                "read_your_writes_view": transaction_view,
            },
            "projection_stamp": projection,
            "audit_envelope": audit,
        },
        "reasoning_run_v1": {
            "state": reasoning.state(),
            "events": reasoning.events(),
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
        computed,
        stored.trim_end(),
        "wire format drifted from the golden vectors — if intentional, \
         regenerate with GOLDEN_WRITE=1 and version the break for every parity engine"
    );
}
