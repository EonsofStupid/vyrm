use super::*;
use rrd_store::PersistentEngine;

fn definition() -> WorkPlanDefinition {
    toml::from_str(
        r#"
schema_version = 1
plan_id = "foundation"
title = "Foundation"
authority = "RRD"
status_policy = "Evidence only"

[[gate]]
id = "G00"
title = "Control"
depends_on = []

[[item]]
id = "G00-W01"
gate = "G00"
title = "Contract"
depends_on = []
acceptance = ["reopens"]

[[item]]
id = "G00-W02"
gate = "G00"
title = "Events"
depends_on = ["G00-W01"]
acceptance = ["rejects invalid transitions"]
"#,
    )
    .unwrap()
}

fn sha(byte: char) -> String {
    byte.to_string().repeat(64)
}

fn plan_record(item: &str) -> WorkItemPlanRecord {
    WorkItemPlanRecord {
        work_item_id: item.into(),
        source_tree_sha256: sha('a'),
        attunement_receipt_sha256: sha('b'),
        plan_payload_sha256: sha('c'),
        verification_commands: vec![vec!["cargo".into(), "test".into()]],
    }
}

fn verification(item: &str, passed: bool) -> WorkItemVerification {
    WorkItemVerification {
        work_item_id: item.into(),
        source_tree_sha256: sha('a'),
        plan_payload_sha256: sha('c'),
        repository_revision: "commit-1".into(),
        platform: "linux-x86_64".into(),
        checks: vec![WorkItemVerificationCheck {
            name: "targeted tests".into(),
            argv: vec!["cargo".into(), "test".into()],
            passed,
            evidence_sha256: sha('e'),
        }],
    }
}

#[test]
fn persistent_work_plan_denies_skips_and_replays_verified_state() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("rrd");
    let store = PersistentEngine::open(&path).unwrap();

    let installed = install_work_plan(&store, definition(), 1, "test", "load-1").unwrap();
    assert_eq!(installed.event_sequence, 1);
    assert!(activate_work_item(&store, "foundation", "G00-W02", 2, "test", "skip-1").is_err());

    activate_work_item(&store, "foundation", "G00-W01", 3, "test", "activate-1").unwrap();
    record_work_item_plan(
        &store,
        "foundation",
        plan_record("G00-W01"),
        4,
        "test",
        "plan-1",
    )
    .unwrap();
    assert!(authorize_work_item_tool(
        &store,
        "foundation",
        "G00-W01",
        &sha('f'),
        &sha('b'),
        &sha('d'),
        WorkPlanEventContext {
            now: 5,
            actor: "test",
            correlation_id: "stale-tree",
        }
    )
    .is_err());
    authorize_work_item_tool(
        &store,
        "foundation",
        "G00-W01",
        &sha('a'),
        &sha('b'),
        &sha('d'),
        WorkPlanEventContext {
            now: 6,
            actor: "test",
            correlation_id: "authorize-1",
        },
    )
    .unwrap();
    assert!(authorize_work_item_tool(
        &store,
        "foundation",
        "G00-W01",
        &sha('a'),
        &sha('b'),
        &sha('f'),
        WorkPlanEventContext {
            now: 7,
            actor: "test",
            correlation_id: "authorize-2",
        }
    )
    .is_err());
    complete_work_item_tool(
        &store,
        "foundation",
        "G00-W01",
        &sha('d'),
        &sha('e'),
        WorkPlanEventContext {
            now: 8,
            actor: "test",
            correlation_id: "complete-1",
        },
    )
    .unwrap();
    assert!(complete_work_item_tool(
        &store,
        "foundation",
        "G00-W01",
        &sha('d'),
        &sha('e'),
        WorkPlanEventContext {
            now: 9,
            actor: "test",
            correlation_id: "complete-again",
        }
    )
    .is_err());
    assert!(verify_work_item(
        &store,
        "foundation",
        verification("G00-W01", false),
        10,
        "test",
        "verify-failed"
    )
    .is_err());
    let verified = verify_work_item(
        &store,
        "foundation",
        verification("G00-W01", true),
        11,
        "test",
        "verify-1",
    )
    .unwrap();
    assert!(verified.active_item_id.is_none());
    assert_eq!(verified.items[0].status, WorkItemStatus::Verified);
    drop(store);

    let reopened = PersistentEngine::open(&path).unwrap();
    let replayed = load_work_plan(&reopened, "foundation").unwrap().unwrap();
    assert_eq!(replayed, verified);
    let advanced =
        activate_work_item(&reopened, "foundation", "G00-W02", 12, "test", "activate-2").unwrap();
    assert_eq!(advanced.active_item_id.as_deref(), Some("G00-W02"));
}

#[test]
fn installed_plan_is_idempotent_and_rejects_unreviewed_source_changes() {
    let root = tempfile::tempdir().unwrap();
    let store = PersistentEngine::open(root.path()).unwrap();
    let plan = definition();
    let first = install_work_plan(&store, plan.clone(), 1, "test", "load-1").unwrap();
    let replay = install_work_plan(&store, plan.clone(), 2, "test", "load-2").unwrap();
    assert_eq!(first, replay);

    let mut changed = plan;
    changed.title = "Changed without revision".into();
    assert!(install_work_plan(&store, changed, 3, "test", "load-3").is_err());
}

#[test]
fn repository_foundation_plan_is_a_valid_closed_dependency_graph() {
    let plan: WorkPlanDefinition =
        toml::from_str(include_str!("../../../../../rrflow.workplan.toml")).unwrap();
    plan.validate().unwrap();
    assert_eq!(plan.gate.len(), 13);
    assert_eq!(plan.item.len(), 65);
    assert_eq!(plan.plan_id, "rrflow-foundation");
}
