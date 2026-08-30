use super::validation;
use crate::{invalid, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub const EXACT_EXECUTION_CONTRACT: &str = "rrflow-exact-argv-v1";
pub const MAX_EXACT_EXECUTION_ARGV: usize = 1_024;
pub const MAX_EXACT_EXECUTION_ARG_BYTES: usize = 1_048_576;
pub const MAX_EXACT_EXECUTION_TIMEOUT_MS: u64 = 900_000;
pub const MAX_EXACT_EXECUTION_OUTPUT_BYTES: u64 = 16 * 1_024 * 1_024;
pub const MAX_EXACT_EXECUTION_CHANGED_PATHS: usize = 100_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExactExecutionRepositoryV1 {
    pub revision: String,
    pub worktree_sha256: String,
    pub changed_paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExactExecutionRequestV1 {
    pub contract: String,
    pub project_root_sha256: String,
    pub cwd: String,
    pub invoked_as: String,
    pub executable: String,
    pub executable_sha256: String,
    pub exact_argv: Vec<String>,
    pub exact_argv_sha256: String,
    pub environment_sha256: String,
    pub environment_entries: u32,
    pub repository_before: ExactExecutionRepositoryV1,
    pub timeout_ms: u64,
    pub max_output_bytes: u64,
    pub verification_policy: String,
    pub request_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExactExecutionStreamV1 {
    pub sha256: String,
    pub bytes: u64,
    pub retained_bytes: u64,
    pub truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retained_utf8: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExactExecutionObservationV1 {
    pub request: ExactExecutionRequestV1,
    pub authorization_sha256: String,
    pub repository_after: ExactExecutionRepositoryV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal: Option<i32>,
    pub success: bool,
    pub timed_out: bool,
    pub duration_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spawn_error: Option<String>,
    pub stdout: ExactExecutionStreamV1,
    pub stderr: ExactExecutionStreamV1,
    pub observation_sha256: String,
}

impl ExactExecutionRepositoryV1 {
    pub fn validate(&self) -> Result<()> {
        validation::text("execution repository revision", &self.revision)?;
        validation::sha256("execution worktree", &self.worktree_sha256)?;
        if self.changed_paths.len() > MAX_EXACT_EXECUTION_CHANGED_PATHS {
            return invalid("execution changed-path evidence exceeds its bound");
        }
        let mut unique = BTreeSet::new();
        let mut bytes = 0_usize;
        for path in &self.changed_paths {
            validation::text("execution changed path", path)?;
            bytes = bytes.saturating_add(path.len());
            if bytes > MAX_EXACT_EXECUTION_ARG_BYTES || !unique.insert(path) {
                return invalid("execution changed paths are duplicated or oversized");
            }
        }
        Ok(())
    }
}

impl ExactExecutionRequestV1 {
    pub fn seal(&mut self) -> Result<()> {
        self.request_sha256.clear();
        self.validate_fields(false)?;
        self.request_sha256 = sealed_sha256(b"rrflow-exact-execution-request-v1\0", self)?;
        Ok(())
    }

    pub fn verify(&self) -> Result<()> {
        self.validate_fields(true)?;
        let expected = self.request_sha256.clone();
        let mut candidate = self.clone();
        candidate.seal()?;
        if candidate.request_sha256 != expected {
            return invalid("exact execution request digest mismatch");
        }
        Ok(())
    }

    fn validate_fields(&self, require_digest: bool) -> Result<()> {
        if self.contract != EXACT_EXECUTION_CONTRACT {
            return invalid("unsupported exact execution contract");
        }
        validation::sha256("execution project root", &self.project_root_sha256)?;
        validation::text("execution cwd", &self.cwd)?;
        validation::text("execution invoked-as", &self.invoked_as)?;
        validation::text("execution executable", &self.executable)?;
        validation::sha256("execution executable", &self.executable_sha256)?;
        if self.exact_argv.is_empty() || self.exact_argv.len() > MAX_EXACT_EXECUTION_ARGV {
            return invalid("exact execution argv is empty or exceeds its item bound");
        }
        let mut bytes = 0_usize;
        for argument in &self.exact_argv {
            if argument.contains('\0') {
                return invalid("exact execution argv contains NUL");
            }
            bytes = bytes.saturating_add(argument.len());
        }
        if bytes > MAX_EXACT_EXECUTION_ARG_BYTES {
            return invalid("exact execution argv exceeds its byte bound");
        }
        validation::sha256("exact execution argv", &self.exact_argv_sha256)?;
        if self.exact_argv_sha256 != sha256(&serde_json::to_vec(&self.exact_argv).map_err(json)?) {
            return invalid("exact execution argv identity does not match its vector");
        }
        validation::sha256("execution environment", &self.environment_sha256)?;
        self.repository_before.validate()?;
        if self.timeout_ms == 0 || self.timeout_ms > MAX_EXACT_EXECUTION_TIMEOUT_MS {
            return invalid("exact execution timeout is outside its bound");
        }
        if self.max_output_bytes == 0 || self.max_output_bytes > MAX_EXACT_EXECUTION_OUTPUT_BYTES {
            return invalid("exact execution output retention is outside its bound");
        }
        validation::text("execution verification policy", &self.verification_policy)?;
        if require_digest {
            validation::sha256("exact execution request", &self.request_sha256)?;
        }
        Ok(())
    }
}

impl ExactExecutionStreamV1 {
    fn validate(&self, retain_limit: u64) -> Result<()> {
        validation::sha256("execution stream", &self.sha256)?;
        if self.retained_bytes > retain_limit || self.retained_bytes > self.bytes {
            return invalid("execution stream retention exceeds its bound or complete byte count");
        }
        if self.truncated != (self.bytes > self.retained_bytes) {
            return invalid("execution stream truncation flag contradicts its byte counts");
        }
        if self
            .retained_utf8
            .as_ref()
            .is_some_and(|value| value.len() as u64 != self.retained_bytes)
        {
            return invalid("execution retained UTF-8 does not match its byte count");
        }
        Ok(())
    }
}

impl ExactExecutionObservationV1 {
    pub fn seal(&mut self) -> Result<()> {
        self.observation_sha256.clear();
        self.validate_fields(false)?;
        self.observation_sha256 = sealed_sha256(b"rrflow-exact-execution-observation-v1\0", self)?;
        Ok(())
    }

    pub fn verify(&self) -> Result<()> {
        self.validate_fields(true)?;
        let expected = self.observation_sha256.clone();
        let mut candidate = self.clone();
        candidate.seal()?;
        if candidate.observation_sha256 != expected {
            return invalid("exact execution observation digest mismatch");
        }
        Ok(())
    }

    fn validate_fields(&self, require_digest: bool) -> Result<()> {
        self.request.verify()?;
        validation::sha256("execution authorization", &self.authorization_sha256)?;
        self.repository_after.validate()?;
        if self.duration_ms > self.request.timeout_ms.saturating_add(60_000) {
            return invalid("execution duration exceeds timeout accounting bound");
        }
        if let Some(error) = &self.spawn_error {
            validation::text("execution spawn error", error)?;
        }
        let expected_success = self.exit_code == Some(0)
            && self.signal.is_none()
            && !self.timed_out
            && self.spawn_error.is_none();
        if self.success != expected_success {
            return invalid(
                "execution success contradicts exit, signal, timeout, or spawn evidence",
            );
        }
        self.stdout.validate(self.request.max_output_bytes)?;
        self.stderr.validate(self.request.max_output_bytes)?;
        if require_digest {
            validation::sha256("exact execution observation", &self.observation_sha256)?;
        }
        Ok(())
    }
}

fn sealed_sha256(prefix: &[u8], value: &impl Serialize) -> Result<String> {
    let mut hasher = Sha256::new();
    hasher.update(prefix);
    hasher.update(serde_json::to_vec(value).map_err(json)?);
    Ok(lowercase_hex(&hasher.finalize()))
}

fn sha256(bytes: &[u8]) -> String {
    lowercase_hex(&Sha256::digest(bytes))
}

fn json(error: serde_json::Error) -> crate::ContractError {
    crate::ContractError(error.to_string())
}

fn lowercase_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}
