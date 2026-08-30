use rrd_graph::{Ecosystem, ProjectAttunement, Runner, TaskClass, TopologyNodeKind};
use std::path::Path;

fn write(root: &Path, relative: &str, body: &str) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, body).unwrap();
}

fn mixed_project() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    write(
        root.path(),
        "Cargo.toml",
        "[workspace]\nmembers=[\"crates/core\"]\n",
    );
    write(
        root.path(),
        "crates/core/Cargo.toml",
        "[package]\nname=\"core\"\nversion=\"0.1.0\"\n",
    );
    write(root.path(), "crates/core/src/lib.rs", "pub fn core() {}\n");
    write(
        root.path(),
        "package.json",
        r#"{"workspaces":["web"],"scripts":{"build":"vite build","test":"vitest"}}"#,
    );
    write(root.path(), "package-lock.json", "{}");
    write(root.path(), "bun.lock", "");
    write(root.path(), "pnpm-workspace.yaml", "packages:\n  - web\n");
    write(
        root.path(),
        "web/package.json",
        r#"{"name":"web","scripts":{"lint":"eslint .","test":"vitest"}}"#,
    );
    write(root.path(), "web/src/app.ts", "export const app = true;\n");
    write(
        root.path(),
        "pyproject.toml",
        "[project]\nname=\"py-root\"\nversion=\"0.1.0\"\n",
    );
    write(root.path(), "uv.lock", "version = 1\nrevision = 1\n");
    write(root.path(), "go.work", "go 1.24\nuse ./go-service\n");
    write(
        root.path(),
        "go-service/go.mod",
        "module example.test/service\ngo 1.24\n",
    );
    write(
        root.path(),
        "go-service/main.go",
        "package main\nfunc main() {}\n",
    );
    write(root.path(), "settings.gradle.kts", "include(\"jvm-app\")\n");
    write(
        root.path(),
        "jvm-app/build.gradle.kts",
        "plugins { java }\n",
    );
    write(
        root.path(),
        "maven-app/pom.xml",
        "<project><modelVersion>4.0.0</modelVersion></project>\n",
    );
    write(
        root.path(),
        "dotnet/App.csproj",
        "<Project Sdk=\"Microsoft.NET.Sdk\"></Project>\n",
    );
    write(
        root.path(),
        "CMakeLists.txt",
        "cmake_minimum_required(VERSION 3.25)\nadd_subdirectory(native)\n",
    );
    write(
        root.path(),
        "native/CMakeLists.txt",
        "add_library(native native.cpp)\n",
    );
    write(
        root.path(),
        "native/native.cpp",
        "int native() { return 1; }\n",
    );
    write(
        root.path(),
        ".github/workflows/ci.yml",
        "name: ci\non: [push]\njobs:\n  test:\n    runs-on: ubuntu-latest\n",
    );
    root
}

#[test]
fn mixed_workspaces_retain_every_ecosystem_runner_member_and_task() {
    let root = mixed_project();
    let attunement = ProjectAttunement::materialize(root.path()).unwrap();
    assert_eq!(
        attunement.profile.ecosystems,
        vec![
            Ecosystem::Cargo,
            Ecosystem::JavaScript,
            Ecosystem::Python,
            Ecosystem::Go,
            Ecosystem::Jvm,
            Ecosystem::Dotnet,
            Ecosystem::Cmake,
            Ecosystem::Ci,
        ]
    );
    assert_eq!(
        attunement.profile.runners,
        vec![
            Runner::Cargo,
            Runner::Bun,
            Runner::Pnpm,
            Runner::Npm,
            Runner::Python,
            Runner::Uv,
            Runner::Go,
            Runner::Gradle,
            Runner::Maven,
            Runner::Dotnet,
            Runner::Cmake,
            Runner::Ctest,
            Runner::GitHubActions,
        ]
    );
    let members = attunement
        .profile
        .members
        .iter()
        .map(|member| member.path.as_str())
        .collect::<Vec<_>>();
    for expected in [
        ".",
        "crates/core",
        "web",
        "go-service",
        "jvm-app",
        "maven-app",
        "dotnet",
        "native",
    ] {
        assert!(
            members.contains(&expected),
            "missing member {expected}: {members:?}"
        );
    }
    for runner in [Runner::Bun, Runner::Pnpm, Runner::Npm] {
        assert!(attunement
            .profile
            .tasks
            .iter()
            .any(|task| task.working_directory == "web"
                && task.runner == runner
                && task.name == "lint"
                && task.class == TaskClass::Lint));
    }
    assert!(attunement.topology.nodes.iter().any(|node| {
        node.path == ".github/workflows/ci.yml" && node.kind == TopologyNodeKind::CiWorkflow
    }));
    assert!(attunement.topology.verify_digest());
    assert!(attunement.profile.verify_digest(&attunement.topology));
}

#[test]
fn incremental_refresh_and_clean_rebuild_have_identical_artifacts() {
    let root = mixed_project();
    let mut incremental = ProjectAttunement::materialize(root.path()).unwrap();
    let unchanged = incremental.refresh(root.path()).unwrap();
    assert!(!unchanged.topology_changed);
    assert!(!unchanged.profile_changed);

    write(
        root.path(),
        "web/package.json",
        r#"{"name":"web","scripts":{"build":"vite build","lint":"eslint .","test":"vitest"}}"#,
    );
    let changed = incremental.refresh(root.path()).unwrap();
    assert!(changed.topology_changed);
    assert!(changed.profile_changed);
    let rebuilt = ProjectAttunement::materialize(root.path()).unwrap();
    assert_eq!(incremental.topology, rebuilt.topology);
    assert_eq!(incremental.profile, rebuilt.profile);
}

#[test]
fn artifact_digests_are_independent_of_absolute_checkout_path() {
    let first = mixed_project();
    let second = mixed_project();
    let first = ProjectAttunement::materialize(first.path()).unwrap();
    let second = ProjectAttunement::materialize(second.path()).unwrap();
    assert_eq!(first.topology.digest, second.topology.digest);
    assert_eq!(first.profile.digest, second.profile.digest);
}

#[test]
fn engine_owned_runtime_directories_do_not_change_project_identity() {
    let root = mixed_project();
    write(
        root.path(),
        ".rrflow/instance.toml",
        "format = 1\nid = \"test\"\nmode = \"dedicated\"\nmembers = [\".\"]\n",
    );
    let before = ProjectAttunement::materialize(root.path()).unwrap();
    assert!(before
        .topology
        .excluded
        .iter()
        .any(|path| path.path == ".rrflow/<runtime-state>"));

    write(root.path(), ".rrflow/rrd/manifests/0001", "runtime bytes");
    write(
        root.path(),
        ".rrflow/verification/check-stdout.log",
        "verification bytes",
    );
    let after = ProjectAttunement::materialize(root.path()).unwrap();

    assert_eq!(before.topology, after.topology);
    assert_eq!(before.profile, after.profile);
}

#[test]
fn unreadable_or_malformed_evidence_fails_closed() {
    let malformed = tempfile::tempdir().unwrap();
    write(malformed.path(), "package.json", "{not-json");
    let error = ProjectAttunement::materialize(malformed.path())
        .unwrap_err()
        .to_string();
    assert!(error.contains("package.json"));
    assert!(error.contains("malformed"));

    let non_file = tempfile::tempdir().unwrap();
    std::fs::create_dir(non_file.path().join("Cargo.toml")).unwrap();
    let error = ProjectAttunement::materialize(non_file.path())
        .unwrap_err()
        .to_string();
    assert!(error.contains("Cargo.toml"));
    assert!(error.contains("not a readable regular file"));
}
