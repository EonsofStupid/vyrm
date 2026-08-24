use crate::{
    DriverEffect, DriverError, DriverObservation, DriverRequest, EstateDriver, ObservedPhase,
    OperationKind,
};
use rrd_contract::CanonicalId;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use sysinfo::{Pid, ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, System, UpdateKind};
use vyrm_core::digest;

pub const LOCAL_DEPLOYMENT_FORMAT: u16 = 1;
const PROCESS_RECORD_FORMAT: u16 = 1;
const PROCESS_DISCOVERY_TIMEOUT: Duration = Duration::from_secs(3);
const PROCESS_START_STABILITY: Duration = Duration::from_millis(250);
const PROCESS_STOP_TIMEOUT: Duration = Duration::from_secs(5);
const PROCESS_STDOUT_LOG: &str = "RRD.PROCESS.STDOUT.LOG";
const PROCESS_STDERR_LOG: &str = "RRD.PROCESS.STDERR.LOG";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "source", content = "value")]
pub enum LocalArgument {
    Literal(String),
    InstanceId,
    InstanceRoot,
    InstancePath(PathBuf),
    DesiredVersion,
    ConfigurationSha256,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case", tag = "strategy")]
pub enum LocalShutdown {
    #[default]
    Immediate,
    RequestFile {
        request: PathBuf,
        complete: PathBuf,
        timeout_ms: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalDeployment {
    pub id: CanonicalId,
    pub version: String,
    pub executable: PathBuf,
    pub executable_sha256: String,
    pub arguments: Vec<LocalArgument>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub environment: BTreeMap<String, String>,
    #[serde(default)]
    pub shutdown: LocalShutdown,
}

impl LocalDeployment {
    pub fn authenticate(
        id: CanonicalId,
        version: impl Into<String>,
        executable: impl AsRef<Path>,
        arguments: Vec<LocalArgument>,
        environment: BTreeMap<String, String>,
        shutdown: LocalShutdown,
    ) -> std::result::Result<Self, String> {
        let executable = std::fs::canonicalize(executable.as_ref())
            .map_err(|error| format!("cannot resolve deployment executable: {error}"))?;
        if !executable.is_file() {
            return Err("deployment executable is not a regular file".into());
        }
        let executable_sha256 = file_sha256(&executable)
            .map_err(|error| format!("cannot authenticate deployment executable: {error}"))?;
        let deployment = Self {
            id,
            version: version.into(),
            executable,
            executable_sha256,
            arguments,
            environment,
            shutdown,
        };
        LocalDeploymentCatalog::single(deployment.clone())?;
        Ok(deployment)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalDeploymentCatalog {
    pub format: u16,
    pub deployments: BTreeMap<String, LocalDeployment>,
}

impl LocalDeploymentCatalog {
    pub fn single(deployment: LocalDeployment) -> std::result::Result<Self, String> {
        let key = deployment.id.to_string();
        let catalog = Self {
            format: LOCAL_DEPLOYMENT_FORMAT,
            deployments: BTreeMap::from([(key, deployment)]),
        };
        catalog.validate()?;
        Ok(catalog)
    }

    pub fn load_json(path: &Path) -> std::result::Result<Self, String> {
        let metadata = std::fs::metadata(path)
            .map_err(|error| format!("cannot inspect local deployment catalogue: {error}"))?;
        if metadata.len() > 1024 * 1024 {
            return Err("local deployment catalogue exceeds one MiB".into());
        }
        let bytes = std::fs::read(path)
            .map_err(|error| format!("cannot read local deployment catalogue: {error}"))?;
        let catalog: Self = serde_json::from_slice(&bytes)
            .map_err(|error| format!("cannot decode local deployment catalogue: {error}"))?;
        catalog.validate()?;
        Ok(catalog)
    }

    pub fn sha256(&self) -> String {
        digest::sha256_hex(
            &serde_json::to_vec(self).expect("validated deployment catalogue fields serialize"),
        )
    }

    pub fn validate(&self) -> std::result::Result<(), String> {
        if self.format != LOCAL_DEPLOYMENT_FORMAT || self.deployments.len() > 256 {
            return Err("unsupported or oversized local deployment catalogue".into());
        }
        for (key, deployment) in &self.deployments {
            if key != deployment.id.as_str() {
                return Err(format!("deployment key {key} does not match its id"));
            }
            if deployment.version.is_empty()
                || deployment.version.len() > 256
                || !deployment
                    .version
                    .bytes()
                    .all(|byte| byte.is_ascii_graphic())
            {
                return Err(format!("deployment {key} has an invalid version"));
            }
            if !deployment.executable.is_absolute() {
                return Err(format!("deployment {key} executable must be absolute"));
            }
            validate_sha256(&deployment.executable_sha256)?;
            #[cfg(windows)]
            if deployment
                .executable
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| {
                    extension.eq_ignore_ascii_case("bat") || extension.eq_ignore_ascii_case("cmd")
                })
            {
                return Err(format!(
                    "deployment {key} cannot use a Windows batch or cmd target"
                ));
            }
            if deployment.arguments.len() > 256 || deployment.environment.len() > 128 {
                return Err(format!(
                    "deployment {key} launch configuration is oversized"
                ));
            }
            for argument in &deployment.arguments {
                match argument {
                    LocalArgument::Literal(value) => validate_argument(value)?,
                    LocalArgument::InstancePath(path) => validate_relative_path(path)?,
                    _ => {}
                }
            }
            for (name, value) in &deployment.environment {
                if name.is_empty()
                    || name.len() > 128
                    || name.contains('=')
                    || name.as_bytes().contains(&0)
                    || value.len() > 4_096
                    || value.as_bytes().contains(&0)
                {
                    return Err(format!("deployment {key} environment is invalid"));
                }
            }
            if let LocalShutdown::RequestFile {
                request,
                complete,
                timeout_ms,
            } = &deployment.shutdown
            {
                validate_control_filename(request)?;
                validate_control_filename(complete)?;
                if request == complete || !(100..=30_000).contains(timeout_ms) {
                    return Err(format!(
                        "deployment {key} graceful shutdown contract is invalid"
                    ));
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProcessRecord {
    format: u16,
    instance_id: CanonicalId,
    operation_id: CanonicalId,
    deployment_ref: CanonicalId,
    version: String,
    configuration_sha256: String,
    executable: PathBuf,
    executable_sha256: String,
    pid: u32,
    process_start_time_unix_s: u64,
    #[serde(default)]
    shutdown: LocalShutdown,
}

pub struct LocalProcessDriver {
    state_root: PathBuf,
    catalog: LocalDeploymentCatalog,
}

impl LocalProcessDriver {
    pub fn new(
        state_root: impl Into<PathBuf>,
        catalog: LocalDeploymentCatalog,
    ) -> std::result::Result<Self, String> {
        catalog.validate()?;
        let state_root = state_root.into();
        if !state_root.is_absolute() {
            return Err("local process state root must be absolute".into());
        }
        std::fs::create_dir_all(state_root.join("instances"))
            .map_err(|error| format!("cannot create local instance root: {error}"))?;
        std::fs::create_dir_all(state_root.join("processes"))
            .map_err(|error| format!("cannot create local process root: {error}"))?;
        Ok(Self {
            state_root,
            catalog,
        })
    }

    pub fn process_record_path(&self, instance: &CanonicalId) -> PathBuf {
        self.state_root
            .join("processes")
            .join(format!("{instance}.json"))
    }

    fn deployment<'a>(
        &'a self,
        request: &DriverRequest,
    ) -> std::result::Result<&'a LocalDeployment, DriverError> {
        let deployment = self
            .catalog
            .deployments
            .get(request.desired.deployment_ref.as_str())
            .ok_or_else(|| {
                permanent(
                    request,
                    "desired deployment reference is not in the trusted catalogue",
                )
            })?;
        if deployment.version != request.desired.version {
            return Err(permanent(
                request,
                "desired version does not match the trusted deployment catalogue",
            ));
        }
        let executable = std::fs::canonicalize(&deployment.executable).map_err(|error| {
            permanent(
                request,
                format!("cannot resolve trusted executable: {error}"),
            )
        })?;
        if executable != deployment.executable {
            return Err(permanent(
                request,
                "trusted executable path must already be canonical",
            ));
        }
        let observed_sha256 = file_sha256(&executable).map_err(|error| {
            permanent(
                request,
                format!("cannot authenticate trusted executable: {error}"),
            )
        })?;
        if observed_sha256 != deployment.executable_sha256 {
            return Err(permanent(request, "trusted executable digest changed"));
        }
        Ok(deployment)
    }

    fn load_record(
        &self,
        instance: &CanonicalId,
    ) -> std::result::Result<Option<ProcessRecord>, DriverError> {
        let path = self.process_record_path(instance);
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(DriverError::retryable(
                    format!("cannot read process record: {error}"),
                    digest::sha256_hex(format!("process-record-read:{instance}").as_bytes()),
                ));
            }
        };
        let record: ProcessRecord = serde_json::from_slice(&bytes).map_err(|error| {
            DriverError::permanent(
                format!("process record is corrupt: {error}"),
                digest::sha256_hex(&bytes),
            )
        })?;
        if record.format != PROCESS_RECORD_FORMAT || record.instance_id != *instance {
            return Err(DriverError::permanent(
                "process record identity is invalid",
                digest::sha256_hex(&bytes),
            ));
        }
        Ok(Some(record))
    }

    fn inspect_record(
        &self,
        record: &ProcessRecord,
    ) -> std::result::Result<ProcessIdentity, DriverError> {
        let system = system_for(record.pid);
        let Some(process) = system.process(Pid::from_u32(record.pid)) else {
            return Ok(ProcessIdentity::Exited);
        };
        let executable = process
            .exe()
            .and_then(|path| std::fs::canonicalize(path).ok());
        if process.start_time() != record.process_start_time_unix_s
            || executable
                .as_deref()
                .is_none_or(|path| !same_executable_file(path, &record.executable))
        {
            return Ok(ProcessIdentity::Foreign);
        }
        Ok(ProcessIdentity::Owned)
    }

    fn spawn(
        &self,
        request: &DriverRequest,
        deployment: &LocalDeployment,
    ) -> std::result::Result<ProcessRecord, DriverError> {
        let instance_root = self
            .state_root
            .join("instances")
            .join(request.instance_id.as_str());
        std::fs::create_dir_all(&instance_root).map_err(|error| {
            retryable(
                request,
                format!("cannot create instance directory: {error}"),
            )
        })?;
        prepare_shutdown_artifacts(&instance_root, &deployment.shutdown).map_err(|error| {
            retryable(
                request,
                format!("cannot prepare graceful shutdown artifacts: {error}"),
            )
        })?;
        let arguments = deployment
            .arguments
            .iter()
            .map(|argument| resolve_argument(argument, request, &instance_root))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let stdout =
            open_process_log(&instance_root.join(PROCESS_STDOUT_LOG)).map_err(|error| {
                retryable(
                    request,
                    format!("cannot open managed process stdout log: {error}"),
                )
            })?;
        let stderr_path = instance_root.join(PROCESS_STDERR_LOG);
        let stderr = open_process_log(&stderr_path).map_err(|error| {
            retryable(
                request,
                format!("cannot open managed process stderr log: {error}"),
            )
        })?;
        let mut command = Command::new(&deployment.executable);
        command
            .args(&arguments)
            .current_dir(&instance_root)
            .env_clear()
            .envs(&deployment.environment)
            .stdin(Stdio::null())
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr));
        configure_platform_environment(&mut command, request)?;
        let mut child = command
            .spawn()
            .map_err(|error| retryable(request, format!("cannot spawn instance: {error}")))?;
        let pid = child.id();

        let discovery_deadline = Instant::now() + PROCESS_DISCOVERY_TIMEOUT;
        let mut candidate: Option<(u64, PathBuf)> = None;
        let mut candidate_since = None;
        let mut mismatched_executable = None;
        let (process_start_time_unix_s, executable) = loop {
            if let Some(status) = child.try_wait().map_err(|error| {
                retryable(request, format!("cannot inspect spawned process: {error}"))
            })? {
                return Err(retryable(
                    request,
                    spawned_exit_message(status, &stderr_path),
                ));
            }
            let system = system_for(pid);
            if let Some(process) = system.process(Pid::from_u32(pid)) {
                if let Some(executable) = process
                    .exe()
                    .and_then(|path| std::fs::canonicalize(path).ok())
                {
                    if !same_executable_file(&executable, &deployment.executable) {
                        if let Some(status) = child.try_wait().map_err(|error| {
                            retryable(request, format!("cannot inspect spawned process: {error}"))
                        })? {
                            return Err(retryable(
                                request,
                                spawned_exit_message(status, &stderr_path),
                            ));
                        }
                        // Process observers can briefly expose the pre-exec
                        // parent image. Never authenticate or persist it.
                        mismatched_executable = Some(executable);
                        candidate = None;
                        candidate_since = None;
                    } else {
                        mismatched_executable = None;
                        let observed = (process.start_time(), executable);
                        if candidate.as_ref() != Some(&observed) {
                            candidate = Some(observed);
                            candidate_since = Some(Instant::now());
                        } else if candidate_since
                            .is_some_and(|since| since.elapsed() >= PROCESS_START_STABILITY)
                        {
                            break candidate
                                .take()
                                .expect("a stable process identity is present");
                        }
                    }
                }
            }
            if Instant::now() >= discovery_deadline {
                let _ = child.kill();
                let _ = child.wait();
                if let Some(executable) = mismatched_executable {
                    return Err(permanent(
                        request,
                        format!(
                            "live spawned process executable does not match the trusted catalogue: expected {}, observed {}",
                            deployment.executable.display(),
                            executable.display()
                        ),
                    ));
                }
                return Err(retryable(
                    request,
                    "spawned process identity did not remain stable",
                ));
            }
            thread::sleep(Duration::from_millis(10));
        };
        debug_assert!(same_executable_file(&executable, &deployment.executable));
        let record = ProcessRecord {
            format: PROCESS_RECORD_FORMAT,
            instance_id: request.instance_id.clone(),
            operation_id: request.operation_id.clone(),
            deployment_ref: request.desired.deployment_ref.clone(),
            version: request.desired.version.clone(),
            configuration_sha256: request.desired.configuration_sha256.clone(),
            executable,
            executable_sha256: deployment.executable_sha256.clone(),
            pid,
            process_start_time_unix_s,
            shutdown: deployment.shutdown.clone(),
        };
        if let Err(error) = write_record(&self.process_record_path(&request.instance_id), &record) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(retryable(
                request,
                format!("cannot persist process record; spawned child was terminated: {error}"),
            ));
        }
        drop(child);
        Ok(record)
    }

    fn stop(
        &self,
        request: &DriverRequest,
        record: &ProcessRecord,
    ) -> std::result::Result<(), DriverError> {
        match self.inspect_record(record)? {
            ProcessIdentity::Exited => {}
            ProcessIdentity::Foreign => {
                return Err(permanent(
                    request,
                    "refusing to signal a PID whose executable/start identity changed",
                ));
            }
            ProcessIdentity::Owned => {
                if let LocalShutdown::RequestFile {
                    request: request_path,
                    complete,
                    timeout_ms,
                } = &record.shutdown
                {
                    let instance_root = self
                        .state_root
                        .join("instances")
                        .join(request.instance_id.as_str());
                    write_shutdown_request(&instance_root.join(request_path), request, record)
                        .map_err(|error| {
                            retryable(
                                request,
                                format!("cannot persist graceful shutdown request: {error}"),
                            )
                        })?;
                    if wait_for_owned_exit(record, Duration::from_millis(*timeout_ms), request)? {
                        if !instance_root.join(complete).is_file() {
                            return Err(permanent(
                                request,
                                "managed process exited without graceful shutdown confirmation",
                            ));
                        }
                        return self.remove_record(request);
                    }
                }
                kill_owned_process(record, request)?;
            }
        }
        self.remove_record(request)
    }

    fn remove_record(&self, request: &DriverRequest) -> std::result::Result<(), DriverError> {
        let path = self.process_record_path(&request.instance_id);
        match std::fs::remove_file(&path) {
            Ok(()) => sync_parent(&path).map_err(|error| {
                retryable(
                    request,
                    format!("cannot sync stopped process record removal: {error}"),
                )
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(retryable(
                request,
                format!("cannot remove stopped process record: {error}"),
            )),
        }
    }

    fn ensure_running(
        &self,
        request: &DriverRequest,
        deployment: &LocalDeployment,
    ) -> std::result::Result<(), DriverError> {
        if let Some(record) = self.load_record(&request.instance_id)? {
            match self.inspect_record(&record)? {
                ProcessIdentity::Foreign => {
                    return Err(permanent(
                        request,
                        "process record now identifies a foreign process",
                    ));
                }
                ProcessIdentity::Exited => {
                    let record_path = self.process_record_path(&request.instance_id);
                    match std::fs::remove_file(&record_path) {
                        Ok(()) => sync_parent(&record_path).map_err(|error| {
                            retryable(
                                request,
                                format!("cannot sync stale process record removal: {error}"),
                            )
                        })?,
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        Err(error) => {
                            return Err(retryable(
                                request,
                                format!("cannot remove stale process record: {error}"),
                            ));
                        }
                    }
                }
                ProcessIdentity::Owned => {
                    let target_matches = record.deployment_ref == request.desired.deployment_ref
                        && record.version == request.desired.version
                        && record.configuration_sha256 == request.desired.configuration_sha256;
                    if record.operation_id == request.operation_id
                        || (target_matches
                            && !matches!(
                                request.kind,
                                OperationKind::Restart | OperationKind::Upgrade
                            ))
                    {
                        return Ok(());
                    }
                    self.stop(request, &record)?;
                }
            }
        }
        self.spawn(request, deployment).map(|_| ())
    }

    fn ensure_stopped(&self, request: &DriverRequest) -> std::result::Result<(), DriverError> {
        if let Some(record) = self.load_record(&request.instance_id)? {
            self.stop(request, &record)?;
        }
        Ok(())
    }
}

impl EstateDriver for LocalProcessDriver {
    fn apply(&mut self, request: &DriverRequest) -> std::result::Result<DriverEffect, DriverError> {
        match request.desired.phase {
            crate::DesiredPhase::Running => {
                let deployment = self.deployment(request)?;
                self.ensure_running(request, deployment)?;
            }
            crate::DesiredPhase::Stopped | crate::DesiredPhase::Absent => {
                self.ensure_stopped(request)?;
            }
        }
        Ok(DriverEffect {
            evidence_sha256: effect_digest(request),
        })
    }

    fn observe(
        &mut self,
        request: &DriverRequest,
    ) -> std::result::Result<DriverObservation, DriverError> {
        let record = self.load_record(&request.instance_id)?;
        let (phase, version, process_id, error) = match (request.desired.phase, record) {
            (crate::DesiredPhase::Running, Some(record)) => match self.inspect_record(&record)? {
                ProcessIdentity::Owned => (
                    ObservedPhase::Running,
                    Some(record.version),
                    Some(record.pid),
                    None,
                ),
                ProcessIdentity::Exited => (
                    ObservedPhase::Failed,
                    Some(record.version),
                    None,
                    Some("managed process exited after apply".into()),
                ),
                ProcessIdentity::Foreign => (
                    ObservedPhase::Failed,
                    Some(record.version),
                    None,
                    Some("managed PID identity changed".into()),
                ),
            },
            (crate::DesiredPhase::Running, None) => (
                ObservedPhase::Failed,
                None,
                None,
                Some("managed process record is absent after apply".into()),
            ),
            (crate::DesiredPhase::Stopped, None) => (ObservedPhase::Stopped, None, None, None),
            (crate::DesiredPhase::Absent, None) => (ObservedPhase::Absent, None, None, None),
            (_, Some(record)) => match self.inspect_record(&record)? {
                ProcessIdentity::Owned => (
                    ObservedPhase::Failed,
                    Some(record.version),
                    Some(record.pid),
                    Some("managed process is still running after stop".into()),
                ),
                ProcessIdentity::Exited => match request.desired.phase {
                    crate::DesiredPhase::Stopped => (ObservedPhase::Stopped, None, None, None),
                    _ => (ObservedPhase::Absent, None, None, None),
                },
                ProcessIdentity::Foreign => (
                    ObservedPhase::Failed,
                    Some(record.version),
                    None,
                    Some("refusing to treat a foreign PID as stopped".into()),
                ),
            },
        };
        let evidence_sha256 = digest::sha256_hex(
            &serde_json::to_vec(&(&request.operation_id, phase, &version, process_id, &error))
                .expect("local observation fields serialize"),
        );
        Ok(DriverObservation {
            phase,
            version,
            process_id,
            evidence_sha256,
            error,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProcessIdentity {
    Owned,
    Exited,
    Foreign,
}

fn resolve_argument(
    argument: &LocalArgument,
    request: &DriverRequest,
    instance_root: &Path,
) -> std::result::Result<String, DriverError> {
    match argument {
        LocalArgument::Literal(value) => Ok(value.clone()),
        LocalArgument::InstanceId => Ok(request.instance_id.to_string()),
        LocalArgument::InstanceRoot => path_argument(instance_root, request),
        LocalArgument::InstancePath(relative) => {
            path_argument(&instance_root.join(relative), request)
        }
        LocalArgument::DesiredVersion => Ok(request.desired.version.clone()),
        LocalArgument::ConfigurationSha256 => Ok(request.desired.configuration_sha256.clone()),
    }
}

fn path_argument(path: &Path, request: &DriverRequest) -> std::result::Result<String, DriverError> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| permanent(request, "instance path is not valid UTF-8"))
}

fn validate_argument(value: &str) -> std::result::Result<(), String> {
    if value.len() > 4_096 || value.as_bytes().contains(&0) {
        return Err("local deployment argument is invalid".into());
    }
    Ok(())
}

fn validate_relative_path(path: &Path) -> std::result::Result<(), String> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_) | Component::CurDir))
    {
        return Err("instance-relative argument path is invalid".into());
    }
    Ok(())
}

fn validate_control_filename(path: &Path) -> std::result::Result<(), String> {
    let mut components = path.components();
    if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
        return Err("graceful shutdown paths must be direct instance filenames".into());
    }
    Ok(())
}

fn validate_sha256(value: &str) -> std::result::Result<(), String> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err("local deployment executable SHA-256 is invalid".into());
    }
    Ok(())
}

fn file_sha256(path: &Path) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let bytes = hasher.finalize();
    let mut encoded = String::with_capacity(bytes.len() * 2);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in bytes {
        encoded.push(HEX[usize::from(byte >> 4)] as char);
        encoded.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    Ok(encoded)
}

fn open_process_log(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

#[cfg(windows)]
fn configure_platform_environment(
    command: &mut Command,
    request: &DriverRequest,
) -> std::result::Result<(), DriverError> {
    let system_root = std::env::var_os("SystemRoot").ok_or_else(|| {
        permanent(
            request,
            "Windows managed process launch requires the SystemRoot platform variable",
        )
    })?;
    command.env("SystemRoot", system_root);
    Ok(())
}

#[cfg(not(windows))]
fn configure_platform_environment(
    _command: &mut Command,
    _request: &DriverRequest,
) -> std::result::Result<(), DriverError> {
    Ok(())
}

fn spawned_exit_message(status: std::process::ExitStatus, stderr_path: &Path) -> String {
    let stderr = std::fs::read(stderr_path)
        .ok()
        .map(|bytes| {
            let start = bytes.len().saturating_sub(4_096);
            String::from_utf8_lossy(&bytes[start..]).trim().to_owned()
        })
        .filter(|stderr| !stderr.is_empty())
        .unwrap_or_else(|| "managed process emitted no stderr".into());
    format!("spawned process exited during startup ({status}): {stderr}")
}

fn system_for(pid: u32) -> System {
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[Pid::from_u32(pid)]),
        true,
        ProcessRefreshKind::nothing()
            .without_tasks()
            .with_exe(UpdateKind::Always),
    );
    system
}

fn prepare_shutdown_artifacts(
    instance_root: &Path,
    shutdown: &LocalShutdown,
) -> std::io::Result<()> {
    let LocalShutdown::RequestFile {
        request, complete, ..
    } = shutdown
    else {
        return Ok(());
    };
    for relative in [request, complete] {
        let path = instance_root.join(relative);
        match std::fs::remove_file(&path) {
            Ok(()) => sync_parent(&path)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn write_shutdown_request(
    path: &Path,
    request: &DriverRequest,
    record: &ProcessRecord,
) -> std::io::Result<()> {
    let bytes = serde_json::to_vec(&serde_json::json!({
        "format": 1,
        "instance_id": request.instance_id,
        "operation_id": request.operation_id,
        "pid": record.pid,
        "process_start_time_unix_s": record.process_start_time_unix_s,
    }))
    .map_err(std::io::Error::other)?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(path) {
        Ok(mut file) => {
            file.write_all(&bytes)?;
            file.sync_all()?;
            sync_parent(path)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(error),
    }
}

fn wait_for_owned_exit(
    record: &ProcessRecord,
    timeout: Duration,
    request: &DriverRequest,
) -> std::result::Result<bool, DriverError> {
    let deadline = Instant::now() + timeout;
    loop {
        let system = system_for(record.pid);
        match system.process(Pid::from_u32(record.pid)) {
            None => return Ok(true),
            Some(process)
                if matches!(
                    process.status(),
                    ProcessStatus::Zombie | ProcessStatus::Dead
                ) =>
            {
                let _ = process.wait();
                return Ok(true);
            }
            Some(process) => {
                if process.start_time() != record.process_start_time_unix_s {
                    return Err(permanent(
                        request,
                        "refusing fallback kill after managed PID start identity changed",
                    ));
                }
                match process
                    .exe()
                    .and_then(|path| std::fs::canonicalize(path).ok())
                {
                    Some(executable) if !same_executable_file(&executable, &record.executable) => {
                        return Err(permanent(
                            request,
                            "refusing fallback kill after managed PID executable identity changed",
                        ));
                    }
                    Some(_) => {
                        if Instant::now() >= deadline {
                            return Ok(false);
                        }
                    }
                    None => {
                        if Instant::now() >= deadline {
                            return Err(permanent(
                                request,
                                "refusing fallback kill because managed PID executable identity could not be authenticated",
                            ));
                        }
                    }
                }
            }
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn kill_owned_process(
    record: &ProcessRecord,
    request: &DriverRequest,
) -> std::result::Result<(), DriverError> {
    let system = system_for(record.pid);
    let Some(process) = system.process(Pid::from_u32(record.pid)) else {
        return Ok(());
    };
    let executable = process
        .exe()
        .and_then(|path| std::fs::canonicalize(path).ok());
    if process.start_time() != record.process_start_time_unix_s
        || executable
            .as_deref()
            .is_none_or(|path| !same_executable_file(path, &record.executable))
    {
        return Err(permanent(
            request,
            "refusing to kill a PID whose executable/start identity changed",
        ));
    }
    if !process.kill() {
        return Err(retryable(request, "operating system rejected process kill"));
    }
    if !wait_for_owned_exit(record, PROCESS_STOP_TIMEOUT, request)? {
        return Err(retryable(request, "process did not exit after kill"));
    }
    Ok(())
}

fn same_executable_file(observed: &Path, expected: &Path) -> bool {
    if observed == expected {
        return true;
    }
    same_platform_file(observed, expected)
}

#[cfg(unix)]
fn same_platform_file(observed: &Path, expected: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;

    let Ok(observed) = std::fs::metadata(observed) else {
        return false;
    };
    let Ok(expected) = std::fs::metadata(expected) else {
        return false;
    };
    observed.dev() == expected.dev() && observed.ino() == expected.ino()
}

#[cfg(windows)]
fn same_platform_file(observed: &Path, expected: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;

    let Ok(observed) = std::fs::metadata(observed) else {
        return false;
    };
    let Ok(expected) = std::fs::metadata(expected) else {
        return false;
    };
    matches!(
        (
            observed.volume_serial_number(),
            observed.file_index(),
            expected.volume_serial_number(),
            expected.file_index(),
        ),
        (Some(observed_volume), Some(observed_index), Some(expected_volume), Some(expected_index))
            if observed_volume == expected_volume && observed_index == expected_index
    )
}

#[cfg(not(any(unix, windows)))]
fn same_platform_file(_observed: &Path, _expected: &Path) -> bool {
    false
}

fn write_record(path: &Path, record: &ProcessRecord) -> std::io::Result<()> {
    let temporary = path.with_extension("json.new");
    let bytes = serde_json::to_vec(record).map_err(std::io::Error::other)?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = match options.open(&temporary) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            std::fs::remove_file(&temporary)?;
            options.open(&temporary)?
        }
        Err(error) => return Err(error),
    };
    file.write_all(&bytes)?;
    file.sync_all()?;
    std::fs::rename(&temporary, path)?;
    sync_parent(path)?;
    Ok(())
}

#[cfg(unix)]
fn sync_parent(path: &Path) -> std::io::Result<()> {
    File::open(path.parent().expect("process record has a parent"))?.sync_all()
}

#[cfg(not(unix))]
fn sync_parent(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

fn effect_digest(request: &DriverRequest) -> String {
    digest::sha256_hex(
        &serde_json::to_vec(&(request, "local-process-effect-v1"))
            .expect("local effect fields serialize"),
    )
}

fn permanent(request: &DriverRequest, message: impl Into<String>) -> DriverError {
    let message = message.into();
    DriverError::permanent(
        message.clone(),
        digest::sha256_hex(
            &serde_json::to_vec(&(&request.operation_id, "permanent", &message))
                .expect("local error fields serialize"),
        ),
    )
}

fn retryable(request: &DriverRequest, message: impl Into<String>) -> DriverError {
    let message = message.into();
    DriverError::retryable(
        message.clone(),
        digest::sha256_hex(
            &serde_json::to_vec(&(&request.operation_id, "retryable", &message))
                .expect("local error fields serialize"),
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::same_executable_file;

    #[test]
    fn executable_identity_accepts_hard_links_but_not_copies() {
        let temporary = tempfile::tempdir().unwrap();
        let expected = temporary.path().join("expected");
        let hard_link = temporary.path().join("hard-link");
        let copy = temporary.path().join("copy");
        std::fs::write(&expected, b"authenticated executable image").unwrap();
        std::fs::hard_link(&expected, &hard_link).unwrap();
        std::fs::copy(&expected, &copy).unwrap();

        assert!(same_executable_file(&expected, &expected));
        assert!(same_executable_file(&hard_link, &expected));
        assert!(!same_executable_file(&copy, &expected));
    }
}
