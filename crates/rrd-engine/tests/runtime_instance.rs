use rrd_engine::{
    InstanceBinding, InstanceManifest, InstanceMode, RrdEngine, ServiceError, INSTANCE_FILE,
    PROJECT_AUTHORITY_FORMAT,
};
use std::path::{Path, PathBuf};

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
fn invalid_or_ambiguous_topologies_fail_closed() {
    let canonical = InstanceManifest::dedicated("x").unwrap();

    let mut missing_root = canonical.clone();
    missing_root.members.clear();
    assert!(missing_root.validate().is_err());

    let mut extra_project = canonical.clone();
    extra_project.members.push(PathBuf::from("another"));
    assert!(extra_project.validate().is_err());

    let mut replaced_root = canonical;
    replaced_root.members = vec![PathBuf::from("another")];
    assert!(replaced_root.validate().is_err());
}

#[test]
fn unknown_fields_and_versions_are_rejected() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join(".rrflow")).unwrap();
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

    std::fs::write(
        root.path().join(INSTANCE_FILE),
        "format = 1\nid = \"x\"\nmode = \"umbrella\"\nmembers = [\"project-a\"]\n",
    )
    .unwrap();
    assert!(InstanceManifest::load(root.path())
        .unwrap_err()
        .to_string()
        .contains("unknown variant"));
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
        .contains("no RRFlow instance"));
}

#[test]
fn nested_project_cannot_bind_to_a_parent_project_instance() {
    let root = tempfile::tempdir().unwrap();
    let nested = root.path().join("nested-project");
    std::fs::create_dir(&nested).unwrap();
    InstanceManifest::ensure_dedicated_as(root.path(), "parent-project").unwrap();

    assert!(InstanceBinding::discover(&nested)
        .unwrap_err()
        .to_string()
        .contains("admits only its project root"));
}

#[test]
fn a_foreign_store_cannot_be_paired_with_an_instance() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    InstanceManifest::ensure_dedicated(first.path()).unwrap();
    InstanceManifest::ensure_dedicated(second.path()).unwrap();
    let foreign = second.path().join(".rrflow/rrd");

    let binding = InstanceBinding::discover(first.path()).unwrap();
    let error = binding.verify_store_path(&foreign).unwrap_err().to_string();
    assert!(error.contains("does not belong"));
    assert!(error.contains(&binding.manifest.id));
}

#[test]
fn database_project_authority_is_persisted_reopen_safe_and_never_rebound_by_startup() {
    let parent = tempfile::tempdir().unwrap();
    let original = parent.path().join("project");
    std::fs::create_dir(&original).unwrap();
    InstanceManifest::ensure_dedicated(&original).unwrap();
    let binding = InstanceBinding::discover(&original).unwrap();

    let engine = RrdEngine::open_bound(&binding).unwrap();
    let authority = engine.project_authority_binding().unwrap().unwrap();
    assert_eq!(authority.format, PROJECT_AUTHORITY_FORMAT);
    assert_eq!(authority.instance_id.as_str(), binding.manifest.id);
    assert_eq!(
        authority.project_root,
        binding.project_root.to_str().unwrap()
    );
    authority.validate().unwrap();
    drop(engine);

    let reopened = RrdEngine::open_bound(&binding).unwrap();
    assert_eq!(
        reopened.project_authority_binding().unwrap().unwrap(),
        authority
    );
    drop(reopened);

    let moved = parent.path().join("moved");
    std::fs::rename(&original, &moved).unwrap();
    let moved_binding = InstanceBinding::discover(&moved).unwrap();
    assert!(matches!(
        RrdEngine::open_bound(&moved_binding),
        Err(ServiceError::ProjectBindingMismatch)
    ));
}
