use rrd_core::digest;
use rrd_engine::{
    append_lifecycle_event, load_active_work_item_plan, load_work_plan,
    LifecycleEnforcementLevelV1, LifecycleEventCommandV1, LifecycleEventTypeV1, LifecyclePayloadV1,
    LifecycleRiskV1, LifecycleTaskKindV1, LifecycleTraceContextV1, ProjectAttunementReceipt,
    REASONING_SCOPE,
};
use rrd_store::Engine;

pub struct PlannedLifecycleFixture<'a> {
    pub project_id: &'a str,
    pub session_id: &'a str,
    pub plan_id: &'a str,
    pub work_item_id: &'a str,
    pub reasoning_run_id: &'a str,
    pub actor: &'a str,
    pub adapter_kind: &'a str,
    pub enforcement_level: LifecycleEnforcementLevelV1,
    pub start_at: u64,
}

pub fn seed_planned_lifecycle<E: Engine>(
    store: &E,
    receipt: &ProjectAttunementReceipt,
    fixture: PlannedLifecycleFixture<'_>,
) {
    let snapshot = load_work_plan(store, fixture.plan_id).unwrap().unwrap();
    let plan = load_active_work_item_plan(store, fixture.plan_id)
        .unwrap()
        .unwrap();
    assert_eq!(plan.work_item_id, fixture.work_item_id);
    let verification_plan_sha256 =
        digest::sha256_hex(&serde_json::to_vec(&plan.verification_commands).unwrap());
    let commands = vec![
        (
            LifecycleEventTypeV1::SessionOpened,
            LifecyclePayloadV1::SessionOpened {
                resumed: false,
                provider_session_sha256: Some(digest::sha256_hex(fixture.session_id.as_bytes())),
            },
        ),
        (
            LifecycleEventTypeV1::ProjectAttunementRequested,
            LifecyclePayloadV1::AttunementRequested {
                source_fingerprint_sha256: receipt.source_tree_sha256.clone(),
            },
        ),
        (
            LifecycleEventTypeV1::ProjectAttunementCompleted,
            LifecyclePayloadV1::AttunementCompleted {
                topology_sha256: digest::sha256_hex(b"test-topology"),
                profile_sha256: receipt.profile_sha256.clone(),
                source_tree_sha256: receipt.source_tree_sha256.clone(),
            },
        ),
        (
            LifecycleEventTypeV1::TurnOpened,
            LifecyclePayloadV1::TurnOpened,
        ),
        (
            LifecycleEventTypeV1::PromptReceived,
            LifecyclePayloadV1::PromptReceived {
                prompt_sha256: digest::sha256_hex(b"test prompt"),
                prompt_bytes: 11,
            },
        ),
        (
            LifecycleEventTypeV1::PreflightStarted,
            LifecyclePayloadV1::PreflightStarted {
                source_tree_sha256: receipt.source_tree_sha256.clone(),
            },
        ),
        (
            LifecycleEventTypeV1::PreflightCompleted,
            LifecyclePayloadV1::PreflightCompleted {
                receipt_sha256: receipt.receipt_sha256.clone(),
                context_sha256: digest::sha256_hex(b"test context"),
            },
        ),
        (
            LifecycleEventTypeV1::TaskClassificationCompleted,
            LifecyclePayloadV1::TaskClassified {
                task: LifecycleTaskKindV1::CodeChange,
                risk: LifecycleRiskV1::Medium,
                classification_sha256: digest::sha256_hex(b"test classification"),
            },
        ),
        (
            LifecycleEventTypeV1::ArchitectureAssessmentRequested,
            LifecyclePayloadV1::ArchitectureRequested {
                questions_sha256: digest::sha256_hex(b"test architecture questions"),
            },
        ),
        (
            LifecycleEventTypeV1::ArchitectureAssessmentCompleted,
            LifecyclePayloadV1::ArchitectureCompleted {
                assessment_sha256: digest::sha256_hex(b"test architecture"),
                owner_boundary: "rrd-engine".into(),
                dependency_direction: "contract -> engine -> adapter".into(),
            },
        ),
        (
            LifecycleEventTypeV1::PatternSelectionCompleted,
            LifecyclePayloadV1::PatternSelected {
                selection_sha256: digest::sha256_hex(b"test pattern"),
                pattern_count: 1,
            },
        ),
        (
            LifecycleEventTypeV1::PlanningAuthorized,
            LifecyclePayloadV1::PlanningAuthorized {
                permit_sha256: digest::sha256_hex(b"test permit"),
                expires_at_unix_ms: fixture.start_at.saturating_add(60 * 60 * 1_000),
                plan_id: fixture.plan_id.into(),
                plan_revision: snapshot.revision,
                work_item_id: fixture.work_item_id.into(),
            },
        ),
        (
            LifecycleEventTypeV1::ContextAssembled,
            LifecyclePayloadV1::ContextAssembled {
                context_sha256: digest::sha256_hex(b"test assembled context"),
                context_bytes: 128,
                truncated: false,
            },
        ),
        (
            LifecycleEventTypeV1::PlanRecorded,
            LifecyclePayloadV1::PlanRecorded {
                plan_sha256: plan.plan_payload_sha256,
                plan_id: fixture.plan_id.into(),
                plan_revision: snapshot.revision,
                work_item_id: fixture.work_item_id.into(),
                source_tree_sha256: plan.source_tree_sha256,
                verification_plan_sha256,
            },
        ),
    ];
    for (index, (event_type, payload)) in commands.into_iter().enumerate() {
        let at = fixture.start_at + index as u64 + 1;
        let session_level = index < 3;
        let plan_recorded = event_type == LifecycleEventTypeV1::PlanRecorded;
        append_lifecycle_event(
            store,
            LifecycleEventCommandV1 {
                event_type,
                occurred_at_unix_ms: at,
                instance_id: fixture.project_id.into(),
                project_id: fixture.project_id.into(),
                member_id: None,
                session_id: fixture.session_id.into(),
                turn_id: (!session_level).then(|| "turn-workplan-1".into()),
                reasoning_run_id: plan_recorded.then(|| fixture.reasoning_run_id.into()),
                attempt_id: None,
                tool_call_id: None,
                correlation_id: format!("workplan-lifecycle-{at}"),
                actor: fixture.actor.into(),
                adapter_kind: fixture.adapter_kind.into(),
                adapter_version: "rrflow-hook-v1".into(),
                enforcement_level: fixture.enforcement_level,
                scope: REASONING_SCOPE.into(),
                payload,
                read_stamp: None,
                trace: LifecycleTraceContextV1 {
                    trace_id: format!("{at:032x}"),
                    span_id: format!("{at:016x}"),
                    parent_span_id: None,
                },
            },
            at,
        )
        .unwrap();
    }
}
