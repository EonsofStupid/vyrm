//! Recoverable local supervisor for the canonical RRD + Connectome topology.
//!
//! The state file contains process identities, endpoints, logs, and shutdown
//! markers but never credentials. RRD is started and proven ready before the
//! authenticated Connectome client is allowed to start.

use rrd_engine::{load_or_create_api_key, InstanceBinding, InstanceManifest};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const STATE_FORMAT: u16 = 1;
const STATE_FILE: &str = "supervisor.json";
const READY_TIMEOUT: Duration = Duration::from_secs(20);
const HTTP_TIMEOUT: Duration = Duration::from_millis(500);
const MAX_PROBE_BYTES: u64 = 64 * 1024;
const MAX_STATE_BYTES: u64 = 1024 * 1024;
const MAX_LOG_BYTES: u64 = 8 * 1024 * 1024;
const DEV_PRINCIPAL: &str = "rrflow-dev-connectome";

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Debug, Clone)]
pub struct UpOptions {
    pub root: PathBuf,
    pub instance: Option<String>,
    pub rrd_bind: SocketAddr,
    pub connectome_bind: SocketAddr,
    pub no_build: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SupervisorReport {
    pub status: String,
    pub project_root: PathBuf,
    pub instance: String,
    pub rrd_pid: Option<u32>,
    pub connectome_pid: Option<u32>,
    pub rrd_ready: bool,
    pub connectome_ready: bool,
    pub rrd_url: String,
    pub connectome_url: String,
    pub state_file: PathBuf,
}

impl SupervisorReport {
    pub fn render(&self) -> String {
        format!(
            "RRFlow development topology: {}\ninstance: {}\nRRD: {} (pid {}, ready={})\nConnectome: {} (pid {}, ready={})\nstate: {}",
            self.status,
            self.instance,
            self.rrd_url,
            display_pid(self.rrd_pid),
            self.rrd_ready,
            self.connectome_url,
            display_pid(self.connectome_pid),
            self.connectome_ready,
            self.state_file.display(),
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SupervisorState {
    format: u16,
    generation: u64,
    status: SupervisorStatus,
    project_root: PathBuf,
    instance: String,
    principal: String,
    credential_file: PathBuf,
    started_at_unix_ms: u64,
    rrd: ServiceState,
    connectome: ServiceState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SupervisorStatus {
    Starting,
    Running,
    Stopping,
    Stopped,
    Failed,
}

impl SupervisorStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Stopping => "stopping",
            Self::Stopped => "stopped",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ServiceState {
    executable: PathBuf,
    pid: Option<u32>,
    bind: SocketAddr,
    probe_path: String,
    identity_path: String,
    identity_marker: String,
    log_file: PathBuf,
    shutdown_request_file: PathBuf,
    shutdown_complete_file: PathBuf,
}

struct Binaries {
    rrd: PathBuf,
    connectome: PathBuf,
    security_bootstrap: PathBuf,
}

pub fn up(options: UpOptions, now: u64) -> Result<SupervisorReport> {
    validate_addresses(options.rrd_bind, options.connectome_bind)?;
    let root = canonical_directory(&options.root)?;
    let state_dir = state_directory(&root);
    std::fs::create_dir_all(&state_dir)?;
    let state_path = state_dir.join(STATE_FILE);

    let prior = read_state_optional(&state_path)?;
    if let Some(prior) = &prior {
        let report = report(prior, &state_path);
        if report.rrd_ready && report.connectome_ready {
            return Ok(report);
        }
        if report.rrd_ready || report.connectome_ready {
            return Err(format!(
                "development topology is partially alive; inspect `{}` and run `rrflow dev stop --root {}` before restarting",
                state_path.display(),
                root.display()
            )
            .into());
        }
    }

    let binaries = resolve_binaries(options.no_build)?;
    let (manifest, _) = match options.instance {
        Some(instance) => InstanceManifest::ensure_dedicated_as(&root, instance)?,
        None => InstanceManifest::ensure_dedicated(&root)?,
    };
    let binding = InstanceBinding::discover(&root)?;
    binding.require_runtime_ready()?;
    let database = binding.verify_store_path(&binding.expected_store())?;
    let credential_file = state_dir.join("connectome.api-key");
    let _credential = load_or_create_api_key(&credential_file)?;
    let bootstrap_manifest = state_dir.join("security-bootstrap.json");
    write_bootstrap_manifest(
        &bootstrap_manifest,
        &manifest.id,
        &credential_file,
        DEV_PRINCIPAL,
    )?;
    run_security_bootstrap(
        &binaries.security_bootstrap,
        &database,
        &manifest.id,
        &bootstrap_manifest,
        now,
    )?;

    let generation = prior
        .as_ref()
        .map_or(1, |state| state.generation.saturating_add(1));
    let rrd = service_state(
        &state_dir,
        "rrd",
        binaries.rrd,
        options.rrd_bind,
        "/v1/health/ready",
        "/v1/capabilities",
        format!("\"id\":\"{}\"", manifest.id),
    );
    let connectome = service_state(
        &state_dir,
        "connectome",
        binaries.connectome,
        options.connectome_bind,
        "/api/snapshot",
        "/api/snapshot",
        format!("\"scope\":\"instance:{}\"", manifest.id),
    );
    clear_shutdown_files(&rrd)?;
    clear_shutdown_files(&connectome)?;
    let mut state = SupervisorState {
        format: STATE_FORMAT,
        generation,
        status: SupervisorStatus::Starting,
        project_root: root.clone(),
        instance: manifest.id,
        principal: DEV_PRINCIPAL.into(),
        credential_file,
        started_at_unix_ms: now,
        rrd,
        connectome,
    };
    write_state(&state_path, &state)?;

    let mut rrd_child = spawn_rrd(&state)?;
    state.rrd.pid = Some(rrd_child.id());
    write_state(&state_path, &state)?;
    if let Err(error) = wait_for_service(&mut rrd_child, &state.rrd) {
        state.status = SupervisorStatus::Failed;
        write_state(&state_path, &state)?;
        request_shutdown(&state.rrd.shutdown_request_file)?;
        return Err(with_log_hint(error, &state.rrd.log_file));
    }

    let mut connectome_child = match spawn_connectome(&state) {
        Ok(child) => child,
        Err(error) => {
            state.status = SupervisorStatus::Failed;
            write_state(&state_path, &state)?;
            request_shutdown(&state.rrd.shutdown_request_file)?;
            return Err(with_log_hint(error, &state.connectome.log_file));
        }
    };
    state.connectome.pid = Some(connectome_child.id());
    write_state(&state_path, &state)?;
    if let Err(error) = wait_for_service(&mut connectome_child, &state.connectome) {
        state.status = SupervisorStatus::Failed;
        write_state(&state_path, &state)?;
        request_shutdown(&state.connectome.shutdown_request_file)?;
        request_shutdown(&state.rrd.shutdown_request_file)?;
        return Err(with_log_hint(error, &state.connectome.log_file));
    }

    state.status = SupervisorStatus::Running;
    write_state(&state_path, &state)?;
    Ok(report(&state, &state_path))
}

pub fn status(root: &Path) -> Result<SupervisorReport> {
    let root = canonical_directory(root)?;
    let state_path = state_directory(&root).join(STATE_FILE);
    let state = read_state(&state_path)?;
    Ok(report(&state, &state_path))
}

pub fn stop(root: &Path, timeout: Duration) -> Result<SupervisorReport> {
    let root = canonical_directory(root)?;
    let state_path = state_directory(&root).join(STATE_FILE);
    let mut state = read_state(&state_path)?;
    let initial = report(&state, &state_path);
    if !initial.rrd_ready && !initial.connectome_ready {
        state.status = SupervisorStatus::Stopped;
        write_state(&state_path, &state)?;
        return Ok(report(&state, &state_path));
    }
    state.status = SupervisorStatus::Stopping;
    write_state(&state_path, &state)?;
    request_shutdown(&state.connectome.shutdown_request_file)?;
    request_shutdown(&state.rrd.shutdown_request_file)?;

    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if state.connectome.shutdown_complete_file.is_file()
            && state.rrd.shutdown_complete_file.is_file()
            && !probe(state.connectome.bind, &state.connectome.probe_path)
            && !probe(state.rrd.bind, &state.rrd.probe_path)
        {
            state.status = SupervisorStatus::Stopped;
            write_state(&state_path, &state)?;
            return Ok(report(&state, &state_path));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    state.status = SupervisorStatus::Failed;
    write_state(&state_path, &state)?;
    Err(format!(
        "development topology did not complete graceful shutdown within {} ms; state retained at {}",
        timeout.as_millis(),
        state_path.display()
    )
    .into())
}

pub fn logs(root: &Path, service: &str, lines: usize) -> Result<String> {
    if lines == 0 || lines > 10_000 {
        return Err("--lines must be in 1..=10000".into());
    }
    let root = canonical_directory(root)?;
    let state_path = state_directory(&root).join(STATE_FILE);
    let state = read_state(&state_path)?;
    match service {
        "rrd" => tail_log("rrd", &state.rrd.log_file, lines),
        "connectome" => tail_log("connectome", &state.connectome.log_file, lines),
        "all" => Ok(format!(
            "{}\n{}",
            tail_log("rrd", &state.rrd.log_file, lines)?,
            tail_log("connectome", &state.connectome.log_file, lines)?
        )),
        _ => Err("--service must be one of: all, rrd, connectome".into()),
    }
}

fn validate_addresses(rrd: SocketAddr, connectome: SocketAddr) -> Result<()> {
    if !rrd.ip().is_loopback() || !connectome.ip().is_loopback() {
        return Err("development services must bind to loopback addresses".into());
    }
    if rrd.port() == 0 || connectome.port() == 0 || rrd == connectome {
        return Err("development services require distinct non-zero ports".into());
    }
    Ok(())
}

fn canonical_directory(root: &Path) -> Result<PathBuf> {
    let root = std::fs::canonicalize(root).map_err(|error| {
        format!(
            "cannot resolve development project root {}: {error}",
            root.display()
        )
    })?;
    if !root.is_dir() {
        return Err(format!(
            "development project root {} is not a directory",
            root.display()
        )
        .into());
    }
    Ok(root)
}

fn state_directory(root: &Path) -> PathBuf {
    root.join(".rrflow/dev")
}

fn service_state(
    state_dir: &Path,
    name: &str,
    executable: PathBuf,
    bind: SocketAddr,
    probe_path: &str,
    identity_path: &str,
    identity_marker: String,
) -> ServiceState {
    ServiceState {
        executable,
        pid: None,
        bind,
        probe_path: probe_path.into(),
        identity_path: identity_path.into(),
        identity_marker,
        log_file: state_dir.join(format!("{name}.log")),
        shutdown_request_file: state_dir.join(format!("{name}.shutdown.request")),
        shutdown_complete_file: state_dir.join(format!("{name}.shutdown.complete")),
    }
}

fn resolve_binaries(no_build: bool) -> Result<Binaries> {
    let suffix = std::env::consts::EXE_SUFFIX;
    if no_build {
        let directory = std::env::var_os("RRFLOW_DEV_BIN_DIR")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::current_exe()
                    .ok()?
                    .parent()
                    .map(Path::to_path_buf)
            })
            .ok_or("cannot resolve RRFlow companion binary directory")?;
        return verify_binaries(&directory, suffix);
    }

    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or("cannot resolve RRFlow source workspace")?;
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let status = Command::new(&cargo)
        .current_dir(workspace)
        .args([
            "build",
            "--locked",
            "-p",
            "rrd-server",
            "-p",
            "connectome-ui",
            "-p",
            "rrd-security",
        ])
        .status()?;
    if !status.success() {
        return Err(format!("cargo build for development services failed with {status}").into());
    }
    let metadata = Command::new(cargo)
        .current_dir(workspace)
        .args(["metadata", "--format-version", "1", "--no-deps", "--locked"])
        .output()?;
    if !metadata.status.success() {
        return Err("cargo metadata failed after building development services".into());
    }
    let value: serde_json::Value = serde_json::from_slice(&metadata.stdout)?;
    let directory = PathBuf::from(
        value
            .get("target_directory")
            .and_then(serde_json::Value::as_str)
            .ok_or("cargo metadata omitted target_directory")?,
    )
    .join("debug");
    verify_binaries(&directory, suffix)
}

fn verify_binaries(directory: &Path, suffix: &str) -> Result<Binaries> {
    let binaries = Binaries {
        rrd: directory.join(format!("rrd-server{suffix}")),
        connectome: directory.join(format!("connectome{suffix}")),
        security_bootstrap: directory.join(format!("rrd-security-bootstrap{suffix}")),
    };
    for path in [
        &binaries.rrd,
        &binaries.connectome,
        &binaries.security_bootstrap,
    ] {
        if !path.is_file() {
            return Err(
                format!("required development binary is missing: {}", path.display()).into(),
            );
        }
    }
    Ok(binaries)
}

fn write_bootstrap_manifest(
    path: &Path,
    instance: &str,
    credential_file: &Path,
    principal: &str,
) -> Result<()> {
    let mut actions = BTreeSet::from([
        "session_create".to_owned(),
        "query_execute".to_owned(),
        "diagnostics_read".to_owned(),
        "runtime_tool_catalogue_read".to_owned(),
    ]);
    for definition in rrd_engine::runtime_tool_catalogue() {
        let encoded = serde_json::to_value(definition.action)?;
        actions.insert(
            encoded
                .as_str()
                .ok_or("runtime-tool security action did not serialize as a string")?
                .to_owned(),
        );
    }
    let resource = serde_json::json!({
        "segments": [{"kind": "instance", "id": instance}],
    });
    let grants = actions
        .into_iter()
        .map(|action| {
            serde_json::json!({
                "action": action,
                "resource_prefix": resource.clone(),
            })
        })
        .collect::<Vec<_>>();
    let document = serde_json::json!({
        "format_version": 1,
        "revision": 1,
        "principals": [{
            "id": principal,
            "kind": "service",
            "credential_file": std::fs::canonicalize(credential_file)?,
            "not_before_unix_ms": 1,
            "expires_at_unix_ms": u64::MAX,
            "grants": grants,
        }],
    });
    write_private_json(path, &document)
}

fn run_security_bootstrap(
    executable: &Path,
    database: &Path,
    instance: &str,
    manifest: &Path,
    now: u64,
) -> Result<()> {
    let output = Command::new(executable)
        .args(["--db", path_arg(database)?, "--instance", instance])
        .args(["--manifest", path_arg(manifest)?, "--at-unix-ms"])
        .arg(now.to_string())
        .output()?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(format!(
        "security bootstrap failed with {}: {}",
        output.status,
        stderr.trim()
    )
    .into())
}

fn spawn_rrd(state: &SupervisorState) -> Result<Child> {
    let (stdout, stderr) = log_streams(&state.rrd.log_file)?;
    Ok(Command::new(&state.rrd.executable)
        .current_dir(&state.project_root)
        .args(["--root", path_arg(&state.project_root)?])
        .args(["--bind", &state.rrd.bind.to_string()])
        .args([
            "--shutdown-request-file",
            path_arg(&state.rrd.shutdown_request_file)?,
            "--shutdown-complete-file",
            path_arg(&state.rrd.shutdown_complete_file)?,
        ])
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(stderr)
        .spawn()?)
}

fn spawn_connectome(state: &SupervisorState) -> Result<Child> {
    let (stdout, stderr) = log_streams(&state.connectome.log_file)?;
    Ok(Command::new(&state.connectome.executable)
        .current_dir(&state.project_root)
        .args(["--instance", &state.instance])
        .args(["--principal", &state.principal])
        .args(["--api-key-file", path_arg(&state.credential_file)?])
        .args(["--rrd-address", &state.rrd.bind.to_string()])
        .args(["--scope", &format!("instance:{}", state.instance)])
        .args(["--bind", &state.connectome.bind.to_string()])
        .args([
            "--shutdown-request-file",
            path_arg(&state.connectome.shutdown_request_file)?,
            "--shutdown-complete-file",
            path_arg(&state.connectome.shutdown_complete_file)?,
        ])
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(stderr)
        .spawn()?)
}

fn log_streams(path: &Path) -> Result<(Stdio, Stdio)> {
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let log = options.open(path)?;
    Ok((Stdio::from(log.try_clone()?), Stdio::from(log)))
}

fn wait_for_service(child: &mut Child, service: &ServiceState) -> Result<()> {
    let deadline = Instant::now() + READY_TIMEOUT;
    while Instant::now() < deadline {
        if service_ready(service) {
            return Ok(());
        }
        if let Some(status) = child.try_wait()? {
            return Err(format!("service exited before readiness with {status}").into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Err(format!(
        "service at http://{}{} did not become ready with identity {}",
        service.bind, service.probe_path, service.identity_marker
    )
    .into())
}

fn probe(address: SocketAddr, path: &str) -> bool {
    probe_response(address, path).is_some()
}

fn probe_response(address: SocketAddr, path: &str) -> Option<String> {
    let Ok(mut stream) = TcpStream::connect_timeout(&address, HTTP_TIMEOUT) else {
        return None;
    };
    let _ = stream.set_read_timeout(Some(HTTP_TIMEOUT));
    let _ = stream.set_write_timeout(Some(HTTP_TIMEOUT));
    if write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
    )
    .is_err()
    {
        return None;
    }
    let mut response = Vec::new();
    if stream
        .take(MAX_PROBE_BYTES)
        .read_to_end(&mut response)
        .is_err()
    {
        return None;
    };
    let response = String::from_utf8_lossy(&response).into_owned();
    if !response
        .lines()
        .next()
        .is_some_and(|line| line.split_whitespace().nth(1) == Some("200"))
    {
        return None;
    }
    Some(response)
}

fn service_ready(service: &ServiceState) -> bool {
    probe(service.bind, &service.probe_path)
        && probe_response(service.bind, &service.identity_path)
            .is_some_and(|response| response.contains(&service.identity_marker))
}

fn report(state: &SupervisorState, state_path: &Path) -> SupervisorReport {
    let rrd_ready = service_ready(&state.rrd);
    let connectome_ready = service_ready(&state.connectome);
    let status = if state.status == SupervisorStatus::Running && (!rrd_ready || !connectome_ready) {
        "degraded"
    } else {
        state.status.as_str()
    };
    SupervisorReport {
        status: status.into(),
        project_root: state.project_root.clone(),
        instance: state.instance.clone(),
        rrd_pid: state.rrd.pid,
        connectome_pid: state.connectome.pid,
        rrd_ready,
        connectome_ready,
        rrd_url: format!("http://{}{}", state.rrd.bind, state.rrd.probe_path),
        connectome_url: format!(
            "http://{}{}",
            state.connectome.bind, state.connectome.probe_path
        ),
        state_file: state_path.to_path_buf(),
    }
}

fn read_state_optional(path: &Path) -> Result<Option<SupervisorState>> {
    match read_state(path) {
        Ok(state) => Ok(Some(state)),
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|io| io.kind() == std::io::ErrorKind::NotFound) =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

fn read_state(path: &Path) -> Result<SupervisorState> {
    let metadata = std::fs::metadata(path)?;
    if !metadata.is_file() || metadata.len() > MAX_STATE_BYTES {
        return Err(format!(
            "supervisor state {} is not a bounded regular file",
            path.display()
        )
        .into());
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    File::open(path)?
        .take(MAX_STATE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    let state: SupervisorState = serde_json::from_slice(&bytes)?;
    if state.format != STATE_FORMAT || !state.project_root.is_absolute() {
        return Err(format!(
            "supervisor state {} has an invalid format or root",
            path.display()
        )
        .into());
    }
    Ok(state)
}

fn write_state(path: &Path, state: &SupervisorState) -> Result<()> {
    write_private_json(path, state)
}

fn write_private_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value)?;
    let temporary = path.with_extension("json.new");
    let _ = std::fs::remove_file(&temporary);
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    std::fs::rename(&temporary, path)?;
    #[cfg(unix)]
    File::open(path.parent().expect("state file has a parent"))?.sync_all()?;
    Ok(())
}

fn clear_shutdown_files(service: &ServiceState) -> Result<()> {
    for path in [
        &service.shutdown_request_file,
        &service.shutdown_complete_file,
    ] {
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn request_shutdown(path: &Path) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(b"rrflow-dev-stop-v1\n")?;
    file.sync_all()?;
    Ok(())
}

fn tail_log(name: &str, path: &Path, lines: usize) -> Result<String> {
    let metadata = std::fs::metadata(path)?;
    if !metadata.is_file() {
        return Err(format!("log {} is not a regular file", path.display()).into());
    }
    let start = metadata.len().saturating_sub(MAX_LOG_BYTES);
    let mut file = File::open(path)?;
    use std::io::{Seek, SeekFrom};
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::new();
    file.take(MAX_LOG_BYTES).read_to_end(&mut bytes)?;
    let text = String::from_utf8_lossy(&bytes);
    let selected = text.lines().rev().take(lines).collect::<Vec<_>>();
    Ok(format!(
        "==> {name}: {} <==\n{}",
        path.display(),
        selected.into_iter().rev().collect::<Vec<_>>().join("\n")
    ))
}

fn with_log_hint(error: Box<dyn std::error::Error>, log: &Path) -> Box<dyn std::error::Error> {
    format!("{error}; inspect {}", log.display()).into()
}

fn path_arg(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| format!("path is not valid UTF-8: {}", path.display()).into())
}

fn display_pid(pid: Option<u32>) -> String {
    pid.map_or_else(|| "none".into(), |pid| pid.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_are_loopback_distinct_and_stable() {
        assert!(validate_addresses(
            "127.0.0.1:9477".parse().unwrap(),
            "127.0.0.1:4387".parse().unwrap()
        )
        .is_ok());
        assert!(validate_addresses(
            "0.0.0.0:9477".parse().unwrap(),
            "127.0.0.1:4387".parse().unwrap()
        )
        .is_err());
        assert!(validate_addresses(
            "127.0.0.1:9477".parse().unwrap(),
            "127.0.0.1:9477".parse().unwrap()
        )
        .is_err());
    }

    #[test]
    fn bounded_tail_returns_last_lines() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("service.log");
        std::fs::write(&path, "one\ntwo\nthree\n").unwrap();
        let tail = tail_log("service", &path, 2).unwrap();
        assert!(tail.ends_with("two\nthree"));
        assert!(!tail.contains("\none\n"));
    }
}
