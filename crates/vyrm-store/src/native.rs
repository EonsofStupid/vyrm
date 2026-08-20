//! Native `vyrmKV` implementation of the semantic [`Engine`] port.
//!
//! Logical keyspaces are encoded as stable byte prefixes inside one atomic
//! native database. One semantic commit becomes one `vyrmKV` write batch; the
//! database's physical MVCC sequence is deliberately independent of claim and
//! runtime cursors stored in the batch.

use crate::engine::{Engine, PhysicalStoreEvidence};
use crate::error::{Error, Result};
use crate::gc::{build_report, RemovalReport, Tally};
use crate::invocation::{self, Invocation, InvocationInput, RecallOutcome};
use crate::keyspaces::{self, Durability};
use crate::store::AppendOutcome;
use serde::de::DeserializeOwned;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use vyrm_core::{
    key, projection_family, AuditEnvelope, Claim, ClaimSource, Millis, ObjectReference, Predicate,
    ProjectionWork, ReadStamp, Reader, RetentionPin, RuntimeChange, RuntimeChangePage,
    RuntimeCommit, RuntimeCommitOutcome, RuntimeMutation, RuntimeRecord, RuntimeRef,
    RuntimeRelation, RuntimeSchemaRegistry, ScopeId, SnapshotHandle, SnapshotId, Subject,
};
use vyrm_kv::{
    CompactionOutcome, Database, GarbageCollectionReport, Manifest, Mutation, Snapshot,
    SnapshotBundleFile, WriteBatch,
};

const RUNTIME_CHECKPOINT_PREFIX: &str = "runtime-";
const NATIVE_SEQUENCE_VALUE_MAGIC: &[u8; 8] = b"VYRNSI01";

pub struct NativeEngine {
    path: PathBuf,
    database: Mutex<Database>,
}

impl NativeEngine {
    /// Opens an existing native database or creates one when `path` is absent.
    /// An existing but invalid directory fails closed rather than being
    /// silently reinitialized.
    pub fn open(path: &Path) -> Result<Self> {
        if !path.exists() {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|error| Error::Substrate(error.to_string()))?;
            }
        }
        let empty = path.exists()
            && path.is_dir()
            && std::fs::read_dir(path)
                .map_err(|error| Error::Substrate(error.to_string()))?
                .next()
                .is_none();
        let mut database = if !path.exists() || empty {
            Database::create(path)?
        } else {
            Database::open(path)?
        };
        reconcile_runtime_checkpoints(&mut database, None, 0)?;
        let path = database.root().to_owned();
        Ok(Self {
            path,
            database: Mutex::new(database),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Publishes the current native memtable as an immutable segment.
    pub fn flush(&self, at: Millis) -> Result<Option<Manifest>> {
        self.lock()?.flush_memtable(at).map_err(Error::from)
    }

    pub fn manifest(&self) -> Result<Manifest> {
        Ok(self.lock()?.manifest().clone())
    }

    /// Compacts native state after reconciling physical manifest pins with the
    /// authoritative logical snapshot catalog.
    pub fn compact(&self, now: Millis, at: Millis) -> Result<Option<CompactionOutcome>> {
        let mut database = self.lock()?;
        reconcile_runtime_checkpoints(&mut database, Some(now), at)?;
        database.compact(&[], at).map_err(Error::from)
    }

    /// Reclaims only objects unreachable from `CURRENT` or a live runtime
    /// snapshot's physical checkpoint.
    pub fn garbage_collect(&self, now: Millis, at: Millis) -> Result<GarbageCollectionReport> {
        let mut database = self.lock()?;
        reconcile_runtime_checkpoints(&mut database, Some(now), at)?;
        database.garbage_collect().map_err(Error::from)
    }

    /// Derives the same evidence-backed removal report as the compatibility
    /// adapter from native claim and access keyspaces.
    pub fn removal_report(&self, since: Millis, evaluated_at: Millis) -> Result<RemovalReport> {
        let database = self.lock()?;
        let snapshot = database.snapshot();
        let mut tallies = BTreeMap::<(String, String), Tally>::new();
        for (stored_key, _) in scan_space(&database, snapshot, keyspaces::CLAIMS, &[])? {
            let (subject, predicate) =
                key::parse_claim_key(strip_space(keyspaces::CLAIMS, &stored_key)?)?;
            tallies
                .entry((subject.to_string(), predicate.to_string()))
                .or_default()
                .claim_count += 1;
        }
        for (stored_key, _) in scan_space_from(
            &database,
            snapshot,
            keyspaces::ACCESS,
            &key::access_bound(since),
        )? {
            let (at, reader, subject, predicate) =
                key::parse_access_key(strip_space(keyspaces::ACCESS, &stored_key)?)?;
            if at > evaluated_at {
                break;
            }
            let tally = tallies
                .entry((subject.to_string(), predicate.to_string()))
                .or_default();
            tally.access_count += 1;
            if tally.last_access.is_none_or(|previous| at >= previous) {
                tally.last_access = Some(at);
                tally.last_reader = Some(reader);
            }
        }
        Ok(build_report(tallies, since, evaluated_at)?)
    }

    /// Exact native access-record count. The compatibility adapter exposes an
    /// approximate count because that is all its keyspace API promises.
    pub fn access_count(&self) -> Result<usize> {
        let database = self.lock()?;
        Ok(scan_space(&database, database.snapshot(), keyspaces::ACCESS, &[])?.len())
    }

    /// Persists one authoritative operator invocation and its ordinal in one
    /// native batch.
    #[tracing::instrument(level = "debug", skip_all, fields(command = input.command))]
    pub fn record_invocation(&self, input: InvocationInput<'_>) -> Result<Invocation> {
        let mut database = self.lock()?;
        let previous = read_sequence(
            &database,
            database.snapshot(),
            keyspaces::INVOCATION_WATERMARK,
        )?;
        let ordinal = previous.checked_add(1).ok_or(Error::SequenceOverflow)?;
        let record = Invocation {
            ordinal,
            at: input.at,
            trigger: input.trigger,
            command: input.command.to_owned(),
            arguments: input.arguments.to_vec(),
            outcome: input.outcome,
            duration_ms: input.duration_ms,
            detail: input.detail,
            effectiveness: input.effectiveness,
        };
        let mut operations = Vec::with_capacity(2);
        put(
            &mut operations,
            keyspaces::INVOCATIONS,
            &invocation::invocation_key(input.at, ordinal),
            serde_json::to_vec(&record)?,
        );
        put_sequence(&mut operations, keyspaces::INVOCATION_WATERMARK, ordinal);
        write(&mut database, operations, Durability::Authoritative)?;
        tracing::debug!(ordinal, "invocation recorded");
        Ok(record)
    }

    pub fn set_recall_outcome(&self, ordinal: u64, outcome: RecallOutcome) -> Result<Invocation> {
        let mut database = self.lock()?;
        let snapshot = database.snapshot();
        let mut found = None;
        for (stored_key, value) in scan_space(&database, snapshot, keyspaces::INVOCATIONS, &[])? {
            let record: Invocation = serde_json::from_slice(&value)?;
            if record.ordinal == ordinal {
                found = Some((
                    strip_space(keyspaces::INVOCATIONS, &stored_key)?.to_vec(),
                    record,
                ));
                break;
            }
        }
        let Some((invocation_key, mut record)) = found else {
            return Err(Error::Substrate(format!(
                "no invocation with ordinal {ordinal}"
            )));
        };
        let Some(effectiveness) = record.effectiveness.as_mut() else {
            return Err(Error::Substrate(format!(
                "invocation {ordinal} is `{}`, not a recall — refusing to judge it",
                record.command
            )));
        };
        effectiveness.outcome = outcome;
        write(
            &mut database,
            vec![Mutation::Put {
                key: storage_key(keyspaces::INVOCATIONS, &invocation_key),
                value: serde_json::to_vec(&record)?,
            }],
            Durability::Authoritative,
        )?;
        Ok(record)
    }

    pub fn invocations_since(&self, since: Millis) -> Result<Vec<Invocation>> {
        let database = self.lock()?;
        scan_space_from(
            &database,
            database.snapshot(),
            keyspaces::INVOCATIONS,
            &invocation::invocation_bound(since),
        )?
        .into_iter()
        .map(|(_, value)| serde_json::from_slice(&value).map_err(Error::from))
        .collect()
    }

    pub fn invocation_count(&self) -> Result<u64> {
        let database = self.lock()?;
        read_sequence(
            &database,
            database.snapshot(),
            keyspaces::INVOCATION_WATERMARK,
        )
    }

    fn lock(&self) -> Result<MutexGuard<'_, Database>> {
        self.database
            .lock()
            .map_err(|_| Error::Substrate("native database mutex poisoned".into()))
    }
}

impl ClaimSource for NativeEngine {
    type Error = Error;

    fn versions_at_or_before(
        &self,
        subject: &Subject,
        predicate: &Predicate,
        as_of: Millis,
    ) -> Result<Vec<Claim>> {
        let database = self.lock()?;
        scan_claims(
            &database,
            key::version_prefix(subject, predicate),
            key::seek_key(subject, predicate, as_of),
        )
    }

    fn all_versions(&self, subject: &Subject, predicate: &Predicate) -> Result<Vec<Claim>> {
        let database = self.lock()?;
        let prefix = key::version_prefix(subject, predicate);
        scan_claims(&database, prefix.clone(), prefix)
    }

    fn subject_versions(&self, subject: &Subject) -> Result<Vec<Claim>> {
        let database = self.lock()?;
        let prefix = key::subject_prefix(subject);
        scan_claims(&database, prefix.clone(), prefix)
    }
}

impl Engine for NativeEngine {
    fn physical_store_evidence(&self) -> Result<PhysicalStoreEvidence> {
        let database = self.lock()?;
        let manifest = database.manifest();
        let cache = database.block_cache_stats();
        Ok(PhysicalStoreEvidence {
            backend: "vyrmkv_native".into(),
            evidence_level: "native_counters".into(),
            physical_sequence: Some(database.snapshot().sequence),
            manifest_generation: Some(manifest.generation),
            durable_sequence: Some(manifest.durable_sequence),
            memtable_versions: Some(database.memtable().version_count() as u64),
            memtable_bytes: Some(database.memtable().approximate_bytes() as u64),
            segment_count: Some(manifest.segments.len() as u64),
            segment_bytes: Some(manifest.segments.iter().map(|segment| segment.bytes).sum()),
            cache_capacity_bytes: Some(cache.capacity_bytes as u64),
            cache_resident_bytes: Some(cache.resident_bytes as u64),
            cache_entries: Some(cache.entries as u64),
            cache_hits: Some(cache.hits),
            cache_misses: Some(cache.misses),
            cache_evictions: Some(cache.evictions),
            block_loads: Some(cache.loads),
            block_bytes_loaded: Some(cache.bytes_loaded),
            block_bytes_decoded: Some(cache.bytes_decoded),
        })
    }

    #[tracing::instrument(level = "debug", skip_all, fields(claims = claims.len()))]
    fn append_batch(&self, claims: &[Claim]) -> Result<AppendOutcome> {
        for claim in claims {
            claim.validate()?;
        }
        let mut database = self.lock()?;
        let snapshot = database.snapshot();
        let start = read_sequence(&database, snapshot, keyspaces::SEQUENCE_WATERMARK)?;
        if claims.is_empty() {
            return Ok(AppendOutcome {
                first_sequence: start,
                last_sequence: start,
                count: 0,
            });
        }
        let mut sequence = start;
        let mut operations = Vec::with_capacity(claims.len() * 2 + 1);
        for claim in claims {
            sequence = sequence.checked_add(1).ok_or(Error::SequenceOverflow)?;
            let claim_key = key::claim_key(
                &claim.subject,
                &claim.predicate,
                claim.valid_from,
                claim.tx_time,
            );
            let encoded_claim = serde_json::to_vec(claim)?;
            put(
                &mut operations,
                keyspaces::SEQUENCE_INDEX,
                &key::sequence_key(sequence),
                encode_native_sequence_value(&encoded_claim),
            );
            put(
                &mut operations,
                keyspaces::CLAIMS,
                &claim_key,
                encoded_claim,
            );
        }
        put_sequence(&mut operations, keyspaces::SEQUENCE_WATERMARK, sequence);
        write(&mut database, operations, Durability::Authoritative)?;
        tracing::debug!(first = start + 1, last = sequence, "append committed");
        Ok(AppendOutcome {
            first_sequence: start + 1,
            last_sequence: sequence,
            count: claims.len(),
        })
    }

    fn sequence(&self) -> Result<u64> {
        let database = self.lock()?;
        read_sequence(
            &database,
            database.snapshot(),
            keyspaces::SEQUENCE_WATERMARK,
        )
    }

    fn claims_in_range(&self, from: u64, to: u64) -> Result<Vec<Claim>> {
        if from >= to {
            return Ok(Vec::new());
        }
        let database = self.lock()?;
        let snapshot = database.snapshot();
        let head = read_sequence(&database, snapshot, keyspaces::SEQUENCE_WATERMARK)?;
        let last = to.min(head);
        if from >= last {
            return Ok(Vec::new());
        }
        let start = storage_key(
            keyspaces::SEQUENCE_INDEX,
            &key::sequence_key(from.saturating_add(1)),
        );
        let inclusive_end = storage_key(keyspaces::SEQUENCE_INDEX, &key::sequence_key(last));
        let end = prefix_end(&inclusive_end)
            .ok_or_else(|| Error::Substrate("native sequence range has no upper bound".into()))?;
        let index = database.scan(&start, Some(&end), snapshot)?;
        let expected = usize::try_from(last - from)
            .map_err(|_| Error::Substrate("native sequence range exceeds usize".into()))?;
        if index.len() != expected {
            return Err(Error::Substrate(format!(
                "native sequence index returned {} rows for expected interval ({from}, {last}]",
                index.len()
            )));
        }
        let mut claims = Vec::with_capacity(index.len());
        for (_, sequence_value) in index {
            if let Some(encoded) = decode_native_sequence_value(&sequence_value)? {
                claims.push(serde_json::from_slice(encoded)?);
                continue;
            }
            let encoded = database
                .get(&storage_key(keyspaces::CLAIMS, &sequence_value), snapshot)?
                .ok_or_else(|| {
                    Error::Substrate(format!(
                        "native sequence index references an absent claim in ({from}, {last}]"
                    ))
                })?;
            claims.push(serde_json::from_slice(&encoded)?);
        }
        Ok(claims)
    }

    fn subjects(&self) -> Result<Vec<Subject>> {
        let database = self.lock()?;
        let snapshot = database.snapshot();
        let mut subjects = Vec::new();
        for (stored_key, _) in scan_space(&database, snapshot, keyspaces::CLAIMS, &[])? {
            let claim_key = strip_space(keyspaces::CLAIMS, &stored_key)?;
            let (subject, _) = key::parse_claim_key(claim_key)?;
            if subjects
                .last()
                .is_none_or(|prior: &Subject| prior.as_str() != subject.as_str())
            {
                subjects.push(subject);
            }
        }
        Ok(subjects)
    }

    fn observe(
        &self,
        reader: &Reader,
        subject: &Subject,
        predicate: &Predicate,
        at: Millis,
    ) -> Result<()> {
        let mut database = self.lock()?;
        write(
            &mut database,
            vec![Mutation::Put {
                key: storage_key(
                    keyspaces::ACCESS,
                    &key::access_key(at, reader, subject, predicate),
                ),
                value: Vec::new(),
            }],
            Durability::Buffered,
        )?;
        Ok(())
    }

    fn get_projection(&self, name: &str) -> Result<Option<Vec<u8>>> {
        let database = self.lock()?;
        get(
            &database,
            database.snapshot(),
            keyspaces::PROJECTIONS,
            name.as_bytes(),
        )
    }

    fn put_projection_with(&self, name: &str, bytes: &[u8], durability: Durability) -> Result<()> {
        let mut database = self.lock()?;
        write(
            &mut database,
            vec![Mutation::Put {
                key: storage_key(keyspaces::PROJECTIONS, name.as_bytes()),
                value: bytes.to_vec(),
            }],
            durability,
        )?;
        Ok(())
    }

    fn runtime_cursor(&self) -> Result<u64> {
        let database = self.lock()?;
        read_sequence(&database, database.snapshot(), keyspaces::RUNTIME_CURSOR)
    }

    fn runtime_schema(&self, scope: &ScopeId) -> Result<Option<RuntimeSchemaRegistry>> {
        let database = self.lock()?;
        get_json(
            &database,
            database.snapshot(),
            keyspaces::RUNTIME_SCHEMAS,
            scope.as_str().as_bytes(),
        )
    }

    fn runtime_read_stamp(&self, scope: &ScopeId) -> Result<ReadStamp> {
        let database = self.lock()?;
        native_read_stamp(&database, database.snapshot(), scope)
    }

    fn open_runtime_snapshot(
        &self,
        scope: &ScopeId,
        owner: &str,
        now: Millis,
        ttl: Millis,
    ) -> Result<SnapshotHandle> {
        let mut database = self.lock()?;
        let handle = SnapshotHandle::new(
            native_read_stamp(&database, database.snapshot(), scope)?,
            owner,
            now,
            ttl,
        )?;
        if let Some(persisted) = get_json::<SnapshotHandle>(
            &database,
            database.snapshot(),
            keyspaces::RUNTIME_SNAPSHOTS,
            handle.id.as_str().as_bytes(),
        )? {
            if persisted != handle {
                return Err(Error::SnapshotMismatch(handle.id.to_string()));
            }
            ensure_runtime_checkpoint(&mut database, &handle, now)?;
            return Ok(handle);
        }
        write(
            &mut database,
            vec![Mutation::Put {
                key: storage_key(keyspaces::RUNTIME_SNAPSHOTS, handle.id.as_str().as_bytes()),
                value: serde_json::to_vec(&handle)?,
            }],
            Durability::Authoritative,
        )?;
        if let Err(error) = ensure_runtime_checkpoint(&mut database, &handle, now) {
            write(
                &mut database,
                vec![Mutation::Delete {
                    key: storage_key(keyspaces::RUNTIME_SNAPSHOTS, handle.id.as_str().as_bytes()),
                }],
                Durability::Authoritative,
            )?;
            return Err(error);
        }
        Ok(handle)
    }

    fn runtime_snapshot_changes(
        &self,
        handle: &SnapshotHandle,
        after: u64,
        limit: usize,
        now: Millis,
    ) -> Result<RuntimeChangePage> {
        handle.validate()?;
        let database = self.lock()?;
        let snapshot = database.snapshot();
        let persisted: SnapshotHandle = get_json(
            &database,
            snapshot,
            keyspaces::RUNTIME_SNAPSHOTS,
            handle.id.as_str().as_bytes(),
        )?
        .ok_or_else(|| Error::SnapshotNotFound(handle.id.to_string()))?;
        if &persisted != handle {
            return Err(Error::SnapshotMismatch(handle.id.to_string()));
        }
        if handle.is_expired(now) {
            return Err(Error::SnapshotExpired {
                id: handle.id.to_string(),
                expired_at: handle.expires_at,
            });
        }
        native_change_page(
            &database,
            snapshot,
            handle.read.commit_cursor,
            after,
            limit,
            Some(&handle.read.scope),
        )
    }

    fn release_runtime_snapshot(&self, id: &SnapshotId) -> Result<bool> {
        let mut database = self.lock()?;
        let key = storage_key(keyspaces::RUNTIME_SNAPSHOTS, id.as_str().as_bytes());
        if database.get(&key, database.snapshot())?.is_none() {
            return Ok(false);
        }
        write(
            &mut database,
            vec![Mutation::Delete { key }],
            Durability::Authoritative,
        )?;
        database.release_checkpoint(&runtime_checkpoint_name(id))?;
        Ok(true)
    }

    fn runtime_snapshots(&self, now: Millis) -> Result<Vec<SnapshotHandle>> {
        let database = self.lock()?;
        let snapshot = database.snapshot();
        let mut handles = scan_space(&database, snapshot, keyspaces::RUNTIME_SNAPSHOTS, &[])?
            .into_iter()
            .map(|(_, bytes)| serde_json::from_slice::<SnapshotHandle>(&bytes).map_err(Error::from))
            .collect::<Result<Vec<_>>>()?;
        for handle in &handles {
            handle.validate()?;
        }
        handles.retain(|handle| !handle.is_expired(now));
        handles.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(handles)
    }

    fn runtime_retention_pins(&self, now: Millis) -> Result<Vec<RetentionPin>> {
        self.runtime_snapshots(now)?
            .iter()
            .map(RetentionPin::from_snapshot)
            .collect::<vyrm_core::Result<Vec<_>>>()
            .map_err(Error::from)
    }

    fn runtime_read_changes(
        &self,
        read: &ReadStamp,
        after: u64,
        limit: usize,
    ) -> Result<RuntimeChangePage> {
        let database = self.lock()?;
        let snapshot = database.snapshot();
        validate_native_read_stamp(&database, snapshot, read)?;
        native_change_page(
            &database,
            snapshot,
            read.commit_cursor,
            after,
            limit,
            Some(&read.scope),
        )
    }

    fn commit_runtime(&self, commit: &RuntimeCommit) -> Result<RuntimeCommitOutcome> {
        let mut database = self.lock()?;
        let plan = prepare_native_runtime_commit(&database, commit)?;
        let (outcome, operations) = plan.into_parts();
        write(&mut database, operations, Durability::Authoritative)?;
        Ok(outcome)
    }

    fn runtime_changes_since(
        &self,
        after: u64,
        limit: usize,
        scope: Option<&ScopeId>,
    ) -> Result<RuntimeChangePage> {
        let database = self.lock()?;
        let snapshot = database.snapshot();
        let head = read_sequence(&database, snapshot, keyspaces::RUNTIME_CURSOR)?;
        native_change_page(&database, snapshot, head, after, limit, scope)
    }

    fn runtime_outbox_since(&self, after: u64, limit: usize) -> Result<Vec<ProjectionWork>> {
        if limit == 0 {
            return Err(Error::Substrate(
                "runtime outbox page limit must be greater than zero".into(),
            ));
        }
        let database = self.lock()?;
        let snapshot = database.snapshot();
        let start = after
            .checked_add(1)
            .ok_or(Error::SequenceOverflow)?
            .to_be_bytes();
        scan_space_from(&database, snapshot, keyspaces::RUNTIME_OUTBOX, &start)?
            .into_iter()
            .take(limit)
            .map(|(_, bytes)| {
                let work: ProjectionWork = serde_json::from_slice(&bytes)?;
                work.validate()?;
                Ok(work)
            })
            .collect()
    }

    fn runtime_audit(&self, commit_id: &str) -> Result<Option<AuditEnvelope>> {
        let database = self.lock()?;
        let audit: Option<AuditEnvelope> = get_json(
            &database,
            database.snapshot(),
            keyspaces::RUNTIME_AUDIT,
            commit_id.as_bytes(),
        )?;
        if let Some(value) = &audit {
            value.validate()?;
        }
        Ok(audit)
    }

    fn runtime_commit_outcome(&self, commit_id: &str) -> Result<Option<RuntimeCommitOutcome>> {
        let database = self.lock()?;
        get_json(
            &database,
            database.snapshot(),
            keyspaces::RUNTIME_COMMITS,
            commit_id.as_bytes(),
        )
    }
}

fn encode_native_sequence_value(encoded_claim: &[u8]) -> Vec<u8> {
    let mut value = Vec::with_capacity(NATIVE_SEQUENCE_VALUE_MAGIC.len() + encoded_claim.len());
    value.extend_from_slice(NATIVE_SEQUENCE_VALUE_MAGIC);
    value.extend_from_slice(encoded_claim);
    value
}

fn decode_native_sequence_value(value: &[u8]) -> Result<Option<&[u8]>> {
    let Some(encoded) = value.strip_prefix(NATIVE_SEQUENCE_VALUE_MAGIC) else {
        return Ok(None);
    };
    if encoded.is_empty() {
        return Err(Error::Substrate("empty native sequence envelope".into()));
    }
    Ok(Some(encoded))
}

/// A validated native runtime transaction that has not yet crossed the WAL
/// durability boundary. The caller may append metadata operations and publish
/// the combined vector as one VyrmKV [`WriteBatch`].
///
/// Planning reads the supplied database's current snapshot. Correct callers
/// therefore hold the database's exclusive writer guard from planning through
/// publication; `NativeEngine` does this internally and the Raft adapter uses
/// the same discipline.
#[derive(Debug)]
pub struct NativeRuntimeCommitPlan {
    outcome: RuntimeCommitOutcome,
    operations: Vec<Mutation>,
}

/// Reads the exact native cursor/schema pair needed to prepare a runtime
/// transaction outside `NativeEngine` while retaining one database snapshot.
/// Coordinators use this before submitting the resulting commit through their
/// own durability boundary (for example, a Raft log).
pub fn native_runtime_commit_context(
    database: &Database,
    scope: &ScopeId,
) -> Result<(ReadStamp, Option<RuntimeSchemaRegistry>)> {
    let snapshot = database.snapshot();
    let read = native_read_stamp(database, snapshot, scope)?;
    let schema = get_json(
        database,
        snapshot,
        keyspaces::RUNTIME_SCHEMAS,
        scope.as_str().as_bytes(),
    )?;
    if schema
        .as_ref()
        .map(|value: &RuntimeSchemaRegistry| value.revision)
        != read.schema_revision
    {
        return Err(Error::Substrate(
            "native runtime schema differs from its read stamp".into(),
        ));
    }
    Ok((read, schema))
}

impl NativeRuntimeCommitPlan {
    pub fn outcome(&self) -> &RuntimeCommitOutcome {
        &self.outcome
    }

    pub fn into_parts(self) -> (RuntimeCommitOutcome, Vec<Mutation>) {
        (self.outcome, self.operations)
    }
}

/// Validates and lowers one canonical [`RuntimeCommit`] into native VyrmKV
/// mutations without writing them. This is the composition boundary used when
/// a coordinator must atomically include its own durable metadata.
pub fn prepare_native_runtime_commit(
    database: &Database,
    commit: &RuntimeCommit,
) -> Result<NativeRuntimeCommitPlan> {
    commit.validate()?;
    let snapshot = database.snapshot();
    let commit_id = commit.digest();
    let start = read_sequence(database, snapshot, keyspaces::RUNTIME_CURSOR)?;
    if start != commit.expected_cursor {
        return Err(Error::RuntimeConflict {
            expected: commit.expected_cursor,
            actual: start,
        });
    }

    let previous_schema: Option<RuntimeSchemaRegistry> = get_json(
        database,
        snapshot,
        keyspaces::RUNTIME_SCHEMAS,
        commit.scope.as_str().as_bytes(),
    )?;
    let proposed_schema = commit.mutations.iter().find_map(|mutation| match mutation {
        RuntimeMutation::Schema { registry } => Some(registry),
        _ => None,
    });
    let effective_schema = match (previous_schema.as_ref(), proposed_schema) {
        (None, Some(registry)) if registry.revision == 1 => registry,
        (None, Some(registry)) => {
            return Err(Error::RuntimeSchemaConflict {
                expected: 1,
                actual: registry.revision,
            });
        }
        (Some(previous), Some(registry))
            if registry.revision == previous.revision.saturating_add(1) =>
        {
            registry
        }
        (Some(previous), Some(registry)) => {
            return Err(Error::RuntimeSchemaConflict {
                expected: previous.revision.saturating_add(1),
                actual: registry.revision,
            });
        }
        (Some(previous), None) => previous,
        (None, None) => {
            return Err(Error::RuntimeSchemaMissing(commit.scope.to_string()));
        }
    };
    let existing_records = if effective_schema
        .records
        .values()
        .any(|schema| !schema.unique_properties.is_empty())
    {
        native_values_for_scope::<RuntimeRecord>(
            database,
            snapshot,
            keyspaces::RUNTIME_RECORDS,
            &commit.scope,
        )?
    } else {
        Vec::new()
    };
    let existing_relations = if effective_schema.relations.values().any(|schema| {
        schema.unique_pair || schema.max_outgoing.is_some() || schema.max_incoming.is_some()
    }) {
        native_values_for_scope::<RuntimeRelation>(
            database,
            snapshot,
            keyspaces::RUNTIME_RELATIONS,
            &commit.scope,
        )?
    } else {
        Vec::new()
    };
    effective_schema.validate_objects(&commit.mutations, &existing_records, &existing_relations)?;

    let new_records = commit
        .mutations
        .iter()
        .filter_map(|mutation| match mutation {
            RuntimeMutation::Record { record } => Some(record.reference.clone()),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    for mutation in &commit.mutations {
        let references: Vec<&RuntimeRef> = match mutation {
            RuntimeMutation::Relation { relation } => vec![&relation.from, &relation.to],
            RuntimeMutation::Event { event } => event.subject.iter().collect(),
            RuntimeMutation::Vector { vector } => vec![&vector.subject],
            RuntimeMutation::SeriesSample { sample } => vec![&sample.series],
            RuntimeMutation::Geo { geo } => vec![&geo.subject],
            RuntimeMutation::Object { object } => object.subject.iter().collect(),
            RuntimeMutation::Claim { .. }
            | RuntimeMutation::Schema { .. }
            | RuntimeMutation::Record { .. } => Vec::new(),
        };
        for reference in references {
            if !new_records.contains(reference)
                && get(
                    database,
                    snapshot,
                    keyspaces::RUNTIME_RECORDS,
                    &runtime_identity_key(&commit.scope, reference),
                )?
                .is_none()
            {
                return Err(Error::DanglingRuntimeReference(format!(
                    "{}/{} in scope {}",
                    reference.kind, reference.id, commit.scope
                )));
            }
        }
    }

    let claim_count = commit
        .mutations
        .iter()
        .filter(|mutation| matches!(mutation, RuntimeMutation::Claim { .. }))
        .count();
    let claim_start = read_sequence(database, snapshot, keyspaces::SEQUENCE_WATERMARK)?;
    let mut claim_sequence = claim_start;
    let mut cursor = start;
    let mut previous_digest = get(
        database,
        snapshot,
        keyspaces::META,
        keyspaces::RUNTIME_LAST_DIGEST,
    )?
    .map(String::from_utf8)
    .transpose()
    .map_err(|error| Error::CorruptWatermark(error.to_string()))?
    .filter(|digest| !digest.is_empty());
    let previous_audit_digest = get(
        database,
        snapshot,
        keyspaces::META,
        keyspaces::RUNTIME_LAST_AUDIT_DIGEST,
    )?
    .map(String::from_utf8)
    .transpose()
    .map_err(|error| Error::CorruptWatermark(error.to_string()))?
    .filter(|digest| !digest.is_empty());
    let mut operations = Vec::new();
    let mut outbox_count = 0;

    for (ordinal, mutation) in commit.mutations.iter().cloned().enumerate() {
        if let RuntimeMutation::Claim { claim } = &mutation {
            claim.validate()?;
            claim_sequence = claim_sequence
                .checked_add(1)
                .ok_or(Error::SequenceOverflow)?;
            let claim_key = key::claim_key(
                &claim.subject,
                &claim.predicate,
                claim.valid_from,
                claim.tx_time,
            );
            put(
                &mut operations,
                keyspaces::SEQUENCE_INDEX,
                &key::sequence_key(claim_sequence),
                claim_key.clone(),
            );
            put(
                &mut operations,
                keyspaces::CLAIMS,
                &claim_key,
                serde_json::to_vec(claim)?,
            );
        }

        cursor = cursor.checked_add(1).ok_or(Error::SequenceOverflow)?;
        let change = RuntimeChange::committed(
            cursor,
            commit,
            &commit_id,
            ordinal as u64,
            mutation.clone(),
            previous_digest.clone(),
        );
        put(
            &mut operations,
            keyspaces::RUNTIME_CHANGES,
            &cursor.to_be_bytes(),
            serde_json::to_vec(&change)?,
        );
        if let Some(family) = projection_family(&mutation) {
            let work = ProjectionWork::for_change(
                commit.scope.clone(),
                cursor,
                commit_id.clone(),
                ordinal as u64,
                family,
            )?;
            put(
                &mut operations,
                keyspaces::RUNTIME_OUTBOX,
                &cursor.to_be_bytes(),
                serde_json::to_vec(&work)?,
            );
            outbox_count += 1;
        }
        match mutation {
            RuntimeMutation::Schema { registry } => put(
                &mut operations,
                keyspaces::RUNTIME_SCHEMAS,
                commit.scope.as_str().as_bytes(),
                serde_json::to_vec(&registry)?,
            ),
            RuntimeMutation::Record { record } => put(
                &mut operations,
                keyspaces::RUNTIME_RECORDS,
                &runtime_identity_key(&commit.scope, &record.reference),
                serde_json::to_vec(&record)?,
            ),
            RuntimeMutation::Relation { relation } => put(
                &mut operations,
                keyspaces::RUNTIME_RELATIONS,
                &runtime_identity_key(&commit.scope, &relation.reference),
                serde_json::to_vec(&relation)?,
            ),
            RuntimeMutation::Vector { vector } => put(
                &mut operations,
                keyspaces::RUNTIME_VECTORS,
                &runtime_identity_key(&commit.scope, &vector.reference),
                serde_json::to_vec(&vector)?,
            ),
            RuntimeMutation::SeriesSample { sample } => put(
                &mut operations,
                keyspaces::RUNTIME_SERIES,
                &runtime_identity_key(&commit.scope, &sample.reference),
                serde_json::to_vec(&sample)?,
            ),
            RuntimeMutation::Geo { geo } => put(
                &mut operations,
                keyspaces::RUNTIME_GEO,
                &runtime_identity_key(&commit.scope, &geo.reference),
                serde_json::to_vec(&geo)?,
            ),
            RuntimeMutation::Object { object } => put(
                &mut operations,
                keyspaces::RUNTIME_OBJECTS,
                &runtime_identity_key(&commit.scope, &object.reference),
                serde_json::to_vec(&object)?,
            ),
            RuntimeMutation::Claim { .. } | RuntimeMutation::Event { .. } => {}
        }
        previous_digest = Some(change.digest);
    }
    if claim_count > 0 {
        put_sequence(
            &mut operations,
            keyspaces::SEQUENCE_WATERMARK,
            claim_sequence,
        );
    }
    put_sequence(&mut operations, keyspaces::RUNTIME_CURSOR, cursor);
    put(
        &mut operations,
        keyspaces::META,
        keyspaces::RUNTIME_LAST_DIGEST,
        previous_digest.as_deref().unwrap_or("").as_bytes().to_vec(),
    );
    let audit = AuditEnvelope::accepted_commit(commit, &commit_id, cursor, previous_audit_digest)?;
    put(
        &mut operations,
        keyspaces::RUNTIME_AUDIT,
        commit_id.as_bytes(),
        serde_json::to_vec(&audit)?,
    );
    put(
        &mut operations,
        keyspaces::META,
        keyspaces::RUNTIME_LAST_AUDIT_DIGEST,
        audit.digest.as_bytes().to_vec(),
    );
    let outcome = RuntimeCommitOutcome {
        commit_id,
        first_cursor: start + 1,
        last_cursor: cursor,
        count: commit.mutations.len(),
        first_claim_sequence: (claim_count > 0).then_some(claim_start + 1),
        last_claim_sequence: (claim_count > 0).then_some(claim_sequence),
        outbox_count,
    };
    put(
        &mut operations,
        keyspaces::RUNTIME_COMMITS,
        outcome.commit_id.as_bytes(),
        serde_json::to_vec(&outcome)?,
    );
    Ok(NativeRuntimeCommitPlan {
        outcome,
        operations,
    })
}

/// Reads a previously accepted native runtime outcome from a caller-held
/// database snapshot. Coordinators use this before planning so content-addressed
/// retries remain idempotent even when the transport request id changes.
pub fn native_runtime_commit_outcome(
    database: &Database,
    commit_id: &str,
) -> Result<Option<RuntimeCommitOutcome>> {
    let outcome: Option<RuntimeCommitOutcome> = get_json(
        database,
        database.snapshot(),
        keyspaces::RUNTIME_COMMITS,
        commit_id.as_bytes(),
    )?;
    if outcome
        .as_ref()
        .is_some_and(|outcome| outcome.commit_id != commit_id)
    {
        return Err(Error::Substrate(
            "runtime commit outcome key does not match its content identity".into(),
        ));
    }
    Ok(outcome)
}

fn scan_claims(database: &Database, prefix: Vec<u8>, from: Vec<u8>) -> Result<Vec<Claim>> {
    let snapshot = database.snapshot();
    let start = storage_key(keyspaces::CLAIMS, &from);
    let full_prefix = storage_key(keyspaces::CLAIMS, &prefix);
    let end = prefix_end(&full_prefix)
        .ok_or_else(|| Error::Substrate("native claim prefix has no upper bound".into()))?;
    database
        .scan(&start, Some(&end), snapshot)?
        .into_iter()
        .map(|(_, value)| serde_json::from_slice(&value).map_err(Error::from))
        .collect()
}

fn ensure_runtime_checkpoint(
    database: &mut Database,
    handle: &SnapshotHandle,
    at: Millis,
) -> Result<()> {
    let name = runtime_checkpoint_name(&handle.id);
    if database
        .checkpoints()?
        .iter()
        .any(|checkpoint| checkpoint.name == name)
    {
        return Ok(());
    }
    let created_at = at.max(database.manifest().created_at);
    database.flush_memtable(created_at)?;
    database.checkpoint(&name, created_at)?;
    Ok(())
}

fn reconcile_runtime_checkpoints(
    database: &mut Database,
    now: Option<Millis>,
    at: Millis,
) -> Result<()> {
    let snapshot = database.snapshot();
    let handles = scan_space(database, snapshot, keyspaces::RUNTIME_SNAPSHOTS, &[])?
        .into_iter()
        .map(|(_, bytes)| serde_json::from_slice::<SnapshotHandle>(&bytes).map_err(Error::from))
        .collect::<Result<Vec<_>>>()?;
    for handle in &handles {
        handle.validate()?;
    }
    let desired = handles
        .iter()
        .filter(|handle| now.is_none_or(|now| !handle.is_expired(now)))
        .map(|handle| runtime_checkpoint_name(&handle.id))
        .collect::<BTreeSet<_>>();
    let checkpoints = database.checkpoints()?;
    let existing = checkpoints
        .iter()
        .filter(|checkpoint| checkpoint.name.starts_with(RUNTIME_CHECKPOINT_PREFIX))
        .map(|checkpoint| checkpoint.name.clone())
        .collect::<BTreeSet<_>>();
    let missing = desired.difference(&existing).cloned().collect::<Vec<_>>();
    if !missing.is_empty() {
        let created_at = at.max(database.manifest().created_at);
        database.flush_memtable(created_at)?;
        for name in missing {
            database.checkpoint(&name, created_at)?;
        }
    }
    for name in existing.difference(&desired) {
        database.release_checkpoint(name)?;
    }
    Ok(())
}

fn runtime_checkpoint_name(id: &SnapshotId) -> String {
    format!("{RUNTIME_CHECKPOINT_PREFIX}{}", id.as_str())
}

fn native_read_stamp(
    database: &Database,
    snapshot: Snapshot,
    scope: &ScopeId,
) -> Result<ReadStamp> {
    let commit_cursor = read_sequence(database, snapshot, keyspaces::RUNTIME_CURSOR)?;
    let schema_revision = get_json::<RuntimeSchemaRegistry>(
        database,
        snapshot,
        keyspaces::RUNTIME_SCHEMAS,
        scope.as_str().as_bytes(),
    )?
    .map(|schema| schema.revision);
    let head_digest = get(
        database,
        snapshot,
        keyspaces::META,
        keyspaces::RUNTIME_LAST_DIGEST,
    )?
    .map(String::from_utf8)
    .transpose()
    .map_err(|error| Error::CorruptWatermark(error.to_string()))?
    .filter(|digest| !digest.is_empty());
    ReadStamp::new(
        scope.clone(),
        schema_revision,
        0,
        commit_cursor,
        head_digest,
    )
    .map_err(Error::from)
}

fn validate_native_read_stamp(
    database: &Database,
    snapshot: Snapshot,
    read: &ReadStamp,
) -> Result<()> {
    read.validate()?;
    let current = read_sequence(database, snapshot, keyspaces::RUNTIME_CURSOR)?;
    if read.commit_cursor > current {
        return Err(Error::ReadStampUnavailable(read.manifest_id.clone()));
    }
    let retained_head = if read.commit_cursor == 0 {
        None
    } else {
        let change: RuntimeChange = get_json(
            database,
            snapshot,
            keyspaces::RUNTIME_CHANGES,
            &read.commit_cursor.to_be_bytes(),
        )?
        .ok_or_else(|| Error::ReadStampUnavailable(read.manifest_id.clone()))?;
        if !change.verify_digest() {
            return Err(Error::Substrate(format!(
                "runtime change {} failed digest verification",
                read.commit_cursor
            )));
        }
        Some(change.digest)
    };
    let page = native_change_page(
        database,
        snapshot,
        read.commit_cursor,
        0,
        usize::MAX,
        Some(&read.scope),
    )?;
    let schema_revision = page
        .changes
        .iter()
        .filter_map(|change| match &change.mutation {
            RuntimeMutation::Schema { registry } => Some(registry.revision),
            _ => None,
        })
        .next_back();
    if read.catalog_revision != 0
        || read.head_digest != retained_head
        || read.schema_revision != schema_revision
    {
        return Err(Error::ReadStampMismatch(read.manifest_id.clone()));
    }
    Ok(())
}

fn native_change_page(
    database: &Database,
    snapshot: Snapshot,
    head: u64,
    after: u64,
    limit: usize,
    scope: Option<&ScopeId>,
) -> Result<RuntimeChangePage> {
    if limit == 0 {
        return Err(Error::Substrate(
            "runtime change page limit must be greater than zero".into(),
        ));
    }
    if after == u64::MAX || after >= head {
        return Ok(RuntimeChangePage {
            requested_after: after,
            through_cursor: after,
            head_cursor: head,
            changes: Vec::new(),
        });
    }
    let mut previous_digest = if after == 0 {
        None
    } else {
        let prior: RuntimeChange = get_json(
            database,
            snapshot,
            keyspaces::RUNTIME_CHANGES,
            &after.to_be_bytes(),
        )?
        .ok_or_else(|| Error::Substrate(format!("runtime log is missing cursor {after}")))?;
        if !prior.verify_digest() {
            return Err(Error::Substrate(format!(
                "runtime change {after} failed digest verification"
            )));
        }
        Some(prior.digest)
    };
    let mut through = after;
    let mut selected = Vec::new();
    for expected in after + 1..=head {
        if through.saturating_sub(after) as usize >= limit {
            break;
        }
        let change: RuntimeChange = get_json(
            database,
            snapshot,
            keyspaces::RUNTIME_CHANGES,
            &expected.to_be_bytes(),
        )?
        .ok_or_else(|| Error::Substrate(format!("runtime log is missing cursor {expected}")))?;
        if change.cursor != expected
            || change.previous_digest != previous_digest
            || !change.verify_digest()
        {
            return Err(Error::Substrate(format!(
                "runtime change {expected} failed cursor/hash-chain verification"
            )));
        }
        through = expected;
        previous_digest = Some(change.digest.clone());
        if scope.is_none_or(|scope| scope == &change.scope) {
            selected.push(change);
        }
    }
    Ok(RuntimeChangePage {
        requested_after: after,
        through_cursor: through,
        head_cursor: head,
        changes: selected,
    })
}

fn native_values_for_scope<T: DeserializeOwned>(
    database: &Database,
    snapshot: Snapshot,
    space: &str,
    scope: &ScopeId,
) -> Result<Vec<T>> {
    let mut prefix = scope.as_str().as_bytes().to_vec();
    prefix.push(0);
    scan_space(database, snapshot, space, &prefix)?
        .into_iter()
        .map(|(_, value)| serde_json::from_slice(&value).map_err(Error::from))
        .collect()
}

/// Reads the exact immutable-object closure for one scope from an authenticated
/// physical snapshot before that snapshot is installed on a replica.
pub fn native_snapshot_object_references(
    bundle: &SnapshotBundleFile,
    scope: &ScopeId,
) -> Result<Vec<ObjectReference>> {
    native_snapshot_artifact_view(bundle, scope).map(|(_, objects)| objects)
}

/// Reads the exact project read stamp and immutable-object closure directly
/// from an authenticated physical snapshot.
pub fn native_snapshot_artifact_view(
    bundle: &SnapshotBundleFile,
    scope: &ScopeId,
) -> Result<(ReadStamp, Vec<ObjectReference>)> {
    let cursor_key = storage_key(keyspaces::META, keyspaces::RUNTIME_CURSOR);
    let digest_key = storage_key(keyspaces::META, keyspaces::RUNTIME_LAST_DIGEST);
    let schema_key = storage_key(keyspaces::RUNTIME_SCHEMAS, scope.as_str().as_bytes());
    let values = bundle
        .get_many(&[&cursor_key, &digest_key, &schema_key])
        .map_err(Error::from)?;
    let commit_cursor = values[0]
        .as_deref()
        .map(decode_sequence)
        .transpose()?
        .unwrap_or_default();
    let head_digest = values[1]
        .clone()
        .map(String::from_utf8)
        .transpose()
        .map_err(|error| Error::CorruptWatermark(error.to_string()))?
        .filter(|digest| !digest.is_empty());
    let schema_revision = values[2]
        .as_deref()
        .map(serde_json::from_slice::<RuntimeSchemaRegistry>)
        .transpose()?
        .map(|schema| schema.revision);
    let read = ReadStamp::new(
        scope.clone(),
        schema_revision,
        0,
        commit_cursor,
        head_digest,
    )?;
    let objects = native_snapshot_objects(bundle, Some(scope))?;
    Ok((read, objects))
}

/// Reads the project artifact view from a live native database snapshot. This
/// is used by the cluster adapter without reopening a second database handle.
pub fn native_database_artifact_view(
    database: &Database,
    scope: &ScopeId,
) -> Result<(ReadStamp, Vec<ObjectReference>)> {
    let snapshot = database.snapshot();
    let read = native_read_stamp(database, snapshot, scope)?;
    let rows = scan_space(database, snapshot, keyspaces::RUNTIME_OBJECTS, &[])?;
    let objects = decode_snapshot_objects(rows, Some(scope))?;
    Ok((read, objects))
}

/// Reads every immutable reference in a physical snapshot. Unlike the
/// project-specific transfer view, this permits multiple scopes and is the
/// final target-side activation gate.
pub fn native_snapshot_all_object_references(
    bundle: &SnapshotBundleFile,
) -> Result<Vec<ObjectReference>> {
    native_snapshot_objects(bundle, None)
}

fn native_snapshot_objects(
    bundle: &SnapshotBundleFile,
    required_scope: Option<&ScopeId>,
) -> Result<Vec<ObjectReference>> {
    let start = storage_key(keyspaces::RUNTIME_OBJECTS, &[]);
    let end = prefix_end(&start);
    let values = bundle.scan(&start, end.as_deref()).map_err(Error::from)?;
    decode_snapshot_objects(values, required_scope)
}

fn decode_snapshot_objects(
    values: Vec<(Vec<u8>, Vec<u8>)>,
    required_scope: Option<&ScopeId>,
) -> Result<Vec<ObjectReference>> {
    if values.len() > 1_000_000 {
        return Err(Error::Substrate(
            "native snapshot object-reference limit exceeded".into(),
        ));
    }
    let mut objects = values
        .into_iter()
        .map(|(stored_key, value)| {
            let object: ObjectReference = serde_json::from_slice(&value)?;
            object.validate()?;
            let logical = strip_space(keyspaces::RUNTIME_OBJECTS, &stored_key)?;
            let split = logical.iter().position(|byte| *byte == 0).ok_or_else(|| {
                Error::Substrate("native snapshot object key has no scope boundary".into())
            })?;
            let encoded_scope = std::str::from_utf8(&logical[..split])
                .map_err(|error| Error::Substrate(error.to_string()))?;
            let encoded_scope = ScopeId::new(encoded_scope)?;
            if required_scope.is_some_and(|scope| scope != &encoded_scope) {
                return Err(Error::Substrate(
                    "native snapshot object project scope differs from the transfer".into(),
                ));
            }
            let expected = storage_key(
                keyspaces::RUNTIME_OBJECTS,
                &runtime_identity_key(&encoded_scope, &object.reference),
            );
            if stored_key != expected {
                return Err(Error::Substrate(
                    "native snapshot object key/value identity differs from its canonical reference"
                        .into(),
                ));
            }
            Ok(object)
        })
        .collect::<Result<Vec<_>>>()?;
    objects.sort_by(|left, right| left.reference.cmp(&right.reference));
    if required_scope.is_some()
        && objects
            .windows(2)
            .any(|pair| pair[0].reference >= pair[1].reference)
    {
        return Err(Error::Substrate(
            "native snapshot contains duplicate object references".into(),
        ));
    }
    Ok(objects)
}

fn runtime_identity_key(scope: &ScopeId, reference: &RuntimeRef) -> Vec<u8> {
    let mut key = Vec::with_capacity(
        scope.as_str().len() + reference.kind.as_str().len() + reference.id.as_str().len() + 2,
    );
    key.extend_from_slice(scope.as_str().as_bytes());
    key.push(0);
    key.extend_from_slice(reference.kind.as_str().as_bytes());
    key.push(0);
    key.extend_from_slice(reference.id.as_str().as_bytes());
    key
}

fn write(database: &mut Database, operations: Vec<Mutation>, durability: Durability) -> Result<()> {
    database.write_owned(
        WriteBatch::new(operations)?,
        match durability {
            Durability::Authoritative => vyrm_kv::Durability::Authoritative,
            Durability::Buffered => vyrm_kv::Durability::Buffered,
        },
    )?;
    Ok(())
}

fn put(operations: &mut Vec<Mutation>, space: &str, key: &[u8], value: Vec<u8>) {
    operations.push(Mutation::Put {
        key: storage_key(space, key),
        value,
    });
}

fn put_sequence(operations: &mut Vec<Mutation>, key: &[u8], sequence: u64) {
    put(
        operations,
        keyspaces::META,
        key,
        sequence.to_string().into_bytes(),
    );
}

fn read_sequence(database: &Database, snapshot: Snapshot, key: &[u8]) -> Result<u64> {
    get(database, snapshot, keyspaces::META, key)?
        .as_deref()
        .map(decode_sequence)
        .transpose()
        .map(Option::unwrap_or_default)
}

fn decode_sequence(value: &[u8]) -> Result<u64> {
    std::str::from_utf8(value)
        .map_err(|error| Error::CorruptWatermark(error.to_string()))?
        .parse::<u64>()
        .map_err(|error| Error::CorruptWatermark(error.to_string()))
}

fn get(
    database: &Database,
    snapshot: Snapshot,
    space: &str,
    key: &[u8],
) -> Result<Option<Vec<u8>>> {
    database
        .get(&storage_key(space, key), snapshot)
        .map_err(Error::from)
}

fn get_json<T: DeserializeOwned>(
    database: &Database,
    snapshot: Snapshot,
    space: &str,
    key: &[u8],
) -> Result<Option<T>> {
    get(database, snapshot, space, key)?
        .map(|bytes| serde_json::from_slice(&bytes).map_err(Error::from))
        .transpose()
}

fn scan_space(
    database: &Database,
    snapshot: Snapshot,
    space: &str,
    prefix: &[u8],
) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
    let start = storage_key(space, prefix);
    let end = prefix_end(&start);
    database
        .scan(&start, end.as_deref(), snapshot)
        .map_err(Error::from)
}

fn scan_space_from(
    database: &Database,
    snapshot: Snapshot,
    space: &str,
    from: &[u8],
) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
    let start = storage_key(space, from);
    let end = prefix_end(&storage_key(space, &[]));
    database
        .scan(&start, end.as_deref(), snapshot)
        .map_err(Error::from)
}

fn storage_key(space: &str, key: &[u8]) -> Vec<u8> {
    let mut stored = Vec::with_capacity(space.len() + key.len() + 1);
    stored.extend_from_slice(space.as_bytes());
    stored.push(0);
    stored.extend_from_slice(key);
    stored
}

fn strip_space<'a>(space: &str, stored: &'a [u8]) -> Result<&'a [u8]> {
    let prefix = storage_key(space, &[]);
    stored
        .strip_prefix(prefix.as_slice())
        .ok_or_else(|| Error::Substrate(format!("key escaped native keyspace {space}")))
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

#[cfg(test)]
mod tests {
    use super::*;
    use vyrm_core::{ObjectReceipt, Producer};

    fn claim() -> Claim {
        Claim::new(
            vyrm_core::Subject::new("legacy-subject").unwrap(),
            vyrm_core::Predicate::new("legacy-predicate").unwrap(),
            "legacy-value",
            10,
            11,
            Producer {
                actor: "native-test".into(),
                on_behalf_of: None,
                session: None,
            },
        )
    }

    #[test]
    fn native_sequence_envelope_is_strict_and_canonical() {
        let claim = claim();
        let claim_key = key::claim_key(
            &claim.subject,
            &claim.predicate,
            claim.valid_from,
            claim.tx_time,
        );
        let encoded = serde_json::to_vec(&claim).unwrap();
        let envelope = encode_native_sequence_value(&encoded);
        assert_eq!(
            decode_native_sequence_value(&envelope).unwrap(),
            Some(encoded.as_slice())
        );
        assert_eq!(decode_native_sequence_value(&claim_key).unwrap(), None);
        assert!(decode_native_sequence_value(NATIVE_SEQUENCE_VALUE_MAGIC).is_err());
    }

    #[test]
    fn native_replay_reads_legacy_key_only_sequence_values() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("legacy-native");
        let claim = claim();
        let claim_key = key::claim_key(
            &claim.subject,
            &claim.predicate,
            claim.valid_from,
            claim.tx_time,
        );
        let mut database = Database::create(&root).unwrap();
        database
            .write_owned(
                WriteBatch::new(vec![
                    Mutation::Put {
                        key: storage_key(keyspaces::SEQUENCE_INDEX, &key::sequence_key(1)),
                        value: claim_key.clone(),
                    },
                    Mutation::Put {
                        key: storage_key(keyspaces::CLAIMS, &claim_key),
                        value: serde_json::to_vec(&claim).unwrap(),
                    },
                    Mutation::Put {
                        key: storage_key(keyspaces::META, keyspaces::SEQUENCE_WATERMARK),
                        value: b"1".to_vec(),
                    },
                ])
                .unwrap(),
                vyrm_kv::Durability::Authoritative,
            )
            .unwrap();
        drop(database);

        let engine = NativeEngine::open(&root).unwrap();
        assert_eq!(Engine::claims_in_range(&engine, 0, 1).unwrap(), vec![claim]);
    }

    #[test]
    fn snapshot_object_closure_denies_foreign_project_references() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("multi-project-native");
        let mut database = Database::create(&root).unwrap();
        let first_scope = ScopeId::new("project:first").unwrap();
        let second_scope = ScopeId::new("project:second").unwrap();
        let object = |id: &str, bytes: &[u8]| {
            let sha256 = vyrm_core::digest::sha256_hex(bytes);
            ObjectReference::for_bytes(
                id,
                None,
                "application/octet-stream",
                bytes,
                ObjectReceipt {
                    backend: "fixture".into(),
                    key: ObjectReference::canonical_key(&sha256).unwrap(),
                    version: None,
                    etag: None,
                },
            )
            .unwrap()
        };
        let first = object("first:bytes", b"first");
        let second = object("second:bytes", b"second");
        database
            .write_owned(
                WriteBatch::new(vec![
                    Mutation::Put {
                        key: storage_key(
                            keyspaces::RUNTIME_OBJECTS,
                            &runtime_identity_key(&first_scope, &first.reference),
                        ),
                        value: serde_json::to_vec(&first).unwrap(),
                    },
                    Mutation::Put {
                        key: storage_key(
                            keyspaces::RUNTIME_OBJECTS,
                            &runtime_identity_key(&second_scope, &second.reference),
                        ),
                        value: serde_json::to_vec(&second).unwrap(),
                    },
                ])
                .unwrap(),
                vyrm_kv::Durability::Authoritative,
            )
            .unwrap();
        let spool = directory.path().join("multi-project.snapshot");
        let bundle = database.export_snapshot_file(1, &spool).unwrap();

        let error = native_snapshot_object_references(&bundle, &first_scope).unwrap_err();
        assert!(error.to_string().contains("project scope"));
    }
}
