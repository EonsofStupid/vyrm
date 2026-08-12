//! Operator surface behaviour. `SPEC.md` §13; `PLAN.md` Step 3.
//!
//! The tests drive the compiled binary, so they exercise the path an operator
//! uses rather than a test-only entry point. That is what makes the recording
//! guarantee meaningful: a command that forgot to record itself would pass a
//! library-level test and fail here.

use std::path::{Path, PathBuf};
use std::process::Command;

fn vyrm(db: &Path, args: &[&str]) -> (bool, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_vyrm"))
        .arg("--db")
        .arg(db)
        .args(args)
        .output()
        .expect("run vyrm");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

/// A directory beside the compiled binary, avoiding tmpfs.
fn scratch(name: &str) -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_BIN_EXE_vyrm"));
    path.pop();
    path.push("cli-scratch");
    path.push(name);
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("create scratch directory");
    path
}

#[test]
fn a_claim_can_be_asserted_and_resolved() {
    let db = scratch("assert-resolve");
    let (ok, out, err) = vyrm(
        &db,
        &["assert", "--subject", "wp3", "--predicate", "status", "--object", "in_progress",
          "--valid-from", "1000"],
    );
    assert!(ok, "assert failed: {err}");
    assert!(out.contains("sequence 1"), "unexpected output: {out}");

    let (ok, out, err) = vyrm(&db, &["as-of", "--subject", "wp3", "--predicate", "status", "--at", "2000"]);
    assert!(ok, "as-of failed: {err}");
    assert!(out.contains("in_progress"), "unexpected output: {out}");
}

#[test]
fn every_invocation_is_recorded_including_failures() {
    let db = scratch("recording");

    vyrm(&db, &["assert", "--subject", "wp3", "--predicate", "status", "--object", "v1"]);
    vyrm(&db, &["as-of", "--subject", "wp3", "--predicate", "status"]);
    vyrm(&db, &["status"]);
    // Invalid: the separator byte is rejected by the identifier type.
    let (ok, _, _) = vyrm(&db, &["as-of", "--subject", "", "--predicate", "status"]);
    assert!(!ok, "an empty subject must fail");

    let (ok, out, err) = vyrm(&db, &["invocations"]);
    assert!(ok, "invocations failed: {err}");

    for expected in ["assert", "as-of", "status"] {
        assert!(out.contains(expected), "{expected} missing from the log:\n{out}");
    }
    assert!(out.contains("error"), "the failed invocation was not recorded:\n{out}");
    // Five commands ran before this one, so this run is the sixth.
    assert!(out.contains("manual"), "trigger not recorded:\n{out}");
}

#[test]
fn the_recorded_log_is_queryable_as_json_with_arguments_and_outcome() {
    let db = scratch("queryable");
    vyrm(&db, &["assert", "--subject", "wp3", "--predicate", "status", "--object", "v1",
                "--valid-from", "500"]);

    let (ok, out, err) = vyrm(&db, &["invocations", "--json"]);
    assert!(ok, "invocations --json failed: {err}");
    let records: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
    let first = &records[0];

    assert_eq!(first["command"], "assert");
    assert_eq!(first["trigger"], "manual");
    assert_eq!(first["outcome"], "ok");
    assert!(first["ordinal"].as_u64().unwrap() >= 1);
    assert!(first["at"].as_u64().unwrap() > 0);

    // Arguments are recorded so a run can be reproduced from its record.
    let arguments = first["arguments"].as_array().unwrap();
    let joined: Vec<String> = arguments.iter().map(|a| a.as_str().unwrap().to_string()).collect();
    assert!(joined.contains(&"subject=wp3".to_string()), "arguments not recorded: {joined:?}");
    assert!(joined.contains(&"valid_from=500".to_string()), "arguments not recorded: {joined:?}");
}

#[test]
fn invocation_ordinals_are_monotonic_across_processes() {
    let db = scratch("ordinals");
    for _ in 0..5 {
        vyrm(&db, &["status"]);
    }
    let (_, out, _) = vyrm(&db, &["invocations", "--json"]);
    let records: serde_json::Value = serde_json::from_str(&out).unwrap();
    let ordinals: Vec<u64> = records
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["ordinal"].as_u64().unwrap())
        .collect();
    assert_eq!(ordinals, vec![1, 2, 3, 4, 5], "ordinals restarted or collided across processes");
}

#[test]
fn history_shows_supersession_newest_first() {
    let db = scratch("history");
    for (object, from) in [("blocked", "100"), ("in_progress", "200"), ("done", "300")] {
        vyrm(&db, &["assert", "--subject", "wp3", "--predicate", "status",
                    "--object", object, "--valid-from", from]);
    }
    let (ok, out, err) = vyrm(&db, &["history", "--subject", "wp3", "--predicate", "status"]);
    assert!(ok, "history failed: {err}");
    let done = out.find("done").expect("done missing");
    let blocked = out.find("blocked").expect("blocked missing");
    assert!(done < blocked, "history is not newest-first:\n{out}");
}

#[test]
fn a_read_through_the_surface_retains_the_pair_under_gc() {
    let db = scratch("gc-retention");
    vyrm(&db, &["assert", "--subject", "read", "--predicate", "status", "--object", "v1"]);
    vyrm(&db, &["assert", "--subject", "unread", "--predicate", "status", "--object", "v1"]);
    // Reading through the surface records an access, which must retain the pair.
    vyrm(&db, &["as-of", "--subject", "read", "--predicate", "status"]);

    let (ok, out, err) = vyrm(&db, &["gc", "--json"]);
    assert!(ok, "gc failed: {err}");
    let report: serde_json::Value = serde_json::from_str(&out).unwrap();

    let candidates: Vec<String> = report["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["subject"].as_str().unwrap().to_string())
        .collect();
    let retained: Vec<String> = report["retained"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["subject"].as_str().unwrap().to_string())
        .collect();

    assert_eq!(retained, vec!["read"]);
    assert_eq!(candidates, vec!["unread"]);
}

#[test]
fn status_reports_counters_that_track_the_commands_run() {
    let db = scratch("status");
    vyrm(&db, &["assert", "--subject", "a", "--predicate", "p", "--object", "1"]);
    vyrm(&db, &["assert", "--subject", "b", "--predicate", "p", "--object", "2"]);

    let (ok, out, err) = vyrm(&db, &["status", "--json"]);
    assert!(ok, "status failed: {err}");
    let status: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(status["claim_sequence"], 2);
    // Two asserts recorded before this status command.
    assert_eq!(status["invocations"], 2);
}

#[test]
fn an_absent_claim_is_reported_rather_than_treated_as_an_error() {
    let db = scratch("absent");
    let (ok, out, err) = vyrm(&db, &["as-of", "--subject", "nothing", "--predicate", "here"]);
    assert!(ok, "a missing claim must not be an error: {err}");
    assert!(out.contains("no claim in force"), "unexpected output: {out}");
}
