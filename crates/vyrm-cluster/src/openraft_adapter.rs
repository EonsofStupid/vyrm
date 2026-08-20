//! OpenRaft adapter over the Vyrm-native durable key/value substrate.
//!
//! Adapter v4 physically separates node-local Raft history from transferable
//! canonical state. Votes, logs, purge/commit cursors, and snapshot cache
//! references stay local; state-machine application and canonical runtime data
//! share one authoritative VyrmKV WAL frame.

// OpenRaft fixes `StorageError` as the error type in its storage traits. The
// enum deliberately carries rich Raft context and is larger than Clippy's
// generic threshold, so adapter helpers preserve that required type too.
#![allow(clippy::result_large_err)]

use crate::{
    transfer_artifacts, ArtifactTransferManifest, ArtifactTransferReceipt, ClusterError, NodeId,
    ReplicaTransferPlan, Result as ClusterResult, ShardId, ShardPlacement, ShardReadStamp,
    CLUSTER_CONTRACT_VERSION,
};
use openraft::storage::{RaftLogStorage, RaftStateMachine};
use openraft::{
    Entry, EntryPayload, ErrorSubject, ErrorVerb, LogId, LogState, RaftLogReader,
    RaftSnapshotBuilder, RaftTypeConfig, Snapshot, SnapshotMeta, StorageError, StoredMembership,
    Vote,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Debug;
use std::io;
use std::ops::{Bound, RangeBounds};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncSeek, AsyncWrite, ReadBuf};
use vyrm_core::{digest::sha256_hex, ObjectReference, RuntimeCommit, RuntimeCommitOutcome};
use vyrm_kv::{
    Database, Durability, Mutation, SnapshotBundleFile, WriteBatch, SNAPSHOT_BUNDLE_MAX_BYTES,
};
use vyrm_store::{
    native_runtime_commit_outcome, native_snapshot_all_object_references,
    native_snapshot_artifact_view, native_snapshot_object_references,
    prepare_native_runtime_commit, Error as StoreError, LocalObjectStore,
};

const ADAPTER_FORMAT_VERSION: u16 = 4;
const LOCAL_DATABASE_DIRECTORY: &str = "raft-local-v4";
const SNAPSHOT_OBJECT_DIRECTORY: &str = "snapshot-objects";
const SNAPSHOT_SPOOL_DIRECTORY: &str = "snapshot-spool";
pub const APPLICATION_OBJECT_DIRECTORY: &str = "application-objects";
const KEY_STATE_CONFIG: &[u8] = b"vyrm/raft/v4/state/config";
const KEY_LOCAL_CONFIG: &[u8] = b"vyrm/raft/v4/local/config";
const KEY_VOTE: &[u8] = b"vyrm/raft/v4/local/vote";
const KEY_COMMITTED: &[u8] = b"vyrm/raft/v4/local/committed";
const KEY_PURGED: &[u8] = b"vyrm/raft/v4/local/purged";
const KEY_STATE: &[u8] = b"vyrm/raft/v4/state/current";
const KEY_SNAPSHOT: &[u8] = b"vyrm/raft/v4/local/snapshot";
const LOG_PREFIX: &[u8] = b"vyrm/raft/v4/local/log/";
const SNAPSHOT_MEDIA_TYPE: &str = "application/vnd.vyrm.raft-state-snapshot.v4";
pub const VYRM_RAFT_REQUEST_RETENTION_LOGS: u64 = 4096;
// JSON is only the correctness-gate codec. Keep worst-case byte-array expansion
// comfortably below VyrmKV's 8 MiB value ceiling until a compact codec lands.
const MAX_COMMAND_BYTES: usize = 1024 * 1024;
static SNAPSHOT_SPOOL_ORDINAL: AtomicU64 = AtomicU64::new(1);

/// File-backed OpenRaft snapshot handle with bounded writes and explicit
/// ephemeral cleanup. Durable object handles opt out of deletion.
#[derive(Debug)]
pub struct VyrmSnapshotData {
    // Declaration order is intentional: close the handle before cleanup tries
    // to unlink the ephemeral path, including on Windows.
    file: tokio::fs::File,
    path: PathBuf,
    position: u64,
    _cleanup: SnapshotCleanup,
}

#[derive(Debug)]
struct SnapshotCleanup(Option<PathBuf>);

impl Drop for SnapshotCleanup {
    fn drop(&mut self) {
        if let Some(path) = &self.0 {
            let _ = std::fs::remove_file(path);
        }
    }
}

impl VyrmSnapshotData {
    pub async fn create_ephemeral(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let file = tokio::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .await?;
        Ok(Self {
            file,
            _cleanup: SnapshotCleanup(Some(path.clone())),
            path,
            position: 0,
        })
    }

    async fn open(path: impl AsRef<Path>, delete_on_drop: bool) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let file = tokio::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .await?;
        Ok(Self {
            file,
            _cleanup: SnapshotCleanup(delete_on_drop.then(|| path.clone())),
            path,
            position: 0,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    async fn sync_all(&self) -> io::Result<()> {
        self.file.sync_all().await
    }
}

impl AsyncRead for VyrmSnapshotData {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let before = buffer.filled().len();
        let result = Pin::new(&mut self.file).poll_read(context, buffer);
        if let Poll::Ready(Ok(())) = &result {
            self.position = self
                .position
                .saturating_add((buffer.filled().len() - before) as u64);
        }
        result
    }
}

impl AsyncWrite for VyrmSnapshotData {
    fn poll_write(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        if self
            .position
            .checked_add(buffer.len() as u64)
            .is_none_or(|end| end > SNAPSHOT_BUNDLE_MAX_BYTES)
        {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::FileTooLarge,
                format!("snapshot exceeds {SNAPSHOT_BUNDLE_MAX_BYTES} bytes"),
            )));
        }
        let result = Pin::new(&mut self.file).poll_write(context, buffer);
        if let Poll::Ready(Ok(written)) = result {
            self.position += written as u64;
            Poll::Ready(Ok(written))
        } else {
            result
        }
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut self.file).poll_flush(context)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut self.file).poll_shutdown(context)
    }
}

impl AsyncSeek for VyrmSnapshotData {
    fn start_seek(mut self: Pin<&mut Self>, position: std::io::SeekFrom) -> io::Result<()> {
        Pin::new(&mut self.file).start_seek(position)
    }

    fn poll_complete(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<u64>> {
        let result = Pin::new(&mut self.file).poll_complete(context);
        if let Poll::Ready(Ok(position)) = result {
            self.position = position;
            Poll::Ready(Ok(position))
        } else {
            result
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VyrmRaftNode {
    pub canonical_id: String,
    pub zone: String,
    pub endpoint: String,
}

impl VyrmRaftNode {
    pub fn validate(&self) -> ClusterResult<()> {
        for (label, value) in [
            ("canonical id", self.canonical_id.as_str()),
            ("zone", self.zone.as_str()),
            ("endpoint", self.endpoint.as_str()),
        ] {
            if value.is_empty() || value.len() > 512 || value.as_bytes().contains(&0) {
                return Err(ClusterError::Invalid(format!(
                    "raft node {label} must contain 1..=512 non-NUL bytes"
                )));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum VyrmRaftOperation {
    PlacementTransition {
        placement: ShardPlacement,
    },
    Probe {
        payload_digest: String,
        payload: Vec<u8>,
    },
    RuntimeCommit {
        commit: RuntimeCommit,
    },
}

impl VyrmRaftOperation {
    fn validate(&self) -> ClusterResult<()> {
        match self {
            Self::PlacementTransition { placement } => placement.validate(),
            Self::Probe {
                payload_digest,
                payload,
            } if !payload.is_empty() && payload_digest == &sha256_hex(payload) => Ok(()),
            Self::Probe { .. } => Err(ClusterError::Invalid(
                "raft probe payload or digest is invalid".into(),
            )),
            Self::RuntimeCommit { commit } => commit
                .validate()
                .map_err(|error| ClusterError::Invalid(error.to_string())),
        }
    }

    fn digest(&self) -> String {
        let mut bytes = b"vyrm.raft.operation.v2".to_vec();
        match self {
            Self::PlacementTransition { placement } => {
                bytes.push(1);
                bytes.extend_from_slice(
                    placement
                        .digest()
                        .expect("validated placement has a canonical digest")
                        .as_bytes(),
                );
            }
            Self::Probe { payload_digest, .. } => {
                bytes.push(2);
                bytes.extend_from_slice(payload_digest.as_bytes());
            }
            Self::RuntimeCommit { commit } => {
                bytes.push(3);
                bytes.extend_from_slice(commit.digest().as_bytes());
            }
        }
        sha256_hex(&bytes)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VyrmRaftCommand {
    pub request_id: String,
    pub shard: ShardId,
    pub placement_epoch: u64,
    pub expected_commit_index: Option<u64>,
    pub operation: VyrmRaftOperation,
}

impl VyrmRaftCommand {
    pub fn placement_transition(
        request_id: impl Into<String>,
        placement: ShardPlacement,
        expected_commit_index: Option<u64>,
    ) -> ClusterResult<Self> {
        let command = Self {
            request_id: request_id.into(),
            shard: placement.shard,
            placement_epoch: placement.epoch,
            expected_commit_index,
            operation: VyrmRaftOperation::PlacementTransition { placement },
        };
        command.validate()?;
        Ok(command)
    }

    pub fn new(
        request_id: impl Into<String>,
        shard: ShardId,
        placement_epoch: u64,
        expected_commit_index: Option<u64>,
        payload: Vec<u8>,
    ) -> ClusterResult<Self> {
        let command = Self {
            request_id: request_id.into(),
            shard,
            placement_epoch,
            expected_commit_index,
            operation: VyrmRaftOperation::Probe {
                payload_digest: sha256_hex(&payload),
                payload,
            },
        };
        command.validate()?;
        Ok(command)
    }

    pub fn runtime_commit(
        request_id: impl Into<String>,
        shard: ShardId,
        placement_epoch: u64,
        expected_commit_index: Option<u64>,
        commit: RuntimeCommit,
    ) -> ClusterResult<Self> {
        let command = Self {
            request_id: request_id.into(),
            shard,
            placement_epoch,
            expected_commit_index,
            operation: VyrmRaftOperation::RuntimeCommit { commit },
        };
        command.validate()?;
        Ok(command)
    }

    pub fn validate(&self) -> ClusterResult<()> {
        if self.request_id.is_empty()
            || self.request_id.len() > 256
            || self.request_id.as_bytes().contains(&0)
            || self.placement_epoch == 0
        {
            return Err(ClusterError::Invalid(
                "raft command identity or placement epoch is invalid".into(),
            ));
        }
        self.operation.validate()?;
        if let VyrmRaftOperation::PlacementTransition { placement } = &self.operation {
            if placement.shard != self.shard || placement.epoch != self.placement_epoch {
                return Err(ClusterError::Invalid(
                    "placement transition command and placement identity differ".into(),
                ));
            }
        }
        let encoded = serde_json::to_vec(&self.operation)
            .map_err(|error| ClusterError::Invalid(error.to_string()))?;
        if encoded.len() > MAX_COMMAND_BYTES {
            return Err(ClusterError::Invalid(format!(
                "raft command operation exceeds {MAX_COMMAND_BYTES} encoded bytes"
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VyrmRaftResponse {
    pub accepted: bool,
    pub duplicate: bool,
    pub term: u64,
    pub index: u64,
    pub state_digest: String,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_outcome: Option<RuntimeCommitOutcome>,
}

openraft::declare_raft_types!(
    pub VyrmRaftTypeConfig:
        D = VyrmRaftCommand,
        R = VyrmRaftResponse,
        NodeId = u64,
        Node = VyrmRaftNode,
        SnapshotData = VyrmSnapshotData,
);

pub type VyrmRaftEntry = Entry<VyrmRaftTypeConfig>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AdapterConfig {
    format_version: u16,
    shard: ShardId,
    domain: AdapterDomain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum AdapterDomain {
    CanonicalState,
    LocalRaft,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AppliedRequest {
    first_applied_index: u64,
    command_digest: String,
    response: VyrmRaftResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StateMachineData {
    last_applied: Option<LogId<u64>>,
    last_membership: StoredMembership<u64, VyrmRaftNode>,
    #[serde(default)]
    placement_epoch: Option<u64>,
    #[serde(default)]
    placement_digest: Option<String>,
    #[serde(default)]
    placement_membership_digest: Option<String>,
    #[serde(default)]
    voter_membership_generation: u64,
    #[serde(default)]
    placement_membership_generation: Option<u64>,
    #[serde(default)]
    runtime_commit_count: u64,
    state_digest: String,
    requests: BTreeMap<String, AppliedRequest>,
}

impl Default for StateMachineData {
    fn default() -> Self {
        Self {
            last_applied: None,
            last_membership: StoredMembership::default(),
            placement_epoch: None,
            placement_digest: None,
            placement_membership_digest: None,
            voter_membership_generation: 0,
            placement_membership_generation: None,
            runtime_commit_count: 0,
            state_digest: sha256_hex(b"vyrm.raft.state.v1"),
            requests: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredSnapshot {
    meta: SnapshotMeta<u64, VyrmRaftNode>,
    object: ObjectReference,
}

type SharedDatabase = Arc<Mutex<Database>>;

#[derive(Clone)]
pub struct VyrmRaftLogStore {
    local_database: SharedDatabase,
}

#[derive(Clone)]
pub struct VyrmRaftStateMachine {
    state_database: SharedDatabase,
    local_database: SharedDatabase,
    snapshot_objects: LocalObjectStore,
    application_objects: LocalObjectStore,
    snapshot_spool: PathBuf,
    shard: ShardId,
}

pub struct VyrmRaftStore;

impl VyrmRaftStore {
    pub fn open(
        root: &Path,
        shard: ShardId,
    ) -> ClusterResult<(VyrmRaftLogStore, VyrmRaftStateMachine)> {
        let mut state_database = open_adapter_database(root)?;
        ensure_adapter_database(
            &mut state_database,
            KEY_STATE_CONFIG,
            AdapterConfig {
                format_version: ADAPTER_FORMAT_VERSION,
                shard,
                domain: AdapterDomain::CanonicalState,
            },
            Some(put_json(KEY_STATE, &StateMachineData::default())?),
        )?;

        let local_root = root.join(LOCAL_DATABASE_DIRECTORY);
        let mut local_database = open_adapter_database(&local_root)?;
        ensure_adapter_database(
            &mut local_database,
            KEY_LOCAL_CONFIG,
            AdapterConfig {
                format_version: ADAPTER_FORMAT_VERSION,
                shard,
                domain: AdapterDomain::LocalRaft,
            },
            None,
        )?;
        let snapshot_objects = LocalObjectStore::open(local_root.join(SNAPSHOT_OBJECT_DIRECTORY))
            .map_err(|error| ClusterError::Unavailable(error.to_string()))?;
        let application_objects = LocalObjectStore::open(root.join(APPLICATION_OBJECT_DIRECTORY))
            .map_err(|error| ClusterError::Unavailable(error.to_string()))?;
        let snapshot_spool = local_root.join(SNAPSHOT_SPOOL_DIRECTORY);
        clean_snapshot_spool(&snapshot_spool)?;

        let database = Arc::new(Mutex::new(state_database));
        let local_database = Arc::new(Mutex::new(local_database));
        Ok((
            VyrmRaftLogStore {
                local_database: local_database.clone(),
            },
            VyrmRaftStateMachine {
                state_database: database,
                local_database,
                snapshot_objects,
                application_objects,
                snapshot_spool,
                shard,
            },
        ))
    }
}

fn open_adapter_database(root: &Path) -> ClusterResult<Database> {
    if root.join("CURRENT").is_file() {
        return Database::open(root).map_err(|error| ClusterError::Unavailable(error.to_string()));
    }
    if root.exists()
        && std::fs::read_dir(root)
            .map_err(|error| ClusterError::Unavailable(error.to_string()))?
            .next()
            .is_some()
    {
        return Err(ClusterError::Denied(format!(
            "adapter database {} has content but no authenticated VyrmKV CURRENT pointer",
            root.display()
        )));
    }
    Database::create(root).map_err(|error| ClusterError::Unavailable(error.to_string()))
}

fn clean_snapshot_spool(path: &Path) -> ClusterResult<()> {
    std::fs::create_dir_all(path).map_err(|error| ClusterError::Unavailable(error.to_string()))?;
    for entry in
        std::fs::read_dir(path).map_err(|error| ClusterError::Unavailable(error.to_string()))?
    {
        let entry = entry.map_err(|error| ClusterError::Unavailable(error.to_string()))?;
        let file_type = entry
            .file_type()
            .map_err(|error| ClusterError::Unavailable(error.to_string()))?;
        if !file_type.is_file() {
            return Err(ClusterError::Denied(format!(
                "snapshot spool contains unexpected non-file entry {}",
                entry.path().display()
            )));
        }
        std::fs::remove_file(entry.path())
            .map_err(|error| ClusterError::Unavailable(error.to_string()))?;
    }
    std::fs::File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| ClusterError::Unavailable(error.to_string()))
}

fn snapshot_spool_path(root: &Path, purpose: &str) -> PathBuf {
    root.join(format!(
        "{purpose}-{}-{}.spool",
        std::process::id(),
        SNAPSHOT_SPOOL_ORDINAL.fetch_add(1, Ordering::Relaxed)
    ))
}

fn ensure_adapter_database(
    database: &mut Database,
    config_key: &[u8],
    expected: AdapterConfig,
    initial: Option<Mutation>,
) -> ClusterResult<()> {
    let snapshot = database.snapshot();
    if let Some(bytes) = database
        .get(config_key, snapshot)
        .map_err(|error| ClusterError::Unavailable(error.to_string()))?
    {
        let actual: AdapterConfig = serde_json::from_slice(&bytes)
            .map_err(|error| ClusterError::Denied(error.to_string()))?;
        if actual != expected {
            return Err(ClusterError::Denied(
                "raft adapter format, shard binding, or storage domain does not match".into(),
            ));
        }
        if expected.domain == AdapterDomain::CanonicalState
            && database
                .get(KEY_STATE, snapshot)
                .map_err(|error| ClusterError::Unavailable(error.to_string()))?
                .is_none()
        {
            return Err(ClusterError::Denied(
                "canonical Raft state database has no state-machine record".into(),
            ));
        }
        return Ok(());
    }
    if snapshot.sequence != 0 {
        return Err(ClusterError::Denied(
            "existing database is not a Vyrm OpenRaft v4 storage domain".into(),
        ));
    }
    let mut operations = vec![put_json(config_key, &expected)?];
    operations.extend(initial);
    database
        .write_owned(
            WriteBatch::new(operations)
                .map_err(|error| ClusterError::Unavailable(error.to_string()))?,
            Durability::Authoritative,
        )
        .map_err(|error| ClusterError::Unavailable(error.to_string()))?;
    Ok(())
}

impl RaftLogReader<VyrmRaftTypeConfig> for VyrmRaftLogStore {
    async fn try_get_log_entries<RB: RangeBounds<u64> + Clone + Debug + Send>(
        &mut self,
        range: RB,
    ) -> std::result::Result<Vec<VyrmRaftEntry>, StorageError<u64>> {
        let database = lock_database(&self.local_database, ErrorSubject::Logs, ErrorVerb::Read)?;
        let rows = database
            .scan(
                LOG_PREFIX,
                prefix_end(LOG_PREFIX).as_deref(),
                database.snapshot(),
            )
            .map_err(|error| {
                storage_error(ErrorSubject::Logs, ErrorVerb::Read, error.to_string())
            })?;
        let mut entries = Vec::new();
        for (key, value) in rows {
            let index = decode_log_key(&key)?;
            if range_contains(&range, index) {
                entries.push(decode_json(
                    &value,
                    ErrorSubject::LogIndex(index),
                    ErrorVerb::Read,
                )?);
            }
        }
        Ok(entries)
    }
}

impl RaftLogStorage<VyrmRaftTypeConfig> for VyrmRaftLogStore {
    type LogReader = Self;

    async fn get_log_state(
        &mut self,
    ) -> std::result::Result<LogState<VyrmRaftTypeConfig>, StorageError<u64>> {
        let last_purged_log_id = read_json(&self.local_database, KEY_PURGED, ErrorSubject::Logs)?;
        let mut reader = self.clone();
        let last_present = reader
            .try_get_log_entries(..)
            .await?
            .into_iter()
            .next_back()
            .map(|entry| entry.log_id);
        Ok(LogState {
            last_log_id: last_present.or(last_purged_log_id),
            last_purged_log_id,
        })
    }

    async fn get_log_reader(&mut self) -> Self::LogReader {
        self.clone()
    }

    async fn save_vote(&mut self, vote: &Vote<u64>) -> std::result::Result<(), StorageError<u64>> {
        let mut database =
            lock_database(&self.local_database, ErrorSubject::Vote, ErrorVerb::Write)?;
        let current: Option<Vote<u64>> = database
            .get(KEY_VOTE, database.snapshot())
            .map_err(|error| storage_error(ErrorSubject::Vote, ErrorVerb::Read, error.to_string()))?
            .map(|bytes| decode_json(&bytes, ErrorSubject::Vote, ErrorVerb::Read))
            .transpose()?;
        if current
            .as_ref()
            .is_some_and(|current| vote.partial_cmp(current).is_none_or(|order| order.is_lt()))
        {
            return Err(storage_error(
                ErrorSubject::Vote,
                ErrorVerb::Write,
                "refusing to persist a regressing or incomparable vote",
            ));
        }
        let operation = put_json_storage(KEY_VOTE, vote, ErrorSubject::Vote)?;
        let batch = WriteBatch::new(vec![operation]).map_err(|error| {
            storage_error(ErrorSubject::Vote, ErrorVerb::Write, error.to_string())
        })?;
        database
            .write_owned(batch, Durability::Authoritative)
            .map_err(|error| {
                storage_error(ErrorSubject::Vote, ErrorVerb::Write, error.to_string())
            })?;
        Ok(())
    }

    async fn read_vote(&mut self) -> std::result::Result<Option<Vote<u64>>, StorageError<u64>> {
        read_json(&self.local_database, KEY_VOTE, ErrorSubject::Vote)
    }

    async fn save_committed(
        &mut self,
        committed: Option<LogId<u64>>,
    ) -> std::result::Result<(), StorageError<u64>> {
        write_json(
            &self.local_database,
            KEY_COMMITTED,
            &committed,
            ErrorSubject::Logs,
        )
    }

    async fn read_committed(
        &mut self,
    ) -> std::result::Result<Option<LogId<u64>>, StorageError<u64>> {
        Ok(read_json::<Option<LogId<u64>>>(
            &self.local_database,
            KEY_COMMITTED,
            ErrorSubject::Logs,
        )?
        .flatten())
    }

    async fn append<I>(
        &mut self,
        entries: I,
        callback: openraft::storage::LogFlushed<VyrmRaftTypeConfig>,
    ) -> std::result::Result<(), StorageError<u64>>
    where
        I: IntoIterator<Item = VyrmRaftEntry> + Send,
        I::IntoIter: Send,
    {
        let entries: Vec<_> = entries.into_iter().collect();
        let result = persist_entries(&self.local_database, &entries);
        match result {
            Ok(()) => {
                callback.log_io_completed(Ok(()));
                Ok(())
            }
            Err(error) => {
                callback.log_io_completed(Err(std::io::Error::other(error.to_string())));
                Err(error)
            }
        }
    }

    async fn truncate(&mut self, log_id: LogId<u64>) -> std::result::Result<(), StorageError<u64>> {
        delete_log_range(&self.local_database, log_id.index..)
    }

    async fn purge(&mut self, log_id: LogId<u64>) -> std::result::Result<(), StorageError<u64>> {
        let mut operations = log_delete_operations(&self.local_database, ..=log_id.index)?;
        operations.push(put_json_storage(KEY_PURGED, &log_id, ErrorSubject::Logs)?);
        write_operations(&self.local_database, operations, ErrorSubject::Logs)
    }
}

impl RaftSnapshotBuilder<VyrmRaftTypeConfig> for VyrmRaftStateMachine {
    async fn build_snapshot(
        &mut self,
    ) -> std::result::Result<Snapshot<VyrmRaftTypeConfig>, StorageError<u64>> {
        let spool = snapshot_spool_path(&self.snapshot_spool, "build");
        let mut spool_cleanup = SnapshotCleanup(Some(spool.clone()));
        let (state, bundle) = {
            let mut database = lock_database(
                &self.state_database,
                ErrorSubject::Snapshot(None),
                ErrorVerb::Read,
            )?;
            let state = read_state_from_database(&database)?;
            let at = state.last_applied.map_or(0, |log_id| log_id.index);
            let bundle = database.export_snapshot_file(at, &spool).map_err(|error| {
                storage_error(
                    ErrorSubject::Snapshot(None),
                    ErrorVerb::Read,
                    error.to_string(),
                )
            })?;
            (state, bundle)
        };
        let snapshot_id = expected_snapshot_file_id(&state, &bundle);
        let meta = SnapshotMeta {
            last_log_id: state.last_applied,
            last_membership: state.last_membership,
            snapshot_id,
        };
        publish_snapshot_file(&self.local_database, &self.snapshot_objects, &meta, &bundle)?;
        let snapshot = VyrmSnapshotData::open(bundle.path(), true)
            .await
            .map_err(|error| {
                storage_error(
                    ErrorSubject::Snapshot(Some(meta.signature())),
                    ErrorVerb::Read,
                    error.to_string(),
                )
            })?;
        spool_cleanup.0.take();
        Ok(Snapshot {
            meta,
            snapshot: Box::new(snapshot),
        })
    }
}

impl RaftStateMachine<VyrmRaftTypeConfig> for VyrmRaftStateMachine {
    type SnapshotBuilder = Self;

    async fn applied_state(
        &mut self,
    ) -> std::result::Result<
        (Option<LogId<u64>>, StoredMembership<u64, VyrmRaftNode>),
        StorageError<u64>,
    > {
        let state = self.read_state()?;
        Ok((state.last_applied, state.last_membership))
    }

    async fn apply<I>(
        &mut self,
        entries: I,
    ) -> std::result::Result<Vec<VyrmRaftResponse>, StorageError<u64>>
    where
        I: IntoIterator<Item = VyrmRaftEntry> + Send,
        I::IntoIter: Send,
    {
        let mut database = lock_database(
            &self.state_database,
            ErrorSubject::StateMachine,
            ErrorVerb::Write,
        )?;
        let mut state = read_state_from_database(&database)?;
        let mut responses = Vec::new();
        for entry in entries {
            let mut operations = Vec::new();
            let response = match entry.payload {
                EntryPayload::Blank => blank_response(&state, entry.log_id),
                EntryPayload::Membership(membership) => {
                    let prior_binding = membership_binding(&state.last_membership)
                        .map(|voters| membership_binding_digest(&voters));
                    state.last_membership = StoredMembership::new(Some(entry.log_id), membership);
                    let next_binding = membership_binding(&state.last_membership)
                        .map(|voters| membership_binding_digest(&voters));
                    if prior_binding != next_binding {
                        state.voter_membership_generation = state
                            .voter_membership_generation
                            .checked_add(1)
                            .ok_or_else(|| {
                                storage_error(
                                    ErrorSubject::Apply(entry.log_id),
                                    ErrorVerb::Write,
                                    "voter membership generation overflowed",
                                )
                            })?;
                    }
                    blank_response(&state, entry.log_id)
                }
                EntryPayload::Normal(command) => {
                    let application =
                        apply_command(&database, self.shard, &mut state, entry.log_id, command)?;
                    operations.extend(application.operations);
                    application.response
                }
            };
            state.last_applied = Some(entry.log_id);
            prune_request_history(&mut state, entry.log_id.index);
            operations.push(put_json_storage(
                KEY_STATE,
                &state,
                ErrorSubject::StateMachine,
            )?);
            let batch = WriteBatch::new(operations).map_err(|error| {
                storage_error(
                    ErrorSubject::Apply(entry.log_id),
                    ErrorVerb::Write,
                    error.to_string(),
                )
            })?;
            database
                .write_owned(batch, Durability::Authoritative)
                .map_err(|error| {
                    storage_error(
                        ErrorSubject::Apply(entry.log_id),
                        ErrorVerb::Write,
                        error.to_string(),
                    )
                })?;
            responses.push(response);
        }
        Ok(responses)
    }

    async fn get_snapshot_builder(&mut self) -> Self::SnapshotBuilder {
        self.clone()
    }

    async fn begin_receiving_snapshot(
        &mut self,
    ) -> std::result::Result<
        Box<<VyrmRaftTypeConfig as RaftTypeConfig>::SnapshotData>,
        StorageError<u64>,
    > {
        let path = snapshot_spool_path(&self.snapshot_spool, "receive");
        VyrmSnapshotData::create_ephemeral(path)
            .await
            .map(Box::new)
            .map_err(|error| {
                storage_error(
                    ErrorSubject::Snapshot(None),
                    ErrorVerb::Write,
                    error.to_string(),
                )
            })
    }

    async fn install_snapshot(
        &mut self,
        meta: &SnapshotMeta<u64, VyrmRaftNode>,
        snapshot: Box<<VyrmRaftTypeConfig as RaftTypeConfig>::SnapshotData>,
    ) -> std::result::Result<(), StorageError<u64>> {
        let subject = ErrorSubject::Snapshot(Some(meta.signature()));
        snapshot
            .sync_all()
            .await
            .map_err(|error| storage_error(subject.clone(), ErrorVerb::Write, error.to_string()))?;
        let bundle = SnapshotBundleFile::open(snapshot.path())
            .map_err(|error| storage_error(subject.clone(), ErrorVerb::Read, error.to_string()))?;
        let state = state_from_snapshot_file(&bundle, self.shard, subject.clone())?;
        if state.last_applied != meta.last_log_id || state.last_membership != meta.last_membership {
            return Err(storage_error(
                subject,
                ErrorVerb::Write,
                "snapshot metadata does not match its state bytes",
            ));
        }
        if meta.snapshot_id != expected_snapshot_file_id(&state, &bundle) {
            return Err(storage_error(
                ErrorSubject::Snapshot(Some(meta.signature())),
                ErrorVerb::Write,
                "snapshot id does not bind the exact VyrmKV bundle",
            ));
        }
        let mut verified_digests = BTreeSet::new();
        for object in native_snapshot_all_object_references(&bundle)
            .map_err(|error| storage_error(subject.clone(), ErrorVerb::Read, error.to_string()))?
        {
            if !verified_digests.insert(object.sha256.clone()) {
                continue;
            }
            let verified = self
                .application_objects
                .verify(&object.sha256)
                .map_err(|error| {
                    storage_error(subject.clone(), ErrorVerb::Read, error.to_string())
                })?;
            if verified.length != object.length {
                return Err(storage_error(
                    subject.clone(),
                    ErrorVerb::Read,
                    "snapshot artifact length differs from its canonical reference",
                ));
            }
        }
        let at = meta.last_log_id.map_or(0, |log_id| log_id.index);
        lock_database(
            &self.state_database,
            ErrorSubject::Snapshot(Some(meta.signature())),
            ErrorVerb::Write,
        )?
        .install_snapshot_file(&bundle, at)
        .map_err(|error| {
            storage_error(
                ErrorSubject::Snapshot(Some(meta.signature())),
                ErrorVerb::Write,
                error.to_string(),
            )
        })?;
        publish_snapshot_file(&self.local_database, &self.snapshot_objects, meta, &bundle)
    }

    async fn get_current_snapshot(
        &mut self,
    ) -> std::result::Result<Option<Snapshot<VyrmRaftTypeConfig>>, StorageError<u64>> {
        let stored: Option<StoredSnapshot> = read_json(
            &self.local_database,
            KEY_SNAPSHOT,
            ErrorSubject::Snapshot(None),
        )?;
        let Some(snapshot) = stored else {
            return Ok(None);
        };
        let subject = ErrorSubject::Snapshot(Some(snapshot.meta.signature()));
        let path = self
            .snapshot_objects
            .verified_path(&snapshot.object)
            .map_err(|error| storage_error(subject.clone(), ErrorVerb::Read, error.to_string()))?;
        let bundle = SnapshotBundleFile::open(&path)
            .map_err(|error| storage_error(subject.clone(), ErrorVerb::Read, error.to_string()))?;
        let state = state_from_snapshot_file(&bundle, self.shard, subject.clone())?;
        if state.last_applied != snapshot.meta.last_log_id
            || state.last_membership != snapshot.meta.last_membership
            || snapshot.meta.snapshot_id != expected_snapshot_file_id(&state, &bundle)
        {
            return Err(storage_error(
                subject.clone(),
                ErrorVerb::Read,
                "cached snapshot metadata does not match its authenticated bundle",
            ));
        }
        let data = VyrmSnapshotData::open(path, false)
            .await
            .map_err(|error| storage_error(subject, ErrorVerb::Read, error.to_string()))?;
        Ok(Some(Snapshot {
            meta: snapshot.meta,
            snapshot: Box::new(data),
        }))
    }
}

impl VyrmRaftStateMachine {
    pub fn application_objects(&self) -> LocalObjectStore {
        self.application_objects.clone()
    }

    /// Returns the metadata of the exact authenticated snapshot persisted by
    /// this node. OpenRaft's `metrics.snapshot` describes locally built
    /// snapshots and is not evidence that a learner activated a received one.
    pub fn persisted_snapshot_meta(
        &self,
    ) -> ClusterResult<Option<SnapshotMeta<u64, VyrmRaftNode>>> {
        let stored: Option<StoredSnapshot> = read_json(
            &self.local_database,
            KEY_SNAPSHOT,
            ErrorSubject::Snapshot(None),
        )
        .map_err(|error| ClusterError::Unavailable(error.to_string()))?;
        let Some(snapshot) = stored else {
            return Ok(None);
        };
        let subject = ErrorSubject::Snapshot(Some(snapshot.meta.signature()));
        let path = self
            .snapshot_objects
            .verified_path(&snapshot.object)
            .map_err(|error| ClusterError::Unavailable(error.to_string()))?;
        let bundle = SnapshotBundleFile::open(path)
            .map_err(|error| ClusterError::Unavailable(error.to_string()))?;
        let state = state_from_snapshot_file(&bundle, self.shard, subject)
            .map_err(|error| ClusterError::Unavailable(error.to_string()))?;
        if state.last_applied != snapshot.meta.last_log_id
            || state.last_membership != snapshot.meta.last_membership
            || snapshot.meta.snapshot_id != expected_snapshot_file_id(&state, &bundle)
        {
            return Err(ClusterError::Unavailable(
                "persisted snapshot metadata does not match its authenticated bundle".into(),
            ));
        }
        Ok(Some(snapshot.meta))
    }

    /// Binds the exact cached physical snapshot to the project-scoped object
    /// closure that must be hydrated before a target may activate it.
    pub fn artifact_manifest_for_cached_snapshot(
        &self,
        meta: &SnapshotMeta<u64, VyrmRaftNode>,
        scope: &vyrm_core::ScopeId,
        source: NodeId,
        target: NodeId,
    ) -> std::result::Result<Option<ArtifactTransferManifest>, StorageError<u64>> {
        let subject = ErrorSubject::Snapshot(Some(meta.signature()));
        let stored: Option<StoredSnapshot> =
            read_json(&self.local_database, KEY_SNAPSHOT, subject.clone())?;
        let stored = stored.ok_or_else(|| {
            storage_error(
                subject.clone(),
                ErrorVerb::Read,
                "artifact transfer requires the exact cached snapshot",
            )
        })?;
        if &stored.meta != meta {
            return Err(storage_error(
                subject.clone(),
                ErrorVerb::Read,
                "cached snapshot metadata differs from the artifact transfer snapshot",
            ));
        }
        let path = self
            .snapshot_objects
            .verified_path(&stored.object)
            .map_err(|error| storage_error(subject.clone(), ErrorVerb::Read, error.to_string()))?;
        let bundle = SnapshotBundleFile::open(path)
            .map_err(|error| storage_error(subject.clone(), ErrorVerb::Read, error.to_string()))?;
        let state = state_from_snapshot_file(&bundle, self.shard, subject.clone())?;
        if state.last_applied != meta.last_log_id
            || state.last_membership != meta.last_membership
            || meta.snapshot_id != expected_snapshot_file_id(&state, &bundle)
        {
            return Err(storage_error(
                subject,
                ErrorVerb::Read,
                "cached snapshot state differs from its transfer metadata",
            ));
        }
        let (read, objects) = native_snapshot_artifact_view(&bundle, scope).map_err(|error| {
            storage_error(
                ErrorSubject::Snapshot(Some(meta.signature())),
                ErrorVerb::Read,
                error.to_string(),
            )
        })?;
        if objects.is_empty() {
            return Ok(None);
        }
        let placement_epoch = state.placement_epoch.ok_or_else(|| {
            storage_error(
                ErrorSubject::Snapshot(Some(meta.signature())),
                ErrorVerb::Read,
                "artifact-bearing snapshot has no applied placement epoch",
            )
        })?;
        let commit_index = meta.last_log_id.map_or(0, |log_id| log_id.index);
        let term = meta.last_log_id.map_or(0, |log_id| log_id.leader_id.term);
        let artifact_digests = objects.iter().map(|object| object.sha256.clone()).collect();
        ArtifactTransferManifest::new(
            ReplicaTransferPlan {
                contract_version: CLUSTER_CONTRACT_VERSION,
                shard: self.shard,
                placement_epoch,
                source,
                target,
                grounded_snapshot: ShardReadStamp {
                    term,
                    commit_index,
                    placement_epoch,
                    state_digest: state.state_digest,
                },
                wal_from_exclusive: commit_index,
                wal_through_inclusive: commit_index,
                artifact_digests,
            },
            scope.clone(),
            read,
            objects,
        )
        .map(Some)
        .map_err(|error| {
            storage_error(
                ErrorSubject::Snapshot(Some(meta.signature())),
                ErrorVerb::Read,
                error.to_string(),
            )
        })
    }

    /// Hydrates the immutable object closure before activating its canonical
    /// snapshot. A failed snapshot install may leave content-addressed orphans,
    /// but the supported path never exposes canonical references first and
    /// hopes their bytes arrive later.
    pub async fn install_snapshot_with_artifacts<S, T>(
        &mut self,
        meta: &SnapshotMeta<u64, VyrmRaftNode>,
        snapshot: Box<VyrmSnapshotData>,
        source: &S,
        target: &T,
        manifest: &ArtifactTransferManifest,
        completed_at: u64,
    ) -> std::result::Result<ArtifactTransferReceipt, StorageError<u64>>
    where
        S: vyrm_store::ImmutableObjectStore,
        T: vyrm_store::ImmutableObjectStore,
    {
        let subject = ErrorSubject::Snapshot(Some(meta.signature()));
        snapshot
            .sync_all()
            .await
            .map_err(|error| storage_error(subject.clone(), ErrorVerb::Write, error.to_string()))?;
        let bundle = SnapshotBundleFile::open(snapshot.path())
            .map_err(|error| storage_error(subject.clone(), ErrorVerb::Read, error.to_string()))?;
        let state = state_from_snapshot_file(&bundle, self.shard, subject.clone())?;
        let snapshot_index = meta.last_log_id.map_or(0, |log_id| log_id.index);
        manifest
            .validate()
            .map_err(|error| storage_error(subject.clone(), ErrorVerb::Write, error.to_string()))?;
        let snapshot_objects = native_snapshot_object_references(&bundle, &manifest.scope)
            .map_err(|error| storage_error(subject.clone(), ErrorVerb::Read, error.to_string()))?;
        if state.last_applied != meta.last_log_id
            || state.last_membership != meta.last_membership
            || meta.snapshot_id != expected_snapshot_file_id(&state, &bundle)
            || manifest.plan.shard != self.shard
            || manifest.plan.grounded_snapshot.commit_index != snapshot_index
            || manifest.plan.grounded_snapshot.state_digest != state.state_digest
            || manifest.plan.wal_from_exclusive != snapshot_index
            || manifest.objects != snapshot_objects
        {
            return Err(storage_error(
                subject,
                ErrorVerb::Write,
                "artifact manifest does not bind the exact canonical snapshot state",
            ));
        }
        let receipt =
            transfer_artifacts(source, target, manifest, completed_at).map_err(|error| {
                storage_error(
                    ErrorSubject::Snapshot(Some(meta.signature())),
                    ErrorVerb::Write,
                    error.to_string(),
                )
            })?;
        for object in &manifest.objects {
            let verified = target.verify(&object.sha256).map_err(|error| {
                storage_error(
                    ErrorSubject::Snapshot(Some(meta.signature())),
                    ErrorVerb::Read,
                    error.to_string(),
                )
            })?;
            if verified.length != object.length {
                return Err(storage_error(
                    ErrorSubject::Snapshot(Some(meta.signature())),
                    ErrorVerb::Read,
                    "installed artifact object length differs from its manifest",
                ));
            }
        }
        <Self as RaftStateMachine<VyrmRaftTypeConfig>>::install_snapshot(self, meta, snapshot)
            .await?;
        Ok(receipt)
    }

    fn read_state(&self) -> std::result::Result<StateMachineData, StorageError<u64>> {
        let database = lock_database(
            &self.state_database,
            ErrorSubject::StateMachine,
            ErrorVerb::Read,
        )?;
        read_state_from_database(&database)
    }
}

fn state_from_snapshot_file(
    bundle: &SnapshotBundleFile,
    shard: ShardId,
    subject: ErrorSubject<u64>,
) -> std::result::Result<StateMachineData, StorageError<u64>> {
    let mut values = bundle
        .get_many(&[KEY_STATE_CONFIG, KEY_LOCAL_CONFIG, KEY_STATE])
        .map_err(|error| storage_error(subject.clone(), ErrorVerb::Read, error.to_string()))?
        .into_iter();
    let config_bytes = values.next().flatten().ok_or_else(|| {
        storage_error(
            subject.clone(),
            ErrorVerb::Read,
            "snapshot bundle has no canonical-state adapter config",
        )
    })?;
    let config: AdapterConfig = decode_json(&config_bytes, subject.clone(), ErrorVerb::Read)?;
    let expected = AdapterConfig {
        format_version: ADAPTER_FORMAT_VERSION,
        shard,
        domain: AdapterDomain::CanonicalState,
    };
    if config != expected {
        return Err(storage_error(
            subject.clone(),
            ErrorVerb::Read,
            "snapshot bundle adapter format, shard, or storage domain does not match",
        ));
    }
    if values.next().flatten().is_some() {
        return Err(storage_error(
            subject.clone(),
            ErrorVerb::Read,
            "snapshot bundle illegally contains node-local Raft configuration",
        ));
    }
    let state_bytes = values.next().flatten().ok_or_else(|| {
        storage_error(
            subject.clone(),
            ErrorVerb::Read,
            "snapshot bundle has no state-machine record",
        )
    })?;
    decode_json(&state_bytes, subject, ErrorVerb::Read)
}

fn expected_snapshot_file_id(state: &StateMachineData, bundle: &SnapshotBundleFile) -> String {
    format!(
        "v4-{}-{}",
        state.last_applied.map_or(0, |log_id| log_id.index),
        bundle.digest
    )
}

fn publish_snapshot_file(
    local_database: &SharedDatabase,
    objects: &LocalObjectStore,
    meta: &SnapshotMeta<u64, VyrmRaftNode>,
    bundle: &SnapshotBundleFile,
) -> std::result::Result<(), StorageError<u64>> {
    let subject = ErrorSubject::Snapshot(Some(meta.signature()));
    let verified = objects
        .put_file(bundle.path())
        .map_err(|error| storage_error(subject.clone(), ErrorVerb::Write, error.to_string()))?;
    if verified.length != bundle.length {
        return Err(storage_error(
            subject,
            ErrorVerb::Write,
            "published snapshot object length differs from its authenticated bundle",
        ));
    }
    let object = ObjectReference::for_verified(
        format!("raft-snapshot-{}", meta.snapshot_id),
        None,
        SNAPSHOT_MEDIA_TYPE,
        verified.sha256,
        verified.length,
        verified.receipt,
    )
    .map_err(|error| storage_error(subject.clone(), ErrorVerb::Write, error.to_string()))?;
    write_json(
        local_database,
        KEY_SNAPSHOT,
        &StoredSnapshot {
            meta: meta.clone(),
            object,
        },
        subject,
    )
}

fn read_state_from_database(
    database: &Database,
) -> std::result::Result<StateMachineData, StorageError<u64>> {
    database
        .get(KEY_STATE, database.snapshot())
        .map_err(|error| {
            storage_error(
                ErrorSubject::StateMachine,
                ErrorVerb::Read,
                error.to_string(),
            )
        })?
        .map(|bytes| decode_json(&bytes, ErrorSubject::StateMachine, ErrorVerb::Read))
        .transpose()?
        .ok_or_else(|| {
            storage_error(
                ErrorSubject::StateMachine,
                ErrorVerb::Read,
                "state machine key is absent",
            )
        })
}

struct CommandApplication {
    response: VyrmRaftResponse,
    operations: Vec<Mutation>,
}

fn apply_command(
    database: &Database,
    shard: ShardId,
    state: &mut StateMachineData,
    log_id: LogId<u64>,
    command: VyrmRaftCommand,
) -> std::result::Result<CommandApplication, StorageError<u64>> {
    command.validate().map_err(|error| {
        storage_error(
            ErrorSubject::Apply(log_id),
            ErrorVerb::Write,
            error.to_string(),
        )
    })?;
    if command.shard != shard {
        return Err(storage_error(
            ErrorSubject::Apply(log_id),
            ErrorVerb::Write,
            "replicated command targets a different shard",
        ));
    }
    let command_digest = command_identity_digest(&command);
    if let Some(previous) = state.requests.get(&command.request_id) {
        if previous.command_digest != command_digest {
            return Ok(CommandApplication {
                response: VyrmRaftResponse {
                    accepted: false,
                    duplicate: false,
                    term: log_id.leader_id.term,
                    index: log_id.index,
                    state_digest: state.state_digest.clone(),
                    reason: "request id was reused with a different command identity".into(),
                    runtime_outcome: None,
                },
                operations: Vec::new(),
            });
        }
        let mut response = previous.response.clone();
        response.duplicate = true;
        response.term = log_id.leader_id.term;
        response.index = log_id.index;
        return Ok(CommandApplication {
            response,
            operations: Vec::new(),
        });
    }

    let current_index = state.last_applied.map_or(0, |applied| applied.index);
    let is_transition = matches!(
        &command.operation,
        VyrmRaftOperation::PlacementTransition { .. }
    );
    let epoch_matches = match &command.operation {
        VyrmRaftOperation::PlacementTransition { placement } => {
            state.placement_epoch.map_or(placement.epoch == 1, |epoch| {
                epoch.checked_add(1) == Some(placement.epoch)
            }) && command.placement_epoch == placement.epoch
        }
        _ => state.placement_epoch == Some(command.placement_epoch),
    };
    let current_membership_digest =
        membership_binding(&state.last_membership).map(|voters| membership_binding_digest(&voters));
    let membership_matches = is_transition
        || (state.placement_membership_digest.as_ref() == current_membership_digest.as_ref()
            && state.placement_membership_generation == Some(state.voter_membership_generation));
    let commit_index_matches = command
        .expected_commit_index
        .is_none_or(|expected| expected == current_index);
    let mut accepted = epoch_matches && membership_matches && commit_index_matches;
    let mut operations = Vec::new();
    let mut runtime_outcome = None;
    let mut reason = if !epoch_matches && is_transition {
        format!(
            "placement transition epoch {} is not the successor of state-machine epoch {}",
            command.placement_epoch,
            state
                .placement_epoch
                .map_or_else(|| "uninitialized".into(), |epoch| epoch.to_string())
        )
    } else if !epoch_matches {
        format!(
            "placement epoch {} does not match established state-machine epoch {}",
            command.placement_epoch,
            state
                .placement_epoch
                .map_or_else(|| "uninitialized".into(), |epoch| epoch.to_string())
        )
    } else if !membership_matches {
        "applied Raft membership no longer matches the established placement epoch".into()
    } else if !commit_index_matches {
        format!(
            "expected commit index {} but state machine was at {current_index}",
            command
                .expected_commit_index
                .expect("rejected CAS has expectation")
        )
    } else {
        String::new()
    };

    if accepted {
        match &command.operation {
            VyrmRaftOperation::PlacementTransition { placement } => {
                if let Some(membership_voters) =
                    placement_membership_binding(placement, &state.last_membership)
                {
                    state.placement_epoch = Some(placement.epoch);
                    state.placement_digest = Some(placement.digest().map_err(|error| {
                        storage_error(
                            ErrorSubject::Apply(log_id),
                            ErrorVerb::Write,
                            error.to_string(),
                        )
                    })?);
                    state.placement_membership_digest =
                        Some(membership_binding_digest(&membership_voters));
                    state.placement_membership_generation = Some(state.voter_membership_generation);
                    reason = "quorum-committed placement epoch transition applied".into();
                } else {
                    accepted = false;
                    reason =
                        "placement voters and zones do not match applied Raft membership".into();
                }
            }
            VyrmRaftOperation::Probe { .. } => {
                reason = "quorum-committed probe applied".into();
            }
            VyrmRaftOperation::RuntimeCommit { commit } => {
                let commit_id = commit.digest();
                match native_runtime_commit_outcome(database, &commit_id) {
                    Ok(Some(outcome)) => {
                        reason = "canonical runtime transaction was already committed".into();
                        runtime_outcome = Some(outcome);
                    }
                    Ok(None) => match prepare_native_runtime_commit(database, commit) {
                        Ok(plan) => {
                            let (outcome, runtime_operations) = plan.into_parts();
                            state.runtime_commit_count =
                                state.runtime_commit_count.checked_add(1).ok_or_else(|| {
                                    storage_error(
                                        ErrorSubject::Apply(log_id),
                                        ErrorVerb::Write,
                                        "runtime commit count overflowed",
                                    )
                                })?;
                            reason =
                                "quorum-committed canonical runtime transaction applied".into();
                            runtime_outcome = Some(outcome);
                            operations = runtime_operations;
                        }
                        Err(error) if deterministic_runtime_rejection(&error) => {
                            accepted = false;
                            reason = format!("canonical runtime transaction denied: {error}");
                        }
                        Err(error) => {
                            return Err(storage_error(
                                ErrorSubject::Apply(log_id),
                                ErrorVerb::Write,
                                error.to_string(),
                            ));
                        }
                    },
                    Err(error) => {
                        return Err(storage_error(
                            ErrorSubject::Apply(log_id),
                            ErrorVerb::Read,
                            error.to_string(),
                        ));
                    }
                }
            }
        }
    }

    if accepted {
        let mut bytes = b"vyrm.raft.state.transition.v1".to_vec();
        bytes.extend_from_slice(state.state_digest.as_bytes());
        bytes.extend_from_slice(&log_id.leader_id.term.to_be_bytes());
        bytes.extend_from_slice(&log_id.index.to_be_bytes());
        bytes.extend_from_slice(command.operation.digest().as_bytes());
        state.state_digest = sha256_hex(&bytes);
    }
    let response = VyrmRaftResponse {
        accepted,
        duplicate: false,
        term: log_id.leader_id.term,
        index: log_id.index,
        state_digest: state.state_digest.clone(),
        reason,
        runtime_outcome,
    };
    state.requests.insert(
        command.request_id,
        AppliedRequest {
            first_applied_index: log_id.index,
            command_digest,
            response: response.clone(),
        },
    );
    Ok(CommandApplication {
        response,
        operations,
    })
}

fn placement_membership_binding(
    placement: &ShardPlacement,
    membership: &StoredMembership<u64, VyrmRaftNode>,
) -> Option<BTreeSet<(String, String)>> {
    let raft_voters = membership_binding(membership)?;
    let declared_voters = placement
        .voters()
        .map(|replica| (replica.node.to_string(), replica.zone.to_string()))
        .collect::<BTreeSet<_>>();
    (raft_voters == declared_voters).then_some(raft_voters)
}

fn membership_binding(
    membership: &StoredMembership<u64, VyrmRaftNode>,
) -> Option<BTreeSet<(String, String)>> {
    let voter_ids = membership.voter_ids().collect::<Vec<_>>();
    let raft_voters = voter_ids
        .iter()
        .copied()
        .map(|id| membership.membership().get_node(&id))
        .collect::<Option<Vec<_>>>()?;
    if raft_voters.is_empty() || raft_voters.iter().any(|node| node.validate().is_err()) {
        return None;
    }
    let raft_voters = raft_voters
        .into_iter()
        .map(|node| (node.canonical_id.clone(), node.zone.clone()))
        .collect::<BTreeSet<_>>();
    (raft_voters.len() == voter_ids.len()).then_some(raft_voters)
}

fn membership_binding_digest(voters: &BTreeSet<(String, String)>) -> String {
    let mut bytes = b"vyrm.raft.membership-binding.v1".to_vec();
    for (canonical_id, zone) in voters {
        bytes.extend_from_slice(&(canonical_id.len() as u64).to_be_bytes());
        bytes.extend_from_slice(canonical_id.as_bytes());
        bytes.extend_from_slice(&(zone.len() as u64).to_be_bytes());
        bytes.extend_from_slice(zone.as_bytes());
    }
    sha256_hex(&bytes)
}

fn prune_request_history(state: &mut StateMachineData, through_index: u64) {
    let oldest_retained = through_index.saturating_sub(VYRM_RAFT_REQUEST_RETENTION_LOGS - 1);
    state
        .requests
        .retain(|_, request| request.first_applied_index >= oldest_retained);
}

fn deterministic_runtime_rejection(error: &StoreError) -> bool {
    matches!(
        error,
        StoreError::Kernel(_)
            | StoreError::RuntimeConflict { .. }
            | StoreError::DanglingRuntimeReference(_)
            | StoreError::RuntimeSchemaMissing(_)
            | StoreError::RuntimeSchemaConflict { .. }
    )
}

fn command_identity_digest(command: &VyrmRaftCommand) -> String {
    let mut bytes = b"vyrm.raft.command.identity.v1".to_vec();
    bytes.extend_from_slice(&command.shard.0.to_be_bytes());
    bytes.extend_from_slice(&command.placement_epoch.to_be_bytes());
    match command.expected_commit_index {
        Some(index) => {
            bytes.push(1);
            bytes.extend_from_slice(&index.to_be_bytes());
        }
        None => bytes.push(0),
    }
    bytes.extend_from_slice(command.operation.digest().as_bytes());
    sha256_hex(&bytes)
}

fn blank_response(state: &StateMachineData, log_id: LogId<u64>) -> VyrmRaftResponse {
    VyrmRaftResponse {
        accepted: true,
        duplicate: false,
        term: log_id.leader_id.term,
        index: log_id.index,
        state_digest: state.state_digest.clone(),
        reason: "raft protocol entry applied".into(),
        runtime_outcome: None,
    }
}

fn persist_entries(
    database: &SharedDatabase,
    entries: &[VyrmRaftEntry],
) -> std::result::Result<(), StorageError<u64>> {
    if entries.is_empty() {
        return Ok(());
    }
    for pair in entries.windows(2) {
        if pair[1].log_id.index != pair[0].log_id.index + 1 {
            return Err(storage_error(
                ErrorSubject::Logs,
                ErrorVerb::Write,
                "append batch contains a log index hole",
            ));
        }
    }
    let operations = entries
        .iter()
        .map(|entry| {
            put_json_storage(
                &log_key(entry.log_id.index),
                entry,
                ErrorSubject::Log(entry.log_id),
            )
        })
        .collect::<std::result::Result<Vec<_>, _>>()?;
    write_operations(database, operations, ErrorSubject::Logs)
}

fn delete_log_range<R: RangeBounds<u64> + Clone + Debug + Send>(
    database: &SharedDatabase,
    range: R,
) -> std::result::Result<(), StorageError<u64>> {
    let operations = log_delete_operations(database, range)?;
    if operations.is_empty() {
        return Ok(());
    }
    write_operations(database, operations, ErrorSubject::Logs)
}

fn log_delete_operations<R: RangeBounds<u64> + Clone + Debug + Send>(
    database: &SharedDatabase,
    range: R,
) -> std::result::Result<Vec<Mutation>, StorageError<u64>> {
    let database = lock_database(database, ErrorSubject::Logs, ErrorVerb::Read)?;
    let rows = database
        .scan(
            LOG_PREFIX,
            prefix_end(LOG_PREFIX).as_deref(),
            database.snapshot(),
        )
        .map_err(|error| storage_error(ErrorSubject::Logs, ErrorVerb::Read, error.to_string()))?;
    rows.into_iter()
        .filter_map(|(key, _)| match decode_log_key(&key) {
            Ok(index) if range_contains(&range, index) => Some(Ok(Mutation::Delete { key })),
            Ok(_) => None,
            Err(error) => Some(Err(error)),
        })
        .collect()
}

fn write_json<T: Serialize>(
    database: &SharedDatabase,
    key: &[u8],
    value: &T,
    subject: ErrorSubject<u64>,
) -> std::result::Result<(), StorageError<u64>> {
    let operation = put_json_storage(key, value, subject.clone())?;
    write_operations(database, vec![operation], subject)
}

fn read_json<T: DeserializeOwned>(
    database: &SharedDatabase,
    key: &[u8],
    subject: ErrorSubject<u64>,
) -> std::result::Result<Option<T>, StorageError<u64>> {
    let database = lock_database(database, subject.clone(), ErrorVerb::Read)?;
    database
        .get(key, database.snapshot())
        .map_err(|error| storage_error(subject.clone(), ErrorVerb::Read, error.to_string()))?
        .map(|bytes| decode_json(&bytes, subject, ErrorVerb::Read))
        .transpose()
}

fn write_operations(
    database: &SharedDatabase,
    operations: Vec<Mutation>,
    subject: ErrorSubject<u64>,
) -> std::result::Result<(), StorageError<u64>> {
    let batch = WriteBatch::new(operations)
        .map_err(|error| storage_error(subject.clone(), ErrorVerb::Write, error.to_string()))?;
    lock_database(database, subject.clone(), ErrorVerb::Write)?
        .write_owned(batch, Durability::Authoritative)
        .map_err(|error| storage_error(subject, ErrorVerb::Write, error.to_string()))?;
    Ok(())
}

fn lock_database<'a>(
    database: &'a SharedDatabase,
    subject: ErrorSubject<u64>,
    verb: ErrorVerb,
) -> std::result::Result<MutexGuard<'a, Database>, StorageError<u64>> {
    database
        .lock()
        .map_err(|_| storage_error(subject, verb, "raft database mutex is poisoned"))
}

fn put_json<T: Serialize>(key: &[u8], value: &T) -> ClusterResult<Mutation> {
    let value = serde_json::to_vec(value)
        .map_err(|error| ClusterError::Invalid(format!("raft value encoding failed: {error}")))?;
    Ok(Mutation::Put {
        key: key.to_vec(),
        value,
    })
}

fn put_json_storage<T: Serialize>(
    key: &[u8],
    value: &T,
    subject: ErrorSubject<u64>,
) -> std::result::Result<Mutation, StorageError<u64>> {
    Ok(Mutation::Put {
        key: key.to_vec(),
        value: encode_json(value, subject, ErrorVerb::Write)?,
    })
}

fn encode_json<T: Serialize>(
    value: &T,
    subject: ErrorSubject<u64>,
    verb: ErrorVerb,
) -> std::result::Result<Vec<u8>, StorageError<u64>> {
    serde_json::to_vec(value).map_err(|error| storage_error(subject, verb, error.to_string()))
}

fn decode_json<T: DeserializeOwned>(
    bytes: &[u8],
    subject: ErrorSubject<u64>,
    verb: ErrorVerb,
) -> std::result::Result<T, StorageError<u64>> {
    serde_json::from_slice(bytes).map_err(|error| storage_error(subject, verb, error.to_string()))
}

fn storage_error(
    subject: ErrorSubject<u64>,
    verb: ErrorVerb,
    message: impl Into<String>,
) -> StorageError<u64> {
    StorageError::from_io_error(subject, verb, std::io::Error::other(message.into()))
}

fn log_key(index: u64) -> Vec<u8> {
    let mut key = Vec::with_capacity(LOG_PREFIX.len() + 8);
    key.extend_from_slice(LOG_PREFIX);
    key.extend_from_slice(&index.to_be_bytes());
    key
}

fn decode_log_key(key: &[u8]) -> std::result::Result<u64, StorageError<u64>> {
    let suffix = key.strip_prefix(LOG_PREFIX).ok_or_else(|| {
        storage_error(
            ErrorSubject::Logs,
            ErrorVerb::Read,
            "raft log key has an invalid prefix",
        )
    })?;
    let bytes: [u8; 8] = suffix.try_into().map_err(|_| {
        storage_error(
            ErrorSubject::Logs,
            ErrorVerb::Read,
            "raft log key has an invalid length",
        )
    })?;
    Ok(u64::from_be_bytes(bytes))
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

fn range_contains<R: RangeBounds<u64>>(range: &R, value: u64) -> bool {
    let after_start = match range.start_bound() {
        Bound::Included(start) => value >= *start,
        Bound::Excluded(start) => value > *start,
        Bound::Unbounded => true,
    };
    let before_end = match range.end_bound() {
        Bound::Included(end) => value <= *end,
        Bound::Excluded(end) => value < *end,
        Bound::Unbounded => true,
    };
    after_start && before_end
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_history_retains_exactly_the_declared_log_window() {
        let mut state = StateMachineData::default();
        for index in 1..=VYRM_RAFT_REQUEST_RETENTION_LOGS {
            state.requests.insert(
                format!("request-{index}"),
                AppliedRequest {
                    first_applied_index: index,
                    command_digest: format!("digest-{index}"),
                    response: VyrmRaftResponse {
                        accepted: true,
                        duplicate: false,
                        term: 1,
                        index,
                        state_digest: "a".repeat(64),
                        reason: "retention fixture".into(),
                        runtime_outcome: None,
                    },
                },
            );
            prune_request_history(&mut state, index);
        }
        assert_eq!(
            state.requests.len() as u64,
            VYRM_RAFT_REQUEST_RETENTION_LOGS
        );
        assert!(state.requests.contains_key("request-1"));

        let next = VYRM_RAFT_REQUEST_RETENTION_LOGS + 1;
        state.requests.insert(
            format!("request-{next}"),
            AppliedRequest {
                first_applied_index: next,
                command_digest: format!("digest-{next}"),
                response: VyrmRaftResponse {
                    accepted: false,
                    duplicate: false,
                    term: 1,
                    index: next,
                    state_digest: "b".repeat(64),
                    reason: "retention boundary".into(),
                    runtime_outcome: None,
                },
            },
        );
        prune_request_history(&mut state, next);
        assert_eq!(
            state.requests.len() as u64,
            VYRM_RAFT_REQUEST_RETENTION_LOGS
        );
        assert!(!state.requests.contains_key("request-1"));
        assert!(state.requests.contains_key("request-2"));
        assert!(state.requests.contains_key(&format!("request-{next}")));

        let reopened: StateMachineData =
            serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
        assert_eq!(reopened.requests, state.requests);
    }
}
