use vyrm_core::{Claim, Predicate, Producer, Reader, Subject};
use vyrm_kv::{
    DatabaseOptions, MaintenancePolicy, DEFAULT_MEMTABLE_MAX_VERSIONS,
    DEFAULT_WAL_PAYLOAD_MAX_BYTES,
};
use vyrm_store::{
    Effectiveness, Engine, InvocationInput, NativeEngine, Outcome, RecallOutcome, Store, Trigger,
};

fn claim(subject: &str, object: &str) -> Claim {
    Claim::new(
        Subject::new(subject).unwrap(),
        Predicate::new("status").unwrap(),
        object,
        100,
        100,
        Producer {
            actor: "test".into(),
            on_behalf_of: None,
            session: None,
        },
    )
}

fn invocation<'a>(arguments: &'a [String]) -> InvocationInput<'a> {
    InvocationInput {
        at: 6_000,
        trigger: Trigger::Event,
        command: "recall",
        arguments,
        outcome: Outcome::Ok,
        duration_ms: 7,
        detail: Some("two claims".into()),
        effectiveness: Some(Effectiveness {
            query: "wp3 wp4".into(),
            claims_returned: 2,
            tokens_emitted: 11,
            baseline_tokens: Some(90),
            baseline_mode: Some("full".into()),
            provider: "frontier:test".into(),
            outcome: RecallOutcome::Unknown,
        }),
    }
}

#[test]
fn native_operator_evidence_matches_fjall_and_survives_reopen() {
    let fjall_root = tempfile::tempdir().unwrap();
    let fjall = Store::open(fjall_root.path()).unwrap();
    let native_root = tempfile::tempdir().unwrap();
    let native_path = native_root.path().join("native");
    let native = NativeEngine::open(&native_path).unwrap();
    let claims = [claim("wp3", "active"), claim("wp4", "planned")];
    Engine::append_batch(&fjall, &claims).unwrap();
    Engine::append_batch(&native, &claims).unwrap();
    let physical = native.physical_store_evidence().unwrap();
    assert_eq!(
        physical.wal_payload_max_bytes,
        Some(DEFAULT_WAL_PAYLOAD_MAX_BYTES as u64)
    );
    assert_eq!(
        physical.memtable_max_versions,
        Some(DEFAULT_MEMTABLE_MAX_VERSIONS as u64)
    );
    assert_eq!(physical.automatic_flushes, Some(0));
    assert_eq!(physical.maintenance_write_stalls, Some(0));
    assert_eq!(physical.automatic_compactions, Some(0));
    assert_eq!(physical.failed_compactions, Some(0));
    assert_eq!(physical.compaction_input_bytes, Some(0));
    assert_eq!(physical.compaction_output_bytes, Some(0));
    assert_eq!(physical.peak_compaction_buffer_bytes, Some(0));
    assert_eq!(physical.l0_segment_count, Some(0));
    assert_eq!(physical.compaction_debt_segments, Some(0));
    assert_eq!(
        physical.l0_compaction_trigger,
        Some(DatabaseOptions::default().compaction.l0_compaction_trigger as u64)
    );
    assert_eq!(physical.filter_checks, Some(0));
    assert_eq!(physical.filter_negatives, Some(0));

    let reader = Reader::new("agent:clyffy").unwrap();
    let subject = Subject::new("wp3").unwrap();
    let predicate = Predicate::new("status").unwrap();
    Engine::observe(&fjall, &reader, &subject, &predicate, 5_000).unwrap();
    Engine::observe(&native, &reader, &subject, &predicate, 5_000).unwrap();
    assert_eq!(
        fjall.removal_report(1_000, 9_000).unwrap(),
        native.removal_report(1_000, 9_000).unwrap()
    );
    assert_eq!(fjall.access_count(), native.access_count().unwrap());

    let arguments = vec!["subject=wp3".into(), "subject=wp4".into()];
    assert_eq!(
        fjall.record_invocation(invocation(&arguments)).unwrap(),
        native.record_invocation(invocation(&arguments)).unwrap()
    );
    assert_eq!(
        fjall.invocation_count().unwrap(),
        native.invocation_count().unwrap()
    );
    assert_eq!(
        fjall
            .set_recall_outcome(1, RecallOutcome::Accepted)
            .unwrap(),
        native
            .set_recall_outcome(1, RecallOutcome::Accepted)
            .unwrap()
    );
    assert_eq!(
        fjall.invocations_since(0).unwrap(),
        native.invocations_since(0).unwrap()
    );

    drop(native);
    let reopened = NativeEngine::open(&native_path).unwrap();
    assert_eq!(reopened.invocation_count().unwrap(), 1);
    assert_eq!(reopened.access_count().unwrap(), 1);
    assert_eq!(
        reopened.invocations_since(0).unwrap()[0]
            .effectiveness
            .as_ref()
            .unwrap()
            .outcome,
        RecallOutcome::Accepted
    );
    assert_eq!(
        reopened.removal_report(1_000, 9_000).unwrap(),
        fjall.removal_report(1_000, 9_000).unwrap()
    );
}

#[test]
fn native_engine_applies_explicit_project_maintenance_bounds() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("native");
    let engine = NativeEngine::open_with_options(
        &path,
        DatabaseOptions {
            maintenance: MaintenancePolicy {
                wal_payload_max_bytes: usize::MAX,
                memtable_max_versions: 1,
            },
            ..DatabaseOptions::default()
        },
    )
    .unwrap();
    Engine::append_batch(&engine, &[claim("bounded", "active")]).unwrap();
    Engine::observe(
        &engine,
        &Reader::new("agent:clyffy").unwrap(),
        &Subject::new("bounded").unwrap(),
        &Predicate::new("status").unwrap(),
        5_000,
    )
    .unwrap();

    let physical = engine.physical_store_evidence().unwrap();
    assert_eq!(physical.memtable_max_versions, Some(1));
    assert_eq!(physical.automatic_flushes, Some(1));
    assert_eq!(physical.maintenance_write_stalls, Some(1));
    assert_eq!(physical.failed_maintenance_flushes, Some(0));
    assert_eq!(physical.l0_segment_count, Some(1));
    assert_eq!(physical.compaction_debt_segments, Some(0));
}
