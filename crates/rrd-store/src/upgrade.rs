//! Resumable exact-successor migration of native application-key formats.

use crate::migration::{read_archive, ArchiveWriter, MigrationInventory};
use crate::{keyspaces, Engine, Error, NativeEngine, Result};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use vyrm_core::digest;
use vyrm_kv::{Database, DatabaseOptions, Durability, Mutation, WriteBatch};

const LEDGER_VERSION: u16 = 1;
const IMPORT_BATCH_OPERATIONS: usize = 4_096;
const IMPORT_BATCH_BYTES: usize = 8 * 1024 * 1024;
static UPGRADE_TEMP_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FormatMigrationPhase {
    Exported,
    Imported,
    Verified,
    SourceMoved,
    Cutover,
    Complete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatMigrationFault {
    AfterExport,
    AfterImport,
    AfterVerify,
    AfterSourceRename,
    AfterSourceMove,
    AfterCutoverRename,
    AfterCutover,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormatMigrationLedger {
    pub ledger_version: u16,
    pub phase: FormatMigrationPhase,
    pub source_application_format: Option<u64>,
    pub target_application_format: u64,
    pub inventory: MigrationInventory,
    pub source_manifest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_manifest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredLedger {
    ledger: FormatMigrationLedger,
    ledger_sha256: String,
}

struct Artifacts {
    marker: PathBuf,
    archive: PathBuf,
    staging: PathBuf,
    backup: PathBuf,
}

impl Artifacts {
    fn for_source(source: &Path) -> Result<Self> {
        let parent = source.parent().unwrap_or_else(|| Path::new("."));
        let name = source
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| Error::Migration("native source must have a UTF-8 file name".into()))?;
        Ok(Self {
            marker: parent.join(format!(".{name}.native-format-ledger.json")),
            archive: parent.join(format!(".{name}.native-format-archive.bin")),
            staging: parent.join(format!(".{name}.native-format-staging")),
            backup: parent.join(format!(".{name}.native-format-v1-backup")),
        })
    }
}

pub fn migrate_native_format(source: &Path, at: u64) -> Result<FormatMigrationLedger> {
    migrate_inner(source, at, None)
}

#[doc(hidden)]
pub fn migrate_native_format_with_fault(
    source: &Path,
    at: u64,
    fault: FormatMigrationFault,
) -> Result<FormatMigrationLedger> {
    migrate_inner(source, at, Some(fault))
}

pub fn native_format_migration_status(source: &Path) -> Result<Option<FormatMigrationLedger>> {
    read_ledger(&Artifacts::for_source(source)?.marker)
}

fn migrate_inner(
    source: &Path,
    at: u64,
    fault: Option<FormatMigrationFault>,
) -> Result<FormatMigrationLedger> {
    let artifacts = Artifacts::for_source(source)?;
    let mut ledger = match read_ledger(&artifacts.marker)? {
        Some(ledger) => ledger,
        None => begin(source, &artifacts)?,
    };
    inject(fault, FormatMigrationFault::AfterExport)?;
    if ledger.ledger_version != LEDGER_VERSION
        || ledger.source_application_format.is_some()
        || ledger.target_application_format != keyspaces::NATIVE_KEYSPACE_TAG_FORMAT_V2
    {
        return Err(Error::Migration(
            "native format ledger does not describe the supported exact successor TextV1 -> TagV2"
                .into(),
        ));
    }
    reconcile(source, &artifacts, &mut ledger)?;
    loop {
        match ledger.phase {
            FormatMigrationPhase::Exported => {
                import_archive(
                    &artifacts.archive,
                    &artifacts.staging,
                    &ledger.inventory,
                    at,
                )?;
                ledger.phase = FormatMigrationPhase::Imported;
                write_ledger(&artifacts.marker, &ledger)?;
                inject(fault, FormatMigrationFault::AfterImport)?;
            }
            FormatMigrationPhase::Imported => {
                verify_target(&artifacts.staging, &artifacts.archive, &ledger.inventory)?;
                ledger.phase = FormatMigrationPhase::Verified;
                write_ledger(&artifacts.marker, &ledger)?;
                inject(fault, FormatMigrationFault::AfterVerify)?;
            }
            FormatMigrationPhase::Verified => {
                if !source.is_dir() || artifacts.backup.exists() {
                    return Err(Error::Migration(
                        "format cutover requires one source and an absent retained-backup path"
                            .into(),
                    ));
                }
                verify_legacy_source(source, &ledger.inventory)?;
                fs::rename(source, &artifacts.backup).map_err(migration_io)?;
                sync_parent(source)?;
                inject(fault, FormatMigrationFault::AfterSourceRename)?;
                ledger.phase = FormatMigrationPhase::SourceMoved;
                write_ledger(&artifacts.marker, &ledger)?;
                inject(fault, FormatMigrationFault::AfterSourceMove)?;
            }
            FormatMigrationPhase::SourceMoved => {
                if source.exists() || !artifacts.backup.is_dir() || !artifacts.staging.is_dir() {
                    return Err(Error::Migration(
                        "format cutover filesystem state is incomplete or ambiguous".into(),
                    ));
                }
                fs::rename(&artifacts.staging, source).map_err(migration_io)?;
                sync_parent(source)?;
                inject(fault, FormatMigrationFault::AfterCutoverRename)?;
                ledger.target_manifest = Some(target_manifest(source)?);
                ledger.phase = FormatMigrationPhase::Cutover;
                write_ledger(&artifacts.marker, &ledger)?;
                inject(fault, FormatMigrationFault::AfterCutover)?;
            }
            FormatMigrationPhase::Cutover => {
                verify_visible(source, &artifacts, &ledger)?;
                ledger.target_manifest = Some(target_manifest(source)?);
                ledger.phase = FormatMigrationPhase::Complete;
                write_ledger(&artifacts.marker, &ledger)?;
            }
            FormatMigrationPhase::Complete => {
                verify_visible(source, &artifacts, &ledger)?;
                return Ok(ledger);
            }
        }
    }
}

fn begin(source: &Path, artifacts: &Artifacts) -> Result<FormatMigrationLedger> {
    for artifact in [&artifacts.archive, &artifacts.staging, &artifacts.backup] {
        if artifact.exists() {
            return Err(Error::Migration(format!(
                "native format artifact exists without an authenticated ledger: {}",
                artifact.display()
            )));
        }
    }
    let database = Database::open(source)?;
    let source_application_format = database.manifest().application_format;
    if source_application_format.is_some() {
        return Err(Error::Migration(
            "native application format is not the supported TextV1 predecessor".into(),
        ));
    }
    let source_manifest = database.manifest().digest.clone();
    let inventory = export_native_archive(&database, &artifacts.archive)?;
    drop(database);
    let ledger = FormatMigrationLedger {
        ledger_version: LEDGER_VERSION,
        phase: FormatMigrationPhase::Exported,
        source_application_format,
        target_application_format: keyspaces::NATIVE_KEYSPACE_TAG_FORMAT_V2,
        inventory,
        source_manifest,
        target_manifest: None,
    };
    write_ledger(&artifacts.marker, &ledger)?;
    Ok(ledger)
}

fn export_native_archive(database: &Database, path: &Path) -> Result<MigrationInventory> {
    let codec =
        keyspaces::NativeKeyCodec::from_application_format(database.manifest().application_format)
            .ok_or_else(|| Error::Migration("source native key codec is unsupported".into()))?;
    let snapshot = database.snapshot();
    let total = database.scan(&[], None, snapshot)?.len();
    let mut exported = 0usize;
    let mut writer = ArchiveWriter::create(path)?;
    for (space, name) in keyspaces::ALL.iter().enumerate() {
        let prefix = codec
            .encode(name, &[])
            .ok_or_else(|| Error::Migration("source keyspace is not canonical".into()))?;
        let end = prefix_end(&prefix)
            .ok_or_else(|| Error::Migration("source keyspace has no upper bound".into()))?;
        for (stored, value) in database.scan(&prefix, Some(&end), snapshot)? {
            let key = codec
                .strip(name, &stored)
                .ok_or_else(|| Error::Migration("source key does not match its keyspace".into()))?;
            writer.record(space, key, &value)?;
            exported += 1;
        }
    }
    if exported != total {
        return Err(Error::Migration(
            "native source contains keys outside the frozen logical keyspace catalogue".into(),
        ));
    }
    writer.finish()
}

fn import_archive(
    archive: &Path,
    staging: &Path,
    expected: &MigrationInventory,
    at: u64,
) -> Result<()> {
    if staging.exists() {
        fs::remove_dir_all(staging).map_err(migration_io)?;
        sync_parent(staging)?;
    }
    let mut database = Database::create_with_application_format(
        staging,
        DatabaseOptions::default(),
        keyspaces::NATIVE_KEYSPACE_TAG_FORMAT_V2,
    )?;
    let mut operations = Vec::new();
    let mut estimated = 0usize;
    let actual = read_archive(archive, |space, key, value| {
        let physical = keyspaces::NativeKeyCodec::TagV2
            .encode(keyspaces::ALL[space], key)
            .expect("archive keyspace is canonical");
        let cost = physical
            .len()
            .saturating_add(value.len())
            .saturating_add(16);
        if !operations.is_empty()
            && (operations.len() >= IMPORT_BATCH_OPERATIONS
                || estimated.saturating_add(cost) > IMPORT_BATCH_BYTES)
        {
            database.write_owned(
                WriteBatch::new(std::mem::take(&mut operations))?,
                Durability::Authoritative,
            )?;
            estimated = 0;
        }
        operations.push(Mutation::Put {
            key: physical,
            value: value.to_vec(),
        });
        estimated = estimated.saturating_add(cost);
        Ok(())
    })?;
    if &actual != expected {
        return Err(Error::Migration(
            "format archive inventory changed during import".into(),
        ));
    }
    if !operations.is_empty() {
        database.write_owned(WriteBatch::new(operations)?, Durability::Authoritative)?;
    }
    database.sync()?;
    database.flush_memtable(at)?;
    drop(database);
    sync_parent(staging)
}

fn verify_target(path: &Path, archive: &Path, expected: &MigrationInventory) -> Result<()> {
    let database = Database::open(path)?;
    if database.manifest().application_format != Some(keyspaces::NATIVE_KEYSPACE_TAG_FORMAT_V2) {
        return Err(Error::Migration(
            "format target does not authenticate TagV2".into(),
        ));
    }
    let snapshot = database.snapshot();
    if database.scan(&[], None, snapshot)?.len() as u64 != expected.entries {
        return Err(Error::Migration("format target entry count differs".into()));
    }
    let actual = read_archive(archive, |space, key, value| {
        let physical = keyspaces::NativeKeyCodec::TagV2
            .encode(keyspaces::ALL[space], key)
            .expect("archive keyspace is canonical");
        if database.get(&physical, snapshot)?.as_deref() != Some(value) {
            return Err(Error::Migration(format!(
                "format target differs in keyspace {}",
                keyspaces::ALL[space]
            )));
        }
        Ok(())
    })?;
    if &actual != expected {
        return Err(Error::Migration("format target inventory differs".into()));
    }
    drop(database);
    let engine = NativeEngine::open(path)?;
    let _ = engine.sequence()?;
    Ok(())
}

fn verify_visible(
    source: &Path,
    artifacts: &Artifacts,
    ledger: &FormatMigrationLedger,
) -> Result<()> {
    if !source.is_dir() || !artifacts.backup.is_dir() || !artifacts.archive.is_file() {
        return Err(Error::Migration(
            "completed format migration is missing retained evidence".into(),
        ));
    }
    verify_target(source, &artifacts.archive, &ledger.inventory)?;
    if ledger.target_manifest.as_deref() != Some(target_manifest(source)?.as_str()) {
        return Err(Error::Migration(
            "visible target manifest diverged after cutover".into(),
        ));
    }
    let backup = Database::open(&artifacts.backup)?;
    if backup.manifest().application_format.is_some()
        || backup.manifest().digest != ledger.source_manifest
    {
        return Err(Error::Migration(
            "retained TextV1 source identity diverged".into(),
        ));
    }
    drop(backup);
    verify_legacy_source(&artifacts.backup, &ledger.inventory)?;
    Ok(())
}

fn verify_legacy_source(path: &Path, expected: &MigrationInventory) -> Result<()> {
    let database = Database::open(path)?;
    if database.manifest().application_format.is_some() {
        return Err(Error::Migration(
            "migration source is no longer the exact TextV1 predecessor".into(),
        ));
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let id = UPGRADE_TEMP_ID.fetch_add(1, Ordering::Relaxed);
    let verification = parent.join(format!(
        ".native-format-verification-{}-{id}.bin",
        std::process::id()
    ));
    let actual = export_native_archive(&database, &verification)?;
    drop(database);
    fs::remove_file(&verification).map_err(migration_io)?;
    sync_parent(&verification)?;
    if &actual != expected {
        return Err(Error::Migration(
            "TextV1 source changed after its authenticated export".into(),
        ));
    }
    Ok(())
}

fn reconcile(
    source: &Path,
    artifacts: &Artifacts,
    ledger: &mut FormatMigrationLedger,
) -> Result<()> {
    let source_native = source.join("CURRENT").is_file();
    if ledger.phase <= FormatMigrationPhase::Verified
        && !source.exists()
        && artifacts.backup.is_dir()
        && artifacts.staging.is_dir()
    {
        ledger.phase = FormatMigrationPhase::SourceMoved;
        write_ledger(&artifacts.marker, ledger)?;
    } else if ledger.phase == FormatMigrationPhase::SourceMoved
        && source_native
        && artifacts.backup.is_dir()
        && !artifacts.staging.exists()
    {
        ledger.target_manifest = Some(target_manifest(source)?);
        ledger.phase = FormatMigrationPhase::Cutover;
        write_ledger(&artifacts.marker, ledger)?;
    }
    Ok(())
}

fn target_manifest(path: &Path) -> Result<String> {
    let database = Database::open(path)?;
    if database.manifest().application_format != Some(keyspaces::NATIVE_KEYSPACE_TAG_FORMAT_V2) {
        return Err(Error::Migration(
            "visible native format is not TagV2".into(),
        ));
    }
    Ok(database.manifest().digest.clone())
}

fn write_ledger(path: &Path, ledger: &FormatMigrationLedger) -> Result<()> {
    let ledger_bytes = serde_json::to_vec(ledger)?;
    let stored = StoredLedger {
        ledger: ledger.clone(),
        ledger_sha256: digest::sha256_hex(&ledger_bytes),
    };
    let bytes = serde_json::to_vec_pretty(&stored)?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let id = UPGRADE_TEMP_ID.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(
        ".native-format-ledger-{}-{id}.tmp",
        std::process::id()
    ));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(migration_io)?;
        file.write_all(&bytes).map_err(migration_io)?;
        file.sync_all().map_err(migration_io)?;
        fs::rename(&temporary, path).map_err(migration_io)?;
        sync_parent(path)
    })();
    if result.is_err() && temporary.exists() {
        fs::remove_file(&temporary).map_err(migration_io)?;
    }
    result
}

fn read_ledger(path: &Path) -> Result<Option<FormatMigrationLedger>> {
    if !path.exists() {
        return Ok(None);
    }
    let stored: StoredLedger = serde_json::from_slice(&fs::read(path).map_err(migration_io)?)?;
    if stored.ledger.ledger_version != LEDGER_VERSION
        || digest::sha256_hex(&serde_json::to_vec(&stored.ledger)?) != stored.ledger_sha256
    {
        return Err(Error::Migration(
            "native format ledger authentication failed".into(),
        ));
    }
    Ok(Some(stored.ledger))
}

fn inject(actual: Option<FormatMigrationFault>, boundary: FormatMigrationFault) -> Result<()> {
    if actual == Some(boundary) {
        return Err(Error::FaultInjected("native format migration boundary"));
    }
    Ok(())
}

fn prefix_end(prefix: &[u8]) -> Option<Vec<u8>> {
    let mut end = prefix.to_vec();
    for index in (0..end.len()).rev() {
        if end[index] != u8::MAX {
            end[index] += 1;
            end.truncate(index + 1);
            return Some(end);
        }
    }
    None
}

fn migration_io(error: std::io::Error) -> Error {
    Error::Migration(error.to_string())
}

fn sync_parent(path: &Path) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    File::open(parent)
        .map_err(migration_io)?
        .sync_all()
        .map_err(migration_io)
}
