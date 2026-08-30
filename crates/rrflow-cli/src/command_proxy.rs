//! Exact-argv process boundary owned by the RRFlow executable.
//!
//! Authorization is delegated to the engine lifecycle used by harness and MCP
//! adapters. This module owns only OS process mechanics and bounded evidence
//! capture; it never parses a shell program.

use crate::command::Execution;
use rrd_engine::{
    digest, ExactExecutionObservationV1, ExactExecutionRepositoryV1, ExactExecutionRequestV1,
    ExactExecutionStreamV1, Reader, RrdEngine, EXACT_EXECUTION_CONTRACT, MAX_EXACT_EXECUTION_ARGV,
    MAX_EXACT_EXECUTION_ARG_BYTES, MAX_EXACT_EXECUTION_OUTPUT_BYTES,
    MAX_EXACT_EXECUTION_TIMEOUT_MS,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::ffi::OsStr;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

const EXEC_TOOL: &str = "RRFlowExec";

pub struct ExactCommandRequest<'a> {
    pub root: &'a Path,
    pub session_id: Option<&'a str>,
    pub requested_cwd: Option<&'a Path>,
    pub exact_argv: &'a [String],
    pub timeout_ms: u64,
    pub max_output_bytes: usize,
}

struct StreamEvidence {
    identity: ExactExecutionStreamV1,
    retained: Vec<u8>,
}

pub fn execute(
    store: &RrdEngine,
    request: ExactCommandRequest<'_>,
    reader: &Reader,
    now: u64,
    json_output: bool,
) -> Result<Execution, Box<dyn std::error::Error>> {
    let ExactCommandRequest {
        root,
        session_id,
        requested_cwd,
        exact_argv,
        timeout_ms,
        max_output_bytes,
    } = request;
    validate_argv(exact_argv)?;
    if timeout_ms == 0 || timeout_ms > MAX_EXACT_EXECUTION_TIMEOUT_MS {
        return Err(format!(
            "exec timeout must be in 1..={MAX_EXACT_EXECUTION_TIMEOUT_MS} milliseconds"
        )
        .into());
    }
    if max_output_bytes == 0
        || u64::try_from(max_output_bytes).unwrap_or(u64::MAX) > MAX_EXACT_EXECUTION_OUTPUT_BYTES
    {
        return Err(format!(
            "exec output evidence limit must be in 1..={MAX_EXACT_EXECUTION_OUTPUT_BYTES} bytes"
        )
        .into());
    }

    let root = std::fs::canonicalize(root)?;
    let cwd = canonical_working_directory(&root, requested_cwd)?;
    let executable = resolve_executable(&exact_argv[0], &cwd)?;
    let executable_bytes = std::fs::read(&executable).map_err(|error| {
        format!(
            "cannot hash exact executable {} before authorization: {error}",
            executable.display()
        )
    })?;
    let executable_sha256 = digest::sha256_hex(&executable_bytes);
    let exact_argv_sha256 = digest::sha256_hex(&serde_json::to_vec(exact_argv)?);
    let (environment_sha256, environment_entries) = inherited_environment_identity();
    let repository_before = repository_evidence(&root)?;

    let mut execution_request = ExactExecutionRequestV1 {
        contract: EXACT_EXECUTION_CONTRACT.into(),
        project_root_sha256: digest::sha256_hex(&os_bytes(root.as_os_str())),
        cwd: cwd.to_string_lossy().into_owned(),
        invoked_as: exact_argv[0].clone(),
        executable: executable.to_string_lossy().into_owned(),
        executable_sha256,
        exact_argv: exact_argv.to_vec(),
        exact_argv_sha256,
        environment_sha256,
        environment_entries: u32::try_from(environment_entries)
            .map_err(|_| "inherited environment entry count exceeds u32")?,
        repository_before,
        timeout_ms,
        max_output_bytes: u64::try_from(max_output_bytes)?,
        verification_policy: "checked_in_work_plan".into(),
        request_sha256: String::new(),
    };
    execution_request.seal()?;
    let tool_input = serde_json::to_value(&execution_request)?;
    let mut lifecycle_input = json!({
        "tool_name": EXEC_TOOL,
        "tool_input": tool_input,
    });
    let request_sha256 = tool_request_sha256(&lifecycle_input)?;
    let tool_call_id = format!("exec-{}", &request_sha256[..24]);
    let lifecycle_object = lifecycle_input
        .as_object_mut()
        .expect("exact lifecycle input is an object");
    lifecycle_object.insert("tool_use_id".into(), Value::String(tool_call_id.clone()));
    if let Some(session_id) = session_id {
        lifecycle_object.insert("session_id".into(), Value::String(session_id.into()));
    }
    let authorization = store.handle_runtime_hook(rrd_engine::RuntimeHookRequest {
        root: &root,
        harness: Some("rrflow-exec"),
        reader,
        now,
        budget: 1_500,
        event: rrd_engine::HookEvent::PreToolUse,
        input: &lifecycle_input,
    })?;
    if response_denied(&authorization.stdout) {
        return Err(denial_reason(&authorization.stdout)
            .unwrap_or_else(|| "exact command was denied by the RRFlow lifecycle gate".into())
            .into());
    }

    let authorization_sha256 = match (
        authorization.lifecycle_context.as_ref(),
        authorization.lifecycle_authorization.as_ref(),
    ) {
        (Some(_), Some(lifecycle_authorization)) => lifecycle_authorization.decision_sha256.clone(),
        (None, None) => digest::sha256_hex(
            [
                b"rrflow-attuned-exec-authorization-v1\0".as_slice(),
                request_sha256.as_bytes(),
            ]
            .concat()
            .as_slice(),
        ),
        _ => return Err("lifecycle gate returned a partial authorization".into()),
    };
    revalidate_authorized_request(&execution_request, &root, &cwd)?;

    // The engine CAS occurs after every policy check and immediately before
    // spawn. Governed projects consume the canonical lifecycle authorization;
    // projects without a checked-in work plan retain the legacy attunement
    // boundary until that compatibility path is retired by the work plan.
    match (
        authorization.lifecycle_context.as_ref(),
        authorization.lifecycle_authorization.as_ref(),
    ) {
        (Some(lifecycle_context), Some(lifecycle_authorization)) => {
            store.consume_lifecycle_authorization(
                lifecycle_context,
                lifecycle_authorization,
                now,
            )?;
        }
        (None, None) => {
            store.consume_attuned_authorization(&root, &request_sha256, now, "cli:rrflow-exec")?;
        }
        _ => unreachable!("authorization shape was validated above"),
    }

    let started = Instant::now();
    let process = run_exact_process(
        &executable,
        &exact_argv[1..],
        &cwd,
        Duration::from_millis(timeout_ms),
        max_output_bytes,
    );
    let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let repository_after =
        repository_evidence(&root).unwrap_or_else(|error| ExactExecutionRepositoryV1 {
            revision: execution_request.repository_before.revision.clone(),
            worktree_sha256: digest::sha256_hex(error.to_string().as_bytes()),
            changed_paths: vec!["<repository evidence unavailable>".into()],
        });
    let mut report = ExactExecutionObservationV1 {
        request: execution_request,
        authorization_sha256,
        repository_after,
        exit_code: process.status.as_ref().and_then(ExitStatus::code),
        signal: exit_signal(process.status.as_ref()),
        success: process.status.as_ref().is_some_and(ExitStatus::success)
            && !process.timed_out
            && process.spawn_error.is_none(),
        timed_out: process.timed_out,
        duration_ms,
        spawn_error: process.spawn_error,
        stdout: process.stdout.identity.clone(),
        stderr: process.stderr.identity.clone(),
        observation_sha256: String::new(),
    };
    report.seal()?;

    let tool_response = serde_json::to_value(&report)?;
    let mut post_input = json!({
        "tool_use_id": tool_call_id,
        "tool_name": EXEC_TOOL,
        "tool_input": lifecycle_input["tool_input"].clone(),
        "tool_response": tool_response,
    });
    if let Some(session_id) = session_id {
        post_input
            .as_object_mut()
            .expect("exact post-tool input is an object")
            .insert("session_id".into(), Value::String(session_id.into()));
    }
    store
        .handle_runtime_hook(rrd_engine::RuntimeHookRequest {
            root: &root,
            harness: Some("rrflow-exec"),
            reader,
            now: now.saturating_add(duration_ms),
            budget: 1_500,
            event: rrd_engine::HookEvent::PostToolUse,
            input: &post_input,
        })
        .map_err(|error| {
            format!("exact command ran but its durable lifecycle observation failed: {error}")
        })?;

    let text = if json_output {
        serde_json::to_string_pretty(&report)?
    } else {
        let mut stdout = std::io::stdout().lock();
        stdout.write_all(&process.stdout.retained)?;
        stdout.flush()?;
        let mut stderr = std::io::stderr().lock();
        stderr.write_all(&process.stderr.retained)?;
        if report.stdout.truncated || report.stderr.truncated {
            writeln!(
                stderr,
                "rrflow: displayed output was truncated; complete stream identities are in the durable observation"
            )?;
        }
        stderr.flush()?;
        String::new()
    };
    let detail = Some(format!(
        "exact argv {} exit={:?} signal={:?} timeout={} stdout={} stderr={}",
        report.request.request_sha256,
        report.exit_code,
        report.signal,
        report.timed_out,
        report.stdout.sha256,
        report.stderr.sha256,
    ));
    Ok(Execution {
        text,
        effectiveness: None,
        detail,
        success: report.success,
    })
}

struct ProcessResult {
    status: Option<ExitStatus>,
    timed_out: bool,
    spawn_error: Option<String>,
    stdout: StreamEvidence,
    stderr: StreamEvidence,
}

fn run_exact_process(
    executable: &Path,
    arguments: &[String],
    cwd: &Path,
    timeout: Duration,
    retain_limit: usize,
) -> ProcessResult {
    let child = Command::new(executable)
        .args(arguments)
        .current_dir(cwd)
        .stdin(Stdio::inherit())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(error) => {
            return ProcessResult {
                status: None,
                timed_out: false,
                spawn_error: Some(error.to_string()),
                stdout: empty_stream_evidence(),
                stderr: empty_stream_evidence(),
            }
        }
    };
    let stdout = child.stdout.take().expect("piped child stdout");
    let stderr = child.stderr.take().expect("piped child stderr");
    let stdout_reader = std::thread::spawn(move || capture_stream(stdout, retain_limit));
    let stderr_reader = std::thread::spawn(move || capture_stream(stderr, retain_limit));
    let started = Instant::now();
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() < timeout => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Ok(None) => {
                timed_out = true;
                let _ = child.kill();
                break child.wait().ok();
            }
            Err(_) => {
                let _ = child.kill();
                break child.wait().ok();
            }
        }
    };
    let stdout = stdout_reader
        .join()
        .unwrap_or_else(|_| empty_stream_evidence());
    let stderr = stderr_reader
        .join()
        .unwrap_or_else(|_| empty_stream_evidence());
    ProcessResult {
        status,
        timed_out,
        spawn_error: None,
        stdout,
        stderr,
    }
}

fn capture_stream<R: Read>(mut input: R, retain_limit: usize) -> StreamEvidence {
    let mut hasher = Sha256::new();
    let mut retained = Vec::with_capacity(retain_limit.min(64 * 1024));
    let mut bytes = 0_u64;
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let read = match input.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
        hasher.update(&buffer[..read]);
        bytes = bytes.saturating_add(read as u64);
        let remaining = retain_limit.saturating_sub(retained.len());
        retained.extend_from_slice(&buffer[..read.min(remaining)]);
    }
    let utf8 = std::str::from_utf8(&retained).ok().map(str::to_owned);
    let sha256 = lowercase_hex(&hasher.finalize());
    StreamEvidence {
        identity: ExactExecutionStreamV1 {
            sha256,
            bytes,
            retained_bytes: retained.len() as u64,
            truncated: bytes > retained.len() as u64,
            retained_utf8: utf8,
        },
        retained,
    }
}

fn empty_stream_evidence() -> StreamEvidence {
    StreamEvidence {
        identity: ExactExecutionStreamV1 {
            sha256: digest::sha256_hex(&[]),
            bytes: 0,
            retained_bytes: 0,
            truncated: false,
            retained_utf8: Some(String::new()),
        },
        retained: Vec::new(),
    }
}

fn validate_argv(argv: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if argv.is_empty() || argv[0].is_empty() {
        return Err("exec requires an executable after `--`".into());
    }
    if argv.iter().any(|argument| argument.contains('\0')) {
        return Err("exec argv cannot contain NUL bytes".into());
    }
    if argv.len() > MAX_EXACT_EXECUTION_ARGV
        || argv.iter().map(String::len).sum::<usize>() > MAX_EXACT_EXECUTION_ARG_BYTES
    {
        return Err("exec argv exceeds its item or byte bound".into());
    }
    Ok(())
}

fn canonical_working_directory(
    root: &Path,
    requested: Option<&Path>,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let requested = requested.unwrap_or_else(|| Path::new("."));
    let candidate = if requested.is_absolute() {
        requested.to_owned()
    } else {
        root.join(requested)
    };
    let cwd = std::fs::canonicalize(candidate)?;
    if !cwd.is_dir() {
        return Err(format!(
            "exec working directory {} is not a directory",
            cwd.display()
        )
        .into());
    }
    if !cwd.starts_with(root) {
        return Err(format!(
            "exec working directory {} escapes project root {}",
            cwd.display(),
            root.display()
        )
        .into());
    }
    Ok(cwd)
}

fn resolve_executable(name: &str, cwd: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let requested = Path::new(name);
    if requested.is_absolute() || requested.components().count() > 1 {
        let candidate = if requested.is_absolute() {
            requested.to_owned()
        } else {
            cwd.join(requested)
        };
        return canonical_executable(candidate, name);
    }
    let path =
        std::env::var_os("PATH").ok_or("exec cannot resolve a bare executable without PATH")?;
    for directory in std::env::split_paths(&path) {
        for candidate in executable_candidates(&directory, name) {
            if candidate.is_file() {
                return canonical_executable(candidate, name);
            }
        }
    }
    Err(format!("executable {name:?} was not found on PATH").into())
}

fn canonical_executable(
    candidate: PathBuf,
    invoked_as: &str,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let canonical = std::fs::canonicalize(&candidate).map_err(|error| {
        format!(
            "cannot resolve executable {invoked_as:?} at {}: {error}",
            candidate.display()
        )
    })?;
    if !canonical.is_file() {
        return Err(format!("resolved executable {} is not a file", canonical.display()).into());
    }
    Ok(canonical)
}

fn executable_candidates(directory: &Path, name: &str) -> Vec<PathBuf> {
    let base = directory.join(name);
    #[cfg(windows)]
    {
        if Path::new(name).extension().is_some() {
            return vec![base];
        }
        let extensions = std::env::var_os("PATHEXT")
            .unwrap_or_else(|| std::ffi::OsString::from(".COM;.EXE;.BAT;.CMD"));
        return std::env::split_paths(&extensions)
            .map(|extension| {
                let mut value = base.as_os_str().to_os_string();
                value.push(extension);
                PathBuf::from(value)
            })
            .collect();
    }
    #[cfg(not(windows))]
    {
        vec![base]
    }
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

fn inherited_environment_identity() -> (String, usize) {
    let mut entries = std::env::vars_os()
        .map(|(name, value)| (os_bytes(&name), os_bytes(&value)))
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    let mut encoded = b"rrflow-inherited-environment-v1\0".to_vec();
    for (name, value) in &entries {
        encoded.extend_from_slice(&(name.len() as u64).to_be_bytes());
        encoded.extend_from_slice(name);
        encoded.extend_from_slice(&(value.len() as u64).to_be_bytes());
        encoded.extend_from_slice(value);
    }
    (digest::sha256_hex(&encoded), entries.len())
}

#[cfg(unix)]
fn os_bytes(value: &OsStr) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    value.as_bytes().to_vec()
}

#[cfg(windows)]
fn os_bytes(value: &OsStr) -> Vec<u8> {
    use std::os::windows::ffi::OsStrExt;
    value.encode_wide().flat_map(u16::to_le_bytes).collect()
}

#[cfg(not(any(unix, windows)))]
fn os_bytes(value: &OsStr) -> Vec<u8> {
    value.to_string_lossy().as_bytes().to_vec()
}

fn repository_evidence(
    root: &Path,
) -> Result<ExactExecutionRepositoryV1, Box<dyn std::error::Error>> {
    let head = git_output(root, &["rev-parse", "--verify", "HEAD"])?;
    let revision = if head.status.success() {
        format!("git:{}", String::from_utf8_lossy(&head.stdout).trim())
    } else {
        "git:unborn-or-unavailable".into()
    };
    let status = git_output(
        root,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    )?;
    if !status.status.success() {
        return Err("git could not capture the exact worktree state".into());
    }
    let diff = git_output(root, &["diff", "--binary", "--no-ext-diff", "HEAD", "--"])?;
    let mut identity = b"rrflow-worktree-v1\0".to_vec();
    identity.extend_from_slice(&head.stdout);
    identity.extend_from_slice(&status.stdout);
    identity.extend_from_slice(&diff.stdout);
    let mut changed_paths = status
        .stdout
        .split(|byte| *byte == 0)
        .filter(|entry| entry.len() > 3)
        .map(|entry| String::from_utf8_lossy(&entry[3..]).into_owned())
        .collect::<Vec<_>>();
    changed_paths.sort();
    for path in changed_paths.iter().filter(|path| {
        status
            .stdout
            .windows(path.len().saturating_add(3))
            .any(|window| window.starts_with(b"?? ") && window[3..] == *path.as_bytes())
    }) {
        let absolute = root.join(path);
        if let Ok(bytes) = std::fs::read(&absolute) {
            identity.extend_from_slice(&(path.len() as u64).to_be_bytes());
            identity.extend_from_slice(path.as_bytes());
            identity.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
            identity.extend_from_slice(&bytes);
        }
    }
    Ok(ExactExecutionRepositoryV1 {
        revision,
        worktree_sha256: digest::sha256_hex(&identity),
        changed_paths,
    })
}

fn revalidate_authorized_request(
    request: &ExactExecutionRequestV1,
    root: &Path,
    cwd: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    request.verify()?;
    if request.project_root_sha256 != digest::sha256_hex(&os_bytes(root.as_os_str()))
        || request.cwd != cwd.to_string_lossy()
    {
        return Err("authorized project root or cwd changed before spawn".into());
    }
    let executable = resolve_executable(&request.invoked_as, cwd)?;
    let executable_sha256 = digest::sha256_hex(&std::fs::read(&executable)?);
    if request.executable != executable.to_string_lossy()
        || request.executable_sha256 != executable_sha256
    {
        return Err("authorized executable identity changed before spawn".into());
    }
    let (environment_sha256, environment_entries) = inherited_environment_identity();
    if request.environment_sha256 != environment_sha256
        || request.environment_entries != u32::try_from(environment_entries)?
    {
        return Err("authorized inherited environment changed before spawn".into());
    }
    if request.repository_before != repository_evidence(root)? {
        return Err("authorized repository revision or worktree changed before spawn".into());
    }
    Ok(())
}

fn git_output(
    root: &Path,
    arguments: &[&str],
) -> Result<std::process::Output, Box<dyn std::error::Error>> {
    Ok(Command::new("git")
        .arg("-C")
        .arg(root)
        .args(arguments)
        .stdin(Stdio::null())
        .output()?)
}

fn tool_request_sha256(input: &Value) -> Result<String, serde_json::Error> {
    serde_json::to_vec(&json!({
        "tool_name": input.get("tool_name").unwrap_or(&Value::Null),
        "tool_input": input.get("tool_input").unwrap_or(&Value::Null),
    }))
    .map(|bytes| digest::sha256_hex(&bytes))
}

fn response_denied(stdout: &str) -> bool {
    serde_json::from_str::<Value>(stdout)
        .ok()
        .and_then(|value| {
            value
                .pointer("/hookSpecificOutput/permissionDecision")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .as_deref()
        == Some("deny")
}

fn denial_reason(stdout: &str) -> Option<String> {
    serde_json::from_str::<Value>(stdout)
        .ok()?
        .pointer("/hookSpecificOutput/permissionDecisionReason")?
        .as_str()
        .map(str::to_owned)
}

#[cfg(unix)]
fn exit_signal(status: Option<&ExitStatus>) -> Option<i32> {
    use std::os::unix::process::ExitStatusExt;
    status.and_then(ExitStatusExt::signal)
}

#[cfg(not(unix))]
fn exit_signal(_status: Option<&ExitStatus>) -> Option<i32> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{Cli, Command as CliCommand};
    use clap::Parser;

    #[test]
    fn exact_argv_digest_preserves_argument_boundaries() {
        let separated = digest::sha256_hex(&serde_json::to_vec(&vec!["a", "b c"]).unwrap());
        let combined = digest::sha256_hex(&serde_json::to_vec(&vec!["a b", "c"]).unwrap());
        assert_ne!(separated, combined);
    }

    #[test]
    fn cwd_cannot_escape_the_bound_project() {
        let project = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        assert!(canonical_working_directory(project.path(), Some(outside.path())).is_err());
    }

    #[test]
    fn clap_preserves_the_vector_after_the_required_separator() {
        let cli = Cli::try_parse_from([
            "rrflow",
            "exec",
            "--root",
            ".",
            "--",
            "printf",
            "%s",
            "one argument; not shell syntax",
        ])
        .unwrap();
        let CliCommand::Exec { exact_argv, .. } = cli.command else {
            panic!("exec did not parse as the exact command variant")
        };
        assert_eq!(
            exact_argv,
            ["printf", "%s", "one argument; not shell syntax"]
        );
    }

    #[test]
    fn stream_capture_retains_a_bound_but_hashes_the_complete_output() {
        let captured = capture_stream(&b"0123456789"[..], 4);
        assert_eq!(captured.retained, b"0123");
        assert_eq!(captured.identity.bytes, 10);
        assert_eq!(captured.identity.retained_bytes, 4);
        assert!(captured.identity.truncated);
        assert_eq!(captured.identity.sha256, digest::sha256_hex(b"0123456789"));
    }

    #[test]
    fn authorized_executable_drift_is_denied_before_spawn() {
        let root = tempfile::tempdir().unwrap();
        std::process::Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(root.path())
            .status()
            .unwrap();
        let executable = root.path().join("runner");
        std::fs::write(&executable, b"version-one").unwrap();
        let root_path = std::fs::canonicalize(root.path()).unwrap();
        let cwd = root_path.clone();
        let exact_argv = vec![
            executable.to_string_lossy().into_owned(),
            "one argument".into(),
        ];
        let (environment_sha256, environment_entries) = inherited_environment_identity();
        let mut request = ExactExecutionRequestV1 {
            contract: EXACT_EXECUTION_CONTRACT.into(),
            project_root_sha256: digest::sha256_hex(&os_bytes(root_path.as_os_str())),
            cwd: cwd.to_string_lossy().into_owned(),
            invoked_as: exact_argv[0].clone(),
            executable: std::fs::canonicalize(&executable)
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            executable_sha256: digest::sha256_hex(b"version-one"),
            exact_argv_sha256: digest::sha256_hex(&serde_json::to_vec(&exact_argv).unwrap()),
            exact_argv,
            environment_sha256,
            environment_entries: u32::try_from(environment_entries).unwrap(),
            repository_before: repository_evidence(&root_path).unwrap(),
            timeout_ms: 1_000,
            max_output_bytes: 1_024,
            verification_policy: "checked_in_work_plan".into(),
            request_sha256: String::new(),
        };
        request.seal().unwrap();
        revalidate_authorized_request(&request, &root_path, &cwd).unwrap();
        std::fs::write(executable, b"version-two").unwrap();
        assert!(revalidate_authorized_request(&request, &root_path, &cwd)
            .unwrap_err()
            .to_string()
            .contains("executable identity changed"));
    }

    #[test]
    fn cargo_and_javascript_vectors_are_distinct_without_shell_retokenization() {
        let cargo = vec![
            "cargo".to_owned(),
            "test".to_owned(),
            "one filter".to_owned(),
        ];
        let javascript = vec![
            "npm".to_owned(),
            "test".to_owned(),
            "--".to_owned(),
            "one spec".to_owned(),
        ];
        validate_argv(&cargo).unwrap();
        validate_argv(&javascript).unwrap();
        assert_ne!(
            digest::sha256_hex(&serde_json::to_vec(&cargo).unwrap()),
            digest::sha256_hex(&serde_json::to_vec(&javascript).unwrap())
        );
        assert_eq!(cargo[2], "one filter");
        assert_eq!(javascript[3], "one spec");
    }
}
