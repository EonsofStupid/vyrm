//! Project-owned package workflow policy.
//!
//! Package script names are identities, not semantics. A strict, versioned
//! manifest binds an exact direct command to its canonical event, instance
//! scope, projection requirements, freshness policy, and verification rule.
//! Lifecycle adapters consume this one contract; none infer that a script
//! named `test`, `deploy`, or `build` is safe or meaningful on its own.

use crate::routing::{RoutingReady, ROUTING_PROJECTION};
use crate::stack::package_run_event;
use crate::InstanceBinding;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use vyrm_core::{digest, ReadStamp, ScopeId};

pub const WORKFLOW_FORMAT: u32 = 1;
pub const WORKFLOW_FILE: &str = ".vyrm/workflows.toml";
pub const SOURCE_ROUTING_REQUIREMENT: &str = "source-routing";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationPolicy {
    ExitZero,
    Observe,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowStatus {
    Passed,
    Failed,
    Observed,
    Unverified,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowRule {
    pub event: String,
    pub command: Vec<String>,
    #[serde(default)]
    pub allow_arguments: bool,
    pub scope: String,
    pub required_projections: Vec<String>,
    pub max_source_lag_generations: u64,
    pub verification: VerificationPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowManifest {
    pub format: u32,
    pub workflows: Vec<WorkflowRule>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowCatalog {
    pub path: PathBuf,
    pub digest: String,
    pub manifest: WorkflowManifest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowAuthorization {
    pub event: String,
    pub scope: ScopeId,
    pub manifest_digest: String,
    pub command: Vec<String>,
    pub allow_arguments: bool,
    pub required_projections: Vec<String>,
    pub max_source_lag_generations: u64,
    pub verification: VerificationPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowDifferential {
    pub event: Option<String>,
    pub actual_command: String,
    pub expected: Vec<String>,
    pub differences: Vec<String>,
}

impl WorkflowDifferential {
    pub fn render(&self) -> String {
        format!(
            "workflow differential: event={}; actual={}; expected={}; {}",
            self.event.as_deref().unwrap_or("unresolved"),
            self.actual_command,
            self.expected.join(" | "),
            self.differences.join("; ")
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkflowDecision {
    NotPackage,
    Allow(WorkflowAuthorization),
    Deny(WorkflowDifferential),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowPreflight {
    pub event: String,
    pub manifest_digest: String,
    pub read: ReadStamp,
}

/// Canonical post-tool evidence stored as the value of the event's temporal
/// status claim. Digests bind the exact command and provider response without
/// forcing potentially large or sensitive tool output into recall context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowObservation {
    pub contract_version: u32,
    pub event: String,
    pub manifest_digest: String,
    /// The declared matcher, never untrusted trailing arguments (which may
    /// contain credentials). `command_digest` still binds the exact command.
    pub command: String,
    pub arguments_supplied: usize,
    pub command_digest: String,
    pub response_digest: String,
    pub exit_code: Option<i64>,
    pub status: WorkflowStatus,
    pub at: u64,
}

impl WorkflowObservation {
    pub fn capture(
        authorization: &WorkflowAuthorization,
        command: &str,
        response: &serde_json::Value,
        exit_code: Option<i64>,
        at: u64,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let status = match authorization.verification {
            VerificationPolicy::ExitZero => match exit_code {
                Some(0) => WorkflowStatus::Passed,
                Some(_) => WorkflowStatus::Failed,
                None => WorkflowStatus::Unverified,
            },
            VerificationPolicy::Observe => WorkflowStatus::Observed,
        };
        let actual_words = direct_words(command).map_err(|reason| {
            format!("post-tool workflow command is no longer a direct command: {reason}")
        })?;
        Ok(Self {
            contract_version: WORKFLOW_FORMAT,
            event: authorization.event.clone(),
            manifest_digest: authorization.manifest_digest.clone(),
            command: authorization.command.join(" "),
            arguments_supplied: actual_words
                .len()
                .saturating_sub(authorization.command.len()),
            command_digest: digest::sha256_hex(command.as_bytes()),
            response_digest: digest::sha256_hex(&serde_json::to_vec(response)?),
            exit_code,
            status,
            at,
        })
    }
}

impl WorkflowCatalog {
    pub fn load(root: &Path) -> Result<Option<Self>, Box<dyn std::error::Error>> {
        let path = root.join(WORKFLOW_FILE);
        let raw = match std::fs::read(&path) {
            Ok(raw) => raw,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(
                    format!("cannot read workflow manifest {}: {error}", path.display()).into(),
                )
            }
        };
        let text = std::str::from_utf8(&raw).map_err(|error| {
            format!("workflow manifest {} is not UTF-8: {error}", path.display())
        })?;
        let manifest: WorkflowManifest = toml::from_str(text).map_err(|error| {
            format!("cannot parse workflow manifest {}: {error}", path.display())
        })?;
        let catalog = Self {
            path,
            digest: digest::sha256_hex(&raw),
            manifest,
        };
        catalog.validate()?;
        Ok(Some(catalog))
    }

    pub fn validate(&self) -> Result<(), Box<dyn std::error::Error>> {
        if self.manifest.format != WORKFLOW_FORMAT {
            return Err(format!(
                "unsupported workflow format {} (expected {WORKFLOW_FORMAT})",
                self.manifest.format
            )
            .into());
        }
        if self.manifest.workflows.is_empty() {
            return Err("workflow manifest must declare at least one workflow".into());
        }
        let mut events = BTreeSet::new();
        for rule in &self.manifest.workflows {
            if !events.insert(rule.event.clone()) {
                return Err(format!("duplicate workflow event {:?}", rule.event).into());
            }
            if rule.command.is_empty() || rule.command.iter().any(|word| !safe_manifest_word(word))
            {
                return Err(format!(
                    "workflow {:?} command must be non-empty direct argv without shell syntax",
                    rule.event
                )
                .into());
            }
            let rendered = rule.command.join(" ");
            let derived = package_run_event(&rendered).ok_or_else(|| {
                format!(
                    "workflow {:?} command is not a package lifecycle command",
                    rule.event
                )
            })?;
            if matches!(derived.action.as_str(), "run" | "unknown")
                || derived.action.ends_with(":unknown")
            {
                return Err(format!(
                    "workflow {:?} command does not identify a concrete script or executable",
                    rule.event
                )
                .into());
            }
            if derived.canonical_subject() != rule.event {
                return Err(format!(
                    "workflow event {:?} differs from command-derived identity {:?}",
                    rule.event,
                    derived.canonical_subject()
                )
                .into());
            }
            ScopeId::new(rule.scope.clone())?;
            if rule.required_projections.is_empty() {
                return Err(
                    format!("workflow {:?} has no required projections", rule.event).into(),
                );
            }
            let mut projections = BTreeSet::new();
            for projection in &rule.required_projections {
                if !projections.insert(projection) {
                    return Err(format!(
                        "workflow {:?} repeats projection {:?}",
                        rule.event, projection
                    )
                    .into());
                }
                if projection != SOURCE_ROUTING_REQUIREMENT {
                    return Err(format!(
                        "workflow {:?} requires unsupported projection {:?}",
                        rule.event, projection
                    )
                    .into());
                }
            }
            if rule.max_source_lag_generations != 0 {
                return Err(format!(
                    "workflow {:?} requests source lag {}; only strict zero-lag policy is implemented",
                    rule.event, rule.max_source_lag_generations
                )
                .into());
            }
        }
        Ok(())
    }

    pub fn resolve(
        &self,
        binding: &InstanceBinding,
        command: &str,
    ) -> Result<WorkflowDecision, Box<dyn std::error::Error>> {
        let Some(event) = package_run_event(command) else {
            return Ok(WorkflowDecision::NotPackage);
        };
        let event_name = event.canonical_subject();
        let actual = match direct_words(command) {
            Ok(words) => words,
            Err(reason) => {
                return Ok(WorkflowDecision::Deny(WorkflowDifferential {
                    event: Some(event_name),
                    actual_command: command.into(),
                    expected: vec![
                        "one direct package-manager command without shell composition".into(),
                    ],
                    differences: vec![reason],
                }))
            }
        };
        let Some(rule) = self
            .manifest
            .workflows
            .iter()
            .find(|rule| rule.event == event_name)
        else {
            return Ok(WorkflowDecision::Deny(WorkflowDifferential {
                event: Some(event_name),
                actual_command: command.into(),
                expected: self
                    .manifest
                    .workflows
                    .iter()
                    .map(|rule| rule.event.clone())
                    .collect(),
                differences: vec!["package event is not declared by the project manifest".into()],
            }));
        };
        let command_matches = if rule.allow_arguments {
            actual.starts_with(&rule.command)
        } else {
            actual == rule.command
        };
        if !command_matches {
            return Ok(WorkflowDecision::Deny(WorkflowDifferential {
                event: Some(event_name),
                actual_command: command.into(),
                expected: vec![render_rule_command(rule)],
                differences: vec!["actual argv does not match the declared command matcher".into()],
            }));
        }
        if rule.scope != binding.manifest.id {
            return Ok(WorkflowDecision::Deny(WorkflowDifferential {
                event: Some(event_name),
                actual_command: command.into(),
                expected: vec![format!("scope={}", binding.manifest.id)],
                differences: vec![format!(
                    "declared scope {:?} is not the bound instance {:?}",
                    rule.scope, binding.manifest.id
                )],
            }));
        }
        Ok(WorkflowDecision::Allow(WorkflowAuthorization {
            event: rule.event.clone(),
            scope: ScopeId::new(rule.scope.clone())?,
            manifest_digest: self.digest.clone(),
            command: rule.command.clone(),
            allow_arguments: rule.allow_arguments,
            required_projections: rule.required_projections.clone(),
            max_source_lag_generations: rule.max_source_lag_generations,
            verification: rule.verification,
        }))
    }
}

impl WorkflowAuthorization {
    pub fn establish_freshness(
        &self,
        routing: &RoutingReady,
    ) -> Result<String, WorkflowDifferential> {
        if self.required_projections.as_slice() != [SOURCE_ROUTING_REQUIREMENT] {
            return Err(WorkflowDifferential {
                event: Some(self.event.clone()),
                actual_command: self.command.join(" "),
                expected: vec![format!("projection={SOURCE_ROUTING_REQUIREMENT}")],
                differences: vec![
                    "required projection set is not executable by this runtime".into()
                ],
            });
        }
        Ok(format!(
            "workflow {} authorized by manifest {}; scope={}; {} generation={} lag=0 verification={:?}",
            self.event,
            self.manifest_digest,
            self.scope,
            ROUTING_PROJECTION,
            routing.generation,
            self.verification,
        ))
    }
}

pub fn resolve_package_command(
    root: &Path,
    binding: &InstanceBinding,
    command: &str,
) -> Result<WorkflowDecision, Box<dyn std::error::Error>> {
    let Some(event) = package_run_event(command) else {
        return Ok(WorkflowDecision::NotPackage);
    };
    match WorkflowCatalog::load(root)? {
        Some(catalog) => catalog.resolve(binding, command),
        None => Ok(WorkflowDecision::Deny(WorkflowDifferential {
            event: Some(event.canonical_subject()),
            actual_command: command.into(),
            expected: vec![format!("a valid project-owned {WORKFLOW_FILE}")],
            differences: vec!["package workflow manifest is absent".into()],
        })),
    }
}

fn render_rule_command(rule: &WorkflowRule) -> String {
    format!(
        "{}{}",
        rule.command.join(" "),
        if rule.allow_arguments {
            " [arguments allowed]"
        } else {
            " [exact]"
        }
    )
}

fn safe_manifest_word(word: &str) -> bool {
    !word.is_empty()
        && !word.chars().any(|character| {
            character.is_whitespace()
                || character.is_control()
                || matches!(
                    character,
                    ';' | '|' | '&' | '>' | '<' | '`' | '$' | '(' | ')' | '\'' | '"'
                )
        })
}

fn direct_words(command: &str) -> Result<Vec<String>, String> {
    let words: Vec<String> = command.split_whitespace().map(str::to_owned).collect();
    if words.is_empty() {
        return Err("command is empty".into());
    }
    if words.iter().any(|word| !safe_manifest_word(word)) {
        return Err(
            "command contains quoting, expansion, redirection, or shell composition".into(),
        );
    }
    Ok(words)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding(root: &Path, id: &str) -> InstanceBinding {
        InstanceBinding {
            manifest: crate::InstanceManifest::dedicated(id).unwrap(),
            instance_root: root.to_owned(),
            project_root: root.to_owned(),
            member: PathBuf::from("."),
        }
    }

    fn write_manifest(root: &Path, extra: &str) {
        std::fs::create_dir_all(root.join(".vyrm")).unwrap();
        std::fs::write(
            root.join(WORKFLOW_FILE),
            format!(
                r#"format = 1

[[workflows]]
event = "package:pnpm:run:typecheck"
command = ["pnpm", "run", "typecheck"]
allow_arguments = true
scope = "app"
required_projections = ["source-routing"]
max_source_lag_generations = 0
verification = "exit_zero"
{extra}"#
            ),
        )
        .unwrap();
    }

    #[test]
    fn declared_direct_command_resolves_and_preserves_script_identity() {
        let root = tempfile::tempdir().unwrap();
        write_manifest(root.path(), "");
        let catalog = WorkflowCatalog::load(root.path()).unwrap().unwrap();
        let resolved = catalog
            .resolve(
                &binding(root.path(), "app"),
                "pnpm run typecheck -- --watch",
            )
            .unwrap();
        let WorkflowDecision::Allow(authorization) = resolved else {
            panic!("declared command should be allowed")
        };
        assert_eq!(authorization.event, "package:pnpm:run:typecheck");
        assert_eq!(authorization.scope.as_str(), "app");
        assert_eq!(authorization.verification, VerificationPolicy::ExitZero);
    }

    #[test]
    fn undeclared_scope_command_and_shell_composition_fail_closed() {
        let root = tempfile::tempdir().unwrap();
        write_manifest(root.path(), "");
        let catalog = WorkflowCatalog::load(root.path()).unwrap().unwrap();
        for (binding, command, expected) in [
            (binding(root.path(), "other"), "pnpm run typecheck", "scope"),
            (
                binding(root.path(), "app"),
                "pnpm run build",
                "not declared",
            ),
            (
                binding(root.path(), "app"),
                "pnpm run typecheck && rm -rf output",
                "shell composition",
            ),
        ] {
            let WorkflowDecision::Deny(differential) = catalog.resolve(&binding, command).unwrap()
            else {
                panic!("{command:?} should be denied")
            };
            assert!(
                differential.render().contains(expected),
                "{}",
                differential.render()
            );
        }
    }

    #[test]
    fn malformed_or_ambiguous_manifests_are_rejected() {
        let root = tempfile::tempdir().unwrap();
        write_manifest(
            root.path(),
            r#"
[[workflows]]
event = "package:pnpm:run:typecheck"
command = ["pnpm", "run", "typecheck"]
scope = "app"
required_projections = ["source-routing"]
max_source_lag_generations = 0
verification = "observe"
"#,
        );
        assert!(WorkflowCatalog::load(root.path())
            .unwrap_err()
            .to_string()
            .contains("duplicate"));

        write_manifest(root.path(), "unknown_field = true\n");
        assert!(WorkflowCatalog::load(root.path())
            .unwrap_err()
            .to_string()
            .contains("unknown field"));

        write_manifest(root.path(), "");
        let manifest_path = root.path().join(WORKFLOW_FILE);
        let vague = std::fs::read_to_string(&manifest_path)
            .unwrap()
            .replace(
                "event = \"package:pnpm:run:typecheck\"",
                "event = \"package:pnpm:run\"",
            )
            .replace(
                "command = [\"pnpm\", \"run\", \"typecheck\"]",
                "command = [\"pnpm\", \"run\"]",
            );
        std::fs::write(&manifest_path, vague).unwrap();
        assert!(WorkflowCatalog::load(root.path())
            .unwrap_err()
            .to_string()
            .contains("concrete script or executable"));
    }

    #[test]
    fn absent_manifest_denies_package_commands_but_not_other_bash() {
        let root = tempfile::tempdir().unwrap();
        let binding = binding(root.path(), "app");
        assert!(matches!(
            resolve_package_command(root.path(), &binding, "cargo test").unwrap(),
            WorkflowDecision::NotPackage
        ));
        assert!(matches!(
            resolve_package_command(root.path(), &binding, "bun test").unwrap(),
            WorkflowDecision::Deny(_)
        ));
    }

    #[test]
    fn workflow_contract_matches_the_golden_vectors() {
        let raw = include_bytes!("../fixtures/workflow-v1.toml");
        let manifest: WorkflowManifest = toml::from_str(std::str::from_utf8(raw).unwrap()).unwrap();
        let catalog = WorkflowCatalog {
            path: PathBuf::from(WORKFLOW_FILE),
            digest: digest::sha256_hex(raw),
            manifest,
        };
        catalog.validate().unwrap();
        let WorkflowDecision::Allow(authorization) = catalog
            .resolve(&binding(Path::new("."), "fixture-app"), "bun test --watch")
            .unwrap()
        else {
            panic!("golden workflow should resolve")
        };
        let observation = WorkflowObservation::capture(
            &authorization,
            "bun test --watch",
            &serde_json::json!({"exitCode": 0}),
            Some(0),
            42,
        )
        .unwrap();
        let expected: WorkflowObservation =
            serde_json::from_str(include_str!("../fixtures/workflow-observation-v1.json")).unwrap();
        assert_eq!(observation, expected);
        assert_eq!(
            serde_json::to_string_pretty(&observation).unwrap(),
            include_str!("../fixtures/workflow-observation-v1.json").trim()
        );
    }
}
