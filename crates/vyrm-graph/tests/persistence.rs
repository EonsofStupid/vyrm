//! Index persistence through `vyrm-store`, replacing per-process rebuild.
//!
//! The index serializes whole into the store's `projections` keyspace. On
//! load, `refresh` is the arbiter of staleness — the persisted mtime and
//! length let an unchanged file skip its read exactly as within one process —
//! and `ground` remains available to prove the loaded projection agrees with
//! a rebuild. Absence of a stored projection is a recovery path (rebuild),
//! not an error.

use std::path::Path;
use vyrm_graph::{Index, Profile};
use vyrm_store::Store;

const PROJECTION: &str = "graph_index";

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

fn rewrite(path: &Path, body: &str) {
    std::fs::write(path, body).unwrap();
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(2);
    let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    file.set_modified(later).unwrap();
}

fn save(store: &Store, index: &Index) {
    store.put_projection(PROJECTION, &index.to_bytes()).unwrap();
}

fn load(store: &Store) -> Option<Index> {
    store
        .get_projection(PROJECTION)
        .unwrap()
        .map(|bytes| Index::from_bytes(&bytes).unwrap())
}

#[test]
fn a_reloaded_index_answers_identically_without_re_extraction() {
    let dir = project(&[
        ("src/a.rs", "pub fn alpha() {}\npub fn shared_name() {}"),
        ("src/b.rs", "pub fn beta() { crate::a::shared_name(); }"),
    ]);
    let profile = Profile::attune(dir.path()).unwrap();
    let index = Index::build(&profile).unwrap();

    let store_dir = tempfile::tempdir().unwrap();
    {
        let store = Store::open(store_dir.path()).unwrap();
        save(&store, &index);
    }

    // A fresh Store models the next process.
    let store = Store::open(store_dir.path()).unwrap();
    let mut loaded = load(&store).expect("projection was stored");

    let refresh = loaded.refresh(&profile).unwrap();
    assert!(refresh.is_noop(), "an unchanged tree must load without work: {}", refresh.render());
    assert_eq!(
        refresh.skipped_unread, 2,
        "persisted mtime and length must let unchanged files skip their reads"
    );
    assert_eq!(loaded.generation(), index.generation(), "generation must survive the round trip");
    assert_eq!(
        loaded.route("shared_name", 5),
        index.route("shared_name", 5),
        "a reloaded index must answer exactly as the index that was saved"
    );
}

#[test]
fn a_change_made_while_the_process_was_down_is_caught_on_load() {
    let dir = project(&[("src/a.rs", "pub fn alpha() {}")]);
    let profile = Profile::attune(dir.path()).unwrap();
    let index = Index::build(&profile).unwrap();

    let store_dir = tempfile::tempdir().unwrap();
    let store = Store::open(store_dir.path()).unwrap();
    save(&store, &index);

    // The tree moves on while no process is running.
    rewrite(&dir.path().join("src/a.rs"), "pub fn alpha() {}\npub fn added_offline() {}");

    let mut loaded = load(&store).expect("projection was stored");
    let refresh = loaded.refresh(&profile).unwrap();
    assert_eq!(refresh.changed, 1, "the offline change must be re-extracted: {}", refresh.render());
    assert!(
        !loaded.route("added_offline", 5).is_empty(),
        "the loaded index must answer for the offline change after refresh"
    );

    let grounding = loaded.ground(&profile).unwrap();
    assert!(grounding.agreed, "{}", grounding.render());
}

#[test]
fn a_missing_projection_is_absence_not_an_error() {
    let store_dir = tempfile::tempdir().unwrap();
    let store = Store::open(store_dir.path()).unwrap();
    assert!(store.get_projection(PROJECTION).unwrap().is_none());
}
