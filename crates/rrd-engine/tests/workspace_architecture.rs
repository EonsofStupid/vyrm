use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

struct WorkspaceMetadata {
    root: PathBuf,
    packages: BTreeMap<String, PackageDependencies>,
    target_sources: BTreeSet<PathBuf>,
}

struct PackageDependencies {
    workspace: BTreeSet<String>,
    all: BTreeSet<String>,
}

#[test]
fn engine_has_exact_composition() {
    let metadata = workspace_metadata();
    assert_exact(
        production_workspace_dependencies(&metadata, "rrd-engine"),
        names(&[
            "rrd-contract",
            "rrd-estate",
            "rrd-security",
            "rrd-cluster",
            "rrd-core",
            "rrd-graph",
            "rrd-inference",
            "rrd-operator-knowledge",
            "rrd-query",
            "rrd-store",
            "rrd-vector",
        ]),
        "rrd-engine production workspace dependencies",
    );
}

#[test]
fn daemon_depends_only_on_engine_and_contract() {
    let metadata = workspace_metadata();
    assert_exact(
        production_workspace_dependencies(&metadata, "rrd-server"),
        names(&["rrd-contract", "rrd-engine"]),
        "rrd-server production workspace dependencies",
    );
}

#[test]
fn mcp_adapter_uses_only_engine_and_public_protocol_boundaries() {
    let metadata = workspace_metadata();
    assert_exact(
        production_workspace_dependencies(&metadata, "rrflow-mcp"),
        names(&["rrd-client", "rrd-contract", "rrd-engine"]),
        "rrflow-mcp embedded/daemon production boundaries",
    );
}

#[test]
fn public_contract_and_client_stay_implementation_free() {
    let metadata = workspace_metadata();
    assert_exact(
        production_workspace_dependencies(&metadata, "rrd-contract"),
        BTreeSet::new(),
        "rrd-contract production workspace dependencies",
    );
    assert_exact(
        production_workspace_dependencies(&metadata, "rrd-client"),
        names(&["rrd-contract"]),
        "rrd-client production workspace dependencies",
    );
}

#[test]
fn outward_consumers_cannot_bypass_the_engine_boundary() {
    let metadata = workspace_metadata();
    let internal_components = names(&[
        "rrd-estate",
        "rrd-security",
        "rrd-cluster",
        "rrd-core",
        "rrd-inference",
        "rrd-graph",
        "rrd-lsm",
        "rrd-query",
        "rrd-operator-knowledge",
        "rrd-store",
        "rrd-vector",
    ]);

    assert_exact(
        &forbidden_edges(&metadata, "connectome-ui", &internal_components),
        BTreeSet::new(),
        "Connectome must use only rrd-client and rrd-contract",
    );
    assert_exact(
        &forbidden_edges(&metadata, "rrflow-cli", &internal_components),
        BTreeSet::new(),
        "CLI must use only the public embedded engine boundary",
    );
    assert_exact(
        &forbidden_edges(&metadata, "rrflow-mcp", &internal_components),
        BTreeSet::new(),
        "MCP adapter must use only the engine boundary",
    );
}

#[test]
fn engine_has_no_transport_dependencies() {
    let metadata = workspace_metadata();
    let forbidden = names(&[
        "axum",
        "hyper",
        "hyper-util",
        "k8s-openapi",
        "kube",
        "reqwest",
        "rustls",
        "rustls-pemfile",
        "tiny_http",
        "tokio-rustls",
        "tonic",
    ]);
    let observed = production_dependencies(&metadata, "rrd-engine")
        .intersection(&forbidden)
        .cloned()
        .collect::<BTreeSet<_>>();
    assert!(
        observed.is_empty(),
        "rrd-engine must remain transport-independent; forbidden direct dependencies: {observed:?}"
    );
}

#[test]
fn legacy_service_name_is_absent() {
    let metadata = workspace_metadata();
    let forbidden = ["Rrd", "Service"].concat();
    let mut violations = Vec::new();

    for relative in ["crates/rrd-engine", "crates/rrd-server"] {
        collect_rust_sources(&metadata.root.join(relative), &mut violations, &forbidden);
    }

    violations.sort();
    assert!(
        violations.is_empty(),
        "the legacy service symbol must not survive as an alias, declaration, import, or use; found in: {violations:#?}"
    );
}

#[test]
fn retired_pre_release_identity_is_absent_from_the_active_repository() {
    let metadata = workspace_metadata();
    let retired_brand = ["vy", "rm"].concat();
    let stale_composition = ["rrflow", "-engine"].concat();
    let stale_runtime = ["rrflow", "-runtime"].concat();
    let mut violations = Vec::new();
    collect_identity_violations(
        &metadata.root,
        &mut violations,
        &[
            retired_brand.as_bytes(),
            stale_composition.as_bytes(),
            stale_runtime.as_bytes(),
        ],
    );
    violations.sort();
    violations.dedup();
    assert!(
        violations.is_empty(),
        "retired or split-engine identity remains in the active repository: {violations:#?}"
    );
}

#[test]
fn every_workspace_target_source_is_tracked() {
    let metadata = workspace_metadata();
    let output = Command::new("git")
        .current_dir(&metadata.root)
        .args(["ls-files", "-z", "--cached"])
        .output()
        .expect("git ls-files must start");
    assert!(
        output.status.success(),
        "git ls-files failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let tracked = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| {
            PathBuf::from(std::str::from_utf8(path).expect("repository paths must be UTF-8"))
        })
        .collect::<BTreeSet<_>>();
    let missing = metadata
        .target_sources
        .difference(&tracked)
        .cloned()
        .collect::<Vec<_>>();

    assert!(
        missing.is_empty(),
        "Cargo target sources must exist in a clean checkout; untracked targets: {missing:#?}"
    );
}

fn workspace_metadata() -> WorkspaceMetadata {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .expect("rrd-engine must live under the workspace crates directory");
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));
    let output = Command::new(cargo)
        .current_dir(workspace_root)
        .args(["metadata", "--format-version=1", "--no-deps", "--locked"])
        .output()
        .expect("cargo metadata must start");
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let document: Value =
        serde_json::from_slice(&output.stdout).expect("cargo metadata must return JSON");
    let root = PathBuf::from(
        document["workspace_root"]
            .as_str()
            .expect("cargo metadata must identify the workspace root"),
    );
    let member_ids = document["workspace_members"]
        .as_array()
        .expect("cargo metadata must list workspace members")
        .iter()
        .map(|member| {
            member
                .as_str()
                .expect("workspace member IDs must be strings")
        })
        .collect::<BTreeSet<_>>();
    let package_values = document["packages"]
        .as_array()
        .expect("cargo metadata must list packages");
    let workspace_names = package_values
        .iter()
        .filter(|package| {
            member_ids.contains(package["id"].as_str().expect("package IDs must be strings"))
        })
        .map(|package| {
            package["name"]
                .as_str()
                .expect("package names must be strings")
        })
        .collect::<BTreeSet<_>>();

    assert_eq!(
        workspace_names.len(),
        member_ids.len(),
        "workspace package names must be unique"
    );

    let mut packages = BTreeMap::new();
    let mut target_sources = BTreeSet::new();
    for package in package_values {
        let id = package["id"].as_str().expect("package IDs must be strings");
        if !member_ids.contains(id) {
            continue;
        }
        let name = package["name"]
            .as_str()
            .expect("package names must be strings");
        let manifest_path = PathBuf::from(
            package["manifest_path"]
                .as_str()
                .expect("package manifest paths must be strings"),
        );
        let package_directory_name = manifest_path
            .parent()
            .and_then(Path::file_name)
            .and_then(|value| value.to_str())
            .expect("workspace package manifests must have UTF-8 parent directories");
        assert_eq!(
            package_directory_name, name,
            "workspace package {name} must live in a same-named directory; forwarding package boundaries are forbidden"
        );
        for target in package["targets"]
            .as_array()
            .expect("package targets must be an array")
        {
            let source = PathBuf::from(
                target["src_path"]
                    .as_str()
                    .expect("target source paths must be strings"),
            );
            let relative = source
                .strip_prefix(&root)
                .unwrap_or_else(|_| {
                    panic!(
                        "workspace target source {} must be under {}",
                        source.display(),
                        root.display()
                    )
                })
                .to_path_buf();
            target_sources.insert(relative);
        }
        let mut workspace = BTreeSet::new();
        let mut all = BTreeSet::new();
        for dependency in package["dependencies"]
            .as_array()
            .expect("package dependencies must be an array")
        {
            let dependency_name = dependency["name"]
                .as_str()
                .expect("dependency names must be strings");
            if workspace_names.contains(dependency_name) {
                assert!(
                    dependency["rename"].is_null(),
                    "workspace package {name} must depend on {dependency_name} by its canonical package name; Cargo rename aliases are forbidden"
                );
            }
            if dependency["kind"].as_str() == Some("dev") {
                continue;
            }
            all.insert(dependency_name.to_owned());
            if workspace_names.contains(dependency_name) {
                workspace.insert(dependency_name.to_owned());
            }
        }
        assert!(
            packages
                .insert(name.to_owned(), PackageDependencies { workspace, all })
                .is_none(),
            "duplicate workspace package name {name}"
        );
    }

    WorkspaceMetadata {
        root,
        packages,
        target_sources,
    }
}

fn production_workspace_dependencies<'a>(
    metadata: &'a WorkspaceMetadata,
    package: &str,
) -> &'a BTreeSet<String> {
    &metadata
        .packages
        .get(package)
        .unwrap_or_else(|| panic!("workspace package {package} is absent"))
        .workspace
}

fn production_dependencies<'a>(
    metadata: &'a WorkspaceMetadata,
    package: &str,
) -> &'a BTreeSet<String> {
    &metadata
        .packages
        .get(package)
        .unwrap_or_else(|| panic!("workspace package {package} is absent"))
        .all
}

fn forbidden_edges(
    metadata: &WorkspaceMetadata,
    package: &str,
    forbidden: &BTreeSet<String>,
) -> BTreeSet<String> {
    production_workspace_dependencies(metadata, package)
        .intersection(forbidden)
        .cloned()
        .collect()
}

fn assert_exact(actual: &BTreeSet<String>, expected: BTreeSet<String>, boundary: &str) {
    let added = actual.difference(&expected).cloned().collect::<Vec<_>>();
    let missing = expected.difference(actual).cloned().collect::<Vec<_>>();
    assert!(
        added.is_empty() && missing.is_empty(),
        "{boundary} changed; added={added:?}, missing={missing:?}, actual={actual:?}"
    );
}

fn names(values: &[&str]) -> BTreeSet<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn collect_rust_sources(directory: &Path, violations: &mut Vec<PathBuf>, forbidden: &str) {
    let mut entries = fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("cannot inspect {}: {error}", directory.display()))
        .collect::<Result<Vec<_>, _>>()
        .unwrap_or_else(|error| panic!("cannot enumerate {}: {error}", directory.display()));
    entries.sort_by_key(fs::DirEntry::path);

    for entry in entries {
        let path = entry.path();
        let file_type = entry
            .file_type()
            .unwrap_or_else(|error| panic!("cannot inspect {}: {error}", path.display()));
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            collect_rust_sources(&path, violations, forbidden);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            let source = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
            if source.contains(forbidden) {
                violations.push(path);
            }
        }
    }
}

fn collect_identity_violations(root: &Path, violations: &mut Vec<PathBuf>, forbidden: &[&[u8]]) {
    let output = Command::new("git")
        .current_dir(root)
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ])
        .output()
        .expect("git ls-files must start");
    assert!(
        output.status.success(),
        "git ls-files failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    for encoded in output.stdout.split(|byte| *byte == 0) {
        if encoded.is_empty() {
            continue;
        }
        let relative = PathBuf::from(
            std::str::from_utf8(encoded).expect("repository paths must be valid UTF-8"),
        );
        let path = root.join(&relative);
        if !path.is_file() {
            continue;
        }

        let path_bytes = relative.to_string_lossy().to_ascii_lowercase();
        let bytes = fs::read(&path)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
        let lowercase_bytes = bytes.iter().map(u8::to_ascii_lowercase).collect::<Vec<_>>();
        if forbidden.iter().any(|value| {
            contains_bytes(path_bytes.as_bytes(), value) || contains_bytes(&lowercase_bytes, value)
        }) {
            violations.push(relative);
        }
    }
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}
