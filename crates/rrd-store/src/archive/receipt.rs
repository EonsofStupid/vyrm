use super::format::{ArchiveHeader, PrefixInventory};
use crate::{Error, Result};
use rrd_core::digest;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const RECEIPT_VERSION: u16 = 1;
static RECEIPT_TEMP_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ExportReceipt {
    pub format_version: u16,
    pub destination_name: String,
    pub claim_sequence: u64,
    pub runtime_cursor: u64,
    pub action_count: u64,
    pub expected_runtime_audit_sha256: Option<String>,
    pub completed_actions: u64,
    pub completed_claim_sequence: u64,
    pub completed_runtime_cursor: u64,
    pub completed_runtime_audit_sha256: Option<String>,
    pub standalone_claims: u64,
    pub runtime_commits: u64,
    pub runtime_mutations: u64,
    pub payload_bytes: u64,
    pub partial_bytes: u64,
    pub partial_sha256: String,
}

impl ExportReceipt {
    pub fn from_prefix(
        destination: &Path,
        expected_runtime_audit_sha256: Option<String>,
        prefix: &PrefixInventory,
    ) -> Result<Self> {
        Ok(Self {
            format_version: RECEIPT_VERSION,
            destination_name: file_name(destination)?,
            claim_sequence: prefix.header.claim_sequence,
            runtime_cursor: prefix.header.runtime_cursor,
            action_count: prefix.header.action_count,
            expected_runtime_audit_sha256,
            completed_actions: prefix.completed_actions,
            completed_claim_sequence: prefix.claim_sequence,
            completed_runtime_cursor: prefix.runtime_cursor,
            completed_runtime_audit_sha256: prefix.runtime_audit_sha256.clone(),
            standalone_claims: prefix.standalone_claims,
            runtime_commits: prefix.runtime_commits,
            runtime_mutations: prefix.runtime_mutations,
            payload_bytes: prefix.payload_bytes,
            partial_bytes: prefix.prefix_bytes,
            partial_sha256: prefix.prefix_sha256.clone(),
        })
    }

    pub fn validate_fixed(
        &self,
        destination: &Path,
        header: ArchiveHeader,
        expected_runtime_audit_sha256: Option<&str>,
    ) -> Result<()> {
        if self.format_version != RECEIPT_VERSION
            || self.destination_name != file_name(destination)?
            || self.claim_sequence != header.claim_sequence
            || self.runtime_cursor != header.runtime_cursor
            || self.action_count != header.action_count
            || self.expected_runtime_audit_sha256.as_deref() != expected_runtime_audit_sha256
            || self.completed_actions > self.action_count
        {
            return Err(Error::Archive(
                "logical export receipt differs from the source cut".into(),
            ));
        }
        Ok(())
    }

    pub fn validate_prefix(&self, prefix: &PrefixInventory) -> Result<()> {
        if self.completed_actions > prefix.completed_actions {
            return Err(Error::Archive(
                "logical export receipt is ahead of its durable partial file".into(),
            ));
        }
        if self.completed_actions == prefix.completed_actions
            && (self.completed_claim_sequence != prefix.claim_sequence
                || self.completed_runtime_cursor != prefix.runtime_cursor
                || self.completed_runtime_audit_sha256 != prefix.runtime_audit_sha256
                || self.standalone_claims != prefix.standalone_claims
                || self.runtime_commits != prefix.runtime_commits
                || self.runtime_mutations != prefix.runtime_mutations
                || self.payload_bytes != prefix.payload_bytes
                || self.partial_bytes != prefix.prefix_bytes
                || self.partial_sha256 != prefix.prefix_sha256)
        {
            return Err(Error::Archive(
                "logical export receipt counters differ from its durable partial file".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RestoreReceipt {
    pub format_version: u16,
    pub target_name: String,
    pub archive_sha256: String,
    pub action_count: u64,
    pub completed_actions: u64,
    pub claim_sequence: u64,
    pub runtime_cursor: u64,
    pub runtime_audit_sha256: Option<String>,
}

impl RestoreReceipt {
    pub fn new(
        target: &Path,
        archive_sha256: String,
        action_count: u64,
        prefix: &PrefixInventory,
    ) -> Result<Self> {
        Ok(Self {
            format_version: RECEIPT_VERSION,
            target_name: file_name(target)?,
            archive_sha256,
            action_count,
            completed_actions: prefix.completed_actions,
            claim_sequence: prefix.claim_sequence,
            runtime_cursor: prefix.runtime_cursor,
            runtime_audit_sha256: prefix.runtime_audit_sha256.clone(),
        })
    }

    pub fn validate_fixed(
        &self,
        target: &Path,
        archive_sha256: &str,
        action_count: u64,
    ) -> Result<()> {
        if self.format_version != RECEIPT_VERSION
            || self.target_name != file_name(target)?
            || self.archive_sha256 != archive_sha256
            || self.action_count != action_count
            || self.completed_actions > action_count
        {
            return Err(Error::Archive(
                "logical restore receipt differs from the selected archive or target".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredReceipt<T> {
    payload: T,
    payload_sha256: String,
}

pub(super) fn export_paths(destination: &Path) -> Result<(PathBuf, PathBuf)> {
    sibling_paths(destination, "rrd-export.partial", "rrd-export.receipt.json")
}

pub(super) fn restore_paths(target: &Path, archive_sha256: &str) -> Result<(PathBuf, PathBuf)> {
    let suffix = archive_sha256
        .get(..16)
        .ok_or_else(|| Error::Archive("archive identity is not a SHA-256 digest".into()))?;
    sibling_paths(
        target,
        &format!("rrd-restore-{suffix}.staging"),
        &format!("rrd-restore-{suffix}.receipt.json"),
    )
}

pub(super) fn load_receipt<T>(path: &Path) -> Result<T>
where
    T: DeserializeOwned + Serialize,
{
    let bytes = fs::read(path).map_err(receipt_io)?;
    let stored: StoredReceipt<T> = serde_json::from_slice(&bytes)?;
    let actual = digest::sha256_hex(&serde_json::to_vec(&stored.payload)?);
    if actual != stored.payload_sha256 {
        return Err(Error::Archive(
            "logical archive resume receipt digest does not match".into(),
        ));
    }
    Ok(stored.payload)
}

pub(super) fn write_receipt<T>(path: &Path, payload: &T) -> Result<()>
where
    T: Serialize + Clone,
{
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(receipt_io)?;
    let stored = StoredReceipt {
        payload: payload.clone(),
        payload_sha256: digest::sha256_hex(&serde_json::to_vec(payload)?),
    };
    let bytes = serde_json::to_vec_pretty(&stored)?;
    let id = RECEIPT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(
        ".rrd-archive-receipt-{}-{id}.tmp",
        std::process::id()
    ));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(receipt_io)?;
        file.write_all(&bytes).map_err(receipt_io)?;
        file.sync_all().map_err(receipt_io)?;
        drop(file);
        rrd_lsm::publish_rename(parent, &temporary, path).map_err(receipt_io)
    })();
    if result.is_err() && temporary.exists() {
        fs::remove_file(&temporary).map_err(receipt_io)?;
    }
    result
}

pub(super) fn remove_receipt(path: &Path) -> Result<()> {
    if path.exists() {
        fs::remove_file(path).map_err(receipt_io)?;
        if let Some(parent) = path.parent() {
            rrd_lsm::sync_directory(parent).map_err(receipt_io)?;
        }
    }
    Ok(())
}

pub(super) fn remove_export_artifacts(partial: &Path, receipt: &Path) -> Result<()> {
    for path in [partial, receipt] {
        if path.exists() {
            fs::remove_file(path).map_err(receipt_io)?;
        }
    }
    if let Some(parent) = partial.parent() {
        rrd_lsm::sync_directory(parent).map_err(receipt_io)?;
    }
    Ok(())
}

fn sibling_paths(
    target: &Path,
    work_suffix: &str,
    receipt_suffix: &str,
) -> Result<(PathBuf, PathBuf)> {
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    let name = file_name(target)?;
    Ok((
        parent.join(format!(".{name}.{work_suffix}")),
        parent.join(format!(".{name}.{receipt_suffix}")),
    ))
}

fn file_name(path: &Path) -> Result<String> {
    path.file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| Error::Archive("archive path must have a UTF-8 file name".into()))
}

fn receipt_io(error: std::io::Error) -> Error {
    Error::Archive(error.to_string())
}
