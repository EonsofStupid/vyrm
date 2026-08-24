use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

struct WorkspaceMetadata {
    root: PathBuf,
    packages: BTreeMap<String, PackageDependencies>,
}

struct PackageDependencies {
    workspace: BTreeSet<String>,
    all: BTreeSet<String>,
}

#[test]
fn engine_has_exact_stage_one_composition() {
    let metadata = workspace_metadata();
    assert_exact(
        production_workspace_dependencies(&metadata, "rrflow-engine"),
        names(&[
            "rrd-contract",
            "rrd-estate",
            "rrd-security",
            "vyrm-core",
            "vyrm-mx",
            "vyrm-ql",
            "rrd-store",
            "vyrm-vector",
        ]),
        "rrflow-engine production workspace dependencies",
    );
}

#[test]
fn daemon_depends_only_on_engine_and_contract() {
    let metadata = workspace_metadata();
    assert_exact(
        production_workspace_dependencies(&metadata, "rrd-server"),
        names(&["rrd-contract", "rrflow-engine"]),
        "rrd-server production workspace dependencies",
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
fn pending_consumer_bypasses_match_frozen_debt() {
    let metadata = workspace_metadata();
    let internal_components = names(&[
        "rrd-estate",
        "rrd-security",
        "vyrm-cluster",
        "vyrm-core",
        "vyrm-embed",
        "vyrm-graph",
        "vyrm-kv",
        "vyrm-mx",
        "vyrm-node",
        "vyrm-operator",
        "vyrm-ql",
        "rrd-store",
        "vyrm-vector",
    ]);

    assert_exact(
        &forbidden_edges(&metadata, "connectome-ui", &internal_components),
        names(&[
            "rrd-estate",
            "vyrm-cluster",
            "vyrm-core",
            "vyrm-mx",
            "vyrm-node",
            "vyrm-ql",
            "rrd-store",
            "vyrm-vector",
        ]),
        "Connectome's frozen pre-migration engine bypasses",
    );
    assert_exact(
        &forbidden_edges(&metadata, "vyrm-cli", &internal_components),
        names(&["vyrm-core", "vyrm-node", "rrd-store"]),
        "CLI's frozen pre-migration engine bypasses",
    );
    assert_exact(
        &forbidden_edges(&metadata, "vyrmd", &internal_components),
        names(&["vyrm-core", "vyrm-node", "rrd-store"]),
        "MCP adapter's frozen pre-migration engine bypasses",
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
    let observed = production_dependencies(&metadata, "rrflow-engine")
        .intersection(&forbidden)
        .cloned()
        .collect::<BTreeSet<_>>();
    assert!(
        observed.is_empty(),
        "rrflow-engine must remain transport-independent; forbidden direct dependencies: {observed:?}"
    );
}

#[test]
fn legacy_service_name_is_absent() {
    let metadata = workspace_metadata();
    let forbidden = ["Rrd", "Service"].concat();
    let mut violations = Vec::new();

    for relative in ["crates/rrflow-engine", "crates/rrd-server"] {
        collect_rust_sources(&metadata.root.join(relative), &mut violations, &forbidden);
    }

    violations.sort();
    assert!(
        violations.is_empty(),
        "the legacy service symbol must not survive as an alias, declaration, import, or use; found in: {violations:#?}"
    );
}

#[test]
fn legacy_store_package_and_namespace_are_absent() {
    let metadata = workspace_metadata();
    let legacy_package = ["vyrm", "-store"].concat();
    let legacy_namespace = ["vyrm", "_store"].concat();
    assert!(
        !metadata.packages.contains_key(&legacy_package),
        "the pre-release storage package must be named rrd-store"
    );

    let mut violations = Vec::new();
    collect_active_sources(
        &metadata.root.join("crates"),
        &mut violations,
        &[legacy_package.as_str(), legacy_namespace.as_str()],
    );
    collect_active_sources(
        &metadata.root.join(".github/workflows"),
        &mut violations,
        &[legacy_package.as_str(), legacy_namespace.as_str()],
    );
    let workspace_manifest = metadata.root.join("Cargo.toml");
    let manifest = fs::read_to_string(&workspace_manifest).expect("workspace manifest is readable");
    if manifest.contains(&legacy_package) || manifest.contains(&legacy_namespace) {
        violations.push(workspace_manifest);
    }
    violations.sort();
    assert!(
        violations.is_empty(),
        "active source still uses the retired storage identity: {violations:#?}"
    );
}

fn workspace_metadata() -> WorkspaceMetadata {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .expect("rrflow-engine must live under the workspace crates directory");
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
    for package in package_values {
        let id = package["id"].as_str().expect("package IDs must be strings");
        if !member_ids.contains(id) {
            continue;
        }
        let name = package["name"]
            .as_str()
            .expect("package names must be strings");
        let mut workspace = BTreeSet::new();
        let mut all = BTreeSet::new();
        for dependency in package["dependencies"]
            .as_array()
            .expect("package dependencies must be an array")
        {
            if dependency["kind"].as_str() == Some("dev") {
                continue;
            }
            let dependency_name = dependency["name"]
                .as_str()
                .expect("dependency names must be strings");
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

    WorkspaceMetadata { root, packages }
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

fn collect_active_sources(directory: &Path, violations: &mut Vec<PathBuf>, forbidden: &[&str]) {
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
            if entry.file_name() != "target" {
                collect_active_sources(&path, violations, forbidden);
            }
            continue;
        }
        if !matches!(
            path.extension().and_then(|extension| extension.to_str()),
            Some("rs" | "toml" | "yaml" | "yml")
        ) {
            continue;
        }
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
        if forbidden.iter().any(|value| source.contains(value)) {
            violations.push(path);
        }
    }
}
