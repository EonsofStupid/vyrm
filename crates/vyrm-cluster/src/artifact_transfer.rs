//! Backend-neutral immutable object hydration for a grounded replica transfer.

use crate::{
    ArtifactObjectProgress, ArtifactReplicaObjectReceipt, ArtifactTransferManifest,
    ArtifactTransferOperation, ArtifactTransferReceipt, ArtifactTransferRpc,
    ArtifactTransferRpcResult, ClusterError, NodeId, Result,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use vyrm_core::{RuntimeMutation, ScopeId};
use vyrm_store::{Engine, Error as StoreError, ImmutableObjectStore, LocalObjectStore};

const REPLAY_PAGE: usize = 4_096;
const TRANSFER_SESSION_DIRECTORY: &str = "transfer-sessions-v1";
static PUBLISH_ORDINAL: AtomicU64 = AtomicU64::new(1);

/// Captures every immutable object reference visible at one exact project read
/// and binds its digest closure into the replica transfer plan.
pub fn prepare_artifact_transfer<E: Engine>(
    mut plan: crate::ReplicaTransferPlan,
    engine: &E,
    scope: &ScopeId,
) -> Result<ArtifactTransferManifest> {
    plan.validate()?;
    let read = engine
        .runtime_read_stamp(scope)
        .map_err(cluster_store_error)?;
    let mut cursor = 0;
    let mut objects = BTreeMap::new();
    loop {
        let page = engine
            .runtime_read_changes(&read, cursor, REPLAY_PAGE)
            .map_err(cluster_store_error)?;
        for change in page.changes {
            if !change.verify_digest() {
                return Err(ClusterError::Invalid(
                    "artifact transfer replay encountered a corrupt change digest".into(),
                ));
            }
            if let RuntimeMutation::Object { object } = change.mutation {
                objects.insert(object.reference.clone(), object);
            }
        }
        if page.through_cursor == read.commit_cursor {
            break;
        }
        if page.through_cursor <= cursor {
            return Err(ClusterError::Unavailable(
                "artifact transfer replay did not reach its project read stamp".into(),
            ));
        }
        cursor = page.through_cursor;
    }
    let objects = objects.into_values().collect::<Vec<_>>();
    plan.artifact_digests = objects
        .iter()
        .map(|object| object.sha256.clone())
        .collect::<BTreeSet<_>>();
    ArtifactTransferManifest::new(plan, scope.clone(), read, objects)
}

/// Hydrates a target object tier from a verified source. Existing verified
/// content is reused; partial success remains harmless unreachable content and
/// no completion receipt exists until the entire manifest verifies.
pub fn transfer_artifacts<S, T>(
    source: &S,
    target: &T,
    manifest: &ArtifactTransferManifest,
    completed_at: u64,
) -> Result<ArtifactTransferReceipt>
where
    S: ImmutableObjectStore,
    T: ImmutableObjectStore,
{
    manifest.validate()?;
    let mut receipts_by_digest = BTreeMap::new();
    let mut transferred_by_digest = BTreeMap::new();
    for object in &manifest.objects {
        if receipts_by_digest.contains_key(&object.sha256) {
            continue;
        }
        let existing = target.verify(&object.sha256);
        let (verified, transferred) = match existing {
            Ok(verified) if verified.length == object.length => (verified, false),
            Ok(verified) => {
                return Err(ClusterError::Invalid(format!(
                    "target object {} has length {}, expected {}",
                    object.sha256, verified.length, object.length
                )))
            }
            Err(StoreError::ObjectMissing(_)) | Err(StoreError::ObjectCorrupt { .. }) => {
                let mut reader = source.open_verified(object).map_err(cluster_store_error)?;
                let verified = target
                    .put_verified_stream(&object.sha256, object.length, reader.as_mut())
                    .map_err(cluster_store_error)?;
                (verified, true)
            }
            Err(error) => return Err(cluster_store_error(error)),
        };
        if verified.sha256 != object.sha256 || verified.length != object.length {
            return Err(ClusterError::Invalid(
                "target object verification differs from the transfer manifest".into(),
            ));
        }
        transferred_by_digest.insert(object.sha256.clone(), transferred);
        receipts_by_digest.insert(object.sha256.clone(), verified.receipt);
    }
    let mut accounted = BTreeSet::new();
    let receipts = manifest
        .objects
        .iter()
        .map(|object| ArtifactReplicaObjectReceipt {
            reference: object.reference.clone(),
            sha256: object.sha256.clone(),
            length: object.length,
            target: receipts_by_digest[&object.sha256].clone(),
            transferred: transferred_by_digest[&object.sha256]
                && accounted.insert(object.sha256.clone()),
        })
        .collect();
    ArtifactTransferReceipt::new(manifest, receipts, completed_at)
}

/// Durable receiver for the authenticated chunk protocol. Session identity is
/// the manifest digest, so reconnects and process restarts resume at the exact
/// fsynced offset instead of creating a second mutable upload identity.
#[derive(Debug, Clone)]
pub struct ArtifactTransferReceiver {
    store: LocalObjectStore,
    sessions: PathBuf,
    serial: Arc<Mutex<()>>,
}

impl ArtifactTransferReceiver {
    pub fn open(store: LocalObjectStore) -> Result<Self> {
        let sessions = store.root().join(TRANSFER_SESSION_DIRECTORY);
        fs::create_dir_all(&sessions).map_err(cluster_io_error)?;
        sync_directory(&sessions)?;
        Ok(Self {
            store,
            sessions,
            serial: Arc::new(Mutex::new(())),
        })
    }

    pub fn store(&self) -> &LocalObjectStore {
        &self.store
    }

    pub fn handle(
        &self,
        authenticated_source: &NodeId,
        local_target: &NodeId,
        request: ArtifactTransferRpc,
    ) -> Result<ArtifactTransferRpcResult> {
        request.validate()?;
        let _guard = self
            .serial
            .lock()
            .map_err(|_| ClusterError::Unavailable("artifact receiver lock is poisoned".into()))?;
        match request.operation {
            ArtifactTransferOperation::Begin { manifest } => {
                self.begin(authenticated_source, local_target, *manifest)
            }
            ArtifactTransferOperation::Chunk {
                manifest_digest,
                sha256,
                offset,
                bytes,
                ..
            } => self.chunk(
                authenticated_source,
                local_target,
                &manifest_digest,
                &sha256,
                offset,
                &bytes,
            ),
            ArtifactTransferOperation::Complete {
                manifest_digest,
                completed_at,
            } => self.complete(
                authenticated_source,
                local_target,
                &manifest_digest,
                completed_at,
            ),
        }
    }

    fn begin(
        &self,
        source: &NodeId,
        target: &NodeId,
        manifest: ArtifactTransferManifest,
    ) -> Result<ArtifactTransferRpcResult> {
        validate_peers(&manifest, source, target)?;
        let directory = self.session_directory(&manifest.manifest_digest);
        fs::create_dir_all(&directory).map_err(cluster_io_error)?;
        let manifest_path = directory.join("manifest.json");
        if manifest_path.exists() {
            let persisted = read_manifest(&manifest_path)?;
            if persisted != manifest {
                return Err(ClusterError::Denied(
                    "artifact session digest is already bound to another manifest".into(),
                ));
            }
        } else {
            publish_json(&manifest_path, &manifest)?;
        }
        let objects = distinct_objects(&manifest)
            .into_iter()
            .map(|(sha256, expected_length)| self.progress(&directory, &sha256, expected_length))
            .collect::<Result<Vec<_>>>()?;
        Ok(ArtifactTransferRpcResult::Progress {
            manifest_digest: manifest.manifest_digest,
            objects,
        })
    }

    fn chunk(
        &self,
        source: &NodeId,
        target: &NodeId,
        manifest_digest: &str,
        sha256: &str,
        offset: u64,
        bytes: &[u8],
    ) -> Result<ArtifactTransferRpcResult> {
        let directory = self.session_directory(manifest_digest);
        let manifest = read_manifest(&directory.join("manifest.json"))?;
        validate_peers(&manifest, source, target)?;
        if manifest.manifest_digest != manifest_digest {
            return Err(ClusterError::Denied(
                "artifact chunk session differs from its persisted manifest".into(),
            ));
        }
        let expected_length = manifest
            .objects
            .iter()
            .find(|object| object.sha256 == sha256)
            .map(|object| object.length)
            .ok_or_else(|| {
                ClusterError::Denied("artifact chunk digest is absent from the manifest".into())
            })?;
        let current = self.progress(&directory, sha256, expected_length)?;
        if current.complete || current.next_offset != offset {
            return Ok(ArtifactTransferRpcResult::ChunkAccepted {
                manifest_digest: manifest_digest.to_owned(),
                object: current,
            });
        }
        let next = offset
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| ClusterError::Invalid("artifact chunk offset overflowed u64".into()))?;
        if next > expected_length {
            return Err(ClusterError::Denied(
                "artifact chunk exceeds its manifest length".into(),
            ));
        }
        let part = directory.join(format!("{sha256}.part"));
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&part)
            .map_err(cluster_io_error)?;
        file.write_all(bytes).map_err(cluster_io_error)?;
        file.sync_data().map_err(cluster_io_error)?;
        drop(file);
        sync_directory(&directory)?;
        if next == expected_length {
            let mut source_file = File::open(&part).map_err(cluster_io_error)?;
            let published = self
                .store
                .put_verified_stream(sha256, expected_length, &mut source_file)
                .map_err(cluster_store_error);
            if let Err(error) = published {
                let _ = fs::remove_file(&part);
                return Err(error);
            }
            let marker = directory.join(format!("{sha256}.transferred"));
            if !marker.exists() {
                publish_bytes(&marker, b"transferred-v1")?;
            }
            fs::remove_file(&part).map_err(cluster_io_error)?;
            sync_directory(&directory)?;
        }
        Ok(ArtifactTransferRpcResult::ChunkAccepted {
            manifest_digest: manifest_digest.to_owned(),
            object: self.progress(&directory, sha256, expected_length)?,
        })
    }

    fn complete(
        &self,
        source: &NodeId,
        target: &NodeId,
        manifest_digest: &str,
        completed_at: u64,
    ) -> Result<ArtifactTransferRpcResult> {
        let directory = self.session_directory(manifest_digest);
        let manifest = read_manifest(&directory.join("manifest.json"))?;
        validate_peers(&manifest, source, target)?;
        if manifest.manifest_digest != manifest_digest {
            return Err(ClusterError::Denied(
                "artifact completion session differs from its manifest".into(),
            ));
        }
        let receipt_path = directory.join("receipt.json");
        if receipt_path.exists() {
            let receipt: ArtifactTransferReceipt = read_json(&receipt_path, "artifact receipt")?;
            receipt.validate(&manifest)?;
            return Ok(ArtifactTransferRpcResult::Completed { receipt });
        }
        let mut accounted = BTreeSet::new();
        let objects = manifest
            .objects
            .iter()
            .map(|object| {
                let verified = self
                    .store
                    .verify(&object.sha256)
                    .map_err(cluster_store_error)?;
                if verified.length != object.length {
                    return Err(ClusterError::Invalid(format!(
                        "completed artifact {} has length {}, expected {}",
                        object.sha256, verified.length, object.length
                    )));
                }
                Ok(ArtifactReplicaObjectReceipt {
                    reference: object.reference.clone(),
                    sha256: object.sha256.clone(),
                    length: object.length,
                    target: verified.receipt,
                    transferred: accounted.insert(object.sha256.clone())
                        && directory
                            .join(format!("{}.transferred", object.sha256))
                            .is_file(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let receipt = ArtifactTransferReceipt::new(&manifest, objects, completed_at)?;
        publish_json(&receipt_path, &receipt)?;
        Ok(ArtifactTransferRpcResult::Completed { receipt })
    }

    fn progress(
        &self,
        directory: &Path,
        sha256: &str,
        expected_length: u64,
    ) -> Result<ArtifactObjectProgress> {
        match self.store.verify(sha256) {
            Ok(verified) if verified.length == expected_length => {
                return Ok(ArtifactObjectProgress {
                    sha256: sha256.to_owned(),
                    expected_length,
                    next_offset: expected_length,
                    complete: true,
                })
            }
            Ok(verified) => {
                return Err(ClusterError::Invalid(format!(
                    "target object {sha256} has length {}, expected {expected_length}",
                    verified.length
                )))
            }
            Err(StoreError::ObjectMissing(_)) | Err(StoreError::ObjectCorrupt { .. }) => {}
            Err(error) => return Err(cluster_store_error(error)),
        }
        if expected_length == 0 {
            let mut empty = std::io::Cursor::new(Vec::<u8>::new());
            self.store
                .put_verified_stream(sha256, 0, &mut empty)
                .map_err(cluster_store_error)?;
            let marker = directory.join(format!("{sha256}.transferred"));
            if !marker.exists() {
                publish_bytes(&marker, b"transferred-v1")?;
            }
            return Ok(ArtifactObjectProgress {
                sha256: sha256.to_owned(),
                expected_length,
                next_offset: 0,
                complete: true,
            });
        }
        let part = directory.join(format!("{sha256}.part"));
        let next_offset = match fs::metadata(&part) {
            Ok(metadata) if metadata.is_file() => metadata.len(),
            Ok(_) => {
                return Err(ClusterError::Denied(
                    "artifact session part is not a regular file".into(),
                ))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
            Err(error) => return Err(cluster_io_error(error)),
        };
        if next_offset > expected_length {
            return Err(ClusterError::Denied(
                "artifact session offset exceeds its manifest length".into(),
            ));
        }
        Ok(ArtifactObjectProgress {
            sha256: sha256.to_owned(),
            expected_length,
            next_offset,
            complete: false,
        })
    }

    fn session_directory(&self, manifest_digest: &str) -> PathBuf {
        self.sessions.join(manifest_digest)
    }
}

fn distinct_objects(manifest: &ArtifactTransferManifest) -> BTreeMap<String, u64> {
    manifest
        .objects
        .iter()
        .map(|object| (object.sha256.clone(), object.length))
        .collect()
}

fn validate_peers(
    manifest: &ArtifactTransferManifest,
    source: &NodeId,
    target: &NodeId,
) -> Result<()> {
    manifest.validate()?;
    if &manifest.plan.source != source || &manifest.plan.target != target {
        return Err(ClusterError::Denied(
            "authenticated artifact peers differ from the transfer manifest".into(),
        ));
    }
    Ok(())
}

fn read_manifest(path: &Path) -> Result<ArtifactTransferManifest> {
    let manifest: ArtifactTransferManifest = read_json(path, "artifact manifest")?;
    manifest.validate()?;
    Ok(manifest)
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path, label: &str) -> Result<T> {
    let metadata = fs::metadata(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ClusterError::NotFound(format!("{label} session is absent"))
        } else {
            cluster_io_error(error)
        }
    })?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > 16 * 1024 * 1024 {
        return Err(ClusterError::Denied(format!(
            "{label} file is outside its bounded contract"
        )));
    }
    let bytes = fs::read(path).map_err(cluster_io_error)?;
    serde_json::from_slice(&bytes)
        .map_err(|error| ClusterError::Invalid(format!("decode {label}: {error}")))
}

fn publish_json(path: &Path, value: &impl serde::Serialize) -> Result<()> {
    let bytes = serde_json::to_vec(value)
        .map_err(|error| ClusterError::Invalid(format!("encode artifact session: {error}")))?;
    if bytes.is_empty() || bytes.len() > 16 * 1024 * 1024 {
        return Err(ClusterError::Invalid(
            "artifact session JSON is outside its bounded contract".into(),
        ));
    }
    publish_bytes(path, &bytes)
}

fn publish_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| ClusterError::Invalid("artifact session path has no parent".into()))?;
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| ClusterError::Invalid("artifact session filename is invalid".into()))?;
    let (pending, mut file) = loop {
        let ordinal = PUBLISH_ORDINAL.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(
            ".{filename}-{}-{ordinal}.pending",
            std::process::id()
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => break (candidate, file),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(cluster_io_error(error)),
        }
    };
    let published = (|| -> Result<()> {
        file.write_all(bytes).map_err(cluster_io_error)?;
        file.sync_all().map_err(cluster_io_error)?;
        drop(file);
        fs::rename(&pending, path).map_err(cluster_io_error)?;
        sync_directory(parent)
    })();
    if published.is_err() {
        let _ = fs::remove_file(&pending);
    }
    published
}

fn sync_directory(path: &Path) -> Result<()> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(cluster_io_error)
}

fn cluster_io_error(error: std::io::Error) -> ClusterError {
    ClusterError::Unavailable(format!("artifact session I/O: {error}"))
}

fn cluster_store_error(error: StoreError) -> ClusterError {
    match error {
        StoreError::ObjectMissing(digest) => ClusterError::NotFound(digest),
        error => ClusterError::Unavailable(error.to_string()),
    }
}
