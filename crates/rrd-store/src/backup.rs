//! Authenticated local backup catalogue over retained logical archives.

use crate::{
    export_logical_archive, inspect_logical_archive, restore_logical_archive_to_new_root,
    restore_logical_archive_to_new_root_with, ControlTransition, Engine, Error,
    ImmutableObjectStore, LocalObjectStore, LogicalArchiveInventory, LogicalRestoreReport,
    NativeEngine, Result,
};
use rrd_core::digest;
use rrd_core::{ObjectReference, RuntimeMutation, ScopeId};
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub const BACKUP_CATALOGUE_VERSION: u16 = 1;
const CATALOGUE_FILE: &str = "catalogue.json";
const ARCHIVE_DIRECTORY: &str = "archives";
const OBJECT_MANIFEST_DIRECTORY: &str = "object-manifests";
const CATALOGUE_MANIFEST_DIRECTORY: &str = "catalogue-manifests";
const OBJECT_PAYLOAD_DIRECTORY: &str = "payloads";
const OBJECT_MANIFEST_VERSION: u16 = 1;
const PAGE_SIZE: usize = 1_024;
const CATALOGUE_MANIFEST_VERSION: u16 = 1;
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_manifest_file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_manifest: Option<ObjectPayloadManifestInventory>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catalogue_manifest_file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catalogue_manifest: Option<CatalogueManifestInventory>,
    pub claims: BackupCoverage,
    pub typed_runtime: BackupCoverage,
    pub catalogues: BackupCoverage,
    pub object_payloads: BackupCoverage,
    pub projections: BackupCoverage,
    pub invocation_telemetry: BackupCoverage,
    pub snapshot_leases: BackupCoverage,
    pub application_complete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectPayloadManifestInventory {
    pub format_version: u16,
    pub manifest_sha256: String,
    pub object_count: u64,
    pub payload_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogueManifestInventory {
    pub format_version: u16,
    pub manifest_sha256: String,
    pub source_control_sequence: u64,
    pub scope_count: u64,
    pub record_count: u64,
    pub payload_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogueScopeSnapshot {
    scope: ScopeId,
    revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogueRecordSnapshot {
    scope: ScopeId,
    key: String,
    value: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogueManifest {
    format_version: u16,
    source_control_sequence: u64,
    scopes: Vec<CatalogueScopeSnapshot>,
    records: Vec<CatalogueRecordSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredCatalogueManifest {
    payload: CatalogueManifest,
    payload_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ObjectPayloadEntry {
    sha256: String,
    length: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ObjectPayloadManifest {
    format_version: u16,
    objects: Vec<ObjectPayloadEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredObjectPayloadManifest {
    payload: ObjectPayloadManifest,
    payload_sha256: String,
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
        rrd_lsm::publish_rename(&archives, &staging, &archive_path).map_err(backup_io)?;
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
        object_manifest_file: None,
        object_manifest: None,
        catalogue_manifest_file: None,
        catalogue_manifest: None,
        claims: BackupCoverage::Included,
        typed_runtime: BackupCoverage::Included,
        catalogues: BackupCoverage::Excluded,
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

/// Creates an application-complete logical backup by retaining the exact
/// immutable-object closure referenced by the exported runtime cut.
///
/// Logical replay and object bytes remain separate physical artifacts, but
/// one authenticated catalogue entry binds their content identities. A
/// concurrent source mutation rejects the backup before it is catalogued.
pub fn create_application_backup<E: Engine, O: ImmutableObjectStore>(
    engine: &E,
    objects: &O,
    catalogue_root: &Path,
    label: &str,
    created_at: u64,
) -> Result<BackupEntry> {
    validate_label(label)?;
    let control_sequence = engine.control_sequence()?;
    let archives = catalogue_root.join(ARCHIVE_DIRECTORY);
    fs::create_dir_all(&archives).map_err(backup_io)?;
    let id = BACKUP_TEMP_ID.fetch_add(1, Ordering::Relaxed);
    let staging = archives.join(format!(".logical-backup-{}-{id}.tmp", std::process::id()));
    let inventory = export_logical_archive(engine, &staging)?;
    let archive_name = format!("{}.rrd-archive", inventory.archive_sha256);
    let archive_path = archives.join(&archive_name);
    retain_content_addressed_file(&staging, &archive_path, &inventory)?;

    let references = object_references_at(engine, inventory.runtime_cursor)?;
    if engine.sequence()? != inventory.claim_sequence
        || engine.runtime_cursor()? != inventory.runtime_cursor
    {
        return Err(Error::Archive(
            "source watermarks changed while retaining object payloads; retry from a stable cut"
                .into(),
        ));
    }
    let (manifest_file, manifest_inventory) =
        retain_object_payloads(objects, catalogue_root, &references)?;
    let (catalogue_manifest_file, catalogue_manifest) = retain_catalogues(engine, catalogue_root)?;
    if engine.sequence()? != inventory.claim_sequence
        || engine.runtime_cursor()? != inventory.runtime_cursor
        || engine.control_sequence()? != control_sequence
        || catalogue_manifest.source_control_sequence != control_sequence
    {
        return Err(Error::Archive(
            "source data or control watermarks changed before application-complete backup publication; retry"
                .into(),
        ));
    }

    let backup_id = digest::sha256_hex(
        format!(
            "rrd-application-backup-v1\0{label}\0{created_at}\0{}\0{}\0{}",
            inventory.archive_sha256,
            manifest_inventory.manifest_sha256,
            catalogue_manifest.manifest_sha256
        )
        .as_bytes(),
    );
    let entry = BackupEntry {
        backup_id,
        label: label.into(),
        created_at,
        archive_file: format!("{ARCHIVE_DIRECTORY}/{archive_name}"),
        archive: inventory,
        object_manifest_file: Some(manifest_file),
        object_manifest: Some(manifest_inventory),
        catalogue_manifest_file: Some(catalogue_manifest_file),
        catalogue_manifest: Some(catalogue_manifest),
        claims: BackupCoverage::Included,
        typed_runtime: BackupCoverage::Included,
        catalogues: BackupCoverage::Included,
        object_payloads: BackupCoverage::Included,
        projections: BackupCoverage::RebuildRequired,
        invocation_telemetry: BackupCoverage::Excluded,
        snapshot_leases: BackupCoverage::Excluded,
        application_complete: true,
    };
    retain_catalogue_entry(catalogue_root, entry)
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
        if entry.application_complete {
            load_catalogue_manifest_for_entry(catalogue_root, entry)?;
            verify_object_payloads(catalogue_root, entry)?;
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
    let archive = resolve_archive(catalogue_root, &entry.archive_file)?;
    if entry.application_complete {
        let object_manifest = load_object_manifest_for_entry(catalogue_root, entry)?;
        let catalogue_manifest = load_catalogue_manifest_for_entry(catalogue_root, entry)?;
        restore_logical_archive_to_new_root_with(&archive, target, at, |staging| {
            restore_catalogues(&catalogue_manifest, staging)?;
            restore_object_payloads(catalogue_root, &object_manifest, staging)
        })
    } else {
        restore_logical_archive_to_new_root(&archive, target, at)
    }
}

/// Verifies that an already published restore still contains the complete,
/// authenticated immutable payload closure for a catalogued backup.
pub fn verify_restored_backup_objects(
    catalogue_root: &Path,
    backup_id: &str,
    restored_root: &Path,
) -> Result<()> {
    let catalogue = verify_backup_catalogue(catalogue_root)?;
    let entry = catalogue
        .backups
        .iter()
        .find(|entry| entry.backup_id == backup_id)
        .ok_or_else(|| Error::Archive(format!("backup is not catalogued: {backup_id}")))?;
    if !entry.application_complete {
        return Err(Error::Archive(
            "logical-only backup cannot verify application-complete restore payloads".into(),
        ));
    }
    let manifest = load_object_manifest_for_entry(catalogue_root, entry)?;
    let restored = LocalObjectStore::open(restored_root.join("immutable"))?;
    for object in &manifest.objects {
        let verified = restored.verify(&object.sha256)?;
        if verified.length != object.length {
            return Err(Error::ObjectLengthMismatch {
                expected: object.length,
                actual: verified.length,
            });
        }
    }
    let catalogues = load_catalogue_manifest_for_entry(catalogue_root, entry)?;
    verify_restored_catalogues(&catalogues, restored_root)?;
    Ok(())
}

fn retain_content_addressed_file(
    staging: &Path,
    retained: &Path,
    inventory: &LogicalArchiveInventory,
) -> Result<()> {
    let parent = retained
        .parent()
        .ok_or_else(|| Error::Archive("backup archive has no parent directory".into()))?;
    if retained.exists() {
        let actual = inspect_logical_archive(retained)?;
        if &actual != inventory {
            let _ = fs::remove_file(staging);
            return Err(Error::Archive(
                "content-addressed backup archive conflicts with retained inventory".into(),
            ));
        }
        fs::remove_file(staging).map_err(backup_io)
    } else {
        rrd_lsm::publish_rename(parent, staging, retained).map_err(backup_io)
    }
}

fn retain_catalogue_entry(catalogue_root: &Path, entry: BackupEntry) -> Result<BackupEntry> {
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

fn object_references_at(engine: &impl Engine, head: u64) -> Result<Vec<ObjectReference>> {
    let mut after = 0u64;
    let mut objects = std::collections::BTreeMap::<String, ObjectReference>::new();
    while after < head {
        let page = engine.runtime_changes_since(after, PAGE_SIZE, None)?;
        if page.head_cursor != head || page.requested_after != after {
            return Err(Error::Archive(
                "runtime watermark changed while collecting backup object references".into(),
            ));
        }
        if page.through_cursor <= after || page.through_cursor > head {
            return Err(Error::Archive(
                "runtime object-reference scan did not advance canonically".into(),
            ));
        }
        for change in page.changes {
            let RuntimeMutation::Object { object } = change.mutation else {
                continue;
            };
            object.validate()?;
            if let Some(existing) = objects.get(&object.sha256) {
                if existing.length != object.length {
                    return Err(Error::Archive(format!(
                        "object digest {} has conflicting lengths in the backup cut",
                        object.sha256
                    )));
                }
            } else {
                objects.insert(object.sha256.clone(), object);
            }
        }
        after = page.through_cursor;
    }
    Ok(objects.into_values().collect())
}

fn retain_catalogues(
    engine: &impl Engine,
    catalogue_root: &Path,
) -> Result<(String, CatalogueManifestInventory)> {
    let source_control_sequence = engine.control_sequence()?;
    let mut after = 0u64;
    let mut records = std::collections::BTreeMap::<String, CatalogueRecordSnapshot>::new();
    let mut scopes = std::collections::BTreeSet::<ScopeId>::new();
    while after < source_control_sequence {
        let page = engine.control_journal_since(after, PAGE_SIZE)?;
        if page.is_empty() {
            return Err(Error::Archive(
                "control journal ended before the captured backup watermark".into(),
            ));
        }
        for entry in page {
            if entry.sequence != after + 1 || entry.sequence > source_control_sequence {
                return Err(Error::Archive(
                    "control journal changed or became discontinuous during catalogue capture"
                        .into(),
                ));
            }
            after = entry.sequence;
            let Some(scope) = catalogue_scope(&entry.key)? else {
                continue;
            };
            scopes.insert(scope.clone());
            match entry.replacement {
                Some(value) => {
                    records.insert(
                        entry.key.clone(),
                        CatalogueRecordSnapshot {
                            scope,
                            key: entry.key,
                            value,
                        },
                    );
                }
                None => {
                    records.remove(&entry.key);
                }
            }
        }
    }
    if engine.control_sequence()? != source_control_sequence {
        return Err(Error::Archive(
            "control journal changed while capturing catalogue state".into(),
        ));
    }
    let scopes = scopes
        .into_iter()
        .map(|scope| {
            Ok(CatalogueScopeSnapshot {
                revision: engine.runtime_read_stamp(&scope)?.catalog_revision,
                scope,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let payload = CatalogueManifest {
        format_version: CATALOGUE_MANIFEST_VERSION,
        source_control_sequence,
        scopes,
        records: records.into_values().collect(),
    };
    validate_catalogue_manifest(&payload)?;
    let inventory = catalogue_manifest_inventory(&payload)?;
    let stored = StoredCatalogueManifest {
        payload,
        payload_sha256: inventory.manifest_sha256.clone(),
    };
    let manifests = catalogue_root.join(CATALOGUE_MANIFEST_DIRECTORY);
    fs::create_dir_all(&manifests).map_err(backup_io)?;
    let file_name = format!("{}.rrd-catalogues.json", inventory.manifest_sha256);
    let retained_path = manifests.join(&file_name);
    if retained_path.exists() {
        let actual = load_catalogue_manifest(&retained_path)?;
        if actual != stored.payload {
            return Err(Error::Archive(
                "content-addressed catalogue manifest conflicts with retained payload".into(),
            ));
        }
    } else {
        let id = BACKUP_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let staging = manifests.join(format!(
            ".catalogue-manifest-{}-{id}.tmp",
            std::process::id()
        ));
        let bytes = serde_json::to_vec_pretty(&stored)?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staging)
            .map_err(backup_io)?;
        file.write_all(&bytes).map_err(backup_io)?;
        file.sync_all().map_err(backup_io)?;
        drop(file);
        rrd_lsm::publish_rename(&manifests, &staging, &retained_path).map_err(backup_io)?;
    }
    Ok((
        format!("{CATALOGUE_MANIFEST_DIRECTORY}/{file_name}"),
        inventory,
    ))
}

fn catalogue_scope(key: &str) -> Result<Option<ScopeId>> {
    for prefix in [
        "server/state/vector-collection-catalogue/",
        "server/state/index-catalogue/",
    ] {
        if let Some(scope) = key.strip_prefix(prefix) {
            return ScopeId::new(scope).map(Some).map_err(Error::from);
        }
    }
    Ok(None)
}

fn load_catalogue_manifest_for_entry(
    catalogue_root: &Path,
    entry: &BackupEntry,
) -> Result<CatalogueManifest> {
    let file = entry.catalogue_manifest_file.as_deref().ok_or_else(|| {
        Error::Archive("application-complete backup has no catalogue manifest path".into())
    })?;
    let expected = entry.catalogue_manifest.as_ref().ok_or_else(|| {
        Error::Archive("application-complete backup has no catalogue manifest inventory".into())
    })?;
    let manifest = load_catalogue_manifest(&resolve_catalogue_manifest(catalogue_root, file)?)?;
    let actual = catalogue_manifest_inventory(&manifest)?;
    if &actual != expected {
        return Err(Error::Archive(format!(
            "backup {} catalogue manifest inventory does not match catalogue",
            entry.backup_id
        )));
    }
    Ok(manifest)
}

fn load_catalogue_manifest(path: &Path) -> Result<CatalogueManifest> {
    let bytes = fs::read(path).map_err(backup_io)?;
    let stored: StoredCatalogueManifest = serde_json::from_slice(&bytes)?;
    validate_catalogue_manifest(&stored.payload)?;
    let actual = digest::sha256_hex(&serde_json::to_vec(&stored.payload)?);
    if actual != stored.payload_sha256 {
        return Err(Error::Archive(
            "catalogue manifest payload digest does not match".into(),
        ));
    }
    Ok(stored.payload)
}

fn catalogue_manifest_inventory(
    manifest: &CatalogueManifest,
) -> Result<CatalogueManifestInventory> {
    validate_catalogue_manifest(manifest)?;
    let payload_bytes = manifest.records.iter().try_fold(0u64, |total, record| {
        total
            .checked_add(record.value.len() as u64)
            .ok_or_else(|| Error::Archive("catalogue manifest bytes overflowed u64".into()))
    })?;
    Ok(CatalogueManifestInventory {
        format_version: manifest.format_version,
        manifest_sha256: digest::sha256_hex(&serde_json::to_vec(manifest)?),
        source_control_sequence: manifest.source_control_sequence,
        scope_count: manifest.scopes.len() as u64,
        record_count: manifest.records.len() as u64,
        payload_bytes,
    })
}

fn validate_catalogue_manifest(manifest: &CatalogueManifest) -> Result<()> {
    if manifest.format_version != CATALOGUE_MANIFEST_VERSION {
        return Err(Error::Archive(format!(
            "unsupported catalogue manifest version {}",
            manifest.format_version
        )));
    }
    if manifest
        .scopes
        .windows(2)
        .any(|pair| pair[0].scope >= pair[1].scope)
        || manifest
            .records
            .windows(2)
            .any(|pair| pair[0].key >= pair[1].key)
    {
        return Err(Error::Archive(
            "catalogue manifest entries are not uniquely ordered".into(),
        ));
    }
    let scope_revisions = manifest
        .scopes
        .iter()
        .map(|scope| (&scope.scope, scope.revision))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut record_counts = std::collections::BTreeMap::<&ScopeId, u64>::new();
    for record in &manifest.records {
        if record.value.is_empty() || record.value.len() > 1024 * 1024 {
            return Err(Error::Archive(
                "catalogue manifest record violates control-state bounds".into(),
            ));
        }
        let parsed = catalogue_scope(&record.key)?.ok_or_else(|| {
            Error::Archive("catalogue manifest contains a non-catalogue key".into())
        })?;
        if parsed != record.scope || !scope_revisions.contains_key(&record.scope) {
            return Err(Error::Archive(
                "catalogue manifest key, scope, or inventory differs".into(),
            ));
        }
        *record_counts.entry(&record.scope).or_default() += 1;
    }
    for scope in &manifest.scopes {
        let records = record_counts.get(&scope.scope).copied().unwrap_or_default();
        if scope.revision < records || (scope.revision > 0 && records == 0) {
            return Err(Error::Archive(
                "catalogue scope revision cannot be reconstructed from its records".into(),
            ));
        }
    }
    Ok(())
}

fn restore_catalogues(manifest: &CatalogueManifest, staging_root: &Path) -> Result<()> {
    validate_catalogue_manifest(manifest)?;
    let engine = NativeEngine::open(staging_root)?;
    let mut applied = std::collections::BTreeMap::<ScopeId, u64>::new();
    for record in &manifest.records {
        engine.commit_catalog_transition(
            &record.scope,
            &ControlTransition {
                key: record.key.clone(),
                expected: None,
                replacement: Some(record.value.clone()),
                at: 1,
                actor: "rrd-restore".into(),
                action: "catalogue.restored".into(),
                request_id: "rrd-restore".into(),
                operation_id: "rrd-restore".into(),
            },
        )?;
        *applied.entry(record.scope.clone()).or_default() += 1;
    }
    for scope in &manifest.scopes {
        let record = manifest
            .records
            .iter()
            .find(|record| record.scope == scope.scope)
            .ok_or_else(|| {
                Error::Archive("catalogue revision has no materialized restore record".into())
            })?;
        let count = applied.entry(scope.scope.clone()).or_default();
        while *count < scope.revision {
            engine.commit_catalog_transition(
                &scope.scope,
                &ControlTransition {
                    key: record.key.clone(),
                    expected: Some(record.value.clone()),
                    replacement: Some(record.value.clone()),
                    at: 1,
                    actor: "rrd-restore".into(),
                    action: "catalogue.revision_restored".into(),
                    request_id: "rrd-restore".into(),
                    operation_id: "rrd-restore".into(),
                },
            )?;
            *count += 1;
        }
    }
    drop(engine);
    verify_restored_catalogues(manifest, staging_root)
}

fn verify_restored_catalogues(manifest: &CatalogueManifest, restored_root: &Path) -> Result<()> {
    let engine = NativeEngine::open(restored_root)?;
    for record in &manifest.records {
        if engine.control_record(&record.key)?.as_deref() != Some(record.value.as_slice()) {
            return Err(Error::Archive(format!(
                "restored catalogue record {} differs from its backup",
                record.key
            )));
        }
    }
    for scope in &manifest.scopes {
        let actual = engine.runtime_read_stamp(&scope.scope)?.catalog_revision;
        if actual != scope.revision {
            return Err(Error::Archive(format!(
                "restored catalogue revision {actual} differs from {} for {}",
                scope.revision, scope.scope
            )));
        }
    }
    Ok(())
}

fn retain_object_payloads(
    source: &impl ImmutableObjectStore,
    catalogue_root: &Path,
    references: &[ObjectReference],
) -> Result<(String, ObjectPayloadManifestInventory)> {
    let retained = LocalObjectStore::open(catalogue_root.join(OBJECT_PAYLOAD_DIRECTORY))?;
    let mut entries = Vec::with_capacity(references.len());
    let mut payload_bytes = 0u64;
    for reference in references {
        let mut reader = source.open_verified(reference)?;
        let verified =
            retained.put_verified_stream(&reference.sha256, reference.length, reader.as_mut())?;
        if verified.length != reference.length {
            return Err(Error::ObjectLengthMismatch {
                expected: reference.length,
                actual: verified.length,
            });
        }
        payload_bytes = payload_bytes
            .checked_add(reference.length)
            .ok_or_else(|| Error::Archive("backup object payload bytes overflowed u64".into()))?;
        entries.push(ObjectPayloadEntry {
            sha256: reference.sha256.clone(),
            length: reference.length,
        });
    }
    entries.sort_by(|left, right| left.sha256.cmp(&right.sha256));
    let payload = ObjectPayloadManifest {
        format_version: OBJECT_MANIFEST_VERSION,
        objects: entries,
    };
    let payload_sha256 = digest::sha256_hex(&serde_json::to_vec(&payload)?);
    let stored = StoredObjectPayloadManifest {
        payload,
        payload_sha256: payload_sha256.clone(),
    };
    let manifests = catalogue_root.join(OBJECT_MANIFEST_DIRECTORY);
    fs::create_dir_all(&manifests).map_err(backup_io)?;
    let file_name = format!("{payload_sha256}.rrd-objects.json");
    let retained_path = manifests.join(&file_name);
    if retained_path.exists() {
        let actual = load_object_manifest(&retained_path)?;
        if actual != stored.payload {
            return Err(Error::Archive(
                "content-addressed object manifest conflicts with retained payload".into(),
            ));
        }
    } else {
        let id = BACKUP_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let staging = manifests.join(format!(".object-manifest-{}-{id}.tmp", std::process::id()));
        let bytes = serde_json::to_vec_pretty(&stored)?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staging)
            .map_err(backup_io)?;
        file.write_all(&bytes).map_err(backup_io)?;
        file.sync_all().map_err(backup_io)?;
        drop(file);
        rrd_lsm::publish_rename(&manifests, &staging, &retained_path).map_err(backup_io)?;
    }
    Ok((
        format!("{OBJECT_MANIFEST_DIRECTORY}/{file_name}"),
        ObjectPayloadManifestInventory {
            format_version: OBJECT_MANIFEST_VERSION,
            manifest_sha256: payload_sha256,
            object_count: references.len() as u64,
            payload_bytes,
        },
    ))
}

fn verify_object_payloads(catalogue_root: &Path, entry: &BackupEntry) -> Result<()> {
    let manifest = load_object_manifest_for_entry(catalogue_root, entry)?;
    let payloads = LocalObjectStore::open(catalogue_root.join(OBJECT_PAYLOAD_DIRECTORY))?;
    for object in manifest.objects {
        let verified = payloads.verify(&object.sha256)?;
        if verified.length != object.length {
            return Err(Error::ObjectLengthMismatch {
                expected: object.length,
                actual: verified.length,
            });
        }
    }
    Ok(())
}

fn restore_object_payloads(
    catalogue_root: &Path,
    manifest: &ObjectPayloadManifest,
    staging_root: &Path,
) -> Result<()> {
    let source = LocalObjectStore::open(catalogue_root.join(OBJECT_PAYLOAD_DIRECTORY))?;
    let target = LocalObjectStore::open(staging_root.join("immutable"))?;
    for object in &manifest.objects {
        let verified = source.verify(&object.sha256)?;
        if verified.length != object.length {
            return Err(Error::ObjectLengthMismatch {
                expected: object.length,
                actual: verified.length,
            });
        }
        let reference = ObjectReference::for_verified(
            format!("backup-{}", object.sha256),
            None,
            "application/octet-stream",
            object.sha256.clone(),
            object.length,
            verified.receipt,
        )?;
        let mut reader = source.open_verified(&reference)?;
        target.put_verified_stream(&object.sha256, object.length, reader.as_mut())?;
    }
    Ok(())
}

fn load_object_manifest_for_entry(
    catalogue_root: &Path,
    entry: &BackupEntry,
) -> Result<ObjectPayloadManifest> {
    let file = entry.object_manifest_file.as_deref().ok_or_else(|| {
        Error::Archive("application-complete backup has no object manifest path".into())
    })?;
    let expected = entry.object_manifest.as_ref().ok_or_else(|| {
        Error::Archive("application-complete backup has no object manifest inventory".into())
    })?;
    let manifest = load_object_manifest(&resolve_object_manifest(catalogue_root, file)?)?;
    let actual = object_manifest_inventory(&manifest)?;
    if &actual != expected {
        return Err(Error::Archive(format!(
            "backup {} object manifest inventory does not match catalogue",
            entry.backup_id
        )));
    }
    Ok(manifest)
}

fn load_object_manifest(path: &Path) -> Result<ObjectPayloadManifest> {
    let bytes = fs::read(path).map_err(backup_io)?;
    let stored: StoredObjectPayloadManifest = serde_json::from_slice(&bytes)?;
    validate_object_manifest(&stored.payload)?;
    let actual = digest::sha256_hex(&serde_json::to_vec(&stored.payload)?);
    if actual != stored.payload_sha256 {
        return Err(Error::Archive(
            "object manifest payload digest does not match".into(),
        ));
    }
    Ok(stored.payload)
}

fn object_manifest_inventory(
    manifest: &ObjectPayloadManifest,
) -> Result<ObjectPayloadManifestInventory> {
    validate_object_manifest(manifest)?;
    let payload_bytes = manifest.objects.iter().try_fold(0u64, |total, object| {
        total
            .checked_add(object.length)
            .ok_or_else(|| Error::Archive("object manifest payload bytes overflowed u64".into()))
    })?;
    Ok(ObjectPayloadManifestInventory {
        format_version: manifest.format_version,
        manifest_sha256: digest::sha256_hex(&serde_json::to_vec(manifest)?),
        object_count: manifest.objects.len() as u64,
        payload_bytes,
    })
}

fn validate_object_manifest(manifest: &ObjectPayloadManifest) -> Result<()> {
    if manifest.format_version != OBJECT_MANIFEST_VERSION {
        return Err(Error::Archive(format!(
            "unsupported object manifest version {}",
            manifest.format_version
        )));
    }
    let mut previous: Option<&str> = None;
    for object in &manifest.objects {
        ObjectReference::canonical_key(&object.sha256)?;
        if previous.is_some_and(|prior| prior >= object.sha256.as_str()) {
            return Err(Error::Archive(
                "object manifest entries are not uniquely ordered".into(),
            ));
        }
        previous = Some(&object.sha256);
    }
    Ok(())
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
        format_version: BACKUP_CATALOGUE_VERSION,
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
        drop(file);
        rrd_lsm::publish_rename(root, &temporary, &path).map_err(backup_io)
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
        let common_coverage = entry.claims == BackupCoverage::Included
            && entry.typed_runtime == BackupCoverage::Included
            && entry.projections == BackupCoverage::RebuildRequired
            && entry.invocation_telemetry == BackupCoverage::Excluded
            && entry.snapshot_leases == BackupCoverage::Excluded;
        let logical_only = entry.object_payloads == BackupCoverage::ReferencedOnly
            && entry.catalogues == BackupCoverage::Excluded
            && !entry.application_complete
            && entry.object_manifest_file.is_none()
            && entry.object_manifest.is_none()
            && entry.catalogue_manifest_file.is_none()
            && entry.catalogue_manifest.is_none();
        let application_complete = entry.object_payloads == BackupCoverage::Included
            && entry.catalogues == BackupCoverage::Included
            && entry.application_complete
            && entry.object_manifest_file.is_some()
            && entry.object_manifest.is_some()
            && entry.catalogue_manifest_file.is_some()
            && entry.catalogue_manifest.is_some();
        if !common_coverage || (!logical_only && !application_complete) {
            return Err(Error::Archive(
                "backup v2 coverage declaration is not canonical".into(),
            ));
        }
        if let (Some(file), Some(inventory)) = (&entry.object_manifest_file, &entry.object_manifest)
        {
            validate_relative_object_manifest(file)?;
            if inventory.format_version != OBJECT_MANIFEST_VERSION
                || inventory.manifest_sha256.len() != 64
                || !inventory
                    .manifest_sha256
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            {
                return Err(Error::Archive(
                    "backup object manifest inventory is invalid".into(),
                ));
            }
            let expected = format!(
                "{OBJECT_MANIFEST_DIRECTORY}/{}.rrd-objects.json",
                inventory.manifest_sha256
            );
            if file != &expected {
                return Err(Error::Archive(
                    "backup object manifest path does not match its content identity".into(),
                ));
            }
        } else if entry.object_manifest_file.is_some() || entry.object_manifest.is_some() {
            return Err(Error::Archive(
                "backup object manifest path and inventory must be present together".into(),
            ));
        }
        if let (Some(file), Some(inventory)) =
            (&entry.catalogue_manifest_file, &entry.catalogue_manifest)
        {
            validate_relative_catalogue_manifest(file)?;
            if inventory.format_version != CATALOGUE_MANIFEST_VERSION
                || inventory.manifest_sha256.len() != 64
                || !inventory
                    .manifest_sha256
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            {
                return Err(Error::Archive(
                    "backup catalogue manifest inventory is invalid".into(),
                ));
            }
            let expected = format!(
                "{CATALOGUE_MANIFEST_DIRECTORY}/{}.rrd-catalogues.json",
                inventory.manifest_sha256
            );
            if file != &expected {
                return Err(Error::Archive(
                    "backup catalogue manifest path does not match its content identity".into(),
                ));
            }
        } else if entry.catalogue_manifest_file.is_some() || entry.catalogue_manifest.is_some() {
            return Err(Error::Archive(
                "backup catalogue manifest path and inventory must be present together".into(),
            ));
        }
        if entry.claims != BackupCoverage::Included
            || entry.typed_runtime != BackupCoverage::Included
            || entry.projections != BackupCoverage::RebuildRequired
            || entry.invocation_telemetry != BackupCoverage::Excluded
            || entry.snapshot_leases != BackupCoverage::Excluded
        {
            return Err(Error::Archive(
                "backup coverage declaration is not canonical".into(),
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

fn validate_relative_object_manifest(value: &str) -> Result<()> {
    let path = Path::new(value);
    let components: Vec<_> = path.components().collect();
    if components.len() != 2
        || components[0] != Component::Normal(OBJECT_MANIFEST_DIRECTORY.as_ref())
        || !matches!(components[1], Component::Normal(_))
        || !value.ends_with(".rrd-objects.json")
    {
        return Err(Error::Archive(
            "backup object manifest path is not canonical".into(),
        ));
    }
    Ok(())
}

fn validate_relative_catalogue_manifest(value: &str) -> Result<()> {
    let path = Path::new(value);
    let components: Vec<_> = path.components().collect();
    if components.len() != 2
        || components[0] != Component::Normal(CATALOGUE_MANIFEST_DIRECTORY.as_ref())
        || !matches!(components[1], Component::Normal(_))
        || !value.ends_with(".rrd-catalogues.json")
    {
        return Err(Error::Archive(
            "backup catalogue manifest path is not canonical".into(),
        ));
    }
    Ok(())
}

fn resolve_archive(root: &Path, value: &str) -> Result<PathBuf> {
    validate_relative_archive(value)?;
    Ok(root.join(value))
}

fn resolve_object_manifest(root: &Path, value: &str) -> Result<PathBuf> {
    validate_relative_object_manifest(value)?;
    Ok(root.join(value))
}

fn resolve_catalogue_manifest(root: &Path, value: &str) -> Result<PathBuf> {
    validate_relative_catalogue_manifest(value)?;
    Ok(root.join(value))
}

fn backup_io(error: std::io::Error) -> Error {
    Error::Archive(error.to_string())
}
