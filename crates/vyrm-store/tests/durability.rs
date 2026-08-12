//! Durability under process termination. `SPEC.md` §12.
//!
//! The prior runtime emitted `"persistMode": "sync_all"` on every write with no
//! test verifying it. These tests are that verification: a child process writes,
//! is terminated with SIGKILL, and the surviving state is inspected from a fresh
//! open.
//!
//! SIGKILL is used deliberately. It runs no destructor and no unwinding, so
//! nothing in the shutdown path can contribute to what survives.
//!
//! These tests require a real block device. `/tmp` is tmpfs on some hosts, where
//! `SyncAll` never reaches a disk and the result would be meaningless, so the
//! database is placed beside the compiled binary in the target directory.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use vyrm_store::Store;

/// A directory on the same filesystem as the build output, avoiding tmpfs.
fn scratch(name: &str) -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_BIN_EXE_durability-child"));
    path.pop();
    path.push("durability-scratch");
    path.push(name);
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("create scratch directory");
    path
}

/// Runs the child to readiness, then terminates it with SIGKILL.
fn run_and_kill(db: &PathBuf, count: usize, mode: &str) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_durability-child"))
        .arg(db)
        .arg(count.to_string())
        .arg(mode)
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn durability child");

    let stdout = child.stdout.take().expect("child stdout");
    let mut line = String::new();
    BufReader::new(stdout)
        .read_line(&mut line)
        .expect("read readiness signal");
    assert_eq!(line.trim(), "READY", "child did not reach readiness");

    // SIGKILL: no destructors, no unwinding, no shutdown path.
    child.kill().expect("kill child");
    child.wait().expect("reap child");
}

#[test]
fn flushed_claims_survive_sigkill() {
    let db = scratch("flushed");
    run_and_kill(&db, 500, "flush");

    let store = Store::open(&db).expect("reopen after kill");
    assert_eq!(
        store.sequence().unwrap(),
        500,
        "flush returned Ok but claims did not survive termination"
    );
}

#[test]
fn unflushed_claims_are_not_claimed_as_durable() {
    let db = scratch("unflushed");
    // The child uses a one-hour interval in this mode, so nothing can have been
    // committed by the timer before the kill.
    run_and_kill(&db, 500, "noflush");

    let store = Store::open(&db).expect("reopen after kill");
    assert_eq!(
        store.sequence().unwrap(),
        0,
        "claims were durable before flush returned, contradicting the documented contract"
    );
}

#[test]
fn the_sequence_index_agrees_with_the_watermark_after_sigkill() {
    // The index entry and the claim are written in one transaction, so a
    // termination between them must be impossible. A watermark that outran the
    // index would make every sequence-derived reconstruction short.
    let db = scratch("index-consistency");
    run_and_kill(&db, 500, "flush");

    let store = Store::open(&db).expect("reopen after kill");
    let watermark = store.sequence().unwrap();
    let scanned = store.all_claims().unwrap().len();
    assert_eq!(watermark, 500);
    assert_eq!(
        scanned, watermark as usize,
        "sequence index and watermark diverged across termination"
    );
}

#[test]
fn an_unflushed_index_is_as_empty_as_the_claims_it_indexes() {
    let db = scratch("index-unflushed");
    run_and_kill(&db, 500, "noflush");

    let store = Store::open(&db).expect("reopen after kill");
    assert_eq!(store.sequence().unwrap(), 0);
    assert_eq!(
        store.all_claims().unwrap().len(),
        0,
        "index entries survived a termination that the claims did not"
    );
}

#[test]
fn a_reopened_store_continues_the_sequence_rather_than_restarting_it() {
    let db = scratch("continued");
    run_and_kill(&db, 100, "flush");
    run_and_kill(&db, 100, "flush");

    let store = Store::open(&db).expect("reopen after kill");
    assert_eq!(
        store.sequence().unwrap(),
        200,
        "sequence restarted after termination, which would overwrite claims"
    );
}
