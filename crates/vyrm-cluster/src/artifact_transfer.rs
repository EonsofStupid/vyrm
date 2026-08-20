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
use std::sync::{Arc, Mutex, RwLock, Weak};
use std::time::{SystemTime, UNIX_EPOCH};
use vyrm_core::{RuntimeMutation, ScopeId};
use vyrm_store::{Engine, Error as StoreError, ImmutableObjectStore, LocalObjectStore};

const REPLAY_PAGE: usize = 4_096;
const TRANSFER_SESSION_DIRECTORY: &str = "transfer-sessions-v1";
const TRANSFER_SESSION_STATE_FILE: &str = "session.json";
const TRANSFER_SESSION_STATE_VERSION: u16 = 1;
pub const ARTIFACT_TRANSFER_TELEMETRY_VERSION: u16 = 1;
static PUBLISH_ORDINAL: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactTransferSessionPolicy {
    pub max_active_sessions: usize,
    pub max_reserved_bytes: u64,
    pub stale_incomplete_after_millis: u64,
    pub completed_receipt_retention_millis: u64,
    pub max_retained_receipts: usize,
}

impl Default for ArtifactTransferSessionPolicy {
    fn default() -> Self {
        Self {
            max_active_sessions: 64,
            max_reserved_bytes: 64 * 1024 * 1024 * 1024,
            stale_incomplete_after_millis: 24 * 60 * 60 * 1_000,
            completed_receipt_retention_millis: 7 * 24 * 60 * 60 * 1_000,
            max_retained_receipts: 4_096,
        }
    }
}

impl ArtifactTransferSessionPolicy {
    pub fn validate(&self) -> Result<()> {
        if self.max_active_sessions == 0
            || self.max_active_sessions > 65_536
            || self.max_reserved_bytes == 0
            || self.stale_incomplete_after_millis == 0
            || self.completed_receipt_retention_millis == 0
            || self.max_retained_receipts == 0
            || self.max_retained_receipts > 1_000_000
        {
            return Err(ClusterError::Invalid(
                "artifact session policy is outside its bounded contract".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactTransferSessionInventory {
    pub active_sessions: usize,
    pub reserved_bytes: u64,
    pub partial_bytes: u64,
    pub retained_receipts: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactTransferGcReport {
    pub scanned_sessions: usize,
    pub removed_incomplete: usize,
    pub removed_completed: usize,
    pub reclaimed_partial_bytes: u64,
    pub remaining: ArtifactTransferSessionInventory,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactTransferTelemetrySnapshot {
    pub contract_version: u16,
    pub started_at: u64,
    pub observed_at: u64,
    pub policy: ArtifactTransferSessionPolicy,
    pub inventory: ArtifactTransferSessionInventory,
    pub begin_requests: u64,
    pub chunk_requests: u64,
    pub complete_requests: u64,
    pub begin_responses: u64,
    pub accepted_chunks: u64,
    pub completed_responses: u64,
    pub completed_receipt_replays: u64,
    pub denied: u64,
    pub failed: u64,
    pub quota_denials: u64,
    pub gc_runs: u64,
    pub gc_removed_incomplete: u64,
    pub gc_removed_completed: u64,
    pub gc_reclaimed_partial_bytes: u64,
    pub overflowed: bool,
}

#[derive(Debug)]
struct ArtifactTransferTelemetryState {
    started_at: u64,
    begin_requests: u64,
    chunk_requests: u64,
    complete_requests: u64,
    begin_responses: u64,
    accepted_chunks: u64,
    completed_responses: u64,
    completed_receipt_replays: u64,
    denied: u64,
    failed: u64,
    quota_denials: u64,
    gc_runs: u64,
    gc_removed_incomplete: u64,
    gc_removed_completed: u64,
    gc_reclaimed_partial_bytes: u64,
    overflowed: bool,
}

#[derive(Debug, Clone, Copy)]
enum ArtifactTransferRequestKind {
    Begin,
    Chunk,
    Complete,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ArtifactTransferSessionState {
    version: u16,
    manifest_digest: String,
    last_activity_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    completed_at: Option<u64>,
}

impl ArtifactTransferSessionState {
    fn active(manifest_digest: String, now: u64) -> Self {
        Self {
            version: TRANSFER_SESSION_STATE_VERSION,
            manifest_digest,
            last_activity_at: now,
            completed_at: None,
        }
    }

    fn validate(&self, expected_digest: &str) -> Result<()> {
        if self.version != TRANSFER_SESSION_STATE_VERSION || self.manifest_digest != expected_digest
        {
            return Err(ClusterError::Denied(
                "artifact session state differs from its directory identity".into(),
            ));
        }
        Ok(())
    }
}

struct SessionInventoryEntry {
    digest: String,
    path: PathBuf,
    state: ArtifactTransferSessionState,
    complete: bool,
    reserved_bytes: u64,
    partial_bytes: u64,
}

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
    policy: ArtifactTransferSessionPolicy,
    lifecycle: Arc<RwLock<()>>,
    session_locks: Arc<Mutex<BTreeMap<String, Weak<Mutex<()>>>>>,
    telemetry: Arc<Mutex<ArtifactTransferTelemetryState>>,
}

impl ArtifactTransferReceiver {
    pub fn open(store: LocalObjectStore) -> Result<Self> {
        Self::open_with_policy(store, ArtifactTransferSessionPolicy::default())
    }

    pub fn open_with_policy(
        store: LocalObjectStore,
        policy: ArtifactTransferSessionPolicy,
    ) -> Result<Self> {
        Self::open_with_policy_at(store, policy, receiver_now_millis())
    }

    /// Deterministic-clock constructor used by failure simulation and contract
    /// tests. Production callers should use [`Self::open_with_policy`].
    pub fn open_with_policy_at(
        store: LocalObjectStore,
        policy: ArtifactTransferSessionPolicy,
        started_at: u64,
    ) -> Result<Self> {
        policy.validate()?;
        let sessions = store.root().join(TRANSFER_SESSION_DIRECTORY);
        fs::create_dir_all(&sessions).map_err(cluster_io_error)?;
        sync_directory(&sessions)?;
        let receiver = Self {
            store,
            sessions,
            policy,
            lifecycle: Arc::new(RwLock::new(())),
            session_locks: Arc::new(Mutex::new(BTreeMap::new())),
            telemetry: Arc::new(Mutex::new(ArtifactTransferTelemetryState {
                started_at,
                begin_requests: 0,
                chunk_requests: 0,
                complete_requests: 0,
                begin_responses: 0,
                accepted_chunks: 0,
                completed_responses: 0,
                completed_receipt_replays: 0,
                denied: 0,
                failed: 0,
                quota_denials: 0,
                gc_runs: 0,
                gc_removed_incomplete: 0,
                gc_removed_completed: 0,
                gc_reclaimed_partial_bytes: 0,
                overflowed: false,
            })),
        };
        receiver.collect_garbage(started_at)?;
        Ok(receiver)
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
        self.handle_at(
            authenticated_source,
            local_target,
            request,
            receiver_now_millis(),
        )
    }

    pub fn handle_at(
        &self,
        authenticated_source: &NodeId,
        local_target: &NodeId,
        request: ArtifactTransferRpc,
        now: u64,
    ) -> Result<ArtifactTransferRpcResult> {
        let kind = match &request.operation {
            ArtifactTransferOperation::Begin { .. } => ArtifactTransferRequestKind::Begin,
            ArtifactTransferOperation::Chunk { .. } => ArtifactTransferRequestKind::Chunk,
            ArtifactTransferOperation::Complete { .. } => ArtifactTransferRequestKind::Complete,
        };
        self.record_request(kind)?;
        let result = (|| {
            request.validate()?;
            match request.operation {
                ArtifactTransferOperation::Begin { manifest } => {
                    let _lifecycle = self.lifecycle.write().map_err(lock_error)?;
                    let lock = self.session_lock(&manifest.manifest_digest)?;
                    let _session = lock.lock().map_err(lock_error)?;
                    self.begin(authenticated_source, local_target, *manifest, now)
                }
                ArtifactTransferOperation::Chunk {
                    manifest_digest,
                    sha256,
                    offset,
                    bytes,
                    ..
                } => {
                    let _lifecycle = self.lifecycle.read().map_err(lock_error)?;
                    let lock = self.session_lock(&manifest_digest)?;
                    let _session = lock.lock().map_err(lock_error)?;
                    self.chunk(
                        (authenticated_source, local_target),
                        &manifest_digest,
                        &sha256,
                        offset,
                        &bytes,
                        now,
                    )
                }
                ArtifactTransferOperation::Complete {
                    manifest_digest,
                    completed_at,
                } => {
                    let _lifecycle = self.lifecycle.read().map_err(lock_error)?;
                    let lock = self.session_lock(&manifest_digest)?;
                    let _session = lock.lock().map_err(lock_error)?;
                    self.complete(
                        authenticated_source,
                        local_target,
                        &manifest_digest,
                        completed_at,
                        now,
                    )
                }
            }
        })();
        self.record_result(&result)?;
        result
    }

    fn begin(
        &self,
        source: &NodeId,
        target: &NodeId,
        manifest: ArtifactTransferManifest,
        now: u64,
    ) -> Result<ArtifactTransferRpcResult> {
        validate_peers(&manifest, source, target)?;
        self.collect_garbage_locked(now)?;
        let directory = self.session_directory(&manifest.manifest_digest);
        if !directory.exists() {
            self.admit(&manifest, now)?;
        }
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
        self.touch_session(&directory, &manifest.manifest_digest, now, false)?;
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
        peers: (&NodeId, &NodeId),
        manifest_digest: &str,
        sha256: &str,
        offset: u64,
        bytes: &[u8],
        now: u64,
    ) -> Result<ArtifactTransferRpcResult> {
        let directory = self.session_directory(manifest_digest);
        let manifest = read_manifest(&directory.join("manifest.json"))?;
        validate_peers(&manifest, peers.0, peers.1)?;
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
        self.touch_session(&directory, manifest_digest, now, false)?;
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
        now: u64,
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
            self.touch_session(&directory, manifest_digest, now, true)?;
            self.record_receipt_replay()?;
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
        self.touch_session(&directory, manifest_digest, now, true)?;
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
        let next_offset = match fs::symlink_metadata(&part) {
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

    fn session_lock(&self, manifest_digest: &str) -> Result<Arc<Mutex<()>>> {
        let mut locks = self.session_locks.lock().map_err(lock_error)?;
        locks.retain(|_, lock| lock.strong_count() > 0);
        if let Some(lock) = locks.get(manifest_digest).and_then(Weak::upgrade) {
            return Ok(lock);
        }
        let lock = Arc::new(Mutex::new(()));
        locks.insert(manifest_digest.to_owned(), Arc::downgrade(&lock));
        Ok(lock)
    }

    fn touch_session(
        &self,
        directory: &Path,
        manifest_digest: &str,
        now: u64,
        completed: bool,
    ) -> Result<()> {
        let path = directory.join(TRANSFER_SESSION_STATE_FILE);
        let mut state = if path.exists() {
            let state: ArtifactTransferSessionState = read_json(&path, "artifact session state")?;
            state.validate(manifest_digest)?;
            state
        } else {
            ArtifactTransferSessionState::active(manifest_digest.to_owned(), now)
        };
        state.last_activity_at = now;
        if completed && state.completed_at.is_none() {
            state.completed_at = Some(now);
        }
        publish_json(&path, &state)
    }

    fn admit(&self, manifest: &ArtifactTransferManifest, now: u64) -> Result<()> {
        let entries = self.session_entries(now)?;
        let inventory = summarize_sessions(&entries)?;
        if inventory.active_sessions >= self.policy.max_active_sessions {
            self.record_quota_denial()?;
            return Err(ClusterError::Unavailable(format!(
                "artifact receiver active-session quota {} is exhausted",
                self.policy.max_active_sessions
            )));
        }
        let reservation = self.manifest_reserved_bytes(manifest)?;
        let next = inventory
            .reserved_bytes
            .checked_add(reservation)
            .ok_or_else(|| ClusterError::Unavailable("artifact reservation overflowed".into()))?;
        if next > self.policy.max_reserved_bytes {
            self.record_quota_denial()?;
            return Err(ClusterError::Unavailable(format!(
                "artifact receiver reserved-byte quota {} would be exceeded",
                self.policy.max_reserved_bytes
            )));
        }
        Ok(())
    }

    fn manifest_reserved_bytes(&self, manifest: &ArtifactTransferManifest) -> Result<u64> {
        distinct_objects(manifest)
            .into_iter()
            .try_fold(0u64, |total, (sha256, length)| {
                let needed = match self.store.verify(&sha256) {
                    Ok(verified) if verified.length == length => 0,
                    Ok(verified) => {
                        return Err(ClusterError::Invalid(format!(
                            "target object {sha256} has length {}, expected {length}",
                            verified.length
                        )))
                    }
                    Err(StoreError::ObjectMissing(_)) | Err(StoreError::ObjectCorrupt { .. }) => {
                        length
                    }
                    Err(error) => return Err(cluster_store_error(error)),
                };
                total.checked_add(needed).ok_or_else(|| {
                    ClusterError::Invalid("artifact manifest reservation overflowed".into())
                })
            })
    }

    pub fn session_inventory(&self, now: u64) -> Result<ArtifactTransferSessionInventory> {
        let _lifecycle = self.lifecycle.write().map_err(lock_error)?;
        summarize_sessions(&self.session_entries(now)?)
    }

    pub fn telemetry_snapshot(&self, now: u64) -> Result<ArtifactTransferTelemetrySnapshot> {
        let started_at = self.telemetry.lock().map_err(lock_error)?.started_at;
        if now < started_at {
            return Err(ClusterError::Invalid(
                "artifact telemetry observation predates this receiver process".into(),
            ));
        }
        let inventory = self.session_inventory(now)?;
        let state = self.telemetry.lock().map_err(lock_error)?;
        Ok(ArtifactTransferTelemetrySnapshot {
            contract_version: ARTIFACT_TRANSFER_TELEMETRY_VERSION,
            started_at: state.started_at,
            observed_at: now,
            policy: self.policy.clone(),
            inventory,
            begin_requests: state.begin_requests,
            chunk_requests: state.chunk_requests,
            complete_requests: state.complete_requests,
            begin_responses: state.begin_responses,
            accepted_chunks: state.accepted_chunks,
            completed_responses: state.completed_responses,
            completed_receipt_replays: state.completed_receipt_replays,
            denied: state.denied,
            failed: state.failed,
            quota_denials: state.quota_denials,
            gc_runs: state.gc_runs,
            gc_removed_incomplete: state.gc_removed_incomplete,
            gc_removed_completed: state.gc_removed_completed,
            gc_reclaimed_partial_bytes: state.gc_reclaimed_partial_bytes,
            overflowed: state.overflowed,
        })
    }

    pub fn collect_garbage(&self, now: u64) -> Result<ArtifactTransferGcReport> {
        let _lifecycle = self.lifecycle.write().map_err(lock_error)?;
        self.collect_garbage_locked(now)
    }

    fn collect_garbage_locked(&self, now: u64) -> Result<ArtifactTransferGcReport> {
        let entries = self.session_entries(now)?;
        let scanned_sessions = entries.len();
        let mut remove = BTreeSet::new();
        for entry in &entries {
            if !entry.complete
                && now.saturating_sub(entry.state.last_activity_at)
                    >= self.policy.stale_incomplete_after_millis
            {
                remove.insert(entry.digest.clone());
            }
            if entry.complete
                && now.saturating_sub(
                    entry
                        .state
                        .completed_at
                        .unwrap_or(entry.state.last_activity_at),
                ) >= self.policy.completed_receipt_retention_millis
            {
                remove.insert(entry.digest.clone());
            }
        }
        let mut retained = entries
            .iter()
            .filter(|entry| entry.complete && !remove.contains(&entry.digest))
            .collect::<Vec<_>>();
        retained.sort_by_key(|entry| {
            (
                entry
                    .state
                    .completed_at
                    .unwrap_or(entry.state.last_activity_at),
                entry.digest.as_str(),
            )
        });
        let excess = retained
            .len()
            .saturating_sub(self.policy.max_retained_receipts);
        for entry in retained.into_iter().take(excess) {
            remove.insert(entry.digest.clone());
        }

        let mut removed_incomplete = 0usize;
        let mut removed_completed = 0usize;
        let mut reclaimed_partial_bytes = 0u64;
        for entry in entries
            .iter()
            .filter(|entry| remove.contains(&entry.digest))
        {
            if entry.complete {
                removed_completed += 1;
            } else {
                removed_incomplete += 1;
            }
            reclaimed_partial_bytes = reclaimed_partial_bytes
                .checked_add(entry.partial_bytes)
                .ok_or_else(|| {
                    ClusterError::Unavailable("artifact GC byte count overflowed".into())
                })?;
            remove_session_directory(&self.sessions, &entry.path, &entry.digest)?;
        }
        if !remove.is_empty() {
            sync_directory(&self.sessions)?;
            let mut locks = self.session_locks.lock().map_err(lock_error)?;
            for digest in &remove {
                locks.remove(digest);
            }
        }
        let remaining = summarize_sessions(&self.session_entries(now)?)?;
        let report = ArtifactTransferGcReport {
            scanned_sessions,
            removed_incomplete,
            removed_completed,
            reclaimed_partial_bytes,
            remaining,
        };
        self.record_gc(&report)?;
        Ok(report)
    }

    fn session_entries(&self, now: u64) -> Result<Vec<SessionInventoryEntry>> {
        let mut entries = Vec::new();
        for entry in fs::read_dir(&self.sessions).map_err(cluster_io_error)? {
            let entry = entry.map_err(cluster_io_error)?;
            let file_type = entry.file_type().map_err(cluster_io_error)?;
            let digest = entry
                .file_name()
                .into_string()
                .map_err(|_| ClusterError::Denied("artifact session name is not UTF-8".into()))?;
            if !file_type.is_dir() || !is_sha256(&digest) {
                return Err(ClusterError::Denied(
                    "artifact session root contains an unexpected entry".into(),
                ));
            }
            let path = entry.path();
            let manifest = read_manifest(&path.join("manifest.json"))?;
            if manifest.manifest_digest != digest {
                return Err(ClusterError::Denied(
                    "artifact session directory differs from its manifest".into(),
                ));
            }
            let receipt_path = path.join("receipt.json");
            let complete = match fs::symlink_metadata(&receipt_path) {
                Ok(metadata) if metadata.file_type().is_file() => true,
                Ok(_) => {
                    return Err(ClusterError::Denied(
                        "artifact receipt is not a regular file".into(),
                    ))
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
                Err(error) => return Err(cluster_io_error(error)),
            };
            if complete {
                let receipt: ArtifactTransferReceipt =
                    read_json(&receipt_path, "artifact receipt")?;
                receipt.validate(&manifest)?;
            }
            let state_path = path.join(TRANSFER_SESSION_STATE_FILE);
            let state_existed = state_path.exists();
            let mut state_changed = !state_existed;
            let mut state = if state_existed {
                let state: ArtifactTransferSessionState =
                    read_json(&state_path, "artifact session state")?;
                state.validate(&digest)?;
                state
            } else {
                ArtifactTransferSessionState::active(digest.clone(), now)
            };
            if complete && state.completed_at.is_none() {
                state.completed_at = Some(now);
                state_changed = true;
            } else if !complete && state.completed_at.is_some() {
                return Err(ClusterError::Denied(
                    "artifact session claims completion without a receipt".into(),
                ));
            }
            if state_changed {
                publish_json(&state_path, &state)?;
            }

            let expected = distinct_objects(&manifest);
            let mut partial_bytes = 0u64;
            for child in fs::read_dir(&path).map_err(cluster_io_error)? {
                let child = child.map_err(cluster_io_error)?;
                let name = child.file_name().into_string().map_err(|_| {
                    ClusterError::Denied("artifact session entry is not UTF-8".into())
                })?;
                let child_type = child.file_type().map_err(cluster_io_error)?;
                if name == "manifest.json"
                    || name == "receipt.json"
                    || name == TRANSFER_SESSION_STATE_FILE
                    || (name.ends_with(".transferred")
                        && expected.contains_key(name.trim_end_matches(".transferred")))
                {
                    if !child_type.is_file() {
                        return Err(ClusterError::Denied(
                            "artifact session metadata is not a regular file".into(),
                        ));
                    }
                    continue;
                }
                if let Some(sha256) = name.strip_suffix(".part") {
                    let Some(expected_length) = expected.get(sha256) else {
                        return Err(ClusterError::Denied(
                            "artifact session part is absent from its manifest".into(),
                        ));
                    };
                    let metadata = child.metadata().map_err(cluster_io_error)?;
                    if !child_type.is_file() || metadata.len() > *expected_length {
                        return Err(ClusterError::Denied(
                            "artifact session part is outside its manifest bound".into(),
                        ));
                    }
                    partial_bytes = partial_bytes.checked_add(metadata.len()).ok_or_else(|| {
                        ClusterError::Unavailable("artifact partial byte count overflowed".into())
                    })?;
                    continue;
                }
                if name.starts_with('.') && name.ends_with(".pending") && child_type.is_file() {
                    fs::remove_file(child.path()).map_err(cluster_io_error)?;
                    continue;
                }
                return Err(ClusterError::Denied(
                    "artifact session contains an unexpected entry".into(),
                ));
            }
            if complete && partial_bytes != 0 {
                return Err(ClusterError::Denied(
                    "completed artifact session retains partial bytes".into(),
                ));
            }
            let reserved_bytes = if complete {
                0
            } else {
                self.manifest_reserved_bytes(&manifest)?
            };
            entries.push(SessionInventoryEntry {
                digest,
                path,
                state,
                complete,
                reserved_bytes,
                partial_bytes,
            });
        }
        entries.sort_by(|left, right| left.digest.cmp(&right.digest));
        Ok(entries)
    }

    fn record_request(&self, kind: ArtifactTransferRequestKind) -> Result<()> {
        let mut state = self.telemetry.lock().map_err(lock_error)?;
        let mut overflowed = state.overflowed;
        let counter = match kind {
            ArtifactTransferRequestKind::Begin => &mut state.begin_requests,
            ArtifactTransferRequestKind::Chunk => &mut state.chunk_requests,
            ArtifactTransferRequestKind::Complete => &mut state.complete_requests,
        };
        telemetry_add(counter, 1, &mut overflowed);
        state.overflowed = overflowed;
        Ok(())
    }

    fn record_result(&self, result: &Result<ArtifactTransferRpcResult>) -> Result<()> {
        let mut state = self.telemetry.lock().map_err(lock_error)?;
        let mut overflowed = state.overflowed;
        match result {
            Ok(ArtifactTransferRpcResult::Progress { .. }) => {
                telemetry_add(&mut state.begin_responses, 1, &mut overflowed)
            }
            Ok(ArtifactTransferRpcResult::ChunkAccepted { .. }) => {
                telemetry_add(&mut state.accepted_chunks, 1, &mut overflowed)
            }
            Ok(ArtifactTransferRpcResult::Completed { .. }) => {
                telemetry_add(&mut state.completed_responses, 1, &mut overflowed)
            }
            Err(ClusterError::Invalid(_) | ClusterError::Denied(_)) => {
                telemetry_add(&mut state.denied, 1, &mut overflowed)
            }
            Err(_) => telemetry_add(&mut state.failed, 1, &mut overflowed),
        }
        state.overflowed = overflowed;
        Ok(())
    }

    fn record_receipt_replay(&self) -> Result<()> {
        let mut state = self.telemetry.lock().map_err(lock_error)?;
        let mut overflowed = state.overflowed;
        telemetry_add(&mut state.completed_receipt_replays, 1, &mut overflowed);
        state.overflowed = overflowed;
        Ok(())
    }

    fn record_quota_denial(&self) -> Result<()> {
        let mut state = self.telemetry.lock().map_err(lock_error)?;
        let mut overflowed = state.overflowed;
        telemetry_add(&mut state.quota_denials, 1, &mut overflowed);
        state.overflowed = overflowed;
        Ok(())
    }

    fn record_gc(&self, report: &ArtifactTransferGcReport) -> Result<()> {
        let mut state = self.telemetry.lock().map_err(lock_error)?;
        let mut overflowed = state.overflowed;
        telemetry_add(&mut state.gc_runs, 1, &mut overflowed);
        telemetry_add(
            &mut state.gc_removed_incomplete,
            report.removed_incomplete as u64,
            &mut overflowed,
        );
        telemetry_add(
            &mut state.gc_removed_completed,
            report.removed_completed as u64,
            &mut overflowed,
        );
        telemetry_add(
            &mut state.gc_reclaimed_partial_bytes,
            report.reclaimed_partial_bytes,
            &mut overflowed,
        );
        state.overflowed = overflowed;
        Ok(())
    }
}

fn distinct_objects(manifest: &ArtifactTransferManifest) -> BTreeMap<String, u64> {
    manifest
        .objects
        .iter()
        .map(|object| (object.sha256.clone(), object.length))
        .collect()
}

fn summarize_sessions(
    entries: &[SessionInventoryEntry],
) -> Result<ArtifactTransferSessionInventory> {
    let mut active_sessions = 0usize;
    let mut reserved_bytes = 0u64;
    let mut partial_bytes = 0u64;
    let mut retained_receipts = 0usize;
    for entry in entries {
        if entry.complete {
            retained_receipts = retained_receipts.checked_add(1).ok_or_else(|| {
                ClusterError::Unavailable("artifact receipt count overflowed".into())
            })?;
        } else {
            active_sessions = active_sessions.checked_add(1).ok_or_else(|| {
                ClusterError::Unavailable("artifact session count overflowed".into())
            })?;
            reserved_bytes = reserved_bytes
                .checked_add(entry.reserved_bytes)
                .ok_or_else(|| {
                    ClusterError::Unavailable("artifact reserved byte count overflowed".into())
                })?;
        }
        partial_bytes = partial_bytes
            .checked_add(entry.partial_bytes)
            .ok_or_else(|| {
                ClusterError::Unavailable("artifact partial byte count overflowed".into())
            })?;
    }
    Ok(ArtifactTransferSessionInventory {
        active_sessions,
        reserved_bytes,
        partial_bytes,
        retained_receipts,
    })
}

fn remove_session_directory(root: &Path, path: &Path, digest: &str) -> Result<()> {
    if !is_sha256(digest)
        || path.parent() != Some(root)
        || path.file_name() != Some(digest.as_ref())
    {
        return Err(ClusterError::Denied(
            "artifact GC target is outside the session root".into(),
        ));
    }
    let metadata = fs::symlink_metadata(path).map_err(cluster_io_error)?;
    if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
        return Err(ClusterError::Denied(
            "artifact GC target is not a session directory".into(),
        ));
    }
    fs::remove_dir_all(path).map_err(cluster_io_error)
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn receiver_now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn lock_error<T>(error: std::sync::PoisonError<T>) -> ClusterError {
    ClusterError::Unavailable(format!("artifact session lock was poisoned: {error}"))
}

fn telemetry_add(counter: &mut u64, value: u64, overflowed: &mut bool) {
    match counter.checked_add(value) {
        Some(next) => *counter = next,
        None => {
            *counter = u64::MAX;
            *overflowed = true;
        }
    }
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
    let metadata = fs::symlink_metadata(path).map_err(|error| {
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
