use vyrm_core::{
    AuditDecision, ClaimReader, Predicate, Reader, ReasoningPayload, RuntimeMutation, RuntimeValue,
    ScopeId, Subject,
};
use vyrm_node::{
    handle, preflight, record_reasoning, HookContext, HookEvent, InstanceManifest,
    WorkflowObservation, WorkflowStatus, WORKFLOW_FILE,
};
use vyrm_store::{Engine, MemoryEngine, NativeEngine, Store};

#[derive(Debug, PartialEq)]
struct Evidence {
    observation: WorkflowObservation,
    audit_request_id: String,
    audit_scope: ScopeId,
    audit_outcome_cursor: Option<u64>,
    audit_previous_present: bool,
    traces: Vec<(String, String)>,
    cursor: u64,
    sequence: u64,
}

fn lifecycle_traces<E: Engine>(store: &E) -> Vec<(String, String)> {
    store
        .runtime_changes_since(
            0,
            usize::MAX,
            Some(&ScopeId::new(vyrm_node::REASONING_SCOPE).unwrap()),
        )
        .unwrap()
        .changes
        .into_iter()
        .filter_map(|change| match change.mutation {
            RuntimeMutation::Event { event } if event.kind.as_str() == "runtime_trace" => {
                let RuntimeValue::String(phase) = &event.properties["phase"] else {
                    panic!("trace phase has the wrong type")
                };
                let RuntimeValue::String(outcome) = &event.properties["outcome"] else {
                    panic!("trace outcome has the wrong type")
                };
                Some((phase.clone(), outcome.clone()))
            }
            _ => None,
        })
        .collect()
}

fn project(parent: &std::path::Path, id: &str, with_manifest: bool) -> std::path::PathBuf {
    let root = parent.join(id);
    std::fs::create_dir(&root).unwrap();
    InstanceManifest::ensure_dedicated(&root).unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"scripts":{"typecheck":"tsc"}}"#,
    )
    .unwrap();
    std::fs::write(root.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n").unwrap();
    std::fs::write(root.join("app.ts"), "export const answer = 42;\n").unwrap();
    if with_manifest {
        std::fs::write(
            root.join(WORKFLOW_FILE),
            format!(
                r#"format = 1

[[workflows]]
event = "package:pnpm:run:typecheck"
command = ["pnpm", "run", "typecheck"]
allow_arguments = false
scope = "{id}"
required_projections = ["source-routing"]
max_source_lag_generations = 0
verification = "exit_zero"
"#
            ),
        )
        .unwrap();
    }
    root
}

fn declare_attempt<E: Engine>(store: &E) {
    for (at, payload) in [
        (
            1,
            ReasoningPayload::Goal {
                statement: "verify the package workflow".into(),
                acceptance: vec!["declared typecheck passes".into()],
            },
        ),
        (
            2,
            ReasoningPayload::Plan {
                hypothesis: "the declared workflow is the canonical verifier".into(),
                steps: vec!["run declared package event".into()],
            },
        ),
        (
            3,
            ReasoningPayload::Attempt {
                summary: "execute package:pnpm:run:typecheck".into(),
                actions: vec!["pnpm run typecheck".into()],
            },
        ),
    ] {
        record_reasoning(store, "workflow-run", at, "agent:test", payload).unwrap();
    }
}

fn exercise<E: Engine>(store: &E, root: &std::path::Path) -> Evidence {
    declare_attempt(store);
    let reader = Reader::new("test:workflow").unwrap();
    let flight = preflight(store, root, Some("test"), &reader, 4, 1_500).unwrap();
    assert_eq!(flight.workflows.len(), 1);
    assert_eq!(flight.workflows[0].event, "package:pnpm:run:typecheck");
    assert!(flight.context.contains("[vyrm] workflow:"));

    let input = serde_json::json!({
        "tool_name": "Bash",
        "tool_input": {"command": "pnpm run typecheck"},
        "tool_response": {"exitCode": 0, "stdout": "types clean"}
    });
    let ctx = HookContext {
        store,
        root,
        harness: Some("test"),
        reader: &reader,
        now: 5,
        budget: 1_500,
    };
    let allowed = handle(&ctx, HookEvent::PreToolUse, &input).unwrap();
    assert!(allowed.stdout.is_empty());
    let detail = allowed.detail.unwrap();
    assert!(detail.contains("package:pnpm:run:typecheck"));
    assert!(detail.contains("lag=0"));

    let post_ctx = HookContext { now: 6, ..ctx };
    let post = handle(&post_ctx, HookEvent::PostToolUse, &input).unwrap();
    assert!(post.detail.unwrap().contains("committed atomically"));

    let subject = Subject::new("package:pnpm:run:typecheck").unwrap();
    let predicate = Predicate::new("status").unwrap();
    let claim = store.as_of(&subject, &predicate, 6).unwrap().unwrap();
    let observation: WorkflowObservation = serde_json::from_str(&claim.object).unwrap();
    assert_eq!(observation.status, WorkflowStatus::Passed);
    assert_eq!(observation.exit_code, Some(0));

    let scope = ScopeId::new(
        root.file_name()
            .and_then(|name| name.to_str())
            .unwrap()
            .to_owned(),
    )
    .unwrap();
    let page = store
        .runtime_changes_since(0, usize::MAX, Some(&scope))
        .unwrap();
    let change = page.changes.last().expect("workflow runtime change");
    assert!(matches!(change.mutation, RuntimeMutation::Claim { .. }));
    let audit = store
        .runtime_audit(&change.commit_id)
        .unwrap()
        .expect("atomic workflow audit");
    audit.validate().unwrap();
    assert_eq!(audit.scope, scope);
    assert_eq!(audit.decision, AuditDecision::Allow);

    Evidence {
        observation,
        audit_request_id: audit.request_id,
        audit_scope: audit.scope,
        audit_outcome_cursor: audit.outcome_cursor,
        audit_previous_present: audit.previous_digest.is_some(),
        traces: lifecycle_traces(store),
        cursor: store.runtime_cursor().unwrap(),
        sequence: store.sequence().unwrap(),
    }
}

#[test]
fn workflow_preflight_gate_and_atomic_evidence_match_all_engines() {
    let roots = tempfile::tempdir().unwrap();
    let root = project(roots.path(), "workflow-app", true);
    let memory = exercise(&MemoryEngine::new(), &root);
    let fjall = exercise(
        &Store::open(&root.join(".vyrm/fjall-store")).unwrap(),
        &root,
    );
    let native = exercise(
        &NativeEngine::open(&root.join(".vyrm/native-store")).unwrap(),
        &root,
    );

    assert_eq!(memory, fjall);
    assert_eq!(memory, native);
    assert_eq!(memory.observation.status, WorkflowStatus::Passed);
    assert_eq!(memory.sequence, 1);
    assert_eq!(
        memory.traces,
        [
            ("start".into(), "running".into()),
            ("finish".into(), "ok".into()),
            ("start".into(), "running".into()),
            ("finish".into(), "ok".into()),
        ]
    );
}

#[test]
fn hook_denies_package_execution_when_project_policy_is_absent() {
    let roots = tempfile::tempdir().unwrap();
    let root = project(roots.path(), "missing-policy", false);
    let store = MemoryEngine::new();
    declare_attempt(&store);
    let reader = Reader::new("test:workflow").unwrap();
    let ctx = HookContext {
        store: &store,
        root: &root,
        harness: Some("test"),
        reader: &reader,
        now: 5,
        budget: 1_500,
    };
    let input = serde_json::json!({
        "tool_name": "Bash",
        "tool_input": {"command": "pnpm run typecheck"}
    });
    let denied = handle(&ctx, HookEvent::PreToolUse, &input).unwrap();
    assert!(denied.stdout.contains("permissionDecision"));
    assert!(denied.stdout.contains("workflows.toml"));
    assert_eq!(
        lifecycle_traces(&store),
        [
            ("start".into(), "running".into()),
            ("finish".into(), "denied".into()),
        ]
    );

    let flight = preflight(&store, &root, Some("test"), &reader, 5, 1_500).unwrap();
    assert!(flight
        .warnings
        .iter()
        .any(|warning| warning.contains("package workflow policy is absent")));
}
