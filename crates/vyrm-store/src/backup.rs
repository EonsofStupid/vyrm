//! Authenticated local backup catalogue over retained logical archives.

use crate::{
    Engine, Error, LogicalArchiveInventory, LogicalRestoreReport, Result, export_logical_archive,
    inspect_logical_archive, restore_logical_archive_to_new_root,
};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use vyrm_core::digest;

pub const BACKUP_CATALOGUE_VERSION: u16 = 1;
const CATALOGUE_FILE: &str = "catalogue.json";
const ARCHIVE_DIRECTORY: &str = "archives";
static BACKUP_TEMP_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackupCoverage {
    Included,
    ReferencedOnly,
    RebuildRequired,
    Excluded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackupEntry {
    pub backup_id: String,
    pub label: String,
    pub created_at: u64,
    pub archive_file: String,
    pub archive: LogicalArchiveInventory,
    pub claims: BackupCoverage,
    pub typed_runtime: BackupCoverage,
    pub object_payloads: BackupCoverage,
    pub projections: BackupCoverage,
    pub invocation_telemetry: BackupCoverage,
    pub snapshot_leases: BackupCoverage,
    pub application_complete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackupCatalogue {
    pub format_version: u16,
    pub revision: u64,
    pub catalogue_sha256: String,
    pub backups: Vec<BackupEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CataloguePayload {
    format_version: u16,
    revision: u64,
    backups: Vec<BackupEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredCatalogue {
    payload: CataloguePayload,
    payload_sha256: String,
}

/// Creates or reuses a content-addressed archive and atomically adds its
/// coverage record to the catalogue.
pub fn create_logical_backup<E: Engine>(
    engine: &E,
    catalogue_root: &Path,
    label: &str,
    created_at: u64,
) -> Result<BackupEntry> {
    validate_label(label)?;
    let archives = catalogue_root.join(ARCHIVE_DIRECTORY);
    fs::create_dir_all(&archives).map_err(backup_io)?;
    let id = BACKUP_TEMP_ID.fetch_add(1, Ordering::Relaxed);
    let staging = archives.join(format!(".logical-backup-{}-{id}.tmp", std::process::id()));
    let inventory = export_logical_archive(engine, &staging)?;
    let archive_name = format!("{}.rrd-archive", inventory.archive_sha256);
    let archive_path = archives.join(&archive_name);
    if archive_path.exists() {
        let retained = inspect_logical_archive(&archive_path)?;
        if retained != inventory {
            let _ = fs::remove_file(&staging);
            return Err(Error::Archive(
                "content-addressed backup archive conflicts with retained inventory".into(),
            ));
        }
        fs::remove_file(&staging).map_err(backup_io)?;
    } else {
        fs::rename(&staging, &archive_path).map_err(backup_io)?;
        sync_parent(&archive_path)?;
    }

    let backup_id = digest::sha256_hex(
        format!(
            "rrd-logical-backup-v1\0{label}\0{created_at}\0{}",
            inventory.archive_sha256
        )
        .as_bytes(),
    );
    let entry = BackupEntry {
        backup_id,
        label: label.into(),
        created_at,
        archive_file: format!("{ARCHIVE_DIRECTORY}/{archive_name}"),
        archive: inventory,
        claims: BackupCoverage::Included,
        typed_runtime: BackupCoverage::Included,
        object_payloads: BackupCoverage::ReferencedOnly,
        projections: BackupCoverage::RebuildRequired,
        invocation_telemetry: BackupCoverage::Excluded,
        snapshot_leases: BackupCoverage::Excluded,
        application_complete: false,
    };

    let mut catalogue = load_backup_catalogue(catalogue_root)?;
    if let Some(existing) = catalogue
        .backups
        .iter()
        .find(|existing| existing.backup_id == entry.backup_id)
    {
        if existing == &entry {
            return Ok(existing.clone());
        }
        return Err(Error::Archive("backup identity collision".into()));
    }
    catalogue.revision = catalogue
        .revision
        .checked_add(1)
        .ok_or_else(|| Error::Archive("backup catalogue revision overflow".into()))?;
    catalogue.backups.push(entry.clone());
    catalogue.backups.sort_by(|left, right| {
        (left.created_at, &left.backup_id).cmp(&(right.created_at, &right.backup_id))
    });
    write_catalogue(catalogue_root, catalogue)?;
    Ok(entry)
}

/// Loads and authenticates the catalogue payload. Archive bytes are verified
/// separately by `verify_backup_catalogue` so listing remains bounded.
pub fn load_backup_catalogue(catalogue_root: &Path) -> Result<BackupCatalogue> {
    let path = catalogue_root.join(CATALOGUE_FILE);
    if !path.exists() {
        return catalogue_from_payload(CataloguePayload {
            format_version: BACKUP_CATALOGUE_VERSION,
            revision: 0,
            backups: Vec::new(),
        });
    }
    let bytes = fs::read(&path).map_err(backup_io)?;
    let stored: StoredCatalogue = serde_json::from_slice(&bytes)?;
    if stored.payload.format_version != BACKUP_CATALOGUE_VERSION {
        return Err(Error::Archive(format!(
            "unsupported backup catalogue version {}",
            stored.payload.format_version
        )));
    }
    let actual = payload_digest(&stored.payload)?;
    if actual != stored.payload_sha256 {
        return Err(Error::Archive(
            "backup catalogue payload digest does not match".into(),
        ));
    }
    validate_entries(&stored.payload.backups)?;
    Ok(BackupCatalogue {
        format_version: stored.payload.format_version,
        revision: stored.payload.revision,
        catalogue_sha256: actual,
        backups: stored.payload.backups,
    })
}

/// Authenticates the catalogue and every retained archive it references.
pub fn verify_backup_catalogue(catalogue_root: &Path) -> Result<BackupCatalogue> {
    let catalogue = load_backup_catalogue(catalogue_root)?;
    for entry in &catalogue.backups {
        let path = resolve_archive(catalogue_root, &entry.archive_file)?;
        let actual = inspect_logical_archive(&path)?;
        if actual != entry.archive {
            return Err(Error::Archive(format!(
                "backup {} archive inventory does not match catalogue",
                entry.backup_id
            )));
        }
    }
    Ok(catalogue)
}

pub fn restore_catalogued_backup(
    catalogue_root: &Path,
    backup_id: &str,
    target: &Path,
    at: u64,
) -> Result<LogicalRestoreReport> {
    let catalogue = verify_backup_catalogue(catalogue_root)?;
    let entry = catalogue
        .backups
        .iter()
        .find(|entry| entry.backup_id == backup_id)
        .ok_or_else(|| Error::Archive(format!("backup is not catalogued: {backup_id}")))?;
    restore_logical_archive_to_new_root(
        &resolve_archive(catalogue_root, &entry.archive_file)?,
        target,
        at,
    )
}

fn catalogue_from_payload(payload: CataloguePayload) -> Result<BackupCatalogue> {
    Ok(BackupCatalogue {
        format_version: payload.format_version,
        revision: payload.revision,
        catalogue_sha256: payload_digest(&payload)?,
        backups: payload.backups,
    })
}

fn write_catalogue(root: &Path, catalogue: BackupCatalogue) -> Result<()> {
    let payload = CataloguePayload {
        format_version: catalogue.format_version,
        revision: catalogue.revision,
        backups: catalogue.backups,
    };
    validate_entries(&payload.backups)?;
    let stored = StoredCatalogue {
        payload_sha256: payload_digest(&payload)?,
        payload,
    };
    let bytes = serde_json::to_vec_pretty(&stored)?;
    let id = BACKUP_TEMP_ID.fetch_add(1, Ordering::Relaxed);
    let temporary = root.join(format!(".catalogue-{}-{id}.tmp", std::process::id()));
    let path = root.join(CATALOGUE_FILE);
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(backup_io)?;
        file.write_all(&bytes).map_err(backup_io)?;
        file.sync_all().map_err(backup_io)?;
        fs::rename(&temporary, &path).map_err(backup_io)?;
        sync_parent(&path)
    })();
    if result.is_err() && temporary.exists() {
        fs::remove_file(&temporary).map_err(backup_io)?;
    }
    result
}

fn payload_digest(payload: &CataloguePayload) -> Result<String> {
    Ok(digest::sha256_hex(&serde_json::to_vec(payload)?))
}

fn validate_entries(entries: &[BackupEntry]) -> Result<()> {
    let mut previous: Option<(u64, &str)> = None;
    let mut identities = std::collections::BTreeSet::new();
    for entry in entries {
        validate_label(&entry.label)?;
        if entry.backup_id.len() != 64
            || !entry
                .backup_id
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            || !identities.insert(&entry.backup_id)
        {
            return Err(Error::Archive(
                "backup catalogue has an invalid or duplicate identity".into(),
            ));
        }
        let order = (entry.created_at, entry.backup_id.as_str());
        if previous.is_some_and(|prior| prior >= order) {
            return Err(Error::Archive(
                "backup catalogue entries are not canonically ordered".into(),
            ));
        }
        previous = Some(order);
        validate_relative_archive(&entry.archive_file)?;
        if entry.claims != BackupCoverage::Included
            || entry.typed_runtime != BackupCoverage::Included
            || entry.object_payloads != BackupCoverage::ReferencedOnly
            || entry.projections != BackupCoverage::RebuildRequired
            || entry.invocation_telemetry != BackupCoverage::Excluded
            || entry.snapshot_leases != BackupCoverage::Excluded
            || entry.application_complete
        {
            return Err(Error::Archive(
                "backup v1 coverage declaration is not canonical".into(),
            ));
        }
    }
    Ok(())
}

fn validate_label(label: &str) -> Result<()> {
    if label.is_empty()
        || label.len() > 128
        || label.trim() != label
        || !label
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(Error::Archive(
            "backup label must be 1-128 ASCII alphanumeric, '.', '-', or '_' characters".into(),
        ));
    }
    Ok(())
}

fn validate_relative_archive(value: &str) -> Result<()> {
    let path = Path::new(value);
    let components: Vec<_> = path.components().collect();
    if components.len() != 2
        || components[0] != Component::Normal(ARCHIVE_DIRECTORY.as_ref())
        || !matches!(components[1], Component::Normal(_))
        || !value.ends_with(".rrd-archive")
    {
        return Err(Error::Archive(
            "backup archive path is not canonical".into(),
        ));
    }
    Ok(())
}

fn resolve_archive(root: &Path, value: &str) -> Result<PathBuf> {
    validate_relative_archive(value)?;
    Ok(root.join(value))
}

fn backup_io(error: std::io::Error) -> Error {
    Error::Archive(error.to_string())
}

fn sync_parent(path: &Path) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    File::open(parent)
        .map_err(backup_io)?
        .sync_all()
        .map_err(backup_io)
}
