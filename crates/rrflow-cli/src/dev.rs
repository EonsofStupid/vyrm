//! Executable development-topology diagnostics.
//!
//! This module does not start partial services. It proves the dependency and
//! repository invariants required before `rrflow dev up` can safely supervise
//! one RRD authority and its client surfaces.

pub mod supervisor;

use serde::Serialize;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

const REPORT_VERSION: u16 = 3;
const CANONICAL_WORKSPACE_PACKAGES: [&str; 22] = [
    "connectome-ui",
    "rrd-client",
    "rrd-cluster",
    "rrd-contract",
    "rrd-core",
    "rrd-engine",
    "rrd-estate",
    "rrd-graph",
    "rrd-inference",
    "rrd-kubernetes",
    "rrd-lsm",
    "rrd-maintenance",
    "rrd-operator-knowledge",
    "rrd-query",
    "rrd-security",
    "rrd-server",
    "rrd-store",
    "rrd-vector",
    "rrflow-cli",
    "rrflow-edge",
    "rrflow-eval",
    "rrflow-mcp",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Passed,
    Blocked,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DoctorCheck {
    pub id: &'static str,
    pub status: CheckStatus,
    pub summary: String,
    pub evidence: String,
    pub required_action: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DevDoctorReport {
    pub report_version: u16,
    pub workspace_root: PathBuf,
    pub canonical_topology: &'static str,
    pub ready: bool,
    pub passed: usize,
    pub blocked: usize,
    pub warnings: usize,
    pub checks: Vec<DoctorCheck>,
}

impl DevDoctorReport {
    pub fn render(&self) -> String {
        let mut lines = vec![
            format!("RRFlow development topology: {}", self.canonical_topology),
            format!("workspace: {}", self.workspace_root.display()),
            format!(
                "result: {} ({} passed, {} blocked, {} warnings)",
                if self.ready { "READY" } else { "BLOCKED" },
                self.passed,
                self.blocked,
                self.warnings
            ),
        ];
        for check in &self.checks {
            let marker = match check.status {
                CheckStatus::Passed => "PASS",
                CheckStatus::Blocked => "BLOCK",
                CheckStatus::Warning => "WARN",
            };
            lines.push(format!("[{marker}] {} — {}", check.id, check.summary));
            lines.push(format!("       evidence: {}", check.evidence));
            if let Some(action) = &check.required_action {
                lines.push(format!("       action: {action}"));
            }
        }
        lines.join("\n")
    }
}

pub fn doctor(root: &Path) -> Result<DevDoctorReport, Box<dyn std::error::Error>> {
    let root = root.canonicalize()?;
    let workspace_manifest = read_manifest(&root.join("Cargo.toml"))?;
    let members = workspace_members(&workspace_manifest)?;
    let packages = package_inventory(&root, &members)?;
    let package_names = packages
        .iter()
        .map(|package| package.name.as_str())
        .collect::<BTreeSet<_>>();
    let canonical_packages = CANONICAL_WORKSPACE_PACKAGES
        .into_iter()
        .collect::<BTreeSet<_>>();
    let mut checks = vec![
        check(
            "workspace.identity",
            package_names == canonical_packages,
            "workspace packages use RRFlow/RRD identities",
            package_names.into_iter().collect::<Vec<_>>().join(", "),
            "make workspace package membership match the canonical RRFlow/RRD rename ledger",
        ),
        surface_boundary(
            &packages,
            "connectome-ui",
            &["rrd-client", "rrd-contract"],
            "surface.connectome-client-only",
            "Connectome must consume RRD protocol/client contracts and must not open physical engine crates",
        ),
        surface_boundary(
            &packages,
            "rrflow-cli",
            &["rrd-client", "rrd-contract", "rrd-engine"],
            "surface.cli-engine-boundary",
            "the CLI may use the public embedded engine or daemon client, never physical storage/query crates",
        ),
        surface_boundary(
            &packages,
            "rrflow-mcp",
            &["rrd-client", "rrd-contract", "rrd-engine"],
            "surface.mcp-engine-boundary",
            "MCP may use the public embedded engine or daemon client, never physical storage/query crates",
        ),
        surface_boundary(
            &packages,
            "rrd-server",
            &["rrd-contract", "rrd-engine"],
            "surface.server-composition-root",
            "the daemon must compose only the public engine and protocol contract",
        ),
    ];

    let capability_path = root.join("docs/rrd-engine-capability-coverage.md");
    let capability_source = std::fs::read_to_string(&capability_path)?;
    let workplan_source = std::fs::read_to_string(root.join("rrflow.workplan.toml"))?;
    let stable_capabilities = (1..=20).all(|number| {
        capability_source
            .lines()
            .any(|line| line.starts_with(&format!("{number}. `CAP-{number:02}`")))
    });
    let closure_gates = coverage_gate_ids(&capability_source);
    let existing_closure_gates = closure_gates
        .iter()
        .filter(|gate| workplan_source.contains(&format!("id = \"{gate}\"")))
        .count();
    let closure_gates_exist =
        !closure_gates.is_empty() && existing_closure_gates == closure_gates.len();
    checks.push(check(
        "capabilities.coverage-ledger",
        stable_capabilities && closure_gates_exist,
        "all twenty requested engine capabilities have stable IDs and real work-plan closure gates",
        format!(
            "{}: 20 ordered CAP rows; rrflow.workplan.toml: {}/{} referenced gates found",
            relative(&root, &capability_path),
            existing_closure_gates,
            closure_gates.len(),
        ),
        "restore CAP-01 through CAP-20 one-for-one and repair every dangling Gxx-Wxx closure mapping",
    ));

    let mcp_source_path = root.join("crates/rrflow-mcp/src/main.rs");
    let mcp_config_path = root.join("crates/rrflow-mcp/src/config.rs");
    let mcp_authority_path = root.join("crates/rrflow-mcp/src/authority.rs");
    let mcp_daemon_test = root.join("crates/rrflow-mcp/tests/stdio_daemon.rs");
    let mcp_source = std::fs::read_to_string(&mcp_source_path)?;
    let mcp_config = std::fs::read_to_string(&mcp_config_path).unwrap_or_default();
    let mcp_authority = std::fs::read_to_string(&mcp_authority_path).unwrap_or_default();
    let mcp_package = packages.iter().find(|package| package.name == "rrflow-mcp");
    let mcp_has_daemon_client = mcp_package
        .is_some_and(|package| package.dependencies.contains("rrd-client"))
        && mcp_source.contains("RuntimeAuthority::open")
        && mcp_config.contains("enum RuntimeConfig")
        && mcp_config.contains("--url")
        && mcp_config.contains("--api-key-file")
        && mcp_authority.contains("RrdClient")
        && mcp_authority.contains("RrdEngine::open_bound")
        && mcp_authority.contains("RuntimeConfig::Daemon")
        && mcp_daemon_test.is_file();
    checks.push(check(
        "surface.mcp-daemon-mode",
        mcp_has_daemon_client,
        "MCP can bind to the supervised RRD authority without opening a second database",
        if mcp_has_daemon_client {
            format!(
                "{}, {}, {}: explicit embedded/daemon modes, authenticated rrd-client, and black-box daemon test",
                relative(&root, &mcp_source_path),
                relative(&root, &mcp_authority_path),
                relative(&root, &mcp_daemon_test)
            )
        } else {
            format!(
                "{} opens RrdEngine directly and has no explicit --url daemon-client mode",
                relative(&root, &mcp_source_path)
            )
        },
        "implement mutually exclusive embedded and daemon MCP modes; daemon mode must use authenticated rrd-client runtime invocation and must never open the database",
    ));

    let router = root.join("crates/rrd-server/src/http/router.rs");
    let router_source = std::fs::read_to_string(&router)?;
    let endpoint_catalogue = rrd_contract::endpoint_catalogue();
    let required_daemon_operations = ["health-live", "health-ready", "capabilities-read"];
    let daemon_operations_catalogued = required_daemon_operations.iter().all(|required| {
        endpoint_catalogue
            .endpoints
            .iter()
            .any(|endpoint| endpoint.operation.as_str() == *required)
    });
    checks.push(check(
        "daemon.readiness",
        daemon_operations_catalogued
            && router_source.contains("HttpDispatch::resolve")
            && router_source.contains("endpoint_catalogue"),
        "RRD exposes liveness, readiness, and capability negotiation",
        format!(
            "{} resolves health-live, health-ready, and capabilities-read from EndpointCatalogue",
            relative(&root, &router)
        ),
        "add contract-backed readiness and capability routes before supervision",
    ));

    let gitignore = std::fs::read_to_string(root.join(".gitignore"))?;
    checks.push(check(
        "state.dev-isolation",
        gitignore.lines().any(|line| line.trim() == "/.rrflow/dev/"),
        "developer runtime state is isolated from source and production data",
        "/.rrflow/dev/ is ignored",
        "reserve and ignore .rrflow/dev for manifests, logs, PIDs, ports, and disposable data",
    ));

    let toolchain_path = root.join("rust-toolchain.toml");
    let toolchain = read_manifest(&toolchain_path)?;
    let pinned_channel = toolchain
        .get("toolchain")
        .and_then(|value| value.get("channel"))
        .and_then(toml::Value::as_str);
    let components = toolchain
        .get("toolchain")
        .and_then(|value| value.get("components"))
        .and_then(toml::Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(toml::Value::as_str)
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    let toolchain_pinned = pinned_channel
        .is_some_and(|channel| channel != "stable" && channel != "beta" && channel != "nightly")
        && components.contains("clippy")
        && components.contains("rustfmt");
    checks.push(check(
        "reproducibility.toolchain",
        toolchain_pinned,
        "Rust, rustfmt, and Clippy are pinned to one repository toolchain",
        format!(
            "{}: channel={} components={}",
            relative(&root, &toolchain_path),
            pinned_channel.unwrap_or("missing"),
            components.iter().copied().collect::<Vec<_>>().join(",")
        ),
        "pin an exact Rust channel and include rustfmt and clippy",
    ));
    checks.push(check(
        "reproducibility.lockfile",
        root.join("Cargo.lock").is_file(),
        "the workspace dependency graph is locked",
        "Cargo.lock",
        "generate and commit the workspace Cargo.lock",
    ));

    let ci_caller = std::fs::read_to_string(root.join(".github/workflows/ci.yml"))?;
    let ci_reusable = std::fs::read_to_string(root.join(".github/workflows/ci-reusable.yml"))?;
    let ci_source = format!("{ci_caller}\n{ci_reusable}");
    let ci_toolchain_lines = ci_source
        .lines()
        .filter(|line| line.contains("uses: dtolnay/rust-toolchain@"))
        .collect::<Vec<_>>();
    let ci_toolchain_pinned = pinned_channel.is_some_and(|channel| {
        !ci_toolchain_lines.is_empty()
            && ci_toolchain_lines.iter().all(|line| {
                let Some((_, suffix)) = line.split_once("dtolnay/rust-toolchain@") else {
                    return false;
                };
                let reference = suffix.split_whitespace().next().unwrap_or_default();
                reference.len() == 40
                    && reference.bytes().all(|byte| byte.is_ascii_hexdigit())
                    && line
                        .split_once('#')
                        .is_some_and(|(_, annotation)| annotation.trim() == channel)
            })
    });
    checks.push(check(
        "reproducibility.ci-toolchain",
        ci_toolchain_pinned,
        "CI uses the same exact Rust release as local development",
        if pinned_channel.is_none() {
            "no pinned local channel".into()
        } else if ci_toolchain_pinned {
            format!(
                "{} immutable dtolnay/rust-toolchain references annotated {}",
                ci_toolchain_lines.len(),
                pinned_channel.unwrap_or_default()
            )
        } else {
            format!(
                "expected full commit SHAs annotated {}; observed {}",
                pinned_channel.unwrap_or_default(),
                ci_toolchain_lines.join(" | ")
            )
        },
        "pin every CI toolchain action by full commit SHA and annotate it with the rust-toolchain.toml channel",
    ));

    checks.push(command_check(
        "toolchain.cargo",
        "cargo",
        &["--version"],
        true,
    ));
    checks.push(command_check(
        "toolchain.rustc",
        "rustc",
        &["--version"],
        true,
    ));
    checks.push(command_check("toolchain.git", "git", &["--version"], true));
    checks.push(command_check(
        "remote.tailscale",
        "tailscale",
        &["version"],
        false,
    ));
    checks.push(command_check(
        "remote.cloudflare",
        "cloudflared",
        &["--version"],
        false,
    ));

    let command_source = std::fs::read_to_string(root.join("crates/rrflow-cli/src/command.rs"))?;
    let has_exec = command_source.contains("Exec {");
    let has_exact_argv = command_source.contains("exact_argv");
    checks.push(check(
        "enforcement.command-proxy",
        has_exec && has_exact_argv,
        "provider-neutral mutations cross an exact-argv RRFlow command boundary",
        if has_exec && has_exact_argv {
            "crates/rrflow-cli/src/command.rs and command_proxy.rs bind an exact argv vector"
        } else if has_exec {
            "an exec command exists but exact argv binding was not detected"
        } else {
            "rrflow exec is absent"
        },
        "implement rrflow exec with one-shot attunement authorization, argv/cwd/environment/revision digests, observation, and verification",
    ));

    let supervisor_path = root.join("crates/rrflow-cli/src/dev/supervisor.rs");
    let supervisor_source = std::fs::read_to_string(&supervisor_path).unwrap_or_default();
    let has_supervisor = supervisor_path.is_file()
        && ["Up {", "Status {", "Logs {", "Stop {"]
            .iter()
            .all(|variant| command_source.contains(variant))
        && supervisor_source.contains("rrd-security-bootstrap")
        && supervisor_source.contains("spawn_rrd")
        && supervisor_source.contains("spawn_connectome")
        && supervisor_source.contains("wait_for_service")
        && supervisor_source.contains("shutdown_complete_file");
    checks.push(check(
        "supervisor.lifecycle",
        has_supervisor,
        "one command owns build, start, readiness, status, logs, and graceful stop",
        if has_supervisor {
            "typed commands, security bootstrap, ordered child startup, readiness probes, and paired shutdown completion markers"
        } else {
            "the supervisor command or one of its required lifecycle controls is absent"
        },
        "implement rrflow dev up|status|logs|stop after every surface crosses the RRD boundary",
    ));

    let has_topology_smoke = ci_source.contains("rrflow dev up")
        && ci_source.contains("/v1/health/ready")
        && ci_source.contains("/api/snapshot")
        && ci_source.contains("rrflow_path\" dev stop");
    checks.push(check(
        "ci.full-topology-smoke",
        has_topology_smoke,
        "CI boots and probes the same RRD/Connectome topology used by developers",
        if has_topology_smoke {
            "CI starts the supervisor, probes RRD readiness and authenticated Connectome snapshot, then requests graceful stop"
        } else {
            "no complete full-topology smoke job detected"
        },
        "add a black-box smoke job after the supervisor and client boundaries are complete",
    ));

    let blocked = checks
        .iter()
        .filter(|check| check.status == CheckStatus::Blocked)
        .count();
    let warnings = checks
        .iter()
        .filter(|check| check.status == CheckStatus::Warning)
        .count();
    let passed = checks.len() - blocked - warnings;
    Ok(DevDoctorReport {
        report_version: REPORT_VERSION,
        workspace_root: root,
        canonical_topology:
            "daemon: one rrd-server authority; Connectome, CLI, MCP, and SDKs are clients",
        ready: blocked == 0,
        passed,
        blocked,
        warnings,
        checks,
    })
}

#[derive(Debug)]
struct Package {
    name: String,
    dependencies: BTreeSet<String>,
}

fn read_manifest(path: &Path) -> Result<toml::Value, Box<dyn std::error::Error>> {
    let source = std::fs::read_to_string(path)?;
    Ok(toml::from_str(&source)?)
}

fn workspace_members(manifest: &toml::Value) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let members = manifest
        .get("workspace")
        .and_then(|workspace| workspace.get("members"))
        .and_then(toml::Value::as_array)
        .ok_or("workspace.members is missing")?;
    members
        .iter()
        .map(|member| {
            member
                .as_str()
                .map(ToOwned::to_owned)
                .ok_or_else(|| "workspace member is not a string".into())
        })
        .collect()
}

fn package_inventory(
    root: &Path,
    members: &[String],
) -> Result<Vec<Package>, Box<dyn std::error::Error>> {
    let mut packages = Vec::with_capacity(members.len());
    for member in members {
        let manifest = read_manifest(&root.join(member).join("Cargo.toml"))?;
        let name = manifest
            .get("package")
            .and_then(|package| package.get("name"))
            .and_then(toml::Value::as_str)
            .ok_or_else(|| format!("{member}/Cargo.toml has no package.name"))?
            .to_owned();
        let dependencies = manifest
            .get("dependencies")
            .and_then(toml::Value::as_table)
            .map(|table| table.keys().cloned().collect())
            .unwrap_or_default();
        packages.push(Package { name, dependencies });
    }
    packages.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(packages)
}

fn surface_boundary(
    packages: &[Package],
    package_name: &str,
    allowed_internal: &[&str],
    id: &'static str,
    summary: &'static str,
) -> DoctorCheck {
    let Some(package) = packages.iter().find(|package| package.name == package_name) else {
        return blocked(
            id,
            summary,
            format!("package {package_name} is absent"),
            "restore the canonical workspace package",
        );
    };
    let allowed = allowed_internal.iter().copied().collect::<BTreeSet<_>>();
    let disallowed = package
        .dependencies
        .iter()
        .filter(|dependency| {
            (dependency.starts_with("rrd-") || dependency.starts_with("rrflow-"))
                && !allowed.contains(dependency.as_str())
        })
        .cloned()
        .collect::<Vec<_>>();
    check(
        id,
        disallowed.is_empty(),
        summary,
        if disallowed.is_empty() {
            format!("{package_name}: no physical-layer bypass")
        } else {
            format!(
                "{package_name} bypasses RRD through {}",
                disallowed.join(", ")
            )
        },
        "move the required operation into rrd-engine/rrd-contract and consume it through rrd-client",
    )
}

fn command_check(
    id: &'static str,
    program: &str,
    arguments: &[&str],
    required: bool,
) -> DoctorCheck {
    match Command::new(program).args(arguments).output() {
        Ok(output) if output.status.success() => DoctorCheck {
            id,
            status: CheckStatus::Passed,
            summary: format!("{program} is available"),
            evidence: String::from_utf8_lossy(&output.stdout).trim().to_owned(),
            required_action: None,
        },
        Ok(output) => DoctorCheck {
            id,
            status: if required {
                CheckStatus::Blocked
            } else {
                CheckStatus::Warning
            },
            summary: format!("{program} is installed but unhealthy"),
            evidence: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            required_action: Some(format!("repair {program} before selecting this topology")),
        },
        Err(error) => DoctorCheck {
            id,
            status: if required {
                CheckStatus::Blocked
            } else {
                CheckStatus::Warning
            },
            summary: format!("{program} is not available"),
            evidence: error.to_string(),
            required_action: Some(if required {
                format!("install {program}")
            } else {
                format!("install {program} only when that remote exposure mode is required")
            }),
        },
    }
}

fn check(
    id: &'static str,
    passed: bool,
    summary: impl Into<String>,
    evidence: impl Into<String>,
    required_action: impl Into<String>,
) -> DoctorCheck {
    DoctorCheck {
        id,
        status: if passed {
            CheckStatus::Passed
        } else {
            CheckStatus::Blocked
        },
        summary: summary.into(),
        evidence: evidence.into(),
        required_action: (!passed).then(|| required_action.into()),
    }
}

fn blocked(
    id: &'static str,
    summary: impl Into<String>,
    evidence: impl Into<String>,
    required_action: impl Into<String>,
) -> DoctorCheck {
    check(id, false, summary, evidence, required_action)
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

fn coverage_gate_ids(source: &str) -> BTreeSet<String> {
    source
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '-'))
        .filter(|token| {
            let bytes = token.as_bytes();
            bytes.len() == 7
                && bytes[0] == b'G'
                && bytes[1..3].iter().all(u8::is_ascii_digit)
                && bytes[3] == b'-'
                && bytes[4] == b'W'
                && bytes[5..7].iter().all(u8::is_ascii_digit)
        })
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_workspace_passes_every_foundation_invariant() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let report = doctor(&root).unwrap();
        assert!(report.ready);
        assert!(report.checks.iter().any(|check| {
            check.id == "workspace.identity" && check.status == CheckStatus::Passed
        }));
        assert!(report.checks.iter().any(|check| {
            check.id == "surface.connectome-client-only" && check.status == CheckStatus::Passed
        }));
        assert!(report.checks.iter().any(|check| {
            check.id == "surface.cli-engine-boundary" && check.status == CheckStatus::Passed
        }));
        assert!(report.checks.iter().any(|check| {
            check.id == "supervisor.lifecycle" && check.status == CheckStatus::Passed
        }));
        assert!(report.checks.iter().any(|check| {
            check.id == "capabilities.coverage-ledger"
                && check.status == CheckStatus::Passed
                && check.evidence.contains("referenced gates found")
        }));
        assert!(report.checks.iter().any(|check| {
            check.id == "surface.mcp-engine-boundary" && check.status == CheckStatus::Passed
        }));
        assert!(report.checks.iter().any(|check| {
            check.id == "surface.mcp-daemon-mode"
                && check.status == CheckStatus::Passed
                && check.evidence.contains("black-box daemon test")
        }));
    }
}
