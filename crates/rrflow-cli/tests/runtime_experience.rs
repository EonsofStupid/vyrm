//! `PLAN.md` Step P acceptance, driven through the compiled binary: the
//! scripted session transcript. Recall arrives before reasoning; a run's
//! outcome becomes a claim and the re-run retires it; a quarantined
//! projection denies mutation until reset; init writes real wiring and
//! refuses a dead harness.

#[path = "../../rrd-engine/tests/support/planned_lifecycle.rs"]
mod planned_lifecycle;

use rrd_core::{RuntimeMutation, RuntimeValue, ScopeId};
use rrd_engine::LifecycleEnforcementLevelV1;
use rrd_store::{Engine, PersistentEngine};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use planned_lifecycle::{seed_planned_lifecycle, PlannedLifecycleFixture};

fn rrflow(db: &Path, args: &[&str], stdin_json: Option<&str>) -> (bool, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rrflow"))
        .arg("--db")
        .arg(db)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn rrflow");
    child
        .stdin
        .as_mut()
        .expect("stdin piped")
        .write_all(stdin_json.unwrap_or("").as_bytes())
        .expect("write stdin");
    let output = child.wait_with_output().expect("run rrflow");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

fn scratch(name: &str) -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_BIN_EXE_rrflow"));
    path.pop();
    path.push("runtime-scratch");
    path.push(name);
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("create scratch directory");
    std::fs::create_dir_all(path.join(".rrflow")).expect("create instance directory");
    std::fs::write(
        path.join(".rrflow/instance.toml"),
        format!(
            "format = 1\nid = {:?}\nmode = \"dedicated\"\nmembers = [\".\"]\n",
            name
        ),
    )
    .expect("write instance manifest");
    path
}

fn initialize_git_fixture(root: &Path) {
    std::fs::write(root.join(".gitignore"), ".rrflow/rrd/\n").unwrap();
    for arguments in [
        vec!["init", "--quiet"],
        vec!["config", "user.email", "rrflow@example.invalid"],
        vec!["config", "user.name", "RRFlow Test"],
        vec!["add", "."],
        vec!["commit", "--quiet", "-m", "fixture"],
    ] {
        let status = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(arguments)
            .status()
            .unwrap();
        assert!(status.success());
    }
}

fn declare_attempt(db: &Path, run: &str, action: &str) {
    for payload in [
        serde_json::json!({
            "kind": "goal",
            "statement": "mutate project",
            "acceptance": ["verified"]
        }),
        serde_json::json!({
            "kind": "plan",
            "hypothesis": "the declared mutation is needed",
            "steps": ["mutate", "verify"]
        }),
        serde_json::json!({
            "kind": "attempt",
            "summary": "execute declared mutation",
            "actions": [action]
        }),
    ] {
        let payload = payload.to_string();
        let (ok, _, err) = rrflow(
            db,
            &["reasoning", "record", "--run", run, "--payload", &payload],
            None,
        );
        assert!(ok, "could not declare reasoning attempt: {err}");
    }
}

#[test]
fn work_plan_is_enforced_verified_and_replayed_through_the_real_cli() {
    let root = scratch("workplan-project");
    let db = root.join(".rrflow/rrd");
    std::fs::write(root.join("lib.rs"), "pub fn item() {}\n").unwrap();
    std::fs::write(
        root.join("rrflow.workplan.toml"),
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
title = "Enforce"
depends_on = []
acceptance = ["real CLI reopens verified state"]

[[item]]
id = "G00-W02"
gate = "G00"
title = "Qualify"
depends_on = ["G00-W01"]
acceptance = ["qualification verifies an unchanged tree without a fake mutation"]
"#,
    )
    .unwrap();
    let plan_file = root.join("implementation-plan.md");
    std::fs::write(
        &plan_file,
        "# Reviewed plan\n\nMake and verify one bounded change.\n",
    )
    .unwrap();
    initialize_git_fixture(&root);
    let root_str = root.to_str().unwrap();

    let (ok, out, err) = rrflow(
        &db,
        &["hook", "session-start", "--root", root_str],
        Some("{}"),
    );
    assert!(ok, "preflight failed: {err}");
    assert!(out.contains("work plan: foundation"), "{out}");

    let (ok, _, err) = rrflow(&db, &["work-plan", "sync", "--root", root_str], None);
    assert!(ok, "work-plan sync failed: {err}");
    let (ok, _, err) = rrflow(
        &db,
        &[
            "work-plan",
            "activate",
            "--plan",
            "foundation",
            "--item",
            "G00-W01",
        ],
        None,
    );
    assert!(ok, "work-plan activate failed: {err}");
    let verification_argv = serde_json::to_string(&vec![
        env!("CARGO_BIN_EXE_rrflow").to_owned(),
        "--help".to_owned(),
    ])
    .unwrap();
    let (ok, _, err) = rrflow(
        &db,
        &[
            "work-plan",
            "record",
            "--root",
            root_str,
            "--plan",
            "foundation",
            "--item",
            "G00-W01",
            "--plan-file",
            plan_file.to_str().unwrap(),
            "--verify-argv",
            &verification_argv,
        ],
        None,
    );
    assert!(ok, "work-plan record failed: {err}");

    declare_attempt(&db, "workplan-run", "RRFlowExec");
    let store = PersistentEngine::open(&db).unwrap();
    let receipt = rrd_engine::load_attunement_receipt(&store, &root)
        .unwrap()
        .unwrap();
    let lifecycle_start = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64)
        .saturating_sub(100);
    seed_planned_lifecycle(
        &store,
        &receipt,
        PlannedLifecycleFixture {
            project_id: "workplan-project",
            session_id: "session-cli-workplan-1",
            plan_id: "foundation",
            work_item_id: "G00-W01",
            reasoning_run_id: "workplan-run",
            actor: "hook:rrflow-exec",
            adapter_kind: "rrflow-exec",
            enforcement_level: LifecycleEnforcementLevelV1::Proxied,
            start_at: lifecycle_start,
        },
    );
    drop(store);
    let (ok, out, err) = rrflow(
        &db,
        &[
            "exec",
            "--root",
            root_str,
            "--session-id",
            "session-cli-workplan-1",
            "--",
            "git",
            "-C",
            root_str,
            "status",
            "--short",
        ],
        None,
    );
    assert!(
        ok,
        "planned exact command failed: stdout={out} stderr={err}"
    );
    assert!(
        out.is_empty(),
        "clean repository unexpectedly changed: {out}"
    );

    let (ok, out, err) = rrflow(
        &db,
        &[
            "work-plan",
            "verify",
            "--root",
            root_str,
            "--plan",
            "foundation",
        ],
        None,
    );
    assert!(ok, "work-plan verification failed: {err}");
    assert!(out.contains("verified=1/2"), "{out}");

    let (ok, _, err) = rrflow(
        &db,
        &[
            "work-plan",
            "activate",
            "--plan",
            "foundation",
            "--item",
            "G00-W02",
        ],
        None,
    );
    assert!(ok, "qualification activate failed: {err}");
    let (ok, _, err) = rrflow(
        &db,
        &[
            "work-plan",
            "record",
            "--root",
            root_str,
            "--plan",
            "foundation",
            "--item",
            "G00-W02",
            "--plan-file",
            plan_file.to_str().unwrap(),
            "--qualification-only",
            "--verify-argv",
            &verification_argv,
        ],
        None,
    );
    assert!(ok, "qualification record failed: {err}");
    let (ok, out, err) = rrflow(
        &db,
        &[
            "work-plan",
            "verify",
            "--root",
            root_str,
            "--plan",
            "foundation",
        ],
        None,
    );
    assert!(ok, "qualification verification failed: {err}");
    assert!(out.contains("verified=2/2"), "{out}");
    let (ok, out, err) = rrflow(&db, &["work-plan", "status", "--plan", "foundation"], None);
    assert!(ok, "reopened work-plan status failed: {err}");
    assert!(out.contains("[x] G00-W01"), "{out}");
    assert!(out.contains("[x] G00-W02"), "{out}");
}

#[test]
fn a_scripted_session_recall_arrives_before_reasoning_and_runs_are_journaled() {
    let root = scratch("session-project");
    let db = root.join(".rrflow/rrd");
    std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"demo\"\n").unwrap();

    // The estate holds one fact before the session starts.
    rrflow(
        &db,
        &[
            "assert",
            "--subject",
            "deploy",
            "--predicate",
            "status",
            "--object",
            "blocked-on-migration",
            "--valid-from",
            "1000",
        ],
        None,
    );

    // Session start: the preflight injects stack, fact, and — because the
    // adapter has never been audited — the drift alarm.
    let root_str = root.to_str().unwrap();
    let (ok, out, err) = rrflow(
        &db,
        &[
            "hook",
            "session-start",
            "--harness",
            "claude-code",
            "--root",
            root_str,
        ],
        Some("{}"),
    );
    assert!(ok, "session-start failed: {err}");
    assert!(out.contains("stack=cargo"), "stack not detected: {out}");
    assert!(
        out.contains("blocked-on-migration"),
        "recall not injected before reasoning: {out}"
    );
    assert!(
        out.contains("never been audited"),
        "drift alarm silent on unaudited adapter: {out}"
    );

    // The audit silences the alarm.
    rrflow(
        &db,
        &[
            "harness",
            "audit",
            "--name",
            "claude-code",
            "--evidence",
            "hooks reference checked in test",
        ],
        None,
    );
    let (_, out, _) = rrflow(
        &db,
        &[
            "hook",
            "session-start",
            "--harness",
            "claude-code",
            "--root",
            root_str,
        ],
        Some("{}"),
    );
    assert!(
        !out.contains("WARNING"),
        "audited adapter still warns: {out}"
    );

    // A prompt naming a known subject gets its recall; one naming nothing
    // injects nothing at all.
    let (ok, out, _) = rrflow(
        &db,
        &["hook", "user-prompt-submit", "--root", root_str],
        Some(r#"{"prompt": "why is deploy stuck?"}"#),
    );
    assert!(ok);
    assert!(
        out.contains("blocked-on-migration"),
        "prompt recall missing: {out}"
    );
    let (ok, out, _) = rrflow(
        &db,
        &["hook", "user-prompt-submit", "--root", root_str],
        Some(r#"{"prompt": "hello there"}"#),
    );
    assert!(ok);
    assert!(
        out.is_empty(),
        "unmatched prompt must inject nothing: {out:?}"
    );

    // The application journal: an exact pre-tool authorization binds the
    // failing observation before it becomes a claim.
    declare_attempt(&db, "scripted-cargo-test", "Bash");
    let cargo_test = format!(
        r#"{{"tool_name":"Bash","tool_input":{{"command":"cargo test --lib"}},"cwd":{root_str:?}}}"#
    );
    let (ok, out, err) = rrflow(
        &db,
        &["hook", "pre-tool-use", "--root", root_str],
        Some(&cargo_test),
    );
    assert!(ok, "pre-tool-use failed: {err}");
    assert!(out.is_empty(), "declared Bash attempt was denied: {out}");
    let failing = format!(
        r#"{{"tool_name":"Bash","tool_input":{{"command":"cargo test --lib"}},"tool_response":{{"exitCode":101}},"cwd":{root_str:?}}}"#
    );
    let (ok, _, err) = rrflow(&db, &["hook", "post-tool-use"], Some(&failing));
    assert!(ok, "post-tool-use failed: {err}");
    let (_, out, _) = rrflow(
        &db,
        &["as-of", "--subject", "cargo-test", "--predicate", "status"],
        None,
    );
    assert!(
        out.contains("failing (exit 101)"),
        "run not journaled: {out}"
    );

    // The required verification Bash call receives its own exact
    // authorization, and the passing re-run retires the failed claim.
    let verification_decision = serde_json::json!({
        "kind": "decision",
        "decision": "verify",
        "rationale": "rerun the same command to verify the observed failure"
    })
    .to_string();
    let (ok, _, err) = rrflow(
        &db,
        &[
            "reasoning",
            "record",
            "--run",
            "scripted-cargo-test",
            "--payload",
            &verification_decision,
        ],
        None,
    );
    assert!(ok, "could not advance the run to verification: {err}");
    let (ok, out, err) = rrflow(
        &db,
        &["hook", "pre-tool-use", "--root", root_str],
        Some(&cargo_test),
    );
    assert!(ok, "verification pre-tool-use failed: {err}");
    assert!(out.is_empty(), "verification Bash call was denied: {out}");
    let passing = format!(
        r#"{{"tool_name":"Bash","tool_input":{{"command":"cargo test --lib"}},"tool_response":{{"exitCode":0}},"cwd":{root_str:?}}}"#
    );
    rrflow(&db, &["hook", "post-tool-use"], Some(&passing));
    let (_, out, _) = rrflow(
        &db,
        &["as-of", "--subject", "cargo-test", "--predicate", "status"],
        None,
    );
    assert!(out.contains("passing"), "re-run did not supersede: {out}");
    let (_, out, _) = rrflow(
        &db,
        &[
            "history",
            "--subject",
            "cargo-test",
            "--predicate",
            "status",
        ],
        None,
    );
    assert!(
        out.contains("failing") && out.contains("passing"),
        "history must keep both readings: {out}"
    );
    let history: serde_json::Value = {
        let (_, out, _) = rrflow(
            &db,
            &[
                "history",
                "--subject",
                "cargo-test",
                "--predicate",
                "status",
                "--json",
            ],
            None,
        );
        serde_json::from_str(&out).unwrap()
    };
    assert!(
        history.as_array().unwrap().iter().any(|claim| {
            claim["object"]
                .as_str()
                .is_some_and(|object| object.contains("failing"))
                && claim["valid_to"].is_number()
        }),
        "the failing run must have an explicit retirement interval: {history}"
    );

    // Hook dispatches are recorded with trigger `event` — automation that
    // still cannot forget to record itself.
    let (_, out, _) = rrflow(&db, &["invocations"], None);
    assert!(
        out.contains("event"),
        "hook invocations must record trigger event: {out}"
    );
}

#[test]
fn the_wait_gate_denies_mutation_while_quarantined_and_reset_reopens() {
    let root = scratch("gated-project");
    let db = root.join(".rrflow/rrd");
    let root_str = root.to_str().unwrap();
    rrflow(
        &db,
        &[
            "assert",
            "--subject",
            "wp3",
            "--predicate",
            "status",
            "--object",
            "active",
            "--valid-from",
            "1000",
        ],
        None,
    );
    rrflow(&db, &["rebuild"], None);
    let edit = r#"{"tool_name":"Edit","tool_input":{"file_path":"x.rs"}}"#;

    // Deny by default: healthy storage is not authority to mutate without a
    // declared goal, plan, and attempt.
    let (ok, out, err) = rrflow(
        &db,
        &["hook", "pre-tool-use", "--root", root_str],
        Some(edit),
    );
    assert!(ok, "policy hook failed: {err}");
    assert!(out.contains("\"permissionDecision\":\"deny\""));
    assert!(out.contains("contract differential"));
    assert!(out.contains("no active run"));

    // Once the attempt is explicit, healthy gates stay out of the way.
    declare_attempt(&db, "projection-gate", "Edit");
    let (ok, _, err) = rrflow(&db, &["preflight", "--root", root_str], None);
    assert!(ok, "attunement preflight failed: {err}");
    let (ok, out, _) = rrflow(
        &db,
        &["hook", "pre-tool-use", "--root", root_str],
        Some(edit),
    );
    assert!(ok);
    assert!(out.is_empty(), "healthy projection must not gate: {out:?}");

    // Corrupt the stored projection, ground, quarantine.
    {
        let store = PersistentEngine::open(&db).unwrap();
        let bytes = store
            .get_projection(rrd_store::CURRENT_PROJECTION)
            .unwrap()
            .unwrap();
        let corrupted = String::from_utf8(bytes)
            .unwrap()
            .replacen("\"active\"", "\"drifted\"", 1);
        store
            .put_projection(rrd_store::CURRENT_PROJECTION, corrupted.as_bytes())
            .unwrap();
    }
    let (_, out, _) = rrflow(&db, &["ground"], None);
    assert!(
        out.contains("DIVERGENCE"),
        "grounding missed the corruption: {out}"
    );

    // The gate: mutation is denied with the reason and the way out.
    let (ok, out, err) = rrflow(
        &db,
        &["hook", "pre-tool-use", "--root", root_str],
        Some(edit),
    );
    assert!(ok, "gate hook errored: {err}");
    assert!(
        out.contains("\"permissionDecision\":\"deny\""),
        "no deny decision: {out}"
    );
    assert!(out.contains("quarantined"), "reason must say why: {out}");
    assert!(
        out.contains("reset-projection"),
        "reason must say the way out: {out}"
    );

    // A read-only tool passes even under quarantine: waiting applies to
    // mutation, not to looking.
    let read = r#"{"tool_name":"Read","tool_input":{"file_path":"x.rs"}}"#;
    let (ok, out, _) = rrflow(
        &db,
        &["hook", "pre-tool-use", "--root", root_str],
        Some(read),
    );
    assert!(ok);
    assert!(out.is_empty(), "reads are not gated: {out:?}");

    // Reset reopens the gate.
    rrflow(&db, &["reset-projection"], None);
    let (ok, out, _) = rrflow(
        &db,
        &["hook", "pre-tool-use", "--root", root_str],
        Some(edit),
    );
    assert!(ok);
    assert!(out.is_empty(), "reset must reopen the gate: {out:?}");
}

#[test]
fn routing_is_refreshed_before_mutation_and_corruption_has_an_explicit_recovery() {
    let root = scratch("routing-gated-project");
    let db = root.join(".rrflow/rrd");
    let source = root.join("lib.rs");
    std::fs::write(&source, "pub fn before() {}\n").unwrap();
    let root_str = root.to_str().unwrap();
    declare_attempt(&db, "routing-gate", "Edit");

    let (ok, out, err) = rrflow(&db, &["preflight", "--root", root_str], None);
    assert!(ok, "preflight failed: {err}");
    assert!(
        out.contains("routing: built generation 1"),
        "routing was not composed: {out}"
    );

    std::fs::write(&source, "pub fn after_() {}\n").unwrap();
    let edit = format!(r#"{{"tool_name":"Edit","tool_input":{{"file_path":{source:?}}}}}"#);
    let (ok, out, err) = rrflow(
        &db,
        &["hook", "pre-tool-use", "--root", root_str],
        Some(&edit),
    );
    assert!(ok, "freshness hook failed: {err}");
    assert!(out.contains("\"permissionDecision\":\"deny\""));
    assert!(out.contains("attunement is absent or stale"));
    let (ok, _, err) = rrflow(&db, &["preflight", "--root", root_str], None);
    assert!(ok, "re-attunement failed after the source change: {err}");
    let (ok, out, err) = rrflow(
        &db,
        &["hook", "pre-tool-use", "--root", root_str],
        Some(&edit),
    );
    assert!(ok, "re-attuned freshness hook failed: {err}");
    assert!(out.is_empty(), "fresh re-attunement should allow: {out}");

    {
        let store = PersistentEngine::open(&db).unwrap();
        store
            .put_projection(rrd_engine::routing::ROUTING_PROJECTION, b"corrupt")
            .unwrap();
    }
    let (ok, out, err) = rrflow(
        &db,
        &["hook", "pre-tool-use", "--root", root_str],
        Some(&edit),
    );
    assert!(ok, "a gate decision is a successful hook response: {err}");
    assert!(out.contains("\"permissionDecision\":\"deny\""));
    assert!(out.contains("reset-routing"));

    let (ok, out, err) = rrflow(&db, &["reset-routing", "--root", root_str], None);
    assert!(ok, "reset-routing failed: {err}");
    assert!(out.contains("routing projection rebuilt"));
    let (ok, _, err) = rrflow(&db, &["preflight", "--root", root_str], None);
    assert!(ok, "re-attunement after routing recovery failed: {err}");
    let (_, out, _) = rrflow(
        &db,
        &["hook", "pre-tool-use", "--root", root_str],
        Some(&edit),
    );
    assert!(
        out.is_empty(),
        "explicit routing reset must reopen the gate: {out}"
    );
}

#[test]
fn exact_argv_exec_is_authorized_observed_and_cannot_reuse_the_attempt() {
    let root = scratch("exact-argv-project");
    let db = root.join(".rrflow/rrd");
    std::fs::write(root.join("lib.rs"), "pub fn exact_argv() {}\n").unwrap();
    initialize_git_fixture(&root);

    declare_attempt(&db, "exact-argv", "RRFlowExec");
    let root_str = root.to_str().unwrap();
    let (ok, _, err) = rrflow(&db, &["preflight", "--root", root_str], None);
    assert!(ok, "preflight failed: {err}");

    let exact = [
        "exec", "--root", root_str, "--", "git", "-C", root_str, "status", "--short",
    ];
    let (ok, out, err) = rrflow(&db, &exact, None);
    assert!(ok, "exact argv execution failed: stdout={out} stderr={err}");

    let (ok, _, err) = rrflow(&db, &exact, None);
    assert!(
        !ok,
        "one reasoning attempt executed the exact command twice"
    );
    assert!(
        err.contains("NeedsDecision") || err.contains("record a continue"),
        "retry did not fail at the lifecycle state gate: {err}"
    );

    let store = PersistentEngine::open(&db).unwrap();
    let exec = store
        .invocations_since(0)
        .unwrap()
        .into_iter()
        .find(|invocation| {
            invocation.command == "exec" && matches!(invocation.outcome, rrd_store::Outcome::Ok)
        })
        .expect("successful exec invocation");
    assert!(
        exec.arguments
            .iter()
            .any(|argument| argument.starts_with("exact_argv_sha256=")),
        "exec invocation did not retain its bounded argv identity: {:?}",
        exec.arguments
    );
}

#[test]
fn init_writes_real_wiring_idempotently_and_refuses_a_dead_harness() {
    let root = scratch("init-project");
    let db = root.join(".rrflow/rrd");
    let root_str = root.to_str().unwrap();

    let (ok, out, err) = rrflow(
        &db,
        &["init", "--harness", "claude-code", "--root", root_str],
        None,
    );
    assert!(ok, "init failed: {err}");
    assert!(out.contains("wrote"), "nothing written: {out}");
    assert!(
        out.contains("runtime trace contract ready"),
        "trace bootstrap was not reported: {out}"
    );

    {
        let store = PersistentEngine::open(&db).unwrap();
        let scope = ScopeId::new(rrd_engine::REASONING_SCOPE).unwrap();
        let schema = store.runtime_schema(&scope).unwrap().unwrap();
        assert!(schema
            .events
            .contains_key(&rrd_core::RuntimeTraceEvent::event_type().unwrap()));
        let trace = store
            .runtime_changes_since(0, usize::MAX, Some(&scope))
            .unwrap()
            .changes
            .into_iter()
            .find_map(|change| match change.mutation {
                RuntimeMutation::Event { event } if event.kind.as_str() == "runtime_trace" => {
                    Some(event)
                }
                _ => None,
            })
            .expect("init must persist its trace event");
        assert_eq!(
            trace.properties["phase"],
            RuntimeValue::String("annotation".into())
        );
        assert_eq!(
            trace.properties["outcome"],
            RuntimeValue::String("ok".into())
        );
    }

    let instance = std::fs::read_to_string(root.join(".rrflow/instance.toml")).unwrap();
    assert!(instance.contains("format = 1"));
    assert!(instance.contains("mode = \"dedicated\""));
    assert!(instance.contains("members = [\".\"]"));

    let settings = std::fs::read_to_string(root.join(".claude/settings.json")).unwrap();
    for expected in [
        "SessionStart",
        "startup|resume|compact",
        "UserPromptSubmit",
        "PreToolUse",
        "PostToolUse",
        "hook session-start",
    ] {
        assert!(
            settings.contains(expected),
            "{expected} missing from wiring:\n{settings}"
        );
    }
    let context = std::fs::read_to_string(root.join("CLAUDE.md")).unwrap();
    assert!(
        context.contains("RRFlow project context"),
        "context block missing:\n{context}"
    );

    // Idempotent: a second init replaces the block rather than stacking it.
    rrflow(
        &db,
        &["init", "--harness", "claude-code", "--root", root_str],
        None,
    );
    let context = std::fs::read_to_string(root.join("CLAUDE.md")).unwrap();
    assert_eq!(
        context.matches("rrflow:begin").count(),
        1,
        "init must be idempotent:\n{context}"
    );

    // The registry's closed interval refuses to wire.
    let (ok, _, err) = rrflow(
        &db,
        &["init", "--harness", "gemini-cli", "--root", root_str],
        None,
    );
    assert!(!ok, "a retired harness must refuse init");
    assert!(
        err.contains("retired"),
        "refusal must state the retirement: {err}"
    );

    // And the status board states every axis.
    let (ok, out, err) = rrflow(&db, &["harness", "status"], None);
    assert!(ok, "status failed: {err}");
    assert!(
        out.contains("RETIRED"),
        "gemini-cli's closed interval missing: {out}"
    );
    assert!(
        out.contains("per_usage") && out.contains("subscription"),
        "billing axes missing: {out}"
    );
}

#[test]
fn query_command_is_project_bound_observer_safe_and_durably_traced() {
    let root = scratch("query-project");
    let db = root.join(".rrflow/rrd");
    let root_str = root.to_str().unwrap();
    let (ok, _, err) = rrflow(
        &db,
        &["init", "--harness", "claude-code", "--root", root_str],
        None,
    );
    assert!(ok, "init failed: {err}");

    let ql = "FROM event:runtime_trace AT VALID 18446744073709551615 KNOWN HEAD PROJECT name, phase EXPLAIN CONTRACT";
    let (ok, out, err) = rrflow(
        &db,
        &[
            "--json",
            "query",
            "--root",
            root_str,
            "--ql",
            ql,
            "--parameter",
            "unused=\"super-secret-query-marker\"",
        ],
        None,
    );
    assert!(ok, "query failed: {err}");
    let result: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(result["execution"]["known_at_cursor"], 2);
    assert_eq!(result["execution"]["returned_rows"], 1);
    assert_eq!(result["execution"]["scanned_changes"], 2);

    let store = PersistentEngine::open(&db).unwrap();
    assert_eq!(store.runtime_cursor().unwrap(), 12);
    let changes = store
        .runtime_changes_since(
            0,
            usize::MAX,
            Some(&ScopeId::new(rrd_engine::REASONING_SCOPE).unwrap()),
        )
        .unwrap();
    let encoded_changes = serde_json::to_string(&changes).unwrap();
    assert!(!encoded_changes.contains("super-secret-query-marker"));
    let invocations = serde_json::to_string(&store.invocations_since(0).unwrap()).unwrap();
    assert!(!invocations.contains("super-secret-query-marker"));
    assert!(!invocations.contains(ql));
    assert!(encoded_changes.contains("rrd.query.parse_bind"));
    assert!(encoded_changes.contains("rrd.query.plan"));
    assert!(encoded_changes.contains("rrd.query.execute"));
    assert!(encoded_changes.contains("rrd.lsm.runtime_read"));
}

#[test]
fn runtime_entry_points_refuse_cross_instance_state() {
    let first = scratch("isolation-first");
    let second = scratch("isolation-second");
    let foreign_db = first.join(".rrflow/rrd");
    let second_root = second.to_str().unwrap();

    let (ok, _, err) = rrflow(&foreign_db, &["preflight", "--root", second_root], None);
    assert!(!ok, "a foreign store/root pairing must fail");
    assert!(err.contains("does not belong"), "wrong refusal: {err}");
}

#[test]
fn traces_are_enableable_off_by_default_and_never_touch_stdout() {
    let root = scratch("traced-project");
    let db = root.join(".rrflow/rrd");

    // Off by default: a normal invocation emits nothing on stderr.
    let (ok, _, err) = rrflow(
        &db,
        &[
            "assert",
            "--subject",
            "wp3",
            "--predicate",
            "status",
            "--object",
            "quiet",
            "--valid-from",
            "1000",
        ],
        None,
    );
    assert!(ok);
    assert!(err.is_empty(), "no subscriber, no trace output: {err:?}");

    // Enabled: spans appear on stderr, with the counts the reports compute.
    let output = Command::new(env!("CARGO_BIN_EXE_rrflow"))
        .arg("--db")
        .arg(&db)
        .args([
            "assert",
            "--subject",
            "wp3",
            "--predicate",
            "status",
            "--object",
            "traced",
            "--valid-from",
            "2000",
        ])
        .env("RRFLOW_TRACE", "rrd_store=debug")
        .stdin(Stdio::null())
        .output()
        .expect("run rrflow traced");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stderr.contains("append_batch"),
        "span missing from stderr: {stderr}"
    );
    assert!(
        stderr.contains("record_invocation"),
        "recording span missing: {stderr}"
    );
    assert!(
        !stdout.contains("append_batch"),
        "stdout is the answer channel and must never carry diagnostics: {stdout}"
    );

    // JSON format for machine consumption.
    let output = Command::new(env!("CARGO_BIN_EXE_rrflow"))
        .arg("--db")
        .arg(&db)
        .args(["rebuild"])
        .env("RRFLOW_TRACE", "rrd_store=debug")
        .env("RRFLOW_TRACE_FORMAT", "json")
        .stdin(Stdio::null())
        .output()
        .expect("run rrflow json-traced");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let json_line = stderr
        .lines()
        .find(|l| l.contains("rebuild advanced"))
        .expect("rebuild span in json output");
    assert!(
        serde_json::from_str::<serde_json::Value>(json_line).is_ok(),
        "trace lines must parse as JSON: {json_line}"
    );
}
