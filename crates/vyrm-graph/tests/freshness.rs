//! Incremental maintenance and grounding. `SPEC.md` §8.2 and §8.3.

use std::path::Path;
use vyrm_graph::{Index, Profile};

fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname=\"t\"\n").unwrap();
    for (name, body) in files {
        let path = dir.path().join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, body).unwrap();
    }
    dir
}

/// Writes a file and forces a distinguishable modification time. Refresh uses
/// mtime and length as its cheap filter, and both have one-second granularity on
/// some filesystems.
fn rewrite(path: &Path, body: &str) {
    std::fs::write(path, body).unwrap();
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(2);
    let _ = filetime_set(path, later);
}

fn filetime_set(path: &Path, when: std::time::SystemTime) -> std::io::Result<()> {
    let file = std::fs::OpenOptions::new().write(true).open(path)?;
    file.set_modified(when)?;
    Ok(())
}

#[test]
fn a_refresh_with_no_changes_reads_nothing_and_reports_a_noop() {
    let dir = project(&[("src/a.rs", "pub fn alpha() {}"), ("src/b.rs", "pub fn beta() {}")]);
    let profile = Profile::attune(dir.path()).unwrap();
    let mut index = Index::build(&profile).unwrap();
    let generation = index.generation();

    let refresh = index.refresh(&profile).unwrap();
    assert!(refresh.is_noop(), "expected a no-op, got {}", refresh.render());
    assert_eq!(refresh.skipped_unread, 2, "unchanged files were read anyway");
    assert_eq!(
        index.generation(),
        generation,
        "generation advanced without a change"
    );
}

#[test]
fn only_the_changed_file_is_re_extracted() {
    let dir = project(&[("src/a.rs", "pub fn alpha() {}"), ("src/b.rs", "pub fn beta() {}")]);
    let profile = Profile::attune(dir.path()).unwrap();
    let mut index = Index::build(&profile).unwrap();

    rewrite(&dir.path().join("src/a.rs"), "pub fn alpha() {}\npub fn alpha_two() {}");
    let refresh = index.refresh(&profile).unwrap();

    assert_eq!(refresh.changed, 1, "{}", refresh.render());
    assert_eq!(refresh.skipped_unread, 1, "the untouched file was read");
    assert_eq!(index.route("alpha_two", 5).len(), 1, "new symbol not routable");
}

#[test]
fn a_touched_but_unmodified_file_is_read_once_and_then_skipped() {
    let dir = project(&[("src/a.rs", "pub fn alpha() {}")]);
    let profile = Profile::attune(dir.path()).unwrap();
    let mut index = Index::build(&profile).unwrap();

    // Same content, new modification time: the cheap filter misses, the digest
    // catches it, and the refreshed stats let the next pass skip the read.
    rewrite(&dir.path().join("src/a.rs"), "pub fn alpha() {}");
    let first = index.refresh(&profile).unwrap();
    assert_eq!(first.read_but_identical, 1, "{}", first.render());
    assert!(first.is_noop(), "an identical rewrite counted as a change");

    let second = index.refresh(&profile).unwrap();
    assert_eq!(second.skipped_unread, 1, "stats were not updated after the read");
}

#[test]
fn additions_and_removals_are_tracked() {
    let dir = project(&[("src/a.rs", "pub fn alpha() {}")]);
    let profile = Profile::attune(dir.path()).unwrap();
    let mut index = Index::build(&profile).unwrap();
    assert_eq!(index.file_count(), 1);

    std::fs::write(dir.path().join("src/c.rs"), "pub fn gamma() {}").unwrap();
    let added = index.refresh(&profile).unwrap();
    assert_eq!(added.added, 1, "{}", added.render());
    assert_eq!(index.route("gamma", 5).len(), 1);

    std::fs::remove_file(dir.path().join("src/c.rs")).unwrap();
    let removed = index.refresh(&profile).unwrap();
    assert_eq!(removed.removed, 1, "{}", removed.render());
    assert!(index.route("gamma", 5).is_empty(), "a removed symbol stayed routable");
}

#[test]
fn grounding_agrees_with_a_full_rebuild_after_incremental_updates() {
    let dir = project(&[
        ("src/a.rs", "pub fn alpha() {}"),
        ("src/b.rs", "pub fn beta() {}"),
        ("src/c.rs", "pub struct Gamma;"),
    ]);
    let profile = Profile::attune(dir.path()).unwrap();
    let mut index = Index::build(&profile).unwrap();

    // A sequence of edits of every kind, applied incrementally.
    rewrite(&dir.path().join("src/a.rs"), "pub fn alpha() {}\npub fn alpha_two() {}");
    std::fs::write(dir.path().join("src/d.rs"), "pub trait Delta {}").unwrap();
    std::fs::remove_file(dir.path().join("src/b.rs")).unwrap();
    index.refresh(&profile).unwrap();

    let grounding = index.ground(&profile).unwrap();
    assert!(grounding.agreed, "{}", grounding.render());
}

#[test]
fn grounding_detects_a_stale_projection_rather_than_repairing_it() {
    let dir = project(&[("src/a.rs", "pub fn alpha() {}")]);
    let profile = Profile::attune(dir.path()).unwrap();
    let index = Index::build(&profile).unwrap();

    // Change the tree without refreshing: the index is now stale by construction.
    rewrite(&dir.path().join("src/a.rs"), "pub fn alpha() {}\npub fn alpha_two() {}");

    let grounding = index.ground(&profile).unwrap();
    assert!(!grounding.agreed, "a stale projection was reported as grounded");
    assert_eq!(grounding.differing.len(), 1);
    assert!(grounding.render().contains("DIVERGENCE"));

    // Grounding reports; it does not mutate. The index is still stale afterwards,
    // so the defect cannot be hidden by the check that found it.
    assert!(
        index.route("alpha_two", 5).is_empty(),
        "grounding silently repaired the projection"
    );
}

#[test]
fn route_fresh_applies_pending_changes_before_answering() {
    let dir = project(&[("src/a.rs", "pub fn alpha() {}")]);
    let profile = Profile::attune(dir.path()).unwrap();
    let mut index = Index::build(&profile).unwrap();

    rewrite(&dir.path().join("src/a.rs"), "pub fn alpha() {}\npub fn alpha_two() {}");

    // Routing without the barrier misses the edit; routing through it does not.
    assert!(index.route("alpha_two", 5).is_empty());
    let (refresh, routed) = index.route_fresh(&profile, "alpha_two", 5).unwrap();
    assert_eq!(refresh.changed, 1);
    assert_eq!(routed.len(), 1);
}
