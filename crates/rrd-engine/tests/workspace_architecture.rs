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
    targets: BTreeSet<String>,
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
fn outward_product_sources_do_not_import_physical_components() {
    let metadata = workspace_metadata();
    let physical = [
        "rrd_cluster",
        "rrd_core",
        "rrd_estate",
        "rrd_graph",
        "rrd_inference",
        "rrd_lsm",
        "rrd_operator_knowledge",
        "rrd_query",
        "rrd_security",
        "rrd_store",
        "rrd_vector",
    ];
    let mut violations = Vec::new();
    for relative in [
        "crates/rrd-client/src",
        "crates/rrd-server/src",
        "crates/rrflow-cli/src",
        "crates/rrflow-cli/examples",
        "crates/rrflow-mcp/src",
        "crates/connectome-ui/src",
    ] {
        let directory = metadata.root.join(relative);
        if directory.is_dir() {
            collect_physical_import_violations(
                &metadata.root,
                &directory,
                &physical,
                &mut violations,
            );
        }
    }
    violations.sort();
    violations.dedup();
    assert!(
        violations.is_empty(),
        "outward product source imports a physical/domain component instead of rrd-engine, rrd-client, or rrd-contract: {violations:#?}"
    );
}

#[test]
fn tracked_and_untracked_text_files_have_no_trailing_horizontal_whitespace() {
    let metadata = workspace_metadata();
    let output = Command::new("git")
        .current_dir(&metadata.root)
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
    let mut violations = Vec::new();
    for encoded in output.stdout.split(|byte| *byte == 0) {
        if encoded.is_empty() {
            continue;
        }
        let relative = PathBuf::from(
            std::str::from_utf8(encoded).expect("repository paths must be valid UTF-8"),
        );
        let path = metadata.root.join(&relative);
        if !path.is_file() {
            continue;
        }
        let bytes = fs::read(&path)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
        if bytes.contains(&0) || std::str::from_utf8(&bytes).is_err() {
            continue;
        }
        for (index, encoded_line) in bytes.split(|byte| *byte == b'\n').enumerate() {
            let line = encoded_line.strip_suffix(b"\r").unwrap_or(encoded_line);
            if line.last().is_some_and(|byte| matches!(byte, b' ' | b'\t')) {
                violations.push(format!("{}:{}", relative.display(), index + 1));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "tracked or untracked text contains trailing horizontal whitespace: {violations:#?}"
    );
}

#[test]
fn one_engine_authority_owns_every_product_storage_opening() {
    let metadata = workspace_metadata();
    let mut violations = Vec::new();
    collect_authority_opening_violations(&metadata.root.join("crates"), &mut violations);
    violations.sort();
    assert!(
        violations.is_empty(),
        "production code outside rrd-engine/rrd-store must not open physical storage or revive a second embedded handle: {violations:#?}"
    );

    let engine_library = fs::read_to_string(metadata.root.join("crates/rrd-engine/src/lib.rs"))
        .expect("rrd-engine library source must be readable");
    assert!(
        !engine_library.contains("pub mod operator;"),
        "the engine-owned operator implementation must remain private"
    );
    let mut duplicate_handles = Vec::new();
    collect_rust_sources(
        &metadata.root.join("crates/rrd-engine/src"),
        &mut duplicate_handles,
        "EmbeddedOperator",
    );
    collect_rust_sources(
        &metadata.root.join("crates/rrd-engine/src"),
        &mut duplicate_handles,
        "pub fn runtime_store",
    );
    duplicate_handles.sort();
    duplicate_handles.dedup();
    assert!(
        duplicate_handles.is_empty(),
        "rrd-engine must expose only RrdEngine as its storage-opening authority: {duplicate_handles:#?}"
    );
}

#[test]
fn outward_cli_owns_product_executables_while_physical_crates_own_none() {
    let metadata = workspace_metadata();
    let product_executables = names(&[
        "rrd-backup-controller",
        "rrd-estate-admin",
        "rrd-estate-controller",
        "rrd-security-bootstrap",
    ]);
    let cli_targets = &metadata.packages["rrflow-cli"].targets;
    let missing = product_executables
        .difference(cli_targets)
        .cloned()
        .collect::<Vec<_>>();
    assert!(
        missing.is_empty(),
        "rrflow-cli must own every thin product executable; missing={missing:?}"
    );
    for package in ["rrd-estate", "rrd-security"] {
        let unexpected = metadata.packages[package]
            .targets
            .intersection(&product_executables)
            .cloned()
            .collect::<Vec<_>>();
        assert!(
            unexpected.is_empty(),
            "{package} must remain a physical component library, not an independent product authority; targets={unexpected:?}"
        );
    }

    let workflow = fs::read_to_string(metadata.root.join(".github/workflows/ci-reusable.yml"))
        .expect("reusable CI workflow must be readable");
    let canonical_fixture = "cargo build -p rrflow-cli --bin rrd-estate-controller --locked";
    assert_eq!(
        workflow.matches(canonical_fixture).count(),
        2,
        "both CI matrices must build the controller fixture from its product owner"
    );
    assert!(
        !workflow.contains("cargo build -p rrd-estate --bin rrd-estate-controller"),
        "CI must not revive the retired physical-crate executable owner"
    );
}

#[test]
fn ci_is_one_bounded_reusable_chain_with_safe_runner_routing() {
    let metadata = workspace_metadata();
    let caller = fs::read_to_string(metadata.root.join(".github/workflows/ci.yml"))
        .expect("CI caller must be readable");
    let workflow = fs::read_to_string(metadata.root.join(".github/workflows/ci-reusable.yml"))
        .expect("reusable CI workflow must be readable");

    assert!(
        !caller
            .lines()
            .any(|line| line.trim_start().starts_with("push:")),
        "candidate CI must not execute once for push and again for pull_request"
    );
    for trigger in ["pull_request:", "merge_group:", "workflow_dispatch:"] {
        assert!(
            caller.lines().any(|line| line.trim() == trigger),
            "CI caller must include {trigger}"
        );
    }
    assert!(
        !caller.contains("pull_request_target"),
        "CI must never execute proposed code through pull_request_target"
    );
    assert_eq!(
        caller
            .matches("uses: ./.github/workflows/ci-reusable.yml")
            .count(),
        1,
        "the candidate caller must delegate to exactly one reusable chain"
    );
    assert!(
        caller.contains("cancel-in-progress: true"),
        "stale candidate runs must be cancelled"
    );
    assert!(
        caller.contains("github.event.pull_request.head.repo.full_name == github.repository")
            && caller.contains("vars.CI_SELF_HOSTED_ENABLED == 'true'"),
        "fork pull requests must have an explicit trust boundary"
    );
    assert_eq!(
        caller.matches("'[\"ubuntu-latest\"]'").count(),
        4,
        "both runner classes must fall back to hosted Linux for forks and unset repository variables"
    );
    assert!(
        workflow.contains("fromJSON(inputs.linux-standard-runner)")
            && workflow.contains("fromJSON(inputs.linux-heavy-runner)"),
        "trusted Linux runner routing must remain data-driven"
    );
    assert!(
        workflow.contains("name: ci-gate")
            && workflow.contains("if: ${{ always() }}")
            && workflow.contains("TOPOLOGY_RESULT")
            && workflow.contains("PORTABILITY_RESULT")
            && workflow.contains("VERIFY_RESULT"),
        "one stable gate must reduce every CI partition"
    );
    assert_eq!(
        workflow.matches("timeout-minutes:").count(),
        4,
        "every CI job, including the gate, must have a bounded runtime"
    );
    assert!(
        caller.contains("permissions:\n  contents: read")
            && workflow.contains("permissions:\n  contents: read"),
        "caller and reusable workflow must default to read-only contents"
    );

    for line in workflow.lines().map(str::trim) {
        let Some(action) = line.strip_prefix("- uses: ") else {
            continue;
        };
        if action.starts_with("./") {
            continue;
        }
        let (repository, reference) = action
            .split_once('@')
            .unwrap_or_else(|| panic!("external action is missing a reference: {action}"));
        let reference = reference
            .split_whitespace()
            .next()
            .expect("action reference must not be empty");
        assert!(
            repository.contains('/')
                && reference.len() == 40
                && reference.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "external action must be pinned by a full immutable commit SHA: {action}"
        );
    }
}

#[test]
fn arc_scale_sets_fit_the_physical_host_and_are_ephemeral() {
    let metadata = workspace_metadata();
    let deployment = metadata.root.join("deploy/ci/github-actions/arc");
    let controller = fs::read_to_string(deployment.join("controller-values.yaml"))
        .expect("ARC controller values must be readable");
    let standard = fs::read_to_string(deployment.join("rrflow-standard.values.yaml"))
        .expect("standard ARC scale-set values must be readable");
    let heavy = fs::read_to_string(deployment.join("rrflow-heavy.values.yaml"))
        .expect("heavy ARC scale-set values must be readable");
    let installer = fs::read_to_string(metadata.root.join("scripts/ci/install-arc.sh"))
        .expect("ARC installer must be readable");

    assert!(
        controller.contains("0.14.2@sha256:"),
        "ARC controller image must be release- and digest-pinned"
    );
    assert!(
        installer.contains("repository_visibility")
            && installer.contains("!= \"PRIVATE\"")
            && installer.contains("refusing to register self-hosted runners"),
        "self-hosted pool registration must fail closed for public repositories"
    );
    for (name, values, maximum) in [
        ("standard", standard.as_str(), "maxRunners: 2"),
        ("heavy", heavy.as_str(), "maxRunners: 1"),
    ] {
        assert!(
            values.contains("minRunners: 0") && values.contains(maximum),
            "{name} scale set must scale from zero to its physical capacity"
        );
        assert!(
            values.contains("githubConfigSecret: rrflow-arc-github"),
            "{name} scale set must use the pre-created Kubernetes secret"
        );
        assert!(
            values.contains("@sha256:") && !values.contains(":latest"),
            "{name} runner images must be immutable"
        );
    }
    assert!(
        standard.contains("runnerScaleSetName: rrflow-standard")
            && standard.contains("cpu: \"16\"")
            && standard.contains("memory: 20Gi"),
        "two standard runners must each consume 16 CPUs and 20 GiB"
    );
    assert!(
        heavy.contains("runnerScaleSetName: rrflow-heavy")
            && heavy.contains("cpu: \"46\"")
            && heavy.contains("memory: 60Gi")
            && heavy.contains("cpu: \"2\"")
            && heavy.contains("memory: 4Gi"),
        "the heavy runner and its Docker sidecar must total 48 CPUs and 64 GiB"
    );
}

#[test]
fn outward_tool_surfaces_serialize_the_authoritative_runtime_catalogue() {
    let metadata = workspace_metadata();
    let mcp = fs::read_to_string(metadata.root.join("crates/rrflow-mcp/src/main.rs"))
        .expect("MCP source must be readable");
    let compact_mcp = mcp.split_whitespace().collect::<String>();
    assert!(
        compact_mcp.contains("authority.catalogue().tools"),
        "MCP tools/list must serialize the authority catalogue"
    );
    assert!(
        !mcp.contains("RuntimeToolDescriptor {") && !mcp.contains("RuntimeToolDefinition {"),
        "MCP must not define a second hardcoded runtime-tool registry"
    );

    let connectome = fs::read_to_string(metadata.root.join("crates/connectome-ui/src/lib.rs"))
        .expect("Connectome source must be readable");
    assert!(
        connectome.contains("runtime_tool_catalogue"),
        "Connectome must fetch the authoritative runtime-tool catalogue"
    );
    assert!(
        !connectome.contains("RuntimeToolDescriptor {")
            && !connectome.contains("RuntimeToolDefinition {"),
        "Connectome must not define a second hardcoded runtime-tool registry"
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
        let targets = package["targets"]
            .as_array()
            .expect("package targets must be an array")
            .iter()
            .map(|target| {
                target["name"]
                    .as_str()
                    .expect("target names must be strings")
                    .to_owned()
            })
            .collect();
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
                .insert(
                    name.to_owned(),
                    PackageDependencies {
                        workspace,
                        all,
                        targets,
                    },
                )
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

fn collect_authority_opening_violations(crates: &Path, violations: &mut Vec<PathBuf>) {
    let mut packages = fs::read_dir(crates)
        .expect("workspace crates directory must be readable")
        .collect::<Result<Vec<_>, _>>()
        .expect("workspace crate entries must be readable");
    packages.sort_by_key(fs::DirEntry::path);
    for package in packages {
        let name = package.file_name();
        if name == "rrd-engine" || name == "rrd-store" {
            continue;
        }
        let source = package.path().join("src");
        if source.is_dir() {
            collect_authority_source_tree(crates, &source, violations);
        }
    }
}

fn collect_physical_import_violations(
    root: &Path,
    directory: &Path,
    physical: &[&str],
    violations: &mut Vec<PathBuf>,
) {
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
            collect_physical_import_violations(root, &path, physical, violations);
            continue;
        }
        if path.extension().is_none_or(|extension| extension != "rs") {
            continue;
        }
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
        if physical.iter().any(|component| {
            source.contains(&format!("use {component}"))
                || source.contains(&format!("{component}::"))
                || source.contains(&format!("extern crate {component}"))
        }) {
            violations.push(
                path.strip_prefix(root)
                    .expect("outward source must be workspace-relative")
                    .to_path_buf(),
            );
        }
    }
}

fn collect_authority_source_tree(crates: &Path, directory: &Path, violations: &mut Vec<PathBuf>) {
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
            collect_authority_source_tree(crates, &path, violations);
            continue;
        }
        if path.extension().is_none_or(|extension| extension != "rs") {
            continue;
        }
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
        // Source-level unit fixtures are allowed to inspect persistence. The
        // product surface before the first cfg(test) boundary is not.
        let production = source.split("#[cfg(test)]").next().unwrap_or(&source);
        if [
            "PersistentEngine::open(",
            "EmbeddedOperator",
            ".runtime_store(",
            "rrd_engine::operator",
        ]
        .iter()
        .any(|forbidden| production.contains(forbidden))
        {
            violations.push(
                path.strip_prefix(crates.parent().expect("crates has a workspace parent"))
                    .expect("crate source must be workspace-relative")
                    .to_path_buf(),
            );
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
