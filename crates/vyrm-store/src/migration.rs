//! Explicit, resumable Fjall -> vyrmKV storage migration.
//!
//! The live database path is never populated incrementally. Fjall is exported
//! from one cross-keyspace snapshot, native state is built and verified in an
//! absent sibling, and two directory renames perform cutover. The Fjall source
//! and archive remain available for rollback and diagnosis.

use crate::{keyspaces, Engine, Error, NativeEngine, Result, Store};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use vyrm_core::digest::Sha256;
use vyrm_kv::{Database, Durability as KvDurability, Mutation, WriteBatch};

const ARCHIVE_MAGIC: &[u8; 8] = b"VYRMIG01";
const ARCHIVE_VERSION: u16 = 1;
const RECORD_TAG: u8 = 1;
const FOOTER_TAG: u8 = 0xff;
const MAX_KEY_BYTES: usize = 1024 * 1024;
const MAX_VALUE_BYTES: usize = 8 * 1024 * 1024;
const IMPORT_BATCH_BYTES: usize = 12 * 1024 * 1024;
const IMPORT_BATCH_OPERATIONS: usize = 512;
static TEMP_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationInventory {
    pub format_version: u16,
    pub archive_sha256: String,
    pub entries: u64,
    pub payload_bytes: u64,
    pub keyspace_counts: Vec<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum MigrationPhase {
    Exported,
    Imported,
    Verified,
    SourceMoved,
    Cutover,
    Complete,
    RollbackNativeMoved,
    RolledBack,
}

/// Deterministic crash boundary used by the migration recovery matrix.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationFault {
    AfterExport,
    AfterImport,
    AfterVerify,
    AfterSourceRename,
    AfterSourceMove,
    AfterCutoverRename,
    AfterCutover,
}

impl MigrationFault {
    const fn label(self) -> &'static str {
        match self {
            Self::AfterExport => "migration.after_export",
            Self::AfterImport => "migration.after_import",
            Self::AfterVerify => "migration.after_verify",
            Self::AfterSourceRename => "migration.after_source_rename",
            Self::AfterSourceMove => "migration.after_source_move",
            Self::AfterCutoverRename => "migration.after_cutover_rename",
            Self::AfterCutover => "migration.after_cutover",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeStateToken {
    pub manifest: String,
    pub sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationReport {
    pub contract_version: u16,
    pub phase: MigrationPhase,
    pub source: PathBuf,
    pub archive: PathBuf,
    pub staging: PathBuf,
    pub fjall_backup: PathBuf,
    pub retired_native: PathBuf,
    pub inventory: MigrationInventory,
    pub native_state: Option<NativeStateToken>,
}

#[derive(Debug, Clone)]
struct Artifacts {
    marker: PathBuf,
    archive: PathBuf,
    staging: PathBuf,
    backup: PathBuf,
    retired: PathBuf,
}

impl Artifacts {
    fn for_source(source: &Path) -> Result<Self> {
        let source = normalized_source(source)?;
        let parent = source.parent().unwrap_or_else(|| Path::new("."));
        let name = source
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| {
                Error::Migration("database path must have a UTF-8 final component".into())
            })?;
        Ok(Self {
            marker: parent.join(format!(".{name}.vyrm-migration.json")),
            archive: parent.join(format!(".{name}.vyrm-migration.bin")),
            staging: parent.join(format!(".{name}.vyrm-native-stage")),
            backup: parent.join(format!(".{name}.fjall-backup")),
            retired: parent.join(format!(".{name}.native-retired")),
        })
    }
}

/// Starts or resumes an offline migration. Repeated calls are idempotent once
/// complete and advance any durable intermediate phase after interruption.
pub fn migrate_fjall_to_native(source: &Path, at: u64) -> Result<MigrationReport> {
    migrate_inner(source, at, None)
}

#[doc(hidden)]
pub fn migrate_fjall_to_native_with_fault(
    source: &Path,
    at: u64,
    fault: MigrationFault,
) -> Result<MigrationReport> {
    migrate_inner(source, at, Some(fault))
}

fn migrate_inner(source: &Path, at: u64, fault: Option<MigrationFault>) -> Result<MigrationReport> {
    let artifacts = Artifacts::for_source(source)?;
    let mut report = match read_report(&artifacts.marker)? {
        Some(mut report) => {
            rebase_report(source, &artifacts, &mut report)?;
            report
        }
        None => {
            let report = begin(source, &artifacts)?;
            inject(fault, MigrationFault::AfterExport)?;
            report
        }
    };

    if report.phase == MigrationPhase::RolledBack {
        return Err(Error::Migration(
            "migration was rolled back; retain the evidence and start a new migration explicitly"
                .into(),
        ));
    }
    reconcile_filesystem(&artifacts, &mut report)?;

    loop {
        match report.phase {
            MigrationPhase::Exported => {
                rebuild_staging(&artifacts, &report.inventory, at)?;
                report.phase = MigrationPhase::Imported;
                write_report(&artifacts.marker, &report)?;
                inject(fault, MigrationFault::AfterImport)?;
            }
            MigrationPhase::Imported => {
                verify_staging(&artifacts, &report.inventory)?;
                report.phase = MigrationPhase::Verified;
                write_report(&artifacts.marker, &report)?;
                inject(fault, MigrationFault::AfterVerify)?;
            }
            MigrationPhase::Verified => {
                if !source.exists() {
                    return Err(Error::Migration(
                        "source disappeared before its retained backup was established".into(),
                    ));
                }
                if artifacts.backup.exists() {
                    return Err(Error::Migration(
                        "Fjall backup target already exists before source move".into(),
                    ));
                }
                fs::rename(source, &artifacts.backup).map_err(migration_io)?;
                sync_parent(source)?;
                inject(fault, MigrationFault::AfterSourceRename)?;
                report.phase = MigrationPhase::SourceMoved;
                write_report(&artifacts.marker, &report)?;
                inject(fault, MigrationFault::AfterSourceMove)?;
            }
            MigrationPhase::SourceMoved => {
                if source.exists() {
                    return Err(Error::Migration(
                        "database path unexpectedly exists during staged cutover".into(),
                    ));
                }
                if !artifacts.staging.is_dir() || !artifacts.backup.is_dir() {
                    return Err(Error::Migration(
                        "cutover requires both native staging and retained Fjall backup".into(),
                    ));
                }
                fs::rename(&artifacts.staging, source).map_err(migration_io)?;
                sync_parent(source)?;
                inject(fault, MigrationFault::AfterCutoverRename)?;
                report.native_state = Some(native_state(source)?);
                report.phase = MigrationPhase::Cutover;
                write_report(&artifacts.marker, &report)?;
                inject(fault, MigrationFault::AfterCutover)?;
            }
            MigrationPhase::Cutover => {
                verify_visible_native(source, report.native_state.as_ref())?;
                let engine = NativeEngine::open(source)?;
                let _ = Engine::sequence(&engine)?;
                drop(engine);
                report.native_state = Some(native_state(source)?);
                report.phase = MigrationPhase::Complete;
                write_report(&artifacts.marker, &report)?;
            }
            MigrationPhase::Complete => {
                if !source.join("CURRENT").is_file()
                    || !artifacts.backup.is_dir()
                    || !artifacts.archive.is_file()
                {
                    return Err(Error::Migration(
                        "completed migration is missing its native store or retained evidence"
                            .into(),
                    ));
                }
                return Ok(report);
            }
            MigrationPhase::RollbackNativeMoved | MigrationPhase::RolledBack => {
                return Err(Error::Migration(
                    "migration is in rollback state; run rollback again to finish recovery".into(),
                ));
            }
        }
    }
}

fn inject(selected: Option<MigrationFault>, boundary: MigrationFault) -> Result<()> {
    if selected == Some(boundary) {
        return Err(Error::FaultInjected(boundary.label()));
    }
    Ok(())
}

pub fn migration_status(source: &Path) -> Result<Option<MigrationReport>> {
    let artifacts = Artifacts::for_source(source)?;
    let mut report = read_report(&artifacts.marker)?;
    if let Some(report) = &mut report {
        rebase_report(source, &artifacts, report)?;
    }
    Ok(report)
}

/// Restores the retained Fjall directory only when native has not changed
/// since cutover. The native directory is retained rather than deleted.
pub fn rollback_fjall_migration(source: &Path) -> Result<MigrationReport> {
    let artifacts = Artifacts::for_source(source)?;
    let mut report = read_report(&artifacts.marker)?
        .ok_or_else(|| Error::Migration("no migration marker exists".into()))?;
    rebase_report(source, &artifacts, &mut report)?;

    if report.phase == MigrationPhase::RolledBack {
        return Ok(report);
    }
    if matches!(
        report.phase,
        MigrationPhase::Cutover | MigrationPhase::Complete
    ) && !source.exists()
        && artifacts.retired.is_dir()
        && artifacts.backup.is_dir()
    {
        report.phase = MigrationPhase::RollbackNativeMoved;
        write_report(&artifacts.marker, &report)?;
    }
    if matches!(
        report.phase,
        MigrationPhase::Cutover | MigrationPhase::Complete
    ) && source.is_dir()
        && !source.join("CURRENT").is_file()
        && artifacts.retired.is_dir()
        && !artifacts.backup.exists()
    {
        verify_fjall_copy(source, &report.inventory)?;
        report.phase = MigrationPhase::RolledBack;
        write_report(&artifacts.marker, &report)?;
        return Ok(report);
    }
    if report.phase == MigrationPhase::RollbackNativeMoved {
        if source.is_dir()
            && !source.join("CURRENT").is_file()
            && artifacts.retired.is_dir()
            && !artifacts.backup.exists()
        {
            verify_fjall_copy(source, &report.inventory)?;
            report.phase = MigrationPhase::RolledBack;
            write_report(&artifacts.marker, &report)?;
            return Ok(report);
        }
        if source.exists() || !artifacts.retired.is_dir() || !artifacts.backup.is_dir() {
            return Err(Error::Migration(
                "ambiguous interrupted rollback filesystem state".into(),
            ));
        }
    } else {
        if report.phase < MigrationPhase::Cutover || report.phase > MigrationPhase::Complete {
            return Err(Error::Migration(
                "rollback is available only after cutover".into(),
            ));
        }
        verify_visible_native(source, report.native_state.as_ref())?;
        verify_fjall_copy(&artifacts.backup, &report.inventory)?;
        if artifacts.retired.exists() {
            return Err(Error::Migration(
                "retired-native target already exists; refusing to overwrite evidence".into(),
            ));
        }
        fs::rename(source, &artifacts.retired).map_err(migration_io)?;
        sync_parent(source)?;
        report.phase = MigrationPhase::RollbackNativeMoved;
        write_report(&artifacts.marker, &report)?;
    }

    verify_fjall_copy(&artifacts.backup, &report.inventory)?;
    fs::rename(&artifacts.backup, source).map_err(migration_io)?;
    sync_parent(source)?;
    let fjall = Store::open(source)?;
    let _ = Engine::sequence(&fjall)?;
    drop(fjall);
    report.phase = MigrationPhase::RolledBack;
    write_report(&artifacts.marker, &report)?;
    Ok(report)
}

fn verify_fjall_copy(path: &Path, expected: &MigrationInventory) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
    let verification = parent.join(format!(
        ".vyrm-rollback-verification-{}-{id}.bin",
        std::process::id()
    ));
    let store = Store::open(path)?;
    let actual = store.export_migration_archive(&verification)?;
    drop(store);
    fs::remove_file(&verification).map_err(migration_io)?;
    sync_directory(parent)?;
    if &actual != expected {
        return Err(Error::Migration(
            "retained Fjall copy differs from the authenticated migration archive".into(),
        ));
    }
    Ok(())
}

fn begin(source: &Path, artifacts: &Artifacts) -> Result<MigrationReport> {
    if !source.is_dir() {
        return Err(Error::Migration(
            "Fjall source directory does not exist".into(),
        ));
    }
    if source.join("CURRENT").is_file() || source.join("MANIFEST.LOCK").is_file() {
        return Err(Error::Migration(
            "source is already a native vyrmKV store".into(),
        ));
    }
    for path in [
        &artifacts.archive,
        &artifacts.staging,
        &artifacts.backup,
        &artifacts.retired,
    ] {
        if path.exists() {
            return Err(Error::Migration(format!(
                "migration artifact already exists without a marker: {}",
                path.display()
            )));
        }
    }

    let store = Store::open(source)?;
    let inventory = store.export_migration_archive(&artifacts.archive)?;
    let source = store.path().to_owned();
    drop(store);
    let report = MigrationReport {
        contract_version: ARCHIVE_VERSION,
        phase: MigrationPhase::Exported,
        source,
        archive: artifacts.archive.clone(),
        staging: artifacts.staging.clone(),
        fjall_backup: artifacts.backup.clone(),
        retired_native: artifacts.retired.clone(),
        inventory,
        native_state: None,
    };
    write_report(&artifacts.marker, &report)?;
    Ok(report)
}

fn reconcile_filesystem(artifacts: &Artifacts, report: &mut MigrationReport) -> Result<()> {
    let source = &report.source;
    let source_native = source.join("CURRENT").is_file();
    if report.phase <= MigrationPhase::Verified && source_native {
        if !artifacts.backup.is_dir() || artifacts.staging.exists() {
            return Err(Error::Migration(
                "native source appeared without the complete cutover artifact set".into(),
            ));
        }
        report.native_state = Some(native_state(source)?);
        report.phase = MigrationPhase::Cutover;
        write_report(&artifacts.marker, report)?;
    } else if report.phase <= MigrationPhase::Verified
        && !source.exists()
        && artifacts.backup.is_dir()
        && artifacts.staging.is_dir()
    {
        report.phase = MigrationPhase::SourceMoved;
        write_report(&artifacts.marker, report)?;
    } else if report.phase == MigrationPhase::SourceMoved && source_native {
        if !artifacts.backup.is_dir() || artifacts.staging.exists() {
            return Err(Error::Migration(
                "ambiguous post-cutover filesystem state".into(),
            ));
        }
        report.native_state = Some(native_state(source)?);
        report.phase = MigrationPhase::Cutover;
        write_report(&artifacts.marker, report)?;
    }
    Ok(())
}

fn rebuild_staging(artifacts: &Artifacts, expected: &MigrationInventory, at: u64) -> Result<()> {
    if artifacts.staging.exists() {
        fs::remove_dir_all(&artifacts.staging).map_err(migration_io)?;
        sync_parent(&artifacts.staging)?;
    }
    let mut database = Database::create(&artifacts.staging)?;
    let mut operations = Vec::new();
    let mut estimated = 0usize;
    let actual = read_archive(&artifacts.archive, |space, key, value| {
        let mut physical = Vec::with_capacity(keyspaces::ALL[space].len() + 1 + key.len());
        physical.extend_from_slice(keyspaces::ALL[space].as_bytes());
        physical.push(0);
        physical.extend_from_slice(key);
        let cost = physical
            .len()
            .saturating_add(value.len())
            .saturating_add(16);
        if !operations.is_empty()
            && (operations.len() >= IMPORT_BATCH_OPERATIONS
                || estimated.saturating_add(cost) > IMPORT_BATCH_BYTES)
        {
            let batch = WriteBatch::new(std::mem::take(&mut operations))?;
            database.write_owned(batch, KvDurability::Authoritative)?;
            estimated = 0;
        }
        operations.push(Mutation::Put {
            key: physical,
            value: value.to_vec(),
        });
        estimated = estimated.saturating_add(cost);
        Ok(())
    })?;
    if actual != *expected {
        return Err(Error::Migration(
            "archive inventory changed after export".into(),
        ));
    }
    if !operations.is_empty() {
        database.write_owned(WriteBatch::new(operations)?, KvDurability::Authoritative)?;
    }
    database.sync()?;
    database.flush_memtable(at)?;
    drop(database);
    sync_parent(&artifacts.staging)?;
    Ok(())
}

fn verify_staging(artifacts: &Artifacts, expected: &MigrationInventory) -> Result<()> {
    if !artifacts.staging.is_dir() {
        return Err(Error::Migration(
            "native staging directory is missing".into(),
        ));
    }
    let database = Database::open(&artifacts.staging)?;
    let snapshot = database.snapshot();
    let visible = database.scan(&[], None, snapshot)?;
    if visible.len() as u64 != expected.entries {
        return Err(Error::Migration(format!(
            "staging entry count differs: expected {}, found {}",
            expected.entries,
            visible.len()
        )));
    }
    let actual = read_archive(&artifacts.archive, |space, key, value| {
        let mut physical = Vec::new();
        physical.extend_from_slice(keyspaces::ALL[space].as_bytes());
        physical.push(0);
        physical.extend_from_slice(key);
        if database.get(&physical, snapshot)?.as_deref() != Some(value) {
            return Err(Error::Migration(format!(
                "staging bytes diverge for keyspace {}",
                keyspaces::ALL[space]
            )));
        }
        Ok(())
    })?;
    if actual != *expected {
        return Err(Error::Migration(
            "staging inventory differs from archive".into(),
        ));
    }
    drop(database);
    let engine = NativeEngine::open(&artifacts.staging)?;
    let _ = Engine::sequence(&engine)?;
    Ok(())
}

fn native_state(path: &Path) -> Result<NativeStateToken> {
    let database = Database::open(path)?;
    Ok(NativeStateToken {
        manifest: database.manifest().digest.clone(),
        sequence: database.snapshot().sequence,
    })
}

fn verify_visible_native(path: &Path, expected: Option<&NativeStateToken>) -> Result<()> {
    let expected =
        expected.ok_or_else(|| Error::Migration("native state token is absent".into()))?;
    if !path.join("CURRENT").is_file() {
        return Err(Error::Migration(
            "visible database is not native vyrmKV".into(),
        ));
    }
    let actual = native_state(path)?;
    if &actual != expected {
        return Err(Error::Migration(format!(
            "native state diverged after cutover: expected {expected:?}, found {actual:?}"
        )));
    }
    Ok(())
}

fn rebase_report(source: &Path, artifacts: &Artifacts, report: &mut MigrationReport) -> Result<()> {
    if report.contract_version != ARCHIVE_VERSION
        || report.archive.file_name() != artifacts.archive.file_name()
        || report.staging.file_name() != artifacts.staging.file_name()
        || report.fjall_backup.file_name() != artifacts.backup.file_name()
        || report.retired_native.file_name() != artifacts.retired.file_name()
    {
        return Err(Error::Migration(
            "migration marker does not match this path or contract version".into(),
        ));
    }
    let supplied = normalized_source(source)?;
    if supplied.file_name() != report.source.file_name() {
        return Err(Error::Migration(format!(
            "migration marker belongs to {}, not {}",
            report.source.display(),
            supplied.display()
        )));
    }
    report.source = supplied;
    report.archive = artifacts.archive.clone();
    report.staging = artifacts.staging.clone();
    report.fjall_backup = artifacts.backup.clone();
    report.retired_native = artifacts.retired.clone();
    Ok(())
}

fn normalized_source(source: &Path) -> Result<PathBuf> {
    if source.exists() {
        return fs::canonicalize(source).map_err(migration_io);
    }
    let absolute = if source.is_absolute() {
        source.to_owned()
    } else {
        std::env::current_dir().map_err(migration_io)?.join(source)
    };
    let mut ancestor = absolute.as_path();
    let mut missing = Vec::new();
    while !ancestor.exists() {
        let name = ancestor
            .file_name()
            .ok_or_else(|| Error::Migration("database path has no existing ancestor".into()))?;
        missing.push(name.to_owned());
        ancestor = ancestor
            .parent()
            .ok_or_else(|| Error::Migration("database path has no existing ancestor".into()))?;
    }
    let mut normalized = fs::canonicalize(ancestor).map_err(migration_io)?;
    for component in missing.into_iter().rev() {
        normalized.push(component);
    }
    Ok(normalized)
}

fn read_report(path: &Path) -> Result<Option<MigrationReport>> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(migration_io(error)),
    };
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|error| Error::Migration(format!("invalid migration marker: {error}")))
}

fn write_report(path: &Path, report: &MigrationReport) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
    let temp = parent.join(format!(
        ".vyrm-migration-marker-{}-{id}.tmp",
        std::process::id()
    ));
    let bytes = serde_json::to_vec_pretty(report)
        .map_err(|error| Error::Migration(format!("cannot encode migration marker: {error}")))?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(migration_io)?;
    file.write_all(&bytes).map_err(migration_io)?;
    file.sync_all().map_err(migration_io)?;
    drop(file);
    fs::rename(&temp, path).map_err(migration_io)?;
    sync_directory(parent)
}

fn sync_parent(path: &Path) -> Result<()> {
    sync_directory(path.parent().unwrap_or_else(|| Path::new(".")))
}

fn sync_directory(path: &Path) -> Result<()> {
    File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(migration_io)
}

fn migration_io(error: std::io::Error) -> Error {
    Error::Migration(error.to_string())
}

pub(crate) struct ArchiveWriter {
    file: File,
    digest: Sha256,
    entries: u64,
    payload_bytes: u64,
    counts: Vec<u64>,
    final_path: PathBuf,
    temporary_path: PathBuf,
}

impl ArchiveWriter {
    pub(crate) fn create(path: &Path) -> Result<Self> {
        if path.exists() {
            return Err(Error::Migration(format!(
                "archive target already exists: {}",
                path.display()
            )));
        }
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let temporary_path = parent.join(format!(
            ".vyrm-migration-export-{}-{id}.tmp",
            std::process::id()
        ));
        let mut writer = Self {
            file: OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary_path)
                .map_err(migration_io)?,
            digest: Sha256::new(),
            entries: 0,
            payload_bytes: 0,
            counts: vec![0; keyspaces::ALL.len()],
            final_path: path.to_owned(),
            temporary_path,
        };
        writer.hashed(ARCHIVE_MAGIC)?;
        writer.hashed(&ARCHIVE_VERSION.to_be_bytes())?;
        writer.hashed(&0u16.to_be_bytes())?;
        writer.hashed(&(keyspaces::ALL.len() as u16).to_be_bytes())?;
        for name in keyspaces::ALL {
            writer.hashed(&(name.len() as u16).to_be_bytes())?;
            writer.hashed(name.as_bytes())?;
        }
        Ok(writer)
    }

    pub(crate) fn record(&mut self, space: usize, key: &[u8], value: &[u8]) -> Result<()> {
        if key.is_empty() || key.len() > MAX_KEY_BYTES || value.len() > MAX_VALUE_BYTES {
            return Err(Error::Migration(
                "source key/value exceeds native limits".into(),
            ));
        }
        self.hashed(&[RECORD_TAG])?;
        self.hashed(&(space as u16).to_be_bytes())?;
        self.hashed(&(key.len() as u32).to_be_bytes())?;
        self.hashed(&(value.len() as u64).to_be_bytes())?;
        self.hashed(key)?;
        self.hashed(value)?;
        self.entries = self
            .entries
            .checked_add(1)
            .ok_or_else(|| Error::Migration("archive entry counter overflow".into()))?;
        self.payload_bytes = self
            .payload_bytes
            .checked_add((key.len() + value.len()) as u64)
            .ok_or_else(|| Error::Migration("archive byte counter overflow".into()))?;
        self.counts[space] += 1;
        Ok(())
    }

    pub(crate) fn finish(mut self) -> Result<MigrationInventory> {
        let archive_sha256 = self.digest.clone().finalize_hex();
        self.file.write_all(&[FOOTER_TAG]).map_err(migration_io)?;
        self.file
            .write_all(&self.entries.to_be_bytes())
            .map_err(migration_io)?;
        self.file
            .write_all(&self.payload_bytes.to_be_bytes())
            .map_err(migration_io)?;
        for count in &self.counts {
            self.file
                .write_all(&count.to_be_bytes())
                .map_err(migration_io)?;
        }
        self.file
            .write_all(&self.digest.finalize())
            .map_err(migration_io)?;
        self.file.sync_all().map_err(migration_io)?;
        drop(self.file);
        fs::rename(&self.temporary_path, &self.final_path).map_err(migration_io)?;
        sync_parent(&self.final_path)?;
        Ok(MigrationInventory {
            format_version: ARCHIVE_VERSION,
            archive_sha256,
            entries: self.entries,
            payload_bytes: self.payload_bytes,
            keyspace_counts: self.counts,
        })
    }

    fn hashed(&mut self, bytes: &[u8]) -> Result<()> {
        self.file.write_all(bytes).map_err(migration_io)?;
        self.digest.update(bytes);
        Ok(())
    }
}

fn read_archive(
    path: &Path,
    mut record: impl FnMut(usize, &[u8], &[u8]) -> Result<()>,
) -> Result<MigrationInventory> {
    let mut file = File::open(path).map_err(migration_io)?;
    let mut digest = Sha256::new();
    let mut fixed = [0u8; 8];
    read_hashed(&mut file, &mut digest, &mut fixed)?;
    if &fixed != ARCHIVE_MAGIC {
        return Err(Error::Migration("archive magic does not match".into()));
    }
    let version = read_u16_hashed(&mut file, &mut digest)?;
    if version != ARCHIVE_VERSION {
        return Err(Error::Migration(format!(
            "unsupported archive version {version}"
        )));
    }
    if read_u16_hashed(&mut file, &mut digest)? != 0 {
        return Err(Error::Migration("archive has unknown flags".into()));
    }
    let spaces = read_u16_hashed(&mut file, &mut digest)? as usize;
    if spaces != keyspaces::ALL.len() {
        return Err(Error::Migration(
            "archive keyspace count is not canonical".into(),
        ));
    }
    for expected in keyspaces::ALL {
        let length = read_u16_hashed(&mut file, &mut digest)? as usize;
        let mut name = vec![0; length];
        read_hashed(&mut file, &mut digest, &mut name)?;
        if name != expected.as_bytes() {
            return Err(Error::Migration(
                "archive keyspace order/name differs".into(),
            ));
        }
    }

    let mut entries = 0u64;
    let mut payload_bytes = 0u64;
    let mut counts = vec![0u64; spaces];
    let mut last_space = 0usize;
    let mut last_keys: Vec<Option<Vec<u8>>> = vec![None; spaces];
    loop {
        let mut tag = [0u8; 1];
        file.read_exact(&mut tag).map_err(archive_read_error)?;
        if tag[0] == FOOTER_TAG {
            break;
        }
        digest.update(&tag);
        if tag[0] != RECORD_TAG {
            return Err(Error::Migration(format!(
                "unknown archive record tag {}",
                tag[0]
            )));
        }
        let space = read_u16_hashed(&mut file, &mut digest)? as usize;
        let key_len = read_u32_hashed(&mut file, &mut digest)? as usize;
        let value_len = read_u64_hashed(&mut file, &mut digest)? as usize;
        if space >= spaces || (entries != 0 && space < last_space) {
            return Err(Error::Migration(
                "archive keyspace ordinal is invalid".into(),
            ));
        }
        if key_len == 0 || key_len > MAX_KEY_BYTES || value_len > MAX_VALUE_BYTES {
            return Err(Error::Migration(
                "archive key/value length exceeds limits".into(),
            ));
        }
        let mut key = vec![0; key_len];
        let mut value = vec![0; value_len];
        read_hashed(&mut file, &mut digest, &mut key)?;
        read_hashed(&mut file, &mut digest, &mut value)?;
        if last_keys[space].as_ref().is_some_and(|last| last >= &key) {
            return Err(Error::Migration(
                "archive keys are not strictly ordered".into(),
            ));
        }
        record(space, &key, &value)?;
        last_keys[space] = Some(key);
        last_space = space;
        entries = entries
            .checked_add(1)
            .ok_or_else(|| Error::Migration("entry overflow".into()))?;
        payload_bytes = payload_bytes
            .checked_add((key_len + value_len) as u64)
            .ok_or_else(|| Error::Migration("payload overflow".into()))?;
        counts[space] += 1;
    }

    let declared_entries = read_u64_raw(&mut file)?;
    let declared_bytes = read_u64_raw(&mut file)?;
    let mut declared_counts = Vec::with_capacity(spaces);
    for _ in 0..spaces {
        declared_counts.push(read_u64_raw(&mut file)?);
    }
    let mut declared_digest = [0u8; 32];
    file.read_exact(&mut declared_digest)
        .map_err(archive_read_error)?;
    let mut trailing = [0u8; 1];
    if file.read(&mut trailing).map_err(migration_io)? != 0 {
        return Err(Error::Migration("archive carries trailing bytes".into()));
    }
    let actual_digest = digest.finalize();
    if declared_entries != entries
        || declared_bytes != payload_bytes
        || declared_counts != counts
        || declared_digest != actual_digest
    {
        return Err(Error::Migration(
            "archive footer or SHA-256 does not match".into(),
        ));
    }
    Ok(MigrationInventory {
        format_version: version,
        archive_sha256: hex(actual_digest),
        entries,
        payload_bytes,
        keyspace_counts: counts,
    })
}

fn read_hashed(file: &mut File, digest: &mut Sha256, bytes: &mut [u8]) -> Result<()> {
    file.read_exact(bytes).map_err(archive_read_error)?;
    digest.update(bytes);
    Ok(())
}

fn read_u16_hashed(file: &mut File, digest: &mut Sha256) -> Result<u16> {
    let mut bytes = [0; 2];
    read_hashed(file, digest, &mut bytes)?;
    Ok(u16::from_be_bytes(bytes))
}

fn read_u32_hashed(file: &mut File, digest: &mut Sha256) -> Result<u32> {
    let mut bytes = [0; 4];
    read_hashed(file, digest, &mut bytes)?;
    Ok(u32::from_be_bytes(bytes))
}

fn read_u64_hashed(file: &mut File, digest: &mut Sha256) -> Result<u64> {
    let mut bytes = [0; 8];
    read_hashed(file, digest, &mut bytes)?;
    Ok(u64::from_be_bytes(bytes))
}

fn read_u64_raw(file: &mut File) -> Result<u64> {
    let mut bytes = [0; 8];
    file.read_exact(&mut bytes).map_err(archive_read_error)?;
    Ok(u64::from_be_bytes(bytes))
}

fn archive_read_error(error: std::io::Error) -> Error {
    if error.kind() == ErrorKind::UnexpectedEof {
        Error::Migration("archive is truncated".into())
    } else {
        migration_io(error)
    }
}

fn hex(bytes: [u8; 32]) -> String {
    let mut output = String::with_capacity(64);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    output
}
