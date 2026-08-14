//! `PLAN.md` Step P acceptance, driven through the compiled binary: the
//! scripted session transcript. Recall arrives before reasoning; a run's
//! outcome becomes a claim and the re-run retires it; a quarantined
//! projection denies mutation until reset; init writes real wiring and
//! refuses a dead harness.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn vyrm(db: &Path, args: &[&str], stdin_json: Option<&str>) -> (bool, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_vyrm"))
        .arg("--db")
        .arg(db)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn vyrm");
    child
        .stdin
        .as_mut()
        .expect("stdin piped")
        .write_all(stdin_json.unwrap_or("").as_bytes())
        .expect("write stdin");
    let output = child.wait_with_output().expect("run vyrm");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

fn scratch(name: &str) -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_BIN_EXE_vyrm"));
    path.pop();
    path.push("runtime-scratch");
    path.push(name);
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("create scratch directory");
    path
}

#[test]
fn a_scripted_session_recall_arrives_before_reasoning_and_runs_are_journaled() {
    let root = scratch("session-project");
    let db = root.join(".vyrm/store");
    std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"demo\"\n").unwrap();

    // The estate holds one fact before the session starts.
    vyrm(&db, &["assert", "--subject", "deploy", "--predicate", "status", "--object",
                "blocked-on-migration", "--valid-from", "1000"], None);

    // Session start: the preflight injects stack, fact, and — because the
    // adapter has never been audited — the drift alarm.
    let root_str = root.to_str().unwrap();
    let (ok, out, err) = vyrm(
        &db,
        &["hook", "session-start", "--harness", "claude-code", "--root", root_str],
        Some("{}"),
    );
    assert!(ok, "session-start failed: {err}");
    assert!(out.contains("stack=cargo"), "stack not detected: {out}");
    assert!(out.contains("blocked-on-migration"), "recall not injected before reasoning: {out}");
    assert!(out.contains("never been audited"), "drift alarm silent on unaudited adapter: {out}");

    // The audit silences the alarm.
    vyrm(&db, &["harness", "audit", "--name", "claude-code", "--evidence",
                "hooks reference checked in test"], None);
    let (_, out, _) = vyrm(
        &db,
        &["hook", "session-start", "--harness", "claude-code", "--root", root_str],
        Some("{}"),
    );
    assert!(!out.contains("WARNING"), "audited adapter still warns: {out}");

    // A prompt naming a known subject gets its recall; one naming nothing
    // injects nothing at all.
    let (ok, out, _) = vyrm(
        &db,
        &["hook", "user-prompt-submit", "--root", root_str],
        Some(r#"{"prompt": "why is deploy stuck?"}"#),
    );
    assert!(ok);
    assert!(out.contains("blocked-on-migration"), "prompt recall missing: {out}");
    let (ok, out, _) = vyrm(
        &db,
        &["hook", "user-prompt-submit", "--root", root_str],
        Some(r#"{"prompt": "hello there"}"#),
    );
    assert!(ok);
    assert!(out.is_empty(), "unmatched prompt must inject nothing: {out:?}");

    // The application journal: a failing run becomes a claim…
    let failing = format!(
        r#"{{"tool_name":"Bash","tool_input":{{"command":"cargo test --lib"}},"tool_response":{{"exitCode":101}},"cwd":{root_str:?}}}"#
    );
    let (ok, _, err) = vyrm(&db, &["hook", "post-tool-use"], Some(&failing));
    assert!(ok, "post-tool-use failed: {err}");
    let (_, out, _) = vyrm(&db, &["as-of", "--subject", "cargo-test", "--predicate", "status"], None);
    assert!(out.contains("failing (exit 101)"), "run not journaled: {out}");

    // …and the passing re-run retires it by supersession.
    let passing = format!(
        r#"{{"tool_name":"Bash","tool_input":{{"command":"cargo test --lib"}},"tool_response":{{"exitCode":0}},"cwd":{root_str:?}}}"#
    );
    vyrm(&db, &["hook", "post-tool-use"], Some(&passing));
    let (_, out, _) = vyrm(&db, &["as-of", "--subject", "cargo-test", "--predicate", "status"], None);
    assert!(out.contains("passing"), "re-run did not supersede: {out}");
    let (_, out, _) = vyrm(&db, &["history", "--subject", "cargo-test", "--predicate", "status"], None);
    assert!(
        out.contains("failing") && out.contains("passing"),
        "history must keep both readings: {out}"
    );

    // Hook dispatches are recorded with trigger `event` — automation that
    // still cannot forget to record itself.
    let (_, out, _) = vyrm(&db, &["invocations"], None);
    assert!(out.contains("event"), "hook invocations must record trigger event: {out}");
}

#[test]
fn the_wait_gate_denies_mutation_while_quarantined_and_reset_reopens() {
    let root = scratch("gated-project");
    let db = root.join(".vyrm/store");
    vyrm(&db, &["assert", "--subject", "wp3", "--predicate", "status", "--object", "active",
                "--valid-from", "1000"], None);
    vyrm(&db, &["rebuild"], None);

    // Healthy: the gate stays out of the way.
    let edit = r#"{"tool_name":"Edit","tool_input":{"file_path":"x.rs"}}"#;
    let (ok, out, _) = vyrm(&db, &["hook", "pre-tool-use"], Some(edit));
    assert!(ok);
    assert!(out.is_empty(), "healthy projection must not gate: {out:?}");

    // Corrupt the stored projection, ground, quarantine.
    {
        let store = vyrm_store::Store::open(&db).unwrap();
        let bytes = store.get_projection(vyrm_store::CURRENT_PROJECTION).unwrap().unwrap();
        let corrupted = String::from_utf8(bytes).unwrap().replacen("\"active\"", "\"drifted\"", 1);
        store.put_projection(vyrm_store::CURRENT_PROJECTION, corrupted.as_bytes()).unwrap();
    }
    let (_, out, _) = vyrm(&db, &["ground"], None);
    assert!(out.contains("DIVERGENCE"), "grounding missed the corruption: {out}");

    // The gate: mutation is denied with the reason and the way out.
    let (ok, out, err) = vyrm(&db, &["hook", "pre-tool-use"], Some(edit));
    assert!(ok, "gate hook errored: {err}");
    assert!(out.contains("\"permissionDecision\":\"deny\""), "no deny decision: {out}");
    assert!(out.contains("quarantined"), "reason must say why: {out}");
    assert!(out.contains("reset-projection"), "reason must say the way out: {out}");

    // A read-only tool passes even under quarantine: waiting applies to
    // mutation, not to looking.
    let read = r#"{"tool_name":"Read","tool_input":{"file_path":"x.rs"}}"#;
    let (ok, out, _) = vyrm(&db, &["hook", "pre-tool-use"], Some(read));
    assert!(ok);
    assert!(out.is_empty(), "reads are not gated: {out:?}");

    // Reset reopens the gate.
    vyrm(&db, &["reset-projection"], None);
    let (ok, out, _) = vyrm(&db, &["hook", "pre-tool-use"], Some(edit));
    assert!(ok);
    assert!(out.is_empty(), "reset must reopen the gate: {out:?}");
}

#[test]
fn init_writes_real_wiring_idempotently_and_refuses_a_dead_harness() {
    let root = scratch("init-project");
    let db = root.join(".vyrm/store");
    let root_str = root.to_str().unwrap();

    let (ok, out, err) = vyrm(&db, &["init", "--harness", "claude-code", "--root", root_str], None);
    assert!(ok, "init failed: {err}");
    assert!(out.contains("wrote"), "nothing written: {out}");

    let settings = std::fs::read_to_string(root.join(".claude/settings.json")).unwrap();
    for expected in ["SessionStart", "startup|resume|compact", "UserPromptSubmit",
                     "PreToolUse", "PostToolUse", "hook session-start"] {
        assert!(settings.contains(expected), "{expected} missing from wiring:\n{settings}");
    }
    let context = std::fs::read_to_string(root.join("CLAUDE.md")).unwrap();
    assert!(context.contains("vyrm memory"), "context block missing:\n{context}");

    // Idempotent: a second init replaces the block rather than stacking it.
    vyrm(&db, &["init", "--harness", "claude-code", "--root", root_str], None);
    let context = std::fs::read_to_string(root.join("CLAUDE.md")).unwrap();
    assert_eq!(
        context.matches("vyrm:begin").count(),
        1,
        "init must be idempotent:\n{context}"
    );

    // The registry's closed interval refuses to wire.
    let (ok, _, err) = vyrm(&db, &["init", "--harness", "gemini-cli", "--root", root_str], None);
    assert!(!ok, "a retired harness must refuse init");
    assert!(err.contains("retired"), "refusal must state the retirement: {err}");

    // And the status board states every axis.
    let (ok, out, err) = vyrm(&db, &["harness", "status"], None);
    assert!(ok, "status failed: {err}");
    assert!(out.contains("RETIRED"), "gemini-cli's closed interval missing: {out}");
    assert!(out.contains("per_usage") && out.contains("subscription"), "billing axes missing: {out}");
}

#[test]
fn traces_are_enableable_off_by_default_and_never_touch_stdout() {
    let root = scratch("traced-project");
    let db = root.join(".vyrm/store");

    // Off by default: a normal invocation emits nothing on stderr.
    let (ok, _, err) = vyrm(&db, &["assert", "--subject", "wp3", "--predicate", "status",
                                   "--object", "quiet", "--valid-from", "1000"], None);
    assert!(ok);
    assert!(err.is_empty(), "no subscriber, no trace output: {err:?}");

    // Enabled: spans appear on stderr, with the counts the reports compute.
    let output = Command::new(env!("CARGO_BIN_EXE_vyrm"))
        .arg("--db").arg(&db)
        .args(["assert", "--subject", "wp3", "--predicate", "status",
               "--object", "traced", "--valid-from", "2000"])
        .env("VYRM_TRACE", "vyrm_store=debug")
        .stdin(Stdio::null())
        .output()
        .expect("run vyrm traced");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stderr.contains("append_batch"), "span missing from stderr: {stderr}");
    assert!(stderr.contains("record_invocation"), "recording span missing: {stderr}");
    assert!(
        !stdout.contains("append_batch"),
        "stdout is the answer channel and must never carry diagnostics: {stdout}"
    );

    // JSON format for machine consumption.
    let output = Command::new(env!("CARGO_BIN_EXE_vyrm"))
        .arg("--db").arg(&db)
        .args(["rebuild"])
        .env("VYRM_TRACE", "vyrm_store=debug")
        .env("VYRM_TRACE_FORMAT", "json")
        .stdin(Stdio::null())
        .output()
        .expect("run vyrm json-traced");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let json_line = stderr.lines().find(|l| l.contains("rebuild advanced"))
        .expect("rebuild span in json output");
    assert!(
        serde_json::from_str::<serde_json::Value>(json_line).is_ok(),
        "trace lines must parse as JSON: {json_line}"
    );
}
