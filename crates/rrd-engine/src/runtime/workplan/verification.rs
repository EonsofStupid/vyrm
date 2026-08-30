//! Engine-owned work-plan preparation and exact verification execution.
//!
//! Adapters may provide reviewed plan text and request verification. They can
//! never provide passing process results, stream digests, repository identity,
//! or the final verified transition.

use super::{
    load_active_work_item_authorization, load_active_work_item_plan, record_work_item_plan,
    verify_work_item, WorkItemExecutionMode, WorkItemPlanRecord, WorkItemVerification,
    WorkItemVerificationArtifact, WorkItemVerificationCheck, WorkPlanSnapshot,
};
use crate::runtime::{ensure_routing_fresh, require_fresh_attunement};
use rrd_core::digest;
use rrd_store::Engine;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const MAX_VERIFICATION_OUTPUT_BYTES: u64 = 16 * 1024 * 1024;
const MAX_VERIFICATION_RUNTIME: Duration = Duration::from_secs(15 * 60);

#[allow(clippy::too_many_arguments)]
pub fn record_project_work_item_plan<E: Engine>(
    store: &E,
    root: &Path,
    plan_id: &str,
    item_id: &str,
    execution_mode: WorkItemExecutionMode,
    plan_payload: &[u8],
    verification_commands: Vec<Vec<String>>,
    now: u64,
    actor: &str,
    correlation_id: &str,
) -> Result<WorkPlanSnapshot, Box<dyn std::error::Error>> {
    if plan_payload.is_empty() {
        return Err("reviewed work-item plan must not be empty".into());
    }
    let ready = ensure_routing_fresh(store, root)?;
    let receipt = require_fresh_attunement(store, root, &ready)?;
    record_work_item_plan(
        store,
        plan_id,
        WorkItemPlanRecord {
            work_item_id: item_id.to_owned(),
            execution_mode,
            source_tree_sha256: receipt.source_tree_sha256,
            attunement_receipt_sha256: receipt.receipt_sha256,
            plan_payload_sha256: digest::sha256_hex(plan_payload),
            verification_commands,
        },
        now,
        actor,
        correlation_id,
    )
}

pub fn verify_recorded_work_item<E: Engine>(
    store: &E,
    root: &Path,
    plan_id: &str,
    now: u64,
    actor: &str,
    correlation_id: &str,
) -> Result<WorkPlanSnapshot, Box<dyn std::error::Error>> {
    let record = load_active_work_item_plan(store, plan_id)?
        .ok_or("work plan has no active recorded implementation plan")?;
    let result_source_tree_sha256 = match record.execution_mode {
        WorkItemExecutionMode::Change => {
            let authorization = load_active_work_item_authorization(store, plan_id)?
                .filter(|authorization| authorization.consumed)
                .ok_or("work plan has no consumed mutation authorization")?;
            authorization
                .result_source_tree_sha256
                .ok_or("consumed mutation has no observed result source tree")?
        }
        WorkItemExecutionMode::Qualification => record.source_tree_sha256.clone(),
    };

    let mut checks = Vec::with_capacity(record.verification_commands.len());
    for argv in &record.verification_commands {
        let output = run_verification(root, argv)?;
        let mut check = WorkItemVerificationCheck {
            name: argv.join(" "),
            argv: argv.clone(),
            passed: output.success,
            exit_code: output.exit_code,
            stdout: WorkItemVerificationArtifact {
                byte_count: output.stdout_byte_count,
                sha256: output.stdout_sha256,
            },
            stderr: WorkItemVerificationArtifact {
                byte_count: output.stderr_byte_count,
                sha256: output.stderr_sha256,
            },
            evidence_sha256: String::new(),
        };
        check.seal_evidence()?;
        if !output.success {
            return Err(format!(
                "verification failed for {:?}: stdout={} stderr={} stderr_sha256={}",
                argv,
                output.stdout_path.display(),
                output.stderr_path.display(),
                check.stderr.sha256
            )
            .into());
        }
        cleanup_verification_files(&output.stdout_path, &output.stderr_path);
        checks.push(check);
    }

    let ready = ensure_routing_fresh(store, root)?;
    let receipt = require_fresh_attunement(store, root, &ready)?;
    if receipt.source_tree_sha256 != result_source_tree_sha256 {
        return Err(
            "verification denied: project tree changed after the last observed mutation".into(),
        );
    }
    verify_work_item(
        store,
        plan_id,
        WorkItemVerification {
            work_item_id: record.work_item_id,
            source_tree_sha256: result_source_tree_sha256.clone(),
            plan_payload_sha256: record.plan_payload_sha256,
            repository_revision: repository_revision(root, &result_source_tree_sha256)?,
            platform: format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
            checks,
        },
        now,
        actor,
        correlation_id,
    )
}

struct VerificationProcessResult {
    success: bool,
    exit_code: Option<i32>,
    stdout_byte_count: u64,
    stdout_sha256: String,
    stderr_byte_count: u64,
    stderr_sha256: String,
    stdout_path: PathBuf,
    stderr_path: PathBuf,
}

fn run_verification(
    root: &Path,
    argv: &[String],
) -> Result<VerificationProcessResult, Box<dyn std::error::Error>> {
    if argv.is_empty() {
        return Err("verification argv must not be empty".into());
    }
    let directory = root.join(".rrflow/verification");
    std::fs::create_dir_all(&directory)?;
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let identity = digest::sha256_hex(&serde_json::to_vec(&(argv, nonce, std::process::id()))?);
    let stdout_path = directory.join(format!("{}-stdout.log", &identity[..24]));
    let stderr_path = directory.join(format!("{}-stderr.log", &identity[..24]));
    let stdout = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&stdout_path)?;
    let stderr = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&stderr_path)?;
    let mut child = Command::new(&argv[0])
        .args(&argv[1..])
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .map_err(|error| format!("cannot execute verification {:?}: {error}", argv))?;
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        let output_bytes =
            std::fs::metadata(&stdout_path)?.len() + std::fs::metadata(&stderr_path)?.len();
        if output_bytes > MAX_VERIFICATION_OUTPUT_BYTES {
            child.kill()?;
            child.wait()?;
            cleanup_verification_files(&stdout_path, &stderr_path);
            return Err(format!(
                "verification {:?} exceeded {} output bytes",
                argv, MAX_VERIFICATION_OUTPUT_BYTES
            )
            .into());
        }
        if started.elapsed() > MAX_VERIFICATION_RUNTIME {
            child.kill()?;
            child.wait()?;
            cleanup_verification_files(&stdout_path, &stderr_path);
            return Err(format!(
                "verification {:?} exceeded {} seconds",
                argv,
                MAX_VERIFICATION_RUNTIME.as_secs()
            )
            .into());
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    let stdout = std::fs::read(&stdout_path)?;
    let stderr = std::fs::read(&stderr_path)?;
    Ok(VerificationProcessResult {
        success: status.success(),
        exit_code: status.code(),
        stdout_byte_count: stdout.len() as u64,
        stdout_sha256: digest::sha256_hex(&stdout),
        stderr_byte_count: stderr.len() as u64,
        stderr_sha256: digest::sha256_hex(&stderr),
        stdout_path,
        stderr_path,
    })
}

fn cleanup_verification_files(stdout: &Path, stderr: &Path) {
    let _ = std::fs::remove_file(stdout);
    let _ = std::fs::remove_file(stderr);
}

fn repository_revision(
    root: &Path,
    source_tree_sha256: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(root)
        .output()?;
    if !output.status.success() {
        return Ok(format!("tree:{source_tree_sha256}"));
    }
    let commit = std::str::from_utf8(&output.stdout)?.trim();
    if commit.is_empty() {
        return Ok(format!("tree:{source_tree_sha256}"));
    }
    Ok(format!("{commit}:{source_tree_sha256}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_verification_retains_bounded_diagnostic_artifacts() {
        let root = tempfile::tempdir().unwrap();
        let argv = vec![
            std::env::current_exe()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            "--rrflow-intentionally-invalid-test-argument".into(),
        ];
        let result = run_verification(root.path(), &argv).unwrap();
        assert!(!result.success);
        assert!(result.stdout_path.is_file());
        assert!(result.stderr_path.is_file());
        cleanup_verification_files(&result.stdout_path, &result.stderr_path);
    }
}
