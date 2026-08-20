use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use vyrm_core::{
    digest::sha256_hex, ObjectReceipt, ObjectReference, ReadStamp, RuntimeRef, ScopeId,
};

pub const CLUSTER_CONTRACT_VERSION: u16 = 1;
pub const METADATA_SHARD_ID: ShardId = ShardId(0);
pub const ARTIFACT_TRANSFER_CHUNK_MAX_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClusterError {
    Invalid(String),
    Denied(String),
    Unavailable(String),
    NotFound(String),
}

impl fmt::Display for ClusterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(message) => write!(f, "invalid cluster contract: {message}"),
            Self::Denied(message) => write!(f, "cluster operation denied: {message}"),
            Self::Unavailable(message) => write!(f, "cluster unavailable: {message}"),
            Self::NotFound(message) => write!(f, "cluster object not found: {message}"),
        }
    }
}

impl std::error::Error for ClusterError {}

pub type Result<T> = std::result::Result<T, ClusterError>;

macro_rules! string_id {
    ($name:ident, $label:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self> {
                let value = value.into();
                if value.is_empty() || value.len() > 128 || value.as_bytes().contains(&0) {
                    return Err(ClusterError::Invalid(format!(
                        "{} must contain 1..=128 non-NUL bytes",
                        $label
                    )));
                }
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl TryFrom<String> for $name {
            type Error = ClusterError;

            fn try_from(value: String) -> Result<Self> {
                Self::new(value)
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

string_id!(ClusterId, "cluster id");
string_id!(NodeId, "node id");
string_id!(ZoneId, "zone id");

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ShardId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplicaRole {
    Voter,
    Learner,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplicaHealth {
    Active,
    Suspect,
    Recovering,
    Lost,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplicaPlacement {
    pub node: NodeId,
    pub zone: ZoneId,
    pub role: ReplicaRole,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlacementPolicy {
    pub voter_count: u8,
    pub minimum_voter_zones: u8,
    pub maximum_voters_per_zone: u8,
}

impl PlacementPolicy {
    pub fn validate(&self) -> Result<()> {
        if self.voter_count < 3 || self.voter_count.is_multiple_of(2) {
            return Err(ClusterError::Invalid(
                "voter_count must be an odd number of at least three".into(),
            ));
        }
        if self.minimum_voter_zones < 2 || self.minimum_voter_zones > self.voter_count {
            return Err(ClusterError::Invalid(
                "minimum_voter_zones must be in 2..=voter_count".into(),
            ));
        }
        if self.maximum_voters_per_zone == 0 {
            return Err(ClusterError::Invalid(
                "maximum_voters_per_zone must be non-zero".into(),
            ));
        }
        Ok(())
    }

    pub fn quorum(&self) -> usize {
        usize::from(self.voter_count / 2 + 1)
    }

    pub fn tolerated_failures(&self) -> usize {
        usize::from((self.voter_count - 1) / 2)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShardPlacement {
    pub contract_version: u16,
    pub cluster: ClusterId,
    pub shard: ShardId,
    pub epoch: u64,
    pub policy: PlacementPolicy,
    pub replicas: Vec<ReplicaPlacement>,
}

impl ShardPlacement {
    pub fn validate(&self) -> Result<()> {
        if self.contract_version != CLUSTER_CONTRACT_VERSION || self.epoch == 0 {
            return Err(ClusterError::Invalid(
                "unsupported contract version or zero placement epoch".into(),
            ));
        }
        self.policy.validate()?;
        if self
            .replicas
            .windows(2)
            .any(|pair| pair[0].node >= pair[1].node)
        {
            return Err(ClusterError::Invalid(
                "replicas must be strictly ordered by unique node id".into(),
            ));
        }
        let voters: Vec<_> = self
            .replicas
            .iter()
            .filter(|replica| replica.role == ReplicaRole::Voter)
            .collect();
        if voters.len() != usize::from(self.policy.voter_count) {
            return Err(ClusterError::Invalid(format!(
                "placement has {} voters but policy requires {}",
                voters.len(),
                self.policy.voter_count
            )));
        }
        let zones: BTreeSet<_> = voters.iter().map(|replica| &replica.zone).collect();
        if zones.len() < usize::from(self.policy.minimum_voter_zones) {
            return Err(ClusterError::Invalid(
                "placement does not satisfy voter zone diversity".into(),
            ));
        }
        let mut per_zone = BTreeMap::<&ZoneId, usize>::new();
        for voter in voters {
            *per_zone.entry(&voter.zone).or_default() += 1;
        }
        if per_zone
            .values()
            .any(|count| *count > usize::from(self.policy.maximum_voters_per_zone))
        {
            return Err(ClusterError::Invalid(
                "placement exceeds maximum voters per zone".into(),
            ));
        }
        Ok(())
    }

    pub fn voters(&self) -> impl Iterator<Item = &ReplicaPlacement> {
        self.replicas
            .iter()
            .filter(|replica| replica.role == ReplicaRole::Voter)
    }

    pub fn digest(&self) -> Result<String> {
        self.validate()?;
        let bytes = serde_json::to_vec(self).map_err(|error| {
            ClusterError::Invalid(format!("placement encoding failed: {error}"))
        })?;
        Ok(sha256_hex(&bytes))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReadConsistency {
    Linearizable,
    BoundedStale { maximum_index_lag: u64 },
    ExactSnapshot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WriteConsistency {
    QuorumDurable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShardReadStamp {
    pub term: u64,
    pub commit_index: u64,
    pub placement_epoch: u64,
    pub state_digest: String,
}

impl ShardReadStamp {
    pub fn validate(&self) -> Result<()> {
        if self.term == 0 || self.placement_epoch == 0 || !is_sha256(&self.state_digest) {
            return Err(ClusterError::Invalid(
                "shard stamp requires non-zero term/epoch and lowercase SHA-256 state digest"
                    .into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VectorRelation {
    Equal,
    Dominates,
    Dominated,
    Concurrent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotVector {
    pub contract_version: u16,
    pub scope: String,
    pub shards: BTreeMap<ShardId, ShardReadStamp>,
}

impl SnapshotVector {
    pub fn validate(&self) -> Result<()> {
        if self.contract_version != CLUSTER_CONTRACT_VERSION
            || self.scope.is_empty()
            || self.scope.len() > 256
            || self.shards.is_empty()
        {
            return Err(ClusterError::Invalid(
                "snapshot vector requires v1, a bounded scope, and at least one shard".into(),
            ));
        }
        for stamp in self.shards.values() {
            stamp.validate()?;
        }
        Ok(())
    }

    /// Partial-order comparison. `Concurrent` is intentionally preserved;
    /// Vyrm never fabricates a total cluster cursor.
    pub fn relation(&self, other: &Self) -> Result<VectorRelation> {
        self.validate()?;
        other.validate()?;
        if self.scope != other.scope || self.shards.keys().ne(other.shards.keys()) {
            return Err(ClusterError::Invalid(
                "snapshot vectors must cover the same scope and shard set".into(),
            ));
        }
        let mut saw_less = false;
        let mut saw_greater = false;
        for (shard, left) in &self.shards {
            let right = &other.shards[shard];
            if left.placement_epoch != right.placement_epoch {
                return Err(ClusterError::Invalid(
                    "snapshot vectors cross a placement epoch".into(),
                ));
            }
            match left.commit_index.cmp(&right.commit_index) {
                Ordering::Less => saw_less = true,
                Ordering::Greater => saw_greater = true,
                Ordering::Equal => {
                    if left.state_digest != right.state_digest || left.term != right.term {
                        return Err(ClusterError::Denied(
                            "equal shard cursors disagree on term or state digest".into(),
                        ));
                    }
                }
            }
        }
        Ok(match (saw_less, saw_greater) {
            (false, false) => VectorRelation::Equal,
            (false, true) => VectorRelation::Dominates,
            (true, false) => VectorRelation::Dominated,
            (true, true) => VectorRelation::Concurrent,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TransactionScope {
    SingleShard { shard: ShardId },
    CrossShard { shards: BTreeSet<ShardId> },
}

impl TransactionScope {
    pub fn enforce_m7(&self) -> Result<ShardId> {
        match self {
            Self::SingleShard { shard } => Ok(*shard),
            Self::CrossShard { .. } => Err(ClusterError::Denied(
                "cross-shard writes require durable intents and a verified commit protocol".into(),
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteEvidence {
    pub contract_version: u16,
    pub shard: ShardId,
    pub placement_epoch: u64,
    pub requested_consistency: ReadConsistency,
    pub selected: Vec<NodeId>,
    pub replica_health: BTreeMap<NodeId, ReplicaHealth>,
    pub observed: Option<ShardReadStamp>,
    pub allowed: bool,
    pub reason: String,
}

impl RouteEvidence {
    pub fn validate(&self) -> Result<()> {
        if self.contract_version != CLUSTER_CONTRACT_VERSION
            || self.placement_epoch == 0
            || self.reason.is_empty()
            || self.selected.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return Err(ClusterError::Invalid("malformed route evidence".into()));
        }
        if self.allowed && (self.selected.is_empty() || self.observed.is_none()) {
            return Err(ClusterError::Invalid(
                "an allowed route requires selected replicas and an observed stamp".into(),
            ));
        }
        if !self.allowed && self.observed.is_some() {
            return Err(ClusterError::Invalid(
                "a denied route cannot claim an observed snapshot".into(),
            ));
        }
        if self.allowed
            && self
                .selected
                .iter()
                .any(|node| self.replica_health.get(node).copied() != Some(ReplicaHealth::Active))
        {
            return Err(ClusterError::Invalid(
                "an allowed route may select only explicitly active replicas".into(),
            ));
        }
        if let Some(stamp) = &self.observed {
            stamp.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplicaTransferPlan {
    pub contract_version: u16,
    pub shard: ShardId,
    pub placement_epoch: u64,
    pub source: NodeId,
    pub target: NodeId,
    pub grounded_snapshot: ShardReadStamp,
    pub wal_from_exclusive: u64,
    pub wal_through_inclusive: u64,
    pub artifact_digests: BTreeSet<String>,
}

/// Exact immutable-object closure required to make one project scope serveable
/// after the grounded canonical snapshot is installed. Artifact bytes remain
/// outside the Raft/VyrmKV snapshot, while this manifest binds their transfer
/// to that snapshot and the exact scoped runtime read that named them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactTransferManifest {
    pub contract_version: u16,
    pub plan: ReplicaTransferPlan,
    pub scope: ScopeId,
    pub read: ReadStamp,
    pub objects: Vec<ObjectReference>,
    pub manifest_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactReplicaObjectReceipt {
    pub reference: RuntimeRef,
    pub sha256: String,
    pub length: u64,
    pub target: ObjectReceipt,
    pub transferred: bool,
}

/// Target-local residency evidence. It never rewrites the source publication
/// receipt embedded in canonical runtime truth.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactTransferReceipt {
    pub contract_version: u16,
    pub manifest_digest: String,
    pub source: NodeId,
    pub target: NodeId,
    pub objects: Vec<ArtifactReplicaObjectReceipt>,
    pub transferred_objects: u64,
    pub transferred_bytes: u64,
    pub completed_at: u64,
    pub receipt_digest: String,
}

/// One authenticated, replayable request in the out-of-band immutable-object
/// channel. The surrounding TLS transport additionally binds cluster, shard,
/// numeric/canonical peers, and the serialized request digest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactTransferRpc {
    pub contract_version: u16,
    pub operation: ArtifactTransferOperation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum ArtifactTransferOperation {
    Begin {
        manifest: Box<ArtifactTransferManifest>,
    },
    Chunk {
        manifest_digest: String,
        sha256: String,
        offset: u64,
        bytes: Vec<u8>,
        chunk_digest: String,
    },
    Complete {
        manifest_digest: String,
        completed_at: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactObjectProgress {
    pub sha256: String,
    pub expected_length: u64,
    pub next_offset: u64,
    pub complete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case", deny_unknown_fields)]
pub enum ArtifactTransferRpcResult {
    Progress {
        manifest_digest: String,
        objects: Vec<ArtifactObjectProgress>,
    },
    ChunkAccepted {
        manifest_digest: String,
        object: ArtifactObjectProgress,
    },
    Completed {
        receipt: ArtifactTransferReceipt,
    },
}

/// Bounded control-plane evidence emitted by the authenticated source. A
/// deployment may persist these observations through its normal project
/// runtime commit path, export them, or both; raw artifact bytes and errors
/// never enter the observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactTransferObservationPhase {
    Prepared,
    ChunkAccepted,
    Completed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactTransferObservation {
    pub contract_version: u16,
    pub phase: ArtifactTransferObservationPhase,
    pub at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_micros: Option<u64>,
    pub attempt: u64,
    pub scope: ScopeId,
    pub manifest_digest: String,
    pub source: NodeId,
    pub target: NodeId,
    pub shard: ShardId,
    pub placement_epoch: u64,
    pub grounded_snapshot: ShardReadStamp,
    pub read: ReadStamp,
    pub object_references: u64,
    pub distinct_objects: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_offset: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_length: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_digest: Option<String>,
    pub transferred_objects: u64,
    pub transferred_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_digest: Option<String>,
}

impl ArtifactTransferObservation {
    fn base(
        manifest: &ArtifactTransferManifest,
        phase: ArtifactTransferObservationPhase,
        attempt: u64,
        at: u64,
    ) -> Self {
        Self {
            contract_version: CLUSTER_CONTRACT_VERSION,
            phase,
            at,
            duration_micros: None,
            attempt,
            scope: manifest.scope.clone(),
            manifest_digest: manifest.manifest_digest.clone(),
            source: manifest.plan.source.clone(),
            target: manifest.plan.target.clone(),
            shard: manifest.plan.shard,
            placement_epoch: manifest.plan.placement_epoch,
            grounded_snapshot: manifest.plan.grounded_snapshot.clone(),
            read: manifest.read.clone(),
            object_references: manifest.objects.len() as u64,
            distinct_objects: manifest.plan.artifact_digests.len() as u64,
            object_digest: None,
            next_offset: None,
            expected_length: None,
            receipt_digest: None,
            transferred_objects: 0,
            transferred_bytes: 0,
            error_digest: None,
        }
    }

    pub fn prepared(manifest: &ArtifactTransferManifest, attempt: u64, at: u64) -> Result<Self> {
        manifest.validate()?;
        let observation = Self::base(
            manifest,
            ArtifactTransferObservationPhase::Prepared,
            attempt,
            at,
        );
        observation.validate()?;
        Ok(observation)
    }

    pub fn progress(
        manifest: &ArtifactTransferManifest,
        attempt: u64,
        at: u64,
        object: &ArtifactObjectProgress,
    ) -> Result<Self> {
        manifest.validate()?;
        let expected = manifest
            .objects
            .iter()
            .find(|candidate| candidate.sha256 == object.sha256)
            .ok_or_else(|| {
                ClusterError::Invalid("artifact progress object is absent from its manifest".into())
            })?;
        if expected.length != object.expected_length
            || object.next_offset > object.expected_length
            || object.complete != (object.next_offset == object.expected_length)
        {
            return Err(ClusterError::Invalid(
                "artifact progress differs from its manifest object".into(),
            ));
        }
        let mut observation = Self::base(
            manifest,
            ArtifactTransferObservationPhase::ChunkAccepted,
            attempt,
            at,
        );
        observation.object_digest = Some(object.sha256.clone());
        observation.next_offset = Some(object.next_offset);
        observation.expected_length = Some(object.expected_length);
        observation.validate()?;
        Ok(observation)
    }

    pub fn completed(
        manifest: &ArtifactTransferManifest,
        attempt: u64,
        at: u64,
        duration_micros: u64,
        receipt: &ArtifactTransferReceipt,
    ) -> Result<Self> {
        receipt.validate(manifest)?;
        let mut observation = Self::base(
            manifest,
            ArtifactTransferObservationPhase::Completed,
            attempt,
            at,
        );
        observation.receipt_digest = Some(receipt.receipt_digest.clone());
        observation.duration_micros = Some(duration_micros);
        observation.transferred_objects = receipt.transferred_objects;
        observation.transferred_bytes = receipt.transferred_bytes;
        observation.validate()?;
        Ok(observation)
    }

    pub fn failed(
        manifest: &ArtifactTransferManifest,
        attempt: u64,
        at: u64,
        duration_micros: u64,
        error: &str,
    ) -> Result<Self> {
        manifest.validate()?;
        let mut observation = Self::base(
            manifest,
            ArtifactTransferObservationPhase::Failed,
            attempt,
            at,
        );
        observation.error_digest = Some(sha256_hex(error.as_bytes()));
        observation.duration_micros = Some(duration_micros);
        observation.validate()?;
        Ok(observation)
    }

    pub fn validate(&self) -> Result<()> {
        self.read
            .validate()
            .map_err(|error| ClusterError::Invalid(error.to_string()))?;
        if self.contract_version != CLUSTER_CONTRACT_VERSION
            || self.attempt == 0
            || self.scope != self.read.scope
            || self.placement_epoch != self.grounded_snapshot.placement_epoch
            || self.distinct_objects > self.object_references
        {
            return Err(ClusterError::Invalid(
                "artifact observation identity or grounded coordinates are invalid".into(),
            ));
        }
        validate_sha256("artifact observation manifest", &self.manifest_digest)?;
        if self.grounded_snapshot.validate().is_err() {
            return Err(ClusterError::Invalid(
                "artifact observation snapshot is invalid".into(),
            ));
        }
        match self.phase {
            ArtifactTransferObservationPhase::Prepared => {
                if self.object_digest.is_some()
                    || self.next_offset.is_some()
                    || self.expected_length.is_some()
                    || self.receipt_digest.is_some()
                    || self.error_digest.is_some()
                    || self.transferred_objects != 0
                    || self.transferred_bytes != 0
                    || self.duration_micros.is_some()
                {
                    return Err(ClusterError::Invalid(
                        "prepared artifact observation contains terminal evidence".into(),
                    ));
                }
            }
            ArtifactTransferObservationPhase::ChunkAccepted => {
                let (Some(digest), Some(offset), Some(length)) = (
                    self.object_digest.as_deref(),
                    self.next_offset,
                    self.expected_length,
                ) else {
                    return Err(ClusterError::Invalid(
                        "artifact progress observation is incomplete".into(),
                    ));
                };
                validate_sha256("artifact observation object", digest)?;
                if offset > length
                    || self.receipt_digest.is_some()
                    || self.error_digest.is_some()
                    || self.transferred_objects != 0
                    || self.transferred_bytes != 0
                    || self.duration_micros.is_some()
                {
                    return Err(ClusterError::Invalid(
                        "artifact progress observation is inconsistent".into(),
                    ));
                }
            }
            ArtifactTransferObservationPhase::Completed => {
                let Some(receipt) = self.receipt_digest.as_deref() else {
                    return Err(ClusterError::Invalid(
                        "completed artifact observation has no receipt".into(),
                    ));
                };
                validate_sha256("artifact observation receipt", receipt)?;
                if self.object_digest.is_some()
                    || self.next_offset.is_some()
                    || self.expected_length.is_some()
                    || self.error_digest.is_some()
                    || self.duration_micros.is_none()
                {
                    return Err(ClusterError::Invalid(
                        "completed artifact observation contains progress or error evidence".into(),
                    ));
                }
            }
            ArtifactTransferObservationPhase::Failed => {
                let Some(error) = self.error_digest.as_deref() else {
                    return Err(ClusterError::Invalid(
                        "failed artifact observation has no error digest".into(),
                    ));
                };
                validate_sha256("artifact observation error", error)?;
                if self.object_digest.is_some()
                    || self.next_offset.is_some()
                    || self.expected_length.is_some()
                    || self.receipt_digest.is_some()
                    || self.transferred_objects != 0
                    || self.transferred_bytes != 0
                    || self.duration_micros.is_none()
                {
                    return Err(ClusterError::Invalid(
                        "failed artifact observation contains success evidence".into(),
                    ));
                }
            }
        }
        Ok(())
    }
}

/// Deployment-owned persistence/export boundary for transfer observations.
/// Implementations must be thread-safe because OpenRaft can replicate several
/// learners concurrently.
pub trait ArtifactTransferObserver: Send + Sync {
    fn observe(&self, observation: ArtifactTransferObservation) -> Result<()>;
}

impl ArtifactTransferRpc {
    pub fn begin(manifest: ArtifactTransferManifest) -> Result<Self> {
        let request = Self {
            contract_version: CLUSTER_CONTRACT_VERSION,
            operation: ArtifactTransferOperation::Begin {
                manifest: Box::new(manifest),
            },
        };
        request.validate()?;
        Ok(request)
    }

    pub fn chunk(
        manifest_digest: impl Into<String>,
        sha256: impl Into<String>,
        offset: u64,
        bytes: Vec<u8>,
    ) -> Result<Self> {
        let chunk_digest = sha256_hex(&bytes);
        let request = Self {
            contract_version: CLUSTER_CONTRACT_VERSION,
            operation: ArtifactTransferOperation::Chunk {
                manifest_digest: manifest_digest.into(),
                sha256: sha256.into(),
                offset,
                bytes,
                chunk_digest,
            },
        };
        request.validate()?;
        Ok(request)
    }

    pub fn complete(manifest_digest: impl Into<String>, completed_at: u64) -> Result<Self> {
        let request = Self {
            contract_version: CLUSTER_CONTRACT_VERSION,
            operation: ArtifactTransferOperation::Complete {
                manifest_digest: manifest_digest.into(),
                completed_at,
            },
        };
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<()> {
        if self.contract_version != CLUSTER_CONTRACT_VERSION {
            return Err(ClusterError::Invalid(
                "artifact RPC contract version is unsupported".into(),
            ));
        }
        match &self.operation {
            ArtifactTransferOperation::Begin { manifest } => manifest.validate(),
            ArtifactTransferOperation::Chunk {
                manifest_digest,
                sha256,
                bytes,
                chunk_digest,
                ..
            } => {
                validate_sha256("artifact RPC manifest", manifest_digest)?;
                validate_sha256("artifact RPC object", sha256)?;
                validate_sha256("artifact RPC chunk", chunk_digest)?;
                if bytes.is_empty() || bytes.len() > ARTIFACT_TRANSFER_CHUNK_MAX_BYTES {
                    return Err(ClusterError::Invalid(format!(
                        "artifact RPC chunks must contain 1..={ARTIFACT_TRANSFER_CHUNK_MAX_BYTES} bytes"
                    )));
                }
                if sha256_hex(bytes) != *chunk_digest {
                    return Err(ClusterError::Invalid(
                        "artifact RPC chunk digest differs from its bytes".into(),
                    ));
                }
                Ok(())
            }
            ArtifactTransferOperation::Complete {
                manifest_digest, ..
            } => validate_sha256("artifact RPC manifest", manifest_digest),
        }
    }
}

fn validate_sha256(label: &str, value: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(ClusterError::Invalid(format!(
            "{label} digest must be 64 lowercase hexadecimal bytes"
        )));
    }
    Ok(())
}

impl ArtifactTransferManifest {
    pub fn new(
        plan: ReplicaTransferPlan,
        scope: ScopeId,
        read: ReadStamp,
        mut objects: Vec<ObjectReference>,
    ) -> Result<Self> {
        objects.sort_by(|left, right| left.reference.cmp(&right.reference));
        let mut manifest = Self {
            contract_version: CLUSTER_CONTRACT_VERSION,
            plan,
            scope,
            read,
            objects,
            manifest_digest: String::new(),
        };
        manifest.validate_components()?;
        manifest.manifest_digest = sha256_hex(&manifest.identity_bytes()?);
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<()> {
        self.validate_components()?;
        if self.manifest_digest != sha256_hex(&self.identity_bytes()?) {
            return Err(ClusterError::Invalid(
                "artifact transfer manifest digest differs from its fields".into(),
            ));
        }
        Ok(())
    }

    fn validate_components(&self) -> Result<()> {
        self.plan.validate()?;
        self.read
            .validate()
            .map_err(|error| ClusterError::Invalid(error.to_string()))?;
        if self.contract_version != CLUSTER_CONTRACT_VERSION || self.read.scope != self.scope {
            return Err(ClusterError::Invalid(
                "artifact manifest version, scope, or read binding is invalid".into(),
            ));
        }
        if self.objects.len() > 1_000_000 {
            return Err(ClusterError::Invalid(
                "artifact manifest object limit exceeded".into(),
            ));
        }
        let mut references = BTreeSet::new();
        let mut digests = BTreeSet::new();
        let mut previous = None;
        for object in &self.objects {
            object
                .validate()
                .map_err(|error| ClusterError::Invalid(error.to_string()))?;
            if previous
                .as_ref()
                .is_some_and(|value| value >= &object.reference)
                || !references.insert(object.reference.clone())
            {
                return Err(ClusterError::Invalid(
                    "artifact manifest objects must have unique canonical ordering".into(),
                ));
            }
            previous = Some(object.reference.clone());
            digests.insert(object.sha256.clone());
        }
        if digests != self.plan.artifact_digests {
            return Err(ClusterError::Invalid(
                "artifact manifest objects differ from the replica transfer plan".into(),
            ));
        }
        Ok(())
    }

    fn identity_bytes(&self) -> Result<Vec<u8>> {
        serde_json::to_vec(&(
            self.contract_version,
            &self.plan,
            &self.scope,
            &self.read,
            &self.objects,
        ))
        .map_err(|error| ClusterError::Invalid(format!("artifact manifest encode failed: {error}")))
    }
}

impl ArtifactTransferReceipt {
    pub fn new(
        manifest: &ArtifactTransferManifest,
        objects: Vec<ArtifactReplicaObjectReceipt>,
        completed_at: u64,
    ) -> Result<Self> {
        let transferred_objects = objects.iter().filter(|object| object.transferred).count() as u64;
        let transferred_bytes = objects
            .iter()
            .filter(|object| object.transferred)
            .try_fold(0u64, |total, object| total.checked_add(object.length))
            .ok_or_else(|| {
                ClusterError::Invalid("artifact transfer byte count overflowed".into())
            })?;
        let mut receipt = Self {
            contract_version: CLUSTER_CONTRACT_VERSION,
            manifest_digest: manifest.manifest_digest.clone(),
            source: manifest.plan.source.clone(),
            target: manifest.plan.target.clone(),
            objects,
            transferred_objects,
            transferred_bytes,
            completed_at,
            receipt_digest: String::new(),
        };
        receipt.validate_components(manifest)?;
        receipt.receipt_digest = sha256_hex(&receipt.identity_bytes()?);
        Ok(receipt)
    }

    pub fn validate(&self, manifest: &ArtifactTransferManifest) -> Result<()> {
        self.validate_components(manifest)?;
        if self.receipt_digest != sha256_hex(&self.identity_bytes()?) {
            return Err(ClusterError::Invalid(
                "artifact transfer receipt digest differs from its fields".into(),
            ));
        }
        Ok(())
    }

    fn validate_components(&self, manifest: &ArtifactTransferManifest) -> Result<()> {
        manifest.validate()?;
        if self.contract_version != CLUSTER_CONTRACT_VERSION
            || self.manifest_digest != manifest.manifest_digest
            || self.source != manifest.plan.source
            || self.target != manifest.plan.target
            || self.objects.len() != manifest.objects.len()
        {
            return Err(ClusterError::Invalid(
                "artifact transfer receipt identity differs from its manifest".into(),
            ));
        }
        let mut transferred_objects = 0u64;
        let mut transferred_bytes = 0u64;
        for (receipt, object) in self.objects.iter().zip(&manifest.objects) {
            let target_reference = ObjectReference {
                reference: receipt.reference.clone(),
                subject: object.subject.clone(),
                sha256: receipt.sha256.clone(),
                length: receipt.length,
                media_type: object.media_type.clone(),
                receipt: receipt.target.clone(),
                properties: object.properties.clone(),
            };
            target_reference
                .validate()
                .map_err(|error| ClusterError::Invalid(error.to_string()))?;
            if receipt.reference != object.reference
                || receipt.sha256 != object.sha256
                || receipt.length != object.length
            {
                return Err(ClusterError::Invalid(
                    "artifact object receipt differs from its manifest object".into(),
                ));
            }
            if receipt.transferred {
                transferred_objects += 1;
                transferred_bytes =
                    transferred_bytes
                        .checked_add(receipt.length)
                        .ok_or_else(|| {
                            ClusterError::Invalid("artifact transfer byte count overflowed".into())
                        })?;
            }
        }
        if transferred_objects != self.transferred_objects
            || transferred_bytes != self.transferred_bytes
        {
            return Err(ClusterError::Invalid(
                "artifact transfer receipt counters differ from its objects".into(),
            ));
        }
        Ok(())
    }

    fn identity_bytes(&self) -> Result<Vec<u8>> {
        serde_json::to_vec(&(
            self.contract_version,
            &self.manifest_digest,
            &self.source,
            &self.target,
            &self.objects,
            self.transferred_objects,
            self.transferred_bytes,
            self.completed_at,
        ))
        .map_err(|error| ClusterError::Invalid(format!("artifact receipt encode failed: {error}")))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReshardState {
    Planned,
    Copying,
    CaughtUp,
    Cutover,
    Retired,
}

/// A reshard cannot cut over by wall clock. The metadata shard commits this
/// plan, and every source must be represented in the exact cutover vector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReshardPlan {
    pub contract_version: u16,
    pub operation_id: String,
    pub metadata_index: u64,
    pub source_shards: BTreeSet<ShardId>,
    pub targets: Vec<ShardPlacement>,
    pub cutover: SnapshotVector,
    pub state: ReshardState,
}

impl ReshardPlan {
    pub fn validate(&self) -> Result<()> {
        if self.contract_version != CLUSTER_CONTRACT_VERSION
            || self.operation_id.is_empty()
            || self.operation_id.len() > 128
            || self.metadata_index == 0
            || self.source_shards.is_empty()
            || self.targets.is_empty()
        {
            return Err(ClusterError::Invalid("malformed reshard plan".into()));
        }
        self.cutover.validate()?;
        let cutover_shards: BTreeSet<_> = self.cutover.shards.keys().copied().collect();
        if cutover_shards != self.source_shards {
            return Err(ClusterError::Invalid(
                "reshard cutover vector must cover exactly the source shards".into(),
            ));
        }
        let source_epoch = self
            .cutover
            .shards
            .values()
            .map(|stamp| stamp.placement_epoch)
            .max()
            .expect("validated snapshot vector is non-empty");
        let mut targets = BTreeSet::new();
        for target in &self.targets {
            target.validate()?;
            if target.epoch <= source_epoch
                || !targets.insert(target.shard)
                || self.source_shards.contains(&target.shard)
            {
                return Err(ClusterError::Invalid(
                    "reshard targets require a newer epoch and identities unique from sources"
                        .into(),
                ));
            }
        }
        Ok(())
    }
}

impl ReplicaTransferPlan {
    pub fn validate(&self) -> Result<()> {
        self.grounded_snapshot.validate()?;
        if self.contract_version != CLUSTER_CONTRACT_VERSION
            || self.placement_epoch != self.grounded_snapshot.placement_epoch
            || self.source == self.target
            || self.wal_from_exclusive != self.grounded_snapshot.commit_index
            || self.wal_through_inclusive < self.wal_from_exclusive
            || self
                .artifact_digests
                .iter()
                .any(|digest| !is_sha256(digest))
        {
            return Err(ClusterError::Invalid(
                "invalid snapshot-plus-WAL replica transfer plan".into(),
            ));
        }
        Ok(())
    }
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
