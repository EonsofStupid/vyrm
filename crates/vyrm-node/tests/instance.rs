use std::path::{Path, PathBuf};
use vyrm_node::{InstanceBinding, InstanceManifest, InstanceMode, INSTANCE_FILE};

#[test]
fn dedicated_initialization_is_versioned_relocatable_and_idempotent() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("major-platform");
    std::fs::create_dir(&root).unwrap();

    let (created, was_created) = InstanceManifest::ensure_dedicated(&root).unwrap();
    assert!(was_created);
    assert_eq!(created.id, "major-platform");
    assert_eq!(created.mode, InstanceMode::Dedicated);
    assert_eq!(created.members, [PathBuf::from(".")]);

    let raw = std::fs::read_to_string(root.join(INSTANCE_FILE)).unwrap();
    assert!(raw.contains("format = 1"));
    assert!(
        !raw.contains(parent.path().to_string_lossy().as_ref()),
        "manifest must be relocatable"
    );

    let (loaded, was_created) = InstanceManifest::ensure_dedicated(&root).unwrap();
    assert!(!was_created);
    assert_eq!(loaded, created);

    let moved = parent.path().join("moved-platform");
    std::fs::rename(&root, &moved).unwrap();
    let rebound = InstanceBinding::discover(&moved).unwrap();
    assert_eq!(
        rebound.manifest.id, "major-platform",
        "identity survives relocation"
    );
    assert_eq!(rebound.project_root, std::fs::canonicalize(&moved).unwrap());
}

#[test]
fn umbrella_membership_is_explicit() {
    let manifest = InstanceManifest::umbrella(
        "small-tools",
        [PathBuf::from("formatter"), PathBuf::from("linter")],
    )
    .unwrap();
    assert!(manifest.admits(Path::new("formatter")));
    assert!(manifest.admits(Path::new("linter")));
    assert!(!manifest.admits(Path::new("unlisted-neighbor")));
}

#[test]
fn invalid_or_ambiguous_topologies_fail_closed() {
    assert!(InstanceManifest::umbrella("x", []).is_err());
    assert!(InstanceManifest::umbrella("x", [PathBuf::from("../outside")]).is_err());
    assert!(InstanceManifest::umbrella("x", [PathBuf::from("/outside")]).is_err());
    assert!(InstanceManifest::umbrella("x", [PathBuf::from(".vyrm/store")]).is_err());
    assert!(InstanceManifest::umbrella("x", [PathBuf::from(".")]).is_err());
    assert!(
        InstanceManifest::umbrella("x", [PathBuf::from("same"), PathBuf::from("same")]).is_err()
    );

    let mut dedicated = InstanceManifest::dedicated("x").unwrap();
    dedicated.members.push(PathBuf::from("another"));
    assert!(dedicated.validate().is_err());
}

#[test]
fn unknown_fields_and_versions_are_rejected() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join(".vyrm")).unwrap();
    std::fs::write(
        root.path().join(INSTANCE_FILE),
        "format = 99\nid = \"future\"\nmode = \"dedicated\"\nmembers = [\".\"]\n",
    )
    .unwrap();
    assert!(InstanceManifest::load(root.path())
        .unwrap_err()
        .to_string()
        .contains("unsupported"));

    std::fs::write(
        root.path().join(INSTANCE_FILE),
        "format = 1\nid = \"x\"\nmode = \"dedicated\"\nmembers = [\".\"]\nsurprise = true\n",
    )
    .unwrap();
    assert!(InstanceManifest::load(root.path())
        .unwrap_err()
        .to_string()
        .contains("unknown field"));
}

#[test]
fn nearest_manifest_binds_dedicated_roots_and_denies_neighbors() {
    let estate = tempfile::tempdir().unwrap();
    let project = estate.path().join("platform");
    let neighbor = estate.path().join("neighbor");
    std::fs::create_dir(&project).unwrap();
    std::fs::create_dir(&neighbor).unwrap();
    InstanceManifest::ensure_dedicated(&project).unwrap();

    let binding = InstanceBinding::discover(&project).unwrap();
    assert_eq!(
        binding.project_root,
        std::fs::canonicalize(&project).unwrap()
    );
    assert_eq!(binding.member, Path::new("."));
    assert!(InstanceBinding::discover(&neighbor)
        .unwrap_err()
        .to_string()
        .contains("no vyrm instance"));
}

#[test]
fn umbrella_binding_requires_exact_members_and_execution_stays_postponed() {
    let root = tempfile::tempdir().unwrap();
    let listed = root.path().join("listed");
    let unlisted = root.path().join("unlisted");
    std::fs::create_dir(&listed).unwrap();
    std::fs::create_dir(&unlisted).unwrap();
    std::fs::create_dir(root.path().join(".vyrm")).unwrap();
    let manifest = InstanceManifest::umbrella("tools", [PathBuf::from("listed")]).unwrap();
    std::fs::write(
        root.path().join(INSTANCE_FILE),
        toml::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let binding = InstanceBinding::discover(&listed).unwrap();
    assert_eq!(binding.member, Path::new("listed"));
    assert!(binding
        .require_runtime_ready()
        .unwrap_err()
        .to_string()
        .contains("postponed"));
    assert!(InstanceBinding::discover(&unlisted)
        .unwrap_err()
        .to_string()
        .contains("not an explicit member"));
}

#[test]
fn a_foreign_store_cannot_be_paired_with_an_instance() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    InstanceManifest::ensure_dedicated(first.path()).unwrap();
    InstanceManifest::ensure_dedicated(second.path()).unwrap();
    let foreign = second.path().join(".vyrm/store");

    let binding = InstanceBinding::discover(first.path()).unwrap();
    let error = binding.verify_store_path(&foreign).unwrap_err().to_string();
    assert!(error.contains("does not belong"));
    assert!(error.contains(&binding.manifest.id));
}
