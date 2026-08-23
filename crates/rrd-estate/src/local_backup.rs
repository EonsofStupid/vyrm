use crate::{BackupDriverRequest, BackupResult, DriverError, EstateBackupDriver};
use std::fs;
use std::path::{Path, PathBuf};
use vyrm_core::digest;
use vyrm_store::{
    create_logical_backup, verify_backup_catalogue, Error as StoreError, PersistentEngine,
};

/// Executes estate backup jobs only inside one canonical local state root.
///
/// Source and catalogue locations are deliberately not caller-controlled:
/// `<state>/instances/<instance>/rrd-data` is archived into
/// `<state>/backups/<instance>`. A retained process record denies the effect.
pub struct LocalEstateBackupDriver {
    state_root: PathBuf,
}

impl LocalEstateBackupDriver {
    pub fn new(state_root: impl AsRef<Path>) -> std::result::Result<Self, String> {
        let supplied = state_root.as_ref();
        if !supplied.is_absolute() {
            return Err("local backup state root must be absolute".into());
        }
        let state_root = fs::canonicalize(supplied)
            .map_err(|error| format!("cannot resolve local backup state root: {error}"))?;
        if !state_root.is_dir() {
            return Err("local backup state root must be a directory".into());
        }
        ensure_direct_directory(&state_root, "instances")?;
        ensure_direct_directory(&state_root, "processes")?;
        ensure_direct_directory(&state_root, "backups")?;
        Ok(Self { state_root })
    }

    pub fn state_root(&self) -> &Path {
        &self.state_root
    }

    pub fn source_path(&self, request: &BackupDriverRequest) -> PathBuf {
        self.state_root
            .join("instances")
            .join(request.instance_id.as_str())
            .join("rrd-data")
    }

    pub fn catalogue_path(&self, request: &BackupDriverRequest) -> PathBuf {
        self.state_root
            .join("backups")
            .join(request.instance_id.as_str())
    }

    fn create_inner(
        &self,
        request: &BackupDriverRequest,
    ) -> std::result::Result<BackupResult, DriverError> {
        let process_record = self
            .state_root
            .join("processes")
            .join(format!("{}.json", request.instance_id));
        match fs::symlink_metadata(&process_record) {
            Ok(_) => {
                return Err(permanent(
                    request,
                    "backup denied because the instance has a retained process record",
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(retryable(
                    request,
                    format!("cannot inspect the instance process record: {error}"),
                ));
            }
        }

        let source = self.source_path(request);
        let canonical_source = fs::canonicalize(&source).map_err(|error| {
            permanent(
                request,
                format!("cannot resolve the fixed instance data root: {error}"),
            )
        })?;
        if canonical_source != source || !canonical_source.is_dir() {
            return Err(permanent(
                request,
                "fixed instance data root is not a direct canonical directory",
            ));
        }

        let catalogue = self.catalogue_path(request);
        create_direct_directory(&catalogue).map_err(|error| {
            retryable(
                request,
                format!("cannot create the fixed backup catalogue root: {error}"),
            )
        })?;
        let canonical_catalogue = fs::canonicalize(&catalogue).map_err(|error| {
            retryable(
                request,
                format!("cannot resolve the fixed backup catalogue root: {error}"),
            )
        })?;
        if canonical_catalogue != catalogue || !canonical_catalogue.is_dir() {
            return Err(permanent(
                request,
                "fixed backup catalogue root is not a direct canonical directory",
            ));
        }

        // Existing catalogue corruption is an operator condition, not a retry loop.
        verify_backup_catalogue(&canonical_catalogue)
            .map_err(|error| permanent_store(request, "backup catalogue authentication", error))?;
        let engine = PersistentEngine::open(&canonical_source)
            .map_err(|error| permanent_store(request, "instance store open", error))?;
        let entry = create_logical_backup(
            &engine,
            &canonical_catalogue,
            &request.label,
            request.created_at,
        )
        .map_err(|error| retryable_store(request, "logical backup creation", error))?;
        let verified = verify_backup_catalogue(&canonical_catalogue)
            .map_err(|error| permanent_store(request, "created backup verification", error))?;
        let retained = verified
            .backups
            .iter()
            .find(|candidate| candidate.backup_id == entry.backup_id)
            .ok_or_else(|| permanent(request, "created backup is absent from its catalogue"))?;
        if retained != &entry {
            return Err(permanent(
                request,
                "created backup entry diverges from its authenticated catalogue",
            ));
        }
        let evidence_sha256 = digest::sha256_hex(
            &serde_json::to_vec(&(
                "rrd-local-estate-backup-v1",
                request,
                &entry.backup_id,
                &entry.archive.archive_sha256,
                &verified.catalogue_sha256,
            ))
            .expect("validated local backup evidence serializes"),
        );
        Ok(BackupResult {
            backup_id: entry.backup_id.clone(),
            archive_sha256: entry.archive.archive_sha256.clone(),
            catalogue_sha256: verified.catalogue_sha256,
            evidence_sha256,
        })
    }
}

impl EstateBackupDriver for LocalEstateBackupDriver {
    fn create(
        &mut self,
        request: &BackupDriverRequest,
    ) -> std::result::Result<BackupResult, DriverError> {
        self.create_inner(request)
    }
}

fn ensure_direct_directory(root: &Path, child: &str) -> std::result::Result<(), String> {
    let path = root.join(child);
    create_direct_directory(&path)
        .map_err(|error| format!("cannot create local {child} root: {error}"))?;
    let canonical = fs::canonicalize(&path)
        .map_err(|error| format!("cannot resolve local {child} root: {error}"))?;
    if canonical != path || !canonical.is_dir() {
        return Err(format!("local {child} root must be a direct directory"));
    }
    Ok(())
}

fn create_direct_directory(path: &Path) -> std::io::Result<()> {
    match fs::create_dir(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let metadata = fs::symlink_metadata(path)?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(std::io::Error::other("path is not a direct directory"));
            }
            Ok(())
        }
        Err(error) => Err(error),
    }
}

fn permanent(request: &BackupDriverRequest, message: impl Into<String>) -> DriverError {
    let message = message.into();
    DriverError::permanent(
        message.clone(),
        digest::sha256_hex(format!("backup-permanent:{}:{message}", request.job_id).as_bytes()),
    )
}

fn retryable(request: &BackupDriverRequest, message: impl Into<String>) -> DriverError {
    let message = message.into();
    DriverError::retryable(
        message.clone(),
        digest::sha256_hex(format!("backup-retryable:{}:{message}", request.job_id).as_bytes()),
    )
}

fn permanent_store(
    request: &BackupDriverRequest,
    boundary: &str,
    error: StoreError,
) -> DriverError {
    permanent(request, format!("{boundary} failed: {error}"))
}

fn retryable_store(
    request: &BackupDriverRequest,
    boundary: &str,
    error: StoreError,
) -> DriverError {
    retryable(request, format!("{boundary} failed: {error}"))
}
