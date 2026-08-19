//! OpenRaft adapter over the Vyrm-native durable key/value substrate.
//!
//! Adapter v3 physically separates node-local Raft history from transferable
//! canonical state. Votes, logs, purge/commit cursors, and snapshot cache
//! references stay local; state-machine application and canonical runtime data
//! share one authoritative VyrmKV WAL frame.

// OpenRaft fixes `StorageError` as the error type in its storage traits. The
// enum deliberately carries rich Raft context and is larger than Clippy's
// generic threshold, so adapter helpers preserve that required type too.
#![allow(clippy::result_large_err)]

use crate::{ClusterError, Result as ClusterResult, ShardId};
use openraft::storage::{RaftLogStorage, RaftStateMachine};
use openraft::{
    Entry, EntryPayload, ErrorSubject, ErrorVerb, LogId, LogState, RaftLogReader,
    RaftSnapshotBuilder, RaftTypeConfig, Snapshot, SnapshotMeta, StorageError, StoredMembership,
    Vote,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::Debug;
use std::io::Cursor;
use std::ops::{Bound, RangeBounds};
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use vyrm_core::{digest::sha256_hex, ObjectReference, RuntimeCommit, RuntimeCommitOutcome};
use vyrm_kv::{Database, Durability, Mutation, SnapshotBundle, WriteBatch};
use vyrm_store::{
    native_runtime_commit_outcome, prepare_native_runtime_commit, Error as StoreError,
    LocalObjectStore,
};

const ADAPTER_FORMAT_VERSION: u16 = 3;
const LOCAL_DATABASE_DIRECTORY: &str = "raft-local-v3";
const SNAPSHOT_OBJECT_DIRECTORY: &str = "snapshot-objects";
const KEY_STATE_CONFIG: &[u8] = b"vyrm/raft/v3/state/config";
const KEY_LOCAL_CONFIG: &[u8] = b"vyrm/raft/v3/local/config";
const KEY_VOTE: &[u8] = b"vyrm/raft/v3/local/vote";
const KEY_COMMITTED: &[u8] = b"vyrm/raft/v3/local/committed";
const KEY_PURGED: &[u8] = b"vyrm/raft/v3/local/purged";
const KEY_STATE: &[u8] = b"vyrm/raft/v3/state/current";
const KEY_SNAPSHOT: &[u8] = b"vyrm/raft/v3/local/snapshot";
const LOG_PREFIX: &[u8] = b"vyrm/raft/v3/local/log/";
const SNAPSHOT_MEDIA_TYPE: &str = "application/vnd.vyrm.raft-state-snapshot.v3";
// JSON is only the correctness-gate codec. Keep worst-case byte-array expansion
// comfortably below VyrmKV's 8 MiB value ceiling until a compact codec lands.
const MAX_COMMAND_BYTES: usize = 1024 * 1024;

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
        let mut bytes = b"vyrm.raft.operation.v1".to_vec();
        match self {
            Self::Probe { payload_digest, .. } => {
                bytes.push(1);
                bytes.extend_from_slice(payload_digest.as_bytes());
            }
            Self::RuntimeCommit { commit } => {
                bytes.push(2);
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

fn ensure_adapter_database(
    database: &mut Database,
    config_key: &[u8],
    expected: AdapterConfig,
    initial: Option<Mutation>,
) -> ClusterResult<()> {
    let snapshot = database.snapshot();
    if let Some(bytes) = database.get(config_key, snapshot) {
        let actual: AdapterConfig = serde_json::from_slice(bytes)
            .map_err(|error| ClusterError::Denied(error.to_string()))?;
        if actual != expected {
            return Err(ClusterError::Denied(
                "raft adapter format, shard binding, or storage domain does not match".into(),
            ));
        }
        if expected.domain == AdapterDomain::CanonicalState
            && database.get(KEY_STATE, snapshot).is_none()
        {
            return Err(ClusterError::Denied(
                "canonical Raft state database has no state-machine record".into(),
            ));
        }
        return Ok(());
    }
    if snapshot.sequence != 0 {
        return Err(ClusterError::Denied(
            "existing database is not a Vyrm OpenRaft v3 storage domain".into(),
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
        let rows = database.scan(
            LOG_PREFIX,
            prefix_end(LOG_PREFIX).as_deref(),
            database.snapshot(),
        );
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
            .map(|bytes| decode_json(bytes, ErrorSubject::Vote, ErrorVerb::Read))
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
        let (state, bundle) = {
            let mut database = lock_database(
                &self.state_database,
                ErrorSubject::Snapshot(None),
                ErrorVerb::Read,
            )?;
            let state = read_state_from_database(&database)?;
            let at = state.last_applied.map_or(0, |log_id| log_id.index);
            let bundle = database.export_snapshot_bundle(at).map_err(|error| {
                storage_error(
                    ErrorSubject::Snapshot(None),
                    ErrorVerb::Read,
                    error.to_string(),
                )
            })?;
            (state, bundle)
        };
        let data = bundle.encode().map_err(|error| {
            storage_error(
                ErrorSubject::Snapshot(None),
                ErrorVerb::Read,
                error.to_string(),
            )
        })?;
        let snapshot_id = expected_snapshot_id(&state, &bundle);
        let meta = SnapshotMeta {
            last_log_id: state.last_applied,
            last_membership: state.last_membership,
            snapshot_id,
        };
        publish_snapshot(&self.local_database, &self.snapshot_objects, &meta, &data)?;
        Ok(Snapshot {
            meta,
            snapshot: Box::new(Cursor::new(data)),
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
                    state.last_membership = StoredMembership::new(Some(entry.log_id), membership);
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
        Ok(Box::new(Cursor::new(Vec::new())))
    }

    async fn install_snapshot(
        &mut self,
        meta: &SnapshotMeta<u64, VyrmRaftNode>,
        snapshot: Box<<VyrmRaftTypeConfig as RaftTypeConfig>::SnapshotData>,
    ) -> std::result::Result<(), StorageError<u64>> {
        let data = snapshot.into_inner();
        let subject = ErrorSubject::Snapshot(Some(meta.signature()));
        let bundle = SnapshotBundle::decode(&data)
            .map_err(|error| storage_error(subject.clone(), ErrorVerb::Read, error.to_string()))?;
        let state = state_from_snapshot_bundle(&bundle, self.shard, subject.clone())?;
        if state.last_applied != meta.last_log_id || state.last_membership != meta.last_membership {
            return Err(storage_error(
                subject,
                ErrorVerb::Write,
                "snapshot metadata does not match its state bytes",
            ));
        }
        if meta.snapshot_id != expected_snapshot_id(&state, &bundle) {
            return Err(storage_error(
                ErrorSubject::Snapshot(Some(meta.signature())),
                ErrorVerb::Write,
                "snapshot id does not bind the exact VyrmKV bundle",
            ));
        }
        let at = meta.last_log_id.map_or(0, |log_id| log_id.index);
        lock_database(
            &self.state_database,
            ErrorSubject::Snapshot(Some(meta.signature())),
            ErrorVerb::Write,
        )?
        .install_snapshot_bundle(&bundle, at)
        .map_err(|error| {
            storage_error(
                ErrorSubject::Snapshot(Some(meta.signature())),
                ErrorVerb::Write,
                error.to_string(),
            )
        })?;
        publish_snapshot(&self.local_database, &self.snapshot_objects, meta, &data)
    }

    async fn get_current_snapshot(
        &mut self,
    ) -> std::result::Result<Option<Snapshot<VyrmRaftTypeConfig>>, StorageError<u64>> {
        let stored: Option<StoredSnapshot> = read_json(
            &self.local_database,
            KEY_SNAPSHOT,
            ErrorSubject::Snapshot(None),
        )?;
        stored
            .map(|snapshot| {
                let data = self
                    .snapshot_objects
                    .get(&snapshot.object)
                    .map_err(|error| {
                        storage_error(
                            ErrorSubject::Snapshot(Some(snapshot.meta.signature())),
                            ErrorVerb::Read,
                            error.to_string(),
                        )
                    })?;
                let bundle = SnapshotBundle::decode(&data).map_err(|error| {
                    storage_error(
                        ErrorSubject::Snapshot(Some(snapshot.meta.signature())),
                        ErrorVerb::Read,
                        error.to_string(),
                    )
                })?;
                let state = state_from_snapshot_bundle(
                    &bundle,
                    self.shard,
                    ErrorSubject::Snapshot(Some(snapshot.meta.signature())),
                )?;
                if state.last_applied != snapshot.meta.last_log_id
                    || state.last_membership != snapshot.meta.last_membership
                    || snapshot.meta.snapshot_id != expected_snapshot_id(&state, &bundle)
                {
                    return Err(storage_error(
                        ErrorSubject::Snapshot(Some(snapshot.meta.signature())),
                        ErrorVerb::Read,
                        "cached snapshot metadata does not match its authenticated bundle",
                    ));
                }
                Ok(Snapshot {
                    meta: snapshot.meta,
                    snapshot: Box::new(Cursor::new(data)),
                })
            })
            .transpose()
    }
}

impl VyrmRaftStateMachine {
    fn read_state(&self) -> std::result::Result<StateMachineData, StorageError<u64>> {
        let database = lock_database(
            &self.state_database,
            ErrorSubject::StateMachine,
            ErrorVerb::Read,
        )?;
        read_state_from_database(&database)
    }
}

fn state_from_snapshot_bundle(
    bundle: &SnapshotBundle,
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

fn expected_snapshot_id(state: &StateMachineData, bundle: &SnapshotBundle) -> String {
    format!(
        "v3-{}-{}",
        state.last_applied.map_or(0, |log_id| log_id.index),
        bundle.digest
    )
}

fn publish_snapshot(
    local_database: &SharedDatabase,
    objects: &LocalObjectStore,
    meta: &SnapshotMeta<u64, VyrmRaftNode>,
    data: &[u8],
) -> std::result::Result<(), StorageError<u64>> {
    let subject = ErrorSubject::Snapshot(Some(meta.signature()));
    let verified = objects
        .put(data)
        .map_err(|error| storage_error(subject.clone(), ErrorVerb::Write, error.to_string()))?;
    let object = ObjectReference::for_bytes(
        format!("raft-snapshot-{}", meta.snapshot_id),
        None,
        SNAPSHOT_MEDIA_TYPE,
        data,
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
        .map(|bytes| decode_json(bytes, ErrorSubject::StateMachine, ErrorVerb::Read))
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
    let epoch_matches = state
        .placement_epoch
        .is_none_or(|epoch| epoch == command.placement_epoch);
    let commit_index_matches = command
        .expected_commit_index
        .is_none_or(|expected| expected == current_index);
    let mut accepted = epoch_matches && commit_index_matches;
    let mut operations = Vec::new();
    let mut runtime_outcome = None;
    let mut reason = if !epoch_matches {
        format!(
            "placement epoch {} does not match state-machine epoch {}",
            command.placement_epoch,
            state
                .placement_epoch
                .expect("mismatched epoch requires an established epoch")
        )
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
        state.placement_epoch = Some(command.placement_epoch);
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
            command_digest,
            response: response.clone(),
        },
    );
    Ok(CommandApplication {
        response,
        operations,
    })
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
    let rows = database.scan(
        LOG_PREFIX,
        prefix_end(LOG_PREFIX).as_deref(),
        database.snapshot(),
    );
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
        .map(|bytes| decode_json(bytes, subject, ErrorVerb::Read))
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
