//! Immutable, content-addressed object tier.
//!
//! Bytes are durable and verified before their canonical reference is handed
//! to the data transaction. A failed data transaction can therefore leave an
//! unreachable object, but never a reachable partial object; inventory and
//! reclamation make that asymmetry explicit.

use crate::{Error, Result};
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use vyrm_core::{digest, ObjectReceipt, ObjectReference};

static STAGE_ORDINAL: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectStep {
    BeforeStageWrite,
    AfterStageSync,
    AfterPublish,
    BeforeVerify,
    AfterVerify,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedObject {
    pub sha256: String,
    pub length: u64,
    pub receipt: ObjectReceipt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectInventoryState {
    Reachable,
    Orphan,
    Corrupt { actual_sha256: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectInventoryEntry {
    pub sha256: String,
    pub length: u64,
    pub state: ObjectInventoryState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectInventory {
    pub entries: Vec<ObjectInventoryEntry>,
    pub staging_files: Vec<PathBuf>,
    pub quarantined_files: Vec<PathBuf>,
}

pub trait ImmutableObjectStore: Send + Sync {
    fn put(&self, bytes: &[u8]) -> Result<VerifiedObject>;
    fn verify(&self, sha256: &str) -> Result<VerifiedObject>;
    fn get(&self, reference: &ObjectReference) -> Result<Vec<u8>>;
    fn inventory(&self, reachable: &BTreeSet<String>) -> Result<ObjectInventory>;
    fn reclaim_orphans(&self, unreachable: &BTreeSet<String>) -> Result<Vec<String>>;
}

#[derive(Debug, Clone)]
pub struct LocalObjectStore {
    root: PathBuf,
}

impl LocalObjectStore {
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(root.join("objects/sha256"))?;
        fs::create_dir_all(root.join("staging"))?;
        fs::create_dir_all(root.join("quarantine"))?;
        sync_directory(&root)?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn put(&self, bytes: &[u8]) -> Result<VerifiedObject> {
        self.put_with_hook(bytes, |_| Ok(()))
    }

    /// Stages, syncs, atomically publishes, and verifies one immutable object.
    /// The hook exists so crash/failure tests can stop at every boundary.
    pub fn put_with_hook(
        &self,
        bytes: &[u8],
        mut hook: impl FnMut(ObjectStep) -> Result<()>,
    ) -> Result<VerifiedObject> {
        hook(ObjectStep::BeforeStageWrite)?;
        let sha256 = digest::sha256_hex(bytes);
        let key = ObjectReference::canonical_key(&sha256).map_err(Error::from)?;
        let final_path = self.root.join(&key);
        let stage_name = format!(
            "{}-{}-{}",
            std::process::id(),
            STAGE_ORDINAL.fetch_add(1, Ordering::Relaxed),
            sha256
        );
        let stage_path = self.root.join("staging").join(stage_name);
        let mut stage = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&stage_path)?;
        stage.write_all(bytes)?;
        stage.sync_all()?;
        drop(stage);
        sync_directory(self.root.join("staging"))?;
        hook(ObjectStep::AfterStageSync)?;

        if let Some(parent) = final_path.parent() {
            fs::create_dir_all(parent)?;
        }
        if final_path.exists() {
            match self.verify(&sha256) {
                Ok(_) => {
                    // Content addressing makes an existing verified value equivalent.
                    fs::remove_file(&stage_path)?;
                }
                Err(Error::ObjectCorrupt { .. }) => {
                    self.quarantine(&sha256)?;
                    fs::rename(&stage_path, &final_path)?;
                }
                Err(error) => return Err(error),
            }
        } else {
            fs::rename(&stage_path, &final_path)?;
            if let Some(parent) = final_path.parent() {
                sync_directory(parent)?;
            }
        }
        hook(ObjectStep::AfterPublish)?;
        hook(ObjectStep::BeforeVerify)?;
        let verified = self.verify(&sha256)?;
        hook(ObjectStep::AfterVerify)?;
        Ok(verified)
    }

    pub fn verify(&self, sha256: &str) -> Result<VerifiedObject> {
        let key = ObjectReference::canonical_key(sha256).map_err(Error::from)?;
        let path = self.root.join(&key);
        let mut file = File::open(&path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                Error::ObjectMissing(sha256.to_owned())
            } else {
                Error::from(error)
            }
        })?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        let actual = digest::sha256_hex(&bytes);
        if actual != sha256 {
            return Err(Error::ObjectCorrupt {
                expected: sha256.to_owned(),
                actual,
            });
        }
        Ok(VerifiedObject {
            sha256: sha256.to_owned(),
            length: bytes.len() as u64,
            receipt: ObjectReceipt {
                backend: "local".into(),
                key,
                version: None,
                etag: Some(sha256.to_owned()),
            },
        })
    }

    pub fn get(&self, reference: &ObjectReference) -> Result<Vec<u8>> {
        reference.validate().map_err(Error::from)?;
        let verified = self.verify(&reference.sha256)?;
        if verified.length != reference.length {
            return Err(Error::ObjectLengthMismatch {
                expected: reference.length,
                actual: verified.length,
            });
        }
        fs::read(self.root.join(&reference.receipt.key)).map_err(Error::from)
    }

    pub fn inventory(&self, reachable: &BTreeSet<String>) -> Result<ObjectInventory> {
        let mut entries = Vec::new();
        let sha_root = self.root.join("objects/sha256");
        for bucket in sorted_paths(&sha_root)? {
            if !bucket.is_dir() {
                continue;
            }
            for path in sorted_paths(&bucket)? {
                if !path.is_file() {
                    continue;
                }
                let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                    continue;
                };
                let bytes = fs::read(&path)?;
                let actual = digest::sha256_hex(&bytes);
                let state = if actual != name {
                    ObjectInventoryState::Corrupt {
                        actual_sha256: actual,
                    }
                } else if reachable.contains(name) {
                    ObjectInventoryState::Reachable
                } else {
                    ObjectInventoryState::Orphan
                };
                entries.push(ObjectInventoryEntry {
                    sha256: name.to_owned(),
                    length: bytes.len() as u64,
                    state,
                });
            }
        }
        Ok(ObjectInventory {
            entries,
            staging_files: sorted_paths(&self.root.join("staging"))?,
            quarantined_files: sorted_paths(&self.root.join("quarantine"))?,
        })
    }

    /// Moves corrupt objects out of the addressable namespace. The returned
    /// path is operator evidence; no reachable reference is silently rewritten.
    pub fn quarantine(&self, sha256: &str) -> Result<PathBuf> {
        let key = ObjectReference::canonical_key(sha256).map_err(Error::from)?;
        let source = self.root.join(key);
        if !source.exists() {
            return Err(Error::ObjectMissing(sha256.to_owned()));
        }
        let target = self.root.join("quarantine").join(format!(
            "{}-{}",
            sha256,
            STAGE_ORDINAL.fetch_add(1, Ordering::Relaxed)
        ));
        fs::rename(&source, &target)?;
        sync_directory(self.root.join("quarantine"))?;
        Ok(target)
    }

    /// Reclaims only digests the caller proved unreachable. A later M4
    /// coordinator supplies that set from current references and retention pins.
    pub fn reclaim_orphans(&self, unreachable: &BTreeSet<String>) -> Result<Vec<String>> {
        let mut removed = Vec::new();
        for sha256 in unreachable {
            let key = ObjectReference::canonical_key(sha256).map_err(Error::from)?;
            let path = self.root.join(key);
            if path.exists() {
                fs::remove_file(path)?;
                removed.push(sha256.clone());
            }
        }
        removed.sort();
        Ok(removed)
    }
}

impl ImmutableObjectStore for LocalObjectStore {
    fn put(&self, bytes: &[u8]) -> Result<VerifiedObject> {
        LocalObjectStore::put(self, bytes)
    }

    fn verify(&self, sha256: &str) -> Result<VerifiedObject> {
        LocalObjectStore::verify(self, sha256)
    }

    fn get(&self, reference: &ObjectReference) -> Result<Vec<u8>> {
        LocalObjectStore::get(self, reference)
    }

    fn inventory(&self, reachable: &BTreeSet<String>) -> Result<ObjectInventory> {
        LocalObjectStore::inventory(self, reachable)
    }

    fn reclaim_orphans(&self, unreachable: &BTreeSet<String>) -> Result<Vec<String>> {
        LocalObjectStore::reclaim_orphans(self, unreachable)
    }
}

fn sorted_paths(path: &Path) -> Result<Vec<PathBuf>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let mut paths = fs::read_dir(path)?
        .map(|entry| entry.map(|entry| entry.path()).map_err(Error::from))
        .collect::<Result<Vec<_>>>()?;
    paths.sort();
    Ok(paths)
}

fn sync_directory(path: impl AsRef<Path>) -> Result<()> {
    File::open(path)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn local_put_is_content_addressed_idempotent_and_verified() {
        let directory = tempdir().unwrap();
        let store = LocalObjectStore::open(directory.path()).unwrap();
        let first = store.put(b"canonical bytes").unwrap();
        let second = store.put(b"canonical bytes").unwrap();
        assert_eq!(first, second);
        assert_eq!(store.verify(&first.sha256).unwrap(), first);
        assert!(
            store.inventory(&BTreeSet::new()).unwrap().entries[0].state
                == ObjectInventoryState::Orphan
        );
    }

    #[test]
    fn failed_after_sync_remains_staged_and_never_visible() {
        let directory = tempdir().unwrap();
        let store = LocalObjectStore::open(directory.path()).unwrap();
        let result = store.put_with_hook(b"not published", |step| {
            if step == ObjectStep::AfterStageSync {
                Err(Error::FaultInjected("after_stage_sync"))
            } else {
                Ok(())
            }
        });
        assert!(matches!(result, Err(Error::FaultInjected(_))));
        let inventory = store.inventory(&BTreeSet::new()).unwrap();
        assert_eq!(inventory.entries.len(), 0);
        assert_eq!(inventory.staging_files.len(), 1);
    }

    #[test]
    fn corrupt_object_is_reported_and_quarantined() {
        let directory = tempdir().unwrap();
        let store = LocalObjectStore::open(directory.path()).unwrap();
        let object = store.put(b"valid").unwrap();
        fs::write(store.root().join(&object.receipt.key), b"corrupt").unwrap();
        assert!(matches!(
            store.verify(&object.sha256),
            Err(Error::ObjectCorrupt { .. })
        ));
        let target = store.quarantine(&object.sha256).unwrap();
        assert!(target.exists());
        assert!(matches!(
            store.verify(&object.sha256),
            Err(Error::ObjectMissing(_))
        ));
    }

    #[test]
    fn every_publication_boundary_has_explicit_recovery_state() {
        for point in [
            ObjectStep::BeforeStageWrite,
            ObjectStep::AfterStageSync,
            ObjectStep::AfterPublish,
            ObjectStep::BeforeVerify,
            ObjectStep::AfterVerify,
        ] {
            let directory = tempdir().unwrap();
            let store = LocalObjectStore::open(directory.path()).unwrap();
            let result = store.put_with_hook(b"boundary", |step| {
                if step == point {
                    Err(Error::FaultInjected("object_boundary"))
                } else {
                    Ok(())
                }
            });
            assert!(matches!(result, Err(Error::FaultInjected(_))));
            let inventory = store.inventory(&BTreeSet::new()).unwrap();
            match point {
                ObjectStep::BeforeStageWrite => {
                    assert!(inventory.entries.is_empty());
                    assert!(inventory.staging_files.is_empty());
                }
                ObjectStep::AfterStageSync => {
                    assert!(inventory.entries.is_empty());
                    assert_eq!(inventory.staging_files.len(), 1);
                }
                ObjectStep::AfterPublish | ObjectStep::BeforeVerify | ObjectStep::AfterVerify => {
                    assert_eq!(inventory.entries.len(), 1);
                    assert_eq!(inventory.entries[0].state, ObjectInventoryState::Orphan);
                }
            }
        }
    }

    #[test]
    fn explicit_orphan_reclamation_removes_only_proven_candidates() {
        let directory = tempdir().unwrap();
        let store = LocalObjectStore::open(directory.path()).unwrap();
        let retained = store.put(b"retained").unwrap();
        let orphan = store.put(b"orphan").unwrap();
        let removed = store
            .reclaim_orphans(&BTreeSet::from([orphan.sha256.clone()]))
            .unwrap();
        assert_eq!(removed, vec![orphan.sha256]);
        assert!(store.verify(&retained.sha256).is_ok());
    }
}
