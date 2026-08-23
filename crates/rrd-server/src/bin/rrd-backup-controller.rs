use rrd_contract::CanonicalId;
use rrd_estate::{
    BackupDriverRequest, BackupReconciler, BackupResult, DriverError, EstateBackupDriver,
    LocalEstateBackupDriver,
};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;
use vyrm_store::PersistentEngine;

const HOLD_AFTER_EFFECT_ENV: &str = "RRD_BACKUP_TEST_HOLD_AFTER_EFFECT_FILE";
const HOLD_AFTER_STEP_ENV: &str = "RRD_BACKUP_TEST_HOLD_AFTER_STEP_FILE";

struct Args {
    db: PathBuf,
    state_root: PathBuf,
    estate: CanonicalId,
    worker: CanonicalId,
    lease_ms: u64,
    at: u64,
}

struct EffectHoldDriver {
    inner: LocalEstateBackupDriver,
}

impl EstateBackupDriver for EffectHoldDriver {
    fn create(
        &mut self,
        request: &BackupDriverRequest,
    ) -> std::result::Result<BackupResult, DriverError> {
        let result = self.inner.create(request)?;
        maybe_test_hold(HOLD_AFTER_EFFECT_ENV, &result.backup_id).map_err(|error| {
            DriverError::retryable(error.to_string(), result.evidence_sha256.clone())
        })?;
        Ok(result)
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("rrd-backup-controller: {error}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_args(std::env::args().skip(1))
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let engine = PersistentEngine::open(&args.db)?;
    let driver = LocalEstateBackupDriver::new(&args.state_root)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let mut reconciler = BackupReconciler::new(
        &engine,
        args.estate,
        args.worker,
        args.lease_ms,
        EffectHoldDriver { inner: driver },
    )?;
    let outcome = reconciler.step(args.at)?;
    let encoded = serde_json::to_vec(&outcome)?;
    io::stdout().write_all(&encoded)?;
    io::stdout().write_all(b"\n")?;
    io::stdout().flush()?;
    maybe_test_hold(
        HOLD_AFTER_STEP_ENV,
        std::str::from_utf8(&encoded).unwrap_or("completed"),
    )?;
    Ok(())
}

#[cfg(debug_assertions)]
fn maybe_test_hold(variable: &str, contents: &str) -> io::Result<()> {
    let Some(path) = std::env::var_os(variable).map(PathBuf::from) else {
        return Ok(());
    };
    write_marker(&path, contents)?;
    loop {
        std::thread::park_timeout(Duration::from_secs(60));
    }
}

#[cfg(not(debug_assertions))]
fn maybe_test_hold(variable: &str, _contents: &str) -> io::Result<()> {
    if std::env::var_os(variable).is_some() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "test hold failpoints are unavailable in release builds",
        ));
    }
    Ok(())
}

#[cfg(debug_assertions)]
fn write_marker(path: &Path, contents: &str) -> io::Result<()> {
    let temporary = path.with_extension("new");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    file.write_all(contents.as_bytes())?;
    file.sync_all()?;
    std::fs::rename(temporary, path)
}

fn parse_args(arguments: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut db = None;
    let mut state_root = None;
    let mut estate = None;
    let mut worker = None;
    let mut lease_ms = 30_000;
    let mut at = None;
    let mut arguments = arguments;
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--db" => db = Some(PathBuf::from(required(&mut arguments, "--db")?)),
            "--state-root" => {
                state_root = Some(PathBuf::from(required(&mut arguments, "--state-root")?));
            }
            "--estate" => {
                estate = Some(
                    CanonicalId::new(required(&mut arguments, "--estate")?)
                        .map_err(|error| error.to_string())?,
                );
            }
            "--worker" => {
                worker = Some(
                    CanonicalId::new(required(&mut arguments, "--worker")?)
                        .map_err(|error| error.to_string())?,
                );
            }
            "--lease-ms" => {
                lease_ms = required(&mut arguments, "--lease-ms")?
                    .parse()
                    .map_err(|error| format!("invalid --lease-ms: {error}"))?;
            }
            "--at" => {
                at = Some(
                    required(&mut arguments, "--at")?
                        .parse()
                        .map_err(|error| format!("invalid --at: {error}"))?,
                );
            }
            value => return Err(format!("unknown argument {value:?}")),
        }
    }
    Ok(Args {
        db: db.ok_or("--db is required")?,
        state_root: state_root.ok_or("--state-root is required")?,
        estate: estate.ok_or("--estate is required")?,
        worker: worker.ok_or("--worker is required")?,
        lease_ms,
        at: at.ok_or("--at is required")?,
    })
}

fn required(arguments: &mut impl Iterator<Item = String>, option: &str) -> Result<String, String> {
    arguments
        .next()
        .ok_or_else(|| format!("{option} requires a value"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requires_the_complete_backup_controller_identity() {
        let error = parse_args(["--db".into(), "/tmp/db".into()].into_iter())
            .err()
            .expect("incomplete arguments must fail");
        assert_eq!(error, "--state-root is required");
    }

    #[test]
    fn parses_a_complete_one_step_request() {
        let args = parse_args(
            [
                "--db",
                "/tmp/db",
                "--state-root",
                "/tmp/state",
                "--estate",
                "estate-a",
                "--worker",
                "worker-a",
                "--lease-ms",
                "9000",
                "--at",
                "42",
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .unwrap();
        assert_eq!(args.db, PathBuf::from("/tmp/db"));
        assert_eq!(args.state_root, PathBuf::from("/tmp/state"));
        assert_eq!(args.estate.as_str(), "estate-a");
        assert_eq!(args.worker.as_str(), "worker-a");
        assert_eq!(args.lease_ms, 9_000);
        assert_eq!(args.at, 42);
    }
}
