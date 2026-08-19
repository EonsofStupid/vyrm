//! OpenRaft adapter over the Vyrm-native durable key/value substrate.
//!
//! The adapter owns its on-disk keyspace and persists every vote, log append,
//! committed pointer, state-machine application, and snapshot publication with
//! `vyrm-kv` authoritative durability before returning success.

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
use vyrm_core::digest::sha256_hex;
use vyrm_kv::{Database, Durability, Mutation, WriteBatch};

const ADAPTER_FORMAT_VERSION: u16 = 1;
const KEY_CONFIG: &[u8] = b"vyrm/raft/v1/meta/config";
const KEY_VOTE: &[u8] = b"vyrm/raft/v1/meta/vote";
const KEY_COMMITTED: &[u8] = b"vyrm/raft/v1/meta/committed";
const KEY_PURGED: &[u8] = b"vyrm/raft/v1/meta/purged";
const KEY_STATE: &[u8] = b"vyrm/raft/v1/state/current";
const KEY_SNAPSHOT: &[u8] = b"vyrm/raft/v1/state/snapshot";
const LOG_PREFIX: &[u8] = b"vyrm/raft/v1/log/";
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VyrmRaftCommand {
    pub request_id: String,
    pub shard: ShardId,
    pub placement_epoch: u64,
    pub expected_commit_index: Option<u64>,
    pub payload_digest: String,
    pub payload: Vec<u8>,
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
            payload_digest: sha256_hex(&payload),
            payload,
        };
        command.validate()?;
        Ok(command)
    }

    pub fn validate(&self) -> ClusterResult<()> {
        if self.request_id.is_empty()
            || self.request_id.len() > 256
            || self.request_id.as_bytes().contains(&0)
            || self.placement_epoch == 0
            || self.payload.is_empty()
            || self.payload.len() > MAX_COMMAND_BYTES
            || self.payload_digest != sha256_hex(&self.payload)
        {
            return Err(ClusterError::Invalid(
                "raft command identity, epoch, size, or payload digest is invalid".into(),
            ));
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
struct AdapterConfig {
    format_version: u16,
    shard: ShardId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct AppliedRequest {
    command_digest: String,
    response: VyrmRaftResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StateMachineData {
    last_applied: Option<LogId<u64>>,
    last_membership: StoredMembership<u64, VyrmRaftNode>,
    #[serde(default)]
    placement_epoch: Option<u64>,
    state_digest: String,
    requests: BTreeMap<String, AppliedRequest>,
}

impl Default for StateMachineData {
    fn default() -> Self {
        Self {
            last_applied: None,
            last_membership: StoredMembership::default(),
            placement_epoch: None,
            state_digest: sha256_hex(b"vyrm.raft.state.v1"),
            requests: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredSnapshot {
    meta: SnapshotMeta<u64, VyrmRaftNode>,
    data: Vec<u8>,
}

type SharedDatabase = Arc<Mutex<Database>>;

#[derive(Clone)]
pub struct VyrmRaftLogStore {
    database: SharedDatabase,
}

#[derive(Clone)]
pub struct VyrmRaftStateMachine {
    database: SharedDatabase,
    shard: ShardId,
}

pub struct VyrmRaftStore;

impl VyrmRaftStore {
    pub fn open(
        root: &Path,
        shard: ShardId,
    ) -> ClusterResult<(VyrmRaftLogStore, VyrmRaftStateMachine)> {
        let fresh = if root.exists() {
            std::fs::read_dir(root)
                .map_err(|error| ClusterError::Unavailable(error.to_string()))?
                .next()
                .is_none()
        } else {
            true
        };
        let mut database = if fresh {
            Database::create(root)
        } else {
            Database::open(root)
        }
        .map_err(|error| ClusterError::Unavailable(error.to_string()))?;

        if fresh {
            let operations = vec![
                put_json(
                    KEY_CONFIG,
                    &AdapterConfig {
                        format_version: ADAPTER_FORMAT_VERSION,
                        shard,
                    },
                )?,
                put_json(KEY_STATE, &StateMachineData::default())?,
            ];
            database
                .write_owned(
                    WriteBatch::new(operations)
                        .map_err(|error| ClusterError::Unavailable(error.to_string()))?,
                    Durability::Authoritative,
                )
                .map_err(|error| ClusterError::Unavailable(error.to_string()))?;
        } else {
            let bytes = database
                .get(KEY_CONFIG, database.snapshot())
                .ok_or_else(|| {
                    ClusterError::Denied(
                        "existing database is not a Vyrm OpenRaft adapter store".into(),
                    )
                })?;
            let config: AdapterConfig = serde_json::from_slice(bytes)
                .map_err(|error| ClusterError::Denied(error.to_string()))?;
            if config.format_version != ADAPTER_FORMAT_VERSION || config.shard != shard {
                return Err(ClusterError::Denied(
                    "raft adapter format or shard binding does not match".into(),
                ));
            }
        }

        let database = Arc::new(Mutex::new(database));
        Ok((
            VyrmRaftLogStore {
                database: database.clone(),
            },
            VyrmRaftStateMachine { database, shard },
        ))
    }
}

impl RaftLogReader<VyrmRaftTypeConfig> for VyrmRaftLogStore {
    async fn try_get_log_entries<RB: RangeBounds<u64> + Clone + Debug + Send>(
        &mut self,
        range: RB,
    ) -> std::result::Result<Vec<VyrmRaftEntry>, StorageError<u64>> {
        let database = lock_database(&self.database, ErrorSubject::Logs, ErrorVerb::Read)?;
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
        let last_purged_log_id = read_json(&self.database, KEY_PURGED, ErrorSubject::Logs)?;
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
        let mut database = lock_database(&self.database, ErrorSubject::Vote, ErrorVerb::Write)?;
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
        read_json(&self.database, KEY_VOTE, ErrorSubject::Vote)
    }

    async fn save_committed(
        &mut self,
        committed: Option<LogId<u64>>,
    ) -> std::result::Result<(), StorageError<u64>> {
        write_json(
            &self.database,
            KEY_COMMITTED,
            &committed,
            ErrorSubject::Logs,
        )
    }

    async fn read_committed(
        &mut self,
    ) -> std::result::Result<Option<LogId<u64>>, StorageError<u64>> {
        Ok(
            read_json::<Option<LogId<u64>>>(&self.database, KEY_COMMITTED, ErrorSubject::Logs)?
                .flatten(),
        )
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
        let result = persist_entries(&self.database, &entries);
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
        delete_log_range(&self.database, log_id.index..)
    }

    async fn purge(&mut self, log_id: LogId<u64>) -> std::result::Result<(), StorageError<u64>> {
        let mut operations = log_delete_operations(&self.database, ..=log_id.index)?;
        operations.push(put_json_storage(KEY_PURGED, &log_id, ErrorSubject::Logs)?);
        write_operations(&self.database, operations, ErrorSubject::Logs)
    }
}

impl RaftSnapshotBuilder<VyrmRaftTypeConfig> for VyrmRaftStateMachine {
    async fn build_snapshot(
        &mut self,
    ) -> std::result::Result<Snapshot<VyrmRaftTypeConfig>, StorageError<u64>> {
        let state = self.read_state()?;
        let data = encode_json(&state, ErrorSubject::StateMachine, ErrorVerb::Read)?;
        let snapshot_id = format!(
            "{}-{}",
            state.last_applied.map_or(0, |log_id| log_id.index),
            sha256_hex(&data)
        );
        let meta = SnapshotMeta {
            last_log_id: state.last_applied,
            last_membership: state.last_membership,
            snapshot_id,
        };
        let stored = StoredSnapshot {
            meta: meta.clone(),
            data: data.clone(),
        };
        write_json(
            &self.database,
            KEY_SNAPSHOT,
            &stored,
            ErrorSubject::Snapshot(None),
        )?;
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
        let mut state = self.read_state()?;
        let mut responses = Vec::new();
        for entry in entries {
            let response = match entry.payload {
                EntryPayload::Blank => blank_response(&state, entry.log_id),
                EntryPayload::Membership(membership) => {
                    state.last_membership = StoredMembership::new(Some(entry.log_id), membership);
                    blank_response(&state, entry.log_id)
                }
                EntryPayload::Normal(command) => {
                    apply_command(self.shard, &mut state, entry.log_id, command)?
                }
            };
            state.last_applied = Some(entry.log_id);
            responses.push(response);
        }
        write_json(
            &self.database,
            KEY_STATE,
            &state,
            ErrorSubject::StateMachine,
        )?;
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
        let state: StateMachineData = decode_json(
            &data,
            ErrorSubject::Snapshot(Some(meta.signature())),
            ErrorVerb::Read,
        )?;
        if state.last_applied != meta.last_log_id || state.last_membership != meta.last_membership {
            return Err(storage_error(
                ErrorSubject::Snapshot(Some(meta.signature())),
                ErrorVerb::Write,
                "snapshot metadata does not match its state bytes",
            ));
        }
        let operations = vec![
            put_json_storage(KEY_STATE, &state, ErrorSubject::StateMachine)?,
            put_json_storage(
                KEY_SNAPSHOT,
                &StoredSnapshot {
                    meta: meta.clone(),
                    data,
                },
                ErrorSubject::Snapshot(Some(meta.signature())),
            )?,
        ];
        write_operations(
            &self.database,
            operations,
            ErrorSubject::Snapshot(Some(meta.signature())),
        )
    }

    async fn get_current_snapshot(
        &mut self,
    ) -> std::result::Result<Option<Snapshot<VyrmRaftTypeConfig>>, StorageError<u64>> {
        let stored: Option<StoredSnapshot> =
            read_json(&self.database, KEY_SNAPSHOT, ErrorSubject::Snapshot(None))?;
        Ok(stored.map(|snapshot| Snapshot {
            meta: snapshot.meta,
            snapshot: Box::new(Cursor::new(snapshot.data)),
        }))
    }
}

impl VyrmRaftStateMachine {
    fn read_state(&self) -> std::result::Result<StateMachineData, StorageError<u64>> {
        read_json(&self.database, KEY_STATE, ErrorSubject::StateMachine)?.ok_or_else(|| {
            storage_error(
                ErrorSubject::StateMachine,
                ErrorVerb::Read,
                "state machine key is absent",
            )
        })
    }
}

fn apply_command(
    shard: ShardId,
    state: &mut StateMachineData,
    log_id: LogId<u64>,
    command: VyrmRaftCommand,
) -> std::result::Result<VyrmRaftResponse, StorageError<u64>> {
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
            return Ok(VyrmRaftResponse {
                accepted: false,
                duplicate: false,
                term: log_id.leader_id.term,
                index: log_id.index,
                state_digest: state.state_digest.clone(),
                reason: "request id was reused with a different command identity".into(),
            });
        }
        let mut response = previous.response.clone();
        response.duplicate = true;
        response.term = log_id.leader_id.term;
        response.index = log_id.index;
        return Ok(response);
    }

    let current_index = state.last_applied.map_or(0, |applied| applied.index);
    let epoch_matches = state
        .placement_epoch
        .is_none_or(|epoch| epoch == command.placement_epoch);
    let commit_index_matches = command
        .expected_commit_index
        .is_none_or(|expected| expected == current_index);
    let accepted = epoch_matches && commit_index_matches;
    if accepted {
        state.placement_epoch = Some(command.placement_epoch);
        let mut bytes = b"vyrm.raft.state.transition.v1".to_vec();
        bytes.extend_from_slice(state.state_digest.as_bytes());
        bytes.extend_from_slice(&log_id.leader_id.term.to_be_bytes());
        bytes.extend_from_slice(&log_id.index.to_be_bytes());
        bytes.extend_from_slice(command.payload_digest.as_bytes());
        state.state_digest = sha256_hex(&bytes);
    }
    let response = VyrmRaftResponse {
        accepted,
        duplicate: false,
        term: log_id.leader_id.term,
        index: log_id.index,
        state_digest: state.state_digest.clone(),
        reason: if accepted {
            "quorum-committed command applied".into()
        } else if !epoch_matches {
            format!(
                "placement epoch {} does not match state-machine epoch {}",
                command.placement_epoch,
                state
                    .placement_epoch
                    .expect("mismatched epoch requires an established epoch")
            )
        } else {
            format!(
                "expected commit index {} but state machine was at {current_index}",
                command
                    .expected_commit_index
                    .expect("rejected CAS has expectation")
            )
        },
    };
    state.requests.insert(
        command.request_id,
        AppliedRequest {
            command_digest,
            response: response.clone(),
        },
    );
    Ok(response)
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
    bytes.extend_from_slice(command.payload_digest.as_bytes());
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
