//! Immutable, content-addressed object tier.
//!
//! Bytes are durable and verified before their canonical reference is handed
//! to the data transaction. A failed data transaction can therefore leave an
//! unreachable object, but never a reachable partial object; inventory and
//! reclamation make that asymmetry explicit.

use crate::{publish_durable_rename, sync_directory_metadata, Error, Result};
use rrd_core::{digest, ObjectReceipt, ObjectReference};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

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
    /// Opens a verified object for bounded-memory transfer. Implementations
    /// must authenticate digest and length before returning the reader.
    fn open_verified(&self, reference: &ObjectReference) -> Result<Box<dyn Read + Send + '_>>;
    /// Consumes exactly one declared content address. Extra, truncated, or
    /// substituted bytes fail before the object becomes addressable.
    fn put_verified_stream(
        &self,
        expected_sha256: &str,
        expected_length: u64,
        reader: &mut dyn Read,
    ) -> Result<VerifiedObject>;
    fn verify(&self, sha256: &str) -> Result<VerifiedObject>;
    fn get(&self, reference: &ObjectReference) -> Result<Vec<u8>>;
    fn inventory(&self, reachable: &BTreeSet<String>) -> Result<ObjectInventory>;
    fn reclaim_orphans(&self, unreachable: &BTreeSet<String>) -> Result<Vec<String>>;
}

/// Process-local immutable bytes for the true in-memory RRD composition.
/// Content addressing and verification remain identical to durable object
/// stores; only persistence and physical recovery are intentionally absent.
#[derive(Default)]
pub struct MemoryObjectStore {
    objects: Mutex<BTreeMap<String, Vec<u8>>>,
}

impl MemoryObjectStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn verified(&self, sha256: &str, bytes: &[u8]) -> Result<VerifiedObject> {
        let actual = digest::sha256_hex(bytes);
        if actual != sha256 {
            return Err(Error::ObjectCorrupt {
                expected: sha256.to_owned(),
                actual,
            });
        }
        let key = ObjectReference::canonical_key(sha256).map_err(Error::from)?;
        Ok(VerifiedObject {
            sha256: sha256.to_owned(),
            length: bytes.len() as u64,
            receipt: ObjectReceipt {
                backend: "memory".into(),
                key,
                version: None,
                etag: Some(sha256.to_owned()),
            },
        })
    }
}

impl ImmutableObjectStore for MemoryObjectStore {
    fn put(&self, bytes: &[u8]) -> Result<VerifiedObject> {
        let sha256 = digest::sha256_hex(bytes);
        let verified = self.verified(&sha256, bytes)?;
        self.objects
            .lock()
            .expect("memory object mutex")
            .entry(sha256)
            .or_insert_with(|| bytes.to_vec());
        Ok(verified)
    }

    fn open_verified(&self, reference: &ObjectReference) -> Result<Box<dyn Read + Send + '_>> {
        Ok(Box::new(Cursor::new(self.get(reference)?)))
    }

    fn put_verified_stream(
        &self,
        expected_sha256: &str,
        expected_length: u64,
        reader: &mut dyn Read,
    ) -> Result<VerifiedObject> {
        let capacity = usize::try_from(expected_length)
            .map_err(|_| Error::Object("object length exceeds process address space".into()))?;
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(capacity).map_err(|_| {
            Error::Object("memory object capacity cannot be reserved within process limits".into())
        })?;
        reader
            .take(expected_length.saturating_add(1))
            .read_to_end(&mut bytes)?;
        let actual_length = bytes.len() as u64;
        if actual_length != expected_length {
            return Err(Error::ObjectLengthMismatch {
                expected: expected_length,
                actual: actual_length,
            });
        }
        let verified = self.verified(expected_sha256, &bytes)?;
        self.objects
            .lock()
            .expect("memory object mutex")
            .entry(expected_sha256.to_owned())
            .or_insert(bytes);
        Ok(verified)
    }

    fn verify(&self, sha256: &str) -> Result<VerifiedObject> {
        let objects = self.objects.lock().expect("memory object mutex");
        let bytes = objects
            .get(sha256)
            .ok_or_else(|| Error::ObjectMissing(sha256.to_owned()))?;
        self.verified(sha256, bytes)
    }

    fn get(&self, reference: &ObjectReference) -> Result<Vec<u8>> {
        reference.validate().map_err(Error::from)?;
        let canonical_key =
            ObjectReference::canonical_key(&reference.sha256).map_err(Error::from)?;
        if reference.receipt.backend != "memory" || reference.receipt.key != canonical_key {
            return Err(Error::Object(
                "object receipt does not belong to the memory object authority".into(),
            ));
        }
        let objects = self.objects.lock().expect("memory object mutex");
        let bytes = objects
            .get(&reference.sha256)
            .ok_or_else(|| Error::ObjectMissing(reference.sha256.clone()))?;
        let verified = self.verified(&reference.sha256, bytes)?;
        if verified.length != reference.length {
            return Err(Error::ObjectLengthMismatch {
                expected: reference.length,
                actual: verified.length,
            });
        }
        Ok(bytes.clone())
    }

    fn inventory(&self, reachable: &BTreeSet<String>) -> Result<ObjectInventory> {
        let objects = self.objects.lock().expect("memory object mutex");
        let entries = objects
            .iter()
            .map(|(sha256, bytes)| {
                let verified = self.verified(sha256, bytes)?;
                Ok(ObjectInventoryEntry {
                    sha256: sha256.clone(),
                    length: verified.length,
                    state: if reachable.contains(sha256) {
                        ObjectInventoryState::Reachable
                    } else {
                        ObjectInventoryState::Orphan
                    },
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(ObjectInventory {
            entries,
            staging_files: Vec::new(),
            quarantined_files: Vec::new(),
        })
    }

    fn reclaim_orphans(&self, unreachable: &BTreeSet<String>) -> Result<Vec<String>> {
        let mut objects = self.objects.lock().expect("memory object mutex");
        let removed = unreachable
            .iter()
            .filter_map(|sha256| objects.remove(sha256).map(|_| sha256.clone()))
            .collect();
        Ok(removed)
    }
}

/// Type-erased immutable-object authority paired with [`crate::EngineBox`].
pub struct ObjectStoreBox(Box<dyn ImmutableObjectStore>);

impl ObjectStoreBox {
    pub fn new(store: impl ImmutableObjectStore + 'static) -> Self {
        Self(Box::new(store))
    }
}

impl ImmutableObjectStore for ObjectStoreBox {
    fn put(&self, bytes: &[u8]) -> Result<VerifiedObject> {
        self.0.put(bytes)
    }

    fn open_verified(&self, reference: &ObjectReference) -> Result<Box<dyn Read + Send + '_>> {
        self.0.open_verified(reference)
    }

    fn put_verified_stream(
        &self,
        expected_sha256: &str,
        expected_length: u64,
        reader: &mut dyn Read,
    ) -> Result<VerifiedObject> {
        self.0
            .put_verified_stream(expected_sha256, expected_length, reader)
    }

    fn verify(&self, sha256: &str) -> Result<VerifiedObject> {
        self.0.verify(sha256)
    }

    fn get(&self, reference: &ObjectReference) -> Result<Vec<u8>> {
        self.0.get(reference)
    }

    fn inventory(&self, reachable: &BTreeSet<String>) -> Result<ObjectInventory> {
        self.0.inventory(reachable)
    }

    fn reclaim_orphans(&self, unreachable: &BTreeSet<String>) -> Result<Vec<String>> {
        self.0.reclaim_orphans(unreachable)
    }
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
        sync_directory_metadata(&root)?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn put(&self, bytes: &[u8]) -> Result<VerifiedObject> {
        self.put_with_hook(bytes, |_| Ok(()))
    }

    pub fn open_verified(&self, reference: &ObjectReference) -> Result<Box<dyn Read + Send + '_>> {
        let path = self.verified_path(reference)?;
        Ok(Box::new(File::open(path)?))
    }

    /// Streams a known content address through the same stage/sync/publish
    /// boundary as ordinary local objects without materializing the payload.
    pub fn put_verified_stream(
        &self,
        expected_sha256: &str,
        expected_length: u64,
        reader: &mut dyn Read,
    ) -> Result<VerifiedObject> {
        let key = ObjectReference::canonical_key(expected_sha256).map_err(Error::from)?;
        let stage_name = format!(
            "{}-{}-transfer",
            std::process::id(),
            STAGE_ORDINAL.fetch_add(1, Ordering::Relaxed)
        );
        let stage_path = self.root.join("staging").join(stage_name);
        let mut stage = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&stage_path)?;
        let staged = (|| -> Result<()> {
            let mut hasher = digest::Sha256::new();
            let mut length = 0u64;
            let mut buffer = [0u8; 64 * 1024];
            loop {
                let read = reader.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                length = length
                    .checked_add(read as u64)
                    .ok_or_else(|| Error::Object("object length overflowed u64".into()))?;
                if length > expected_length {
                    return Err(Error::ObjectLengthMismatch {
                        expected: expected_length,
                        actual: length,
                    });
                }
                hasher.update(&buffer[..read]);
                stage.write_all(&buffer[..read])?;
            }
            if length != expected_length {
                return Err(Error::ObjectLengthMismatch {
                    expected: expected_length,
                    actual: length,
                });
            }
            let actual = hasher.finalize_hex();
            if actual != expected_sha256 {
                return Err(Error::ObjectCorrupt {
                    expected: expected_sha256.to_owned(),
                    actual,
                });
            }
            stage.sync_all()?;
            sync_directory_metadata(&self.root.join("staging"))?;
            Ok(())
        })();
        if let Err(error) = staged {
            drop(stage);
            let _ = fs::remove_file(&stage_path);
            return Err(error);
        }
        drop(stage);
        let final_path = self.root.join(key);
        self.publish_stage(&stage_path, &final_path, expected_sha256)?;
        let verified = self.verify(expected_sha256)?;
        if verified.length != expected_length {
            return Err(Error::ObjectLengthMismatch {
                expected: expected_length,
                actual: verified.length,
            });
        }
        Ok(verified)
    }

    /// Publishes one immutable file without materializing it as a `Vec`.
    pub fn put_file(&self, source: impl AsRef<Path>) -> Result<VerifiedObject> {
        self.put_file_with_hook(source, |_| Ok(()))
    }

    pub fn put_file_with_hook(
        &self,
        source: impl AsRef<Path>,
        mut hook: impl FnMut(ObjectStep) -> Result<()>,
    ) -> Result<VerifiedObject> {
        hook(ObjectStep::BeforeStageWrite)?;
        let mut source = File::open(source)?;
        let stage_name = format!(
            "{}-{}-stream",
            std::process::id(),
            STAGE_ORDINAL.fetch_add(1, Ordering::Relaxed)
        );
        let stage_path = self.root.join("staging").join(stage_name);
        let mut stage = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&stage_path)?;
        let mut digest = digest::Sha256::new();
        let mut length = 0u64;
        let mut buffer = [0u8; 64 * 1024];
        let staged = (|| -> Result<(String, u64)> {
            loop {
                let read = source.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                length = length
                    .checked_add(read as u64)
                    .ok_or_else(|| Error::Object("object length overflowed u64".into()))?;
                digest.update(&buffer[..read]);
                stage.write_all(&buffer[..read])?;
            }
            stage.sync_all()?;
            sync_directory_metadata(&self.root.join("staging"))?;
            Ok((digest.finalize_hex(), length))
        })();
        let (sha256, length) = match staged {
            Ok(value) => value,
            Err(error) => {
                drop(stage);
                let _ = fs::remove_file(&stage_path);
                return Err(error);
            }
        };
        drop(stage);
        hook(ObjectStep::AfterStageSync)?;
        let key = ObjectReference::canonical_key(&sha256).map_err(Error::from)?;
        let final_path = self.root.join(&key);
        self.publish_stage(&stage_path, &final_path, &sha256)?;
        hook(ObjectStep::AfterPublish)?;
        hook(ObjectStep::BeforeVerify)?;
        let verified = self.verify(&sha256)?;
        if verified.length != length {
            return Err(Error::ObjectLengthMismatch {
                expected: length,
                actual: verified.length,
            });
        }
        hook(ObjectStep::AfterVerify)?;
        Ok(verified)
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
        sync_directory_metadata(&self.root.join("staging"))?;
        hook(ObjectStep::AfterStageSync)?;

        self.publish_stage(&stage_path, &final_path, &sha256)?;
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
        let (actual, length) = digest_reader(&mut file)?;
        if actual != sha256 {
            return Err(Error::ObjectCorrupt {
                expected: sha256.to_owned(),
                actual,
            });
        }
        Ok(VerifiedObject {
            sha256: sha256.to_owned(),
            length,
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

    /// Returns a verified canonical path suitable for bounded file I/O.
    pub fn verified_path(&self, reference: &ObjectReference) -> Result<PathBuf> {
        reference.validate().map_err(Error::from)?;
        let verified = self.verify(&reference.sha256)?;
        if verified.length != reference.length {
            return Err(Error::ObjectLengthMismatch {
                expected: reference.length,
                actual: verified.length,
            });
        }
        Ok(self.root.join(&reference.receipt.key))
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
                let mut file = File::open(&path)?;
                let (actual, length) = digest_reader(&mut file)?;
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
                    length,
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
        publish_object_path(&source, &target)?;
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

    fn publish_stage(&self, stage_path: &Path, final_path: &Path, sha256: &str) -> Result<()> {
        let parent = final_path
            .parent()
            .ok_or_else(|| Error::Object("object publication target has no parent".into()))?;
        fs::create_dir_all(parent)?;
        if final_path.exists() {
            match self.verify(sha256) {
                Ok(_) => {
                    // Content addressing makes an existing verified value equivalent.
                    fs::remove_file(stage_path)?;
                }
                Err(Error::ObjectCorrupt { .. }) => {
                    self.quarantine(sha256)?;
                    publish_object_path(stage_path, final_path)?;
                }
                Err(error) => return Err(error),
            }
        } else {
            publish_object_path(stage_path, final_path)?;
        }
        Ok(())
    }
}

fn digest_reader(reader: &mut impl Read) -> Result<(String, u64)> {
    let mut digest = digest::Sha256::new();
    let mut length = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        length = length
            .checked_add(read as u64)
            .ok_or_else(|| Error::Object("object length overflowed u64".into()))?;
        digest.update(&buffer[..read]);
    }
    Ok((digest.finalize_hex(), length))
}

impl ImmutableObjectStore for LocalObjectStore {
    fn put(&self, bytes: &[u8]) -> Result<VerifiedObject> {
        LocalObjectStore::put(self, bytes)
    }

    fn open_verified(&self, reference: &ObjectReference) -> Result<Box<dyn Read + Send + '_>> {
        LocalObjectStore::open_verified(self, reference)
    }

    fn put_verified_stream(
        &self,
        expected_sha256: &str,
        expected_length: u64,
        reader: &mut dyn Read,
    ) -> Result<VerifiedObject> {
        LocalObjectStore::put_verified_stream(self, expected_sha256, expected_length, reader)
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

fn publish_object_path(source: &Path, target: &Path) -> Result<()> {
    let source_parent = source
        .parent()
        .ok_or_else(|| Error::Object("object publication source has no parent".into()))?;
    let target_parent = target
        .parent()
        .ok_or_else(|| Error::Object("object publication target has no parent".into()))?;
    publish_durable_rename(target_parent, source, target)?;
    if source_parent != target_parent {
        sync_directory_metadata(source_parent)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn memory_objects_keep_the_same_content_address_and_receipt_boundary() {
        let store = MemoryObjectStore::new();
        let verified = store.put(b"memory canonical bytes").unwrap();
        assert_eq!(verified.receipt.backend, "memory");
        let reference = ObjectReference::for_bytes(
            "memory-object",
            None,
            "application/octet-stream",
            b"memory canonical bytes",
            verified.receipt.clone(),
        )
        .unwrap();
        assert_eq!(store.get(&reference).unwrap(), b"memory canonical bytes");
        assert_eq!(store.verify(&verified.sha256).unwrap(), verified);

        let removed = store
            .reclaim_orphans(&BTreeSet::from([reference.sha256.clone()]))
            .unwrap();
        assert_eq!(removed, vec![reference.sha256.clone()]);
        assert!(matches!(
            store.verify(&reference.sha256),
            Err(Error::ObjectMissing(_))
        ));
    }

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
    fn local_file_put_streams_to_the_same_content_address() {
        let directory = tempdir().unwrap();
        let source = directory.path().join("source.bin");
        let bytes = vec![0x5a; 256 * 1024 + 17];
        fs::write(&source, &bytes).unwrap();
        let store = LocalObjectStore::open(directory.path().join("objects")).unwrap();
        let streamed = store.put_file(&source).unwrap();
        let in_memory = store.put(&bytes).unwrap();
        assert_eq!(streamed, in_memory);

        let reference = ObjectReference::for_verified(
            "streamed",
            None,
            "application/octet-stream",
            streamed.sha256.clone(),
            streamed.length,
            streamed.receipt.clone(),
        )
        .unwrap();
        assert_eq!(
            store.verified_path(&reference).unwrap(),
            store.root().join(&streamed.receipt.key)
        );
    }

    #[test]
    fn verified_stream_is_bounded_content_addressed_and_fail_closed() {
        let directory = tempdir().unwrap();
        let store = LocalObjectStore::open(directory.path()).unwrap();
        let bytes = vec![0x3c; 192 * 1024 + 9];
        let sha256 = digest::sha256_hex(&bytes);
        let mut reader = std::io::Cursor::new(bytes.clone());
        let streamed = store
            .put_verified_stream(&sha256, bytes.len() as u64, &mut reader)
            .unwrap();
        assert_eq!(streamed, store.put(&bytes).unwrap());

        let wrong = "11".repeat(32);
        let mut substituted = std::io::Cursor::new(bytes.clone());
        assert!(matches!(
            store.put_verified_stream(&wrong, bytes.len() as u64, &mut substituted),
            Err(Error::ObjectCorrupt { .. })
        ));
        assert!(matches!(store.verify(&wrong), Err(Error::ObjectMissing(_))));

        let mut truncated = std::io::Cursor::new(&bytes[..bytes.len() - 1]);
        assert!(matches!(
            store.put_verified_stream(&sha256, bytes.len() as u64, &mut truncated),
            Err(Error::ObjectLengthMismatch { .. })
        ));
        assert!(store
            .inventory(&BTreeSet::new())
            .unwrap()
            .staging_files
            .is_empty());
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
