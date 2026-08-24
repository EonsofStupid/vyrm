//! Stable, transport-neutral public contracts for RRD.
//!
//! Internal storage, query, cluster, and UI types must not leak into this
//! boundary. HTTP, gRPC, embedded clients, SDKs, and Connectome consume the
//! same serialized vocabulary. Version 1 intentionally freezes only the
//! coordinates needed before those outward surfaces are implemented.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

pub const PROTOCOL: &str = "rrd";
pub const PROTOCOL_VERSION: u16 = 1;
pub const MAX_ID_BYTES: usize = 128;
pub const MAX_MESSAGE_BYTES: usize = 4_096;
pub const MAX_CAPABILITIES: usize = 512;
pub const MAX_TRANSACTION_CLAIMS: usize = 4_096;
pub const MAX_QUERY_BYTES: usize = 64 * 1024;
pub const MAX_QUERY_PARAMETERS: usize = 128;
pub const MAX_QUERY_PARAMETER_BYTES: usize = 64 * 1024;
pub const MAX_QUERY_SCANNED_CHANGES: u64 = 1_000_000;
pub const MAX_QUERY_ROWS: u64 = 100_000;
pub const MAX_QUERY_OUTPUT_BYTES: u64 = 768 * 1024;
pub const MAX_QUERY_BATCH_ROWS: u64 = 1_024;
pub const MIN_LEASE_MS: u64 = 1_000;
pub const MAX_LEASE_MS: u64 = 3_600_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum QueryValue {
    Null,
    Bool(bool),
    Integer(i64),
    Unsigned(u64),
    Decimal(String),
    String(String),
    Digest(String),
    List(Vec<QueryValue>),
    Map(BTreeMap<String, QueryValue>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueryBudget {
    pub max_scanned_changes: u64,
    pub max_rows: u64,
    pub max_output_bytes: u64,
    pub max_batch_rows: u64,
}

impl Default for QueryBudget {
    fn default() -> Self {
        Self {
            max_scanned_changes: 100_000,
            max_rows: 10_000,
            max_output_bytes: 512 * 1024,
            max_batch_rows: 256,
        }
    }
}

impl QueryBudget {
    pub fn validate(&self) -> Result<()> {
        for (name, value, maximum) in [
            (
                "max_scanned_changes",
                self.max_scanned_changes,
                MAX_QUERY_SCANNED_CHANGES,
            ),
            ("max_rows", self.max_rows, MAX_QUERY_ROWS),
            (
                "max_output_bytes",
                self.max_output_bytes,
                MAX_QUERY_OUTPUT_BYTES,
            ),
            ("max_batch_rows", self.max_batch_rows, MAX_QUERY_BATCH_ROWS),
        ] {
            if value == 0 || value > maximum {
                return invalid(format!("query {name} must be in 1..={maximum}"));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecuteQuery {
    pub scope: String,
    pub query: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub parameters: BTreeMap<String, QueryValue>,
    #[serde(default)]
    pub budget: QueryBudget,
}

impl ExecuteQuery {
    pub fn validate(&self) -> Result<()> {
        if self.scope.is_empty() || self.scope.len() > MAX_ID_BYTES || self.scope.contains('\0') {
            return invalid(format!(
                "query scope length must be in 1..={MAX_ID_BYTES} bytes and contain no NUL"
            ));
        }
        if self.query.trim().is_empty() || self.query.len() > MAX_QUERY_BYTES {
            return invalid(format!(
                "query text length must be in 1..={MAX_QUERY_BYTES} bytes"
            ));
        }
        if self.parameters.len() > MAX_QUERY_PARAMETERS {
            return invalid(format!(
                "query parameters may contain at most {MAX_QUERY_PARAMETERS} entries"
            ));
        }
        if self.parameters.values().any(|value| {
            !matches!(
                value,
                QueryValue::Null
                    | QueryValue::Bool(_)
                    | QueryValue::Integer(_)
                    | QueryValue::Unsigned(_)
                    | QueryValue::String(_)
            )
        }) {
            return invalid(
                "query parameters support only null, boolean, integer, unsigned, and string values",
            );
        }
        let parameter_bytes = serde_json::to_vec(&self.parameters)
            .map_err(|error| ContractError(error.to_string()))?
            .len();
        if parameter_bytes > MAX_QUERY_PARAMETER_BYTES {
            return invalid(format!(
                "query parameters may encode at most {MAX_QUERY_PARAMETER_BYTES} bytes"
            ));
        }
        self.budget.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueryPlanCandidate {
    pub name: String,
    pub selected: bool,
    pub exact: bool,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueryPlanSnapshot {
    pub plan_sha256: String,
    pub exact: bool,
    pub deterministic_order: String,
    pub authorization_boundary: String,
    pub candidates: Vec<QueryPlanCandidate>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueryRowSnapshot {
    pub identity: String,
    pub values: BTreeMap<String, QueryValue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueryExecutionSnapshot {
    pub scanned_changes: u64,
    pub stamp_validation: String,
    pub stamp_validation_max_changes: u64,
    pub stamp_validation_proof_nodes: u16,
    pub returned_rows: u64,
    pub output_bytes: u64,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueryResult {
    pub canonical_query: String,
    pub scope: String,
    pub read_manifest_sha256: String,
    pub known_at_cursor: u64,
    pub schema_revision: u64,
    pub plan: QueryPlanSnapshot,
    pub execution: QueryExecutionSnapshot,
    pub rows: Vec<QueryRowSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadEstate {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EstateDesiredPhase {
    Running,
    Stopped,
    Absent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EstateObservedPhase {
    Unknown,
    Provisioning,
    Starting,
    Running,
    Stopping,
    Stopped,
    Deleting,
    Absent,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EstateActivityClass {
    Unknown,
    Active,
    Idle,
    Stale,
    Neglected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EstateOperationKind {
    Provision,
    Start,
    Stop,
    Restart,
    Upgrade,
    Delete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EstateOperationState {
    Pending,
    Leased,
    Prepared,
    Applied,
    Succeeded,
    Failed,
    Superseded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EstateReceiptBoundary {
    Prepared,
    Applied,
    Completed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EstateActivityPolicySnapshot {
    pub idle_after_ms: u64,
    pub stale_after_ms: u64,
    pub neglected_after_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EstateDesiredSnapshot {
    pub generation: u64,
    pub phase: EstateDesiredPhase,
    pub deployment_ref: CanonicalId,
    pub version: String,
    pub configuration_sha256: String,
    pub updated_at_unix_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EstateObservedSnapshot {
    pub generation: u64,
    pub phase: EstateObservedPhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process_id: Option<u32>,
    pub observed_at_unix_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EstateActivitySnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_meaningful_runtime_at_unix_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_heartbeat_at_unix_ms: Option<u64>,
    pub class: EstateActivityClass,
    pub evaluated_at_unix_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EstateInstanceSnapshot {
    pub id: CanonicalId,
    pub desired: EstateDesiredSnapshot,
    pub observed: EstateObservedSnapshot,
    pub activity: EstateActivitySnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EstateLeaseSnapshot {
    pub owner: CanonicalId,
    pub epoch: u64,
    pub acquired_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EstateReceiptSnapshot {
    pub boundary: EstateReceiptBoundary,
    pub lease_epoch: u64,
    pub at_unix_ms: u64,
    pub evidence_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EstateOperationSnapshot {
    pub id: CanonicalId,
    pub instance_id: CanonicalId,
    pub kind: EstateOperationKind,
    pub desired_generation: u64,
    pub request_sha256: String,
    pub state: EstateOperationState,
    pub attempts: u32,
    pub created_at_unix_ms: u64,
    pub updated_at_unix_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease: Option<EstateLeaseSnapshot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub receipts: Vec<EstateReceiptSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EstateSnapshot {
    pub format_version: u16,
    pub id: CanonicalId,
    pub revision: u64,
    pub created_at_unix_ms: u64,
    pub updated_at_unix_ms: u64,
    pub activity_policy: EstateActivityPolicySnapshot,
    pub instances: Vec<EstateInstanceSnapshot>,
    pub operations: Vec<EstateOperationSnapshot>,
    pub idempotency_binding_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EstateMutationResult {
    pub estate: EstateSnapshot,
    pub idempotent_replay: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EstateBackupJobState {
    Pending,
    Leased,
    Prepared,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EstateBackupReceiptBoundary {
    Prepared,
    Completed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EstateBackupReceiptSnapshot {
    pub boundary: EstateBackupReceiptBoundary,
    pub lease_epoch: u64,
    pub at_unix_ms: u64,
    pub evidence_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EstateBackupJobSnapshot {
    pub id: CanonicalId,
    pub instance_id: CanonicalId,
    pub source_generation: u64,
    pub label: String,
    pub request_sha256: String,
    pub state: EstateBackupJobState,
    pub attempts: u32,
    pub created_at_unix_ms: u64,
    pub updated_at_unix_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease: Option<EstateLeaseSnapshot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub receipts: Vec<EstateBackupReceiptSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catalogue_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EstateBackupJobsSnapshot {
    pub estate_id: CanonicalId,
    pub estate_revision: u64,
    pub jobs: Vec<EstateBackupJobSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EstateBackupMutationResult {
    pub estate: EstateSnapshot,
    pub job: EstateBackupJobSnapshot,
    pub idempotent_replay: bool,
}

pub type Result<T> = std::result::Result<T, ContractError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractError(pub String);

impl fmt::Display for ContractError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ContractError {}

/// A canonical public identifier component.
///
/// IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
/// labels are separate data and may use arbitrary Unicode.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CanonicalId(String);

impl CanonicalId {
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        if value.is_empty() || value.len() > MAX_ID_BYTES {
            return invalid(format!(
                "canonical id length must be in 1..={MAX_ID_BYTES} bytes"
            ));
        }
        if !value.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || (index > 0 && matches!(byte, b'-' | b'_' | b'.'))
        }) {
            return invalid(
                "canonical id must start with lowercase ASCII or a digit and contain only lowercase ASCII, digits, '-', '_', or '.'",
            );
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl fmt::Display for CanonicalId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Serialize for CanonicalId {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for CanonicalId {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    Organization,
    Estate,
    Project,
    Instance,
    Node,
    Shard,
    Collection,
    Table,
    Record,
    Transaction,
    Snapshot,
    Backup,
    Operation,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceId {
    pub kind: ResourceKind,
    pub id: CanonicalId,
}

impl ResourceId {
    pub fn new(kind: ResourceKind, id: impl Into<String>) -> Result<Self> {
        Ok(Self {
            kind,
            id: CanonicalId::new(id)?,
        })
    }
}

/// A fully explicit hierarchical identity. No field is inferred from process
/// cwd, connection state, or a human label.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourcePath {
    pub segments: Vec<ResourceId>,
}

impl ResourcePath {
    pub fn validate(&self) -> Result<()> {
        if self.segments.is_empty() {
            return invalid("resource path must contain at least one segment");
        }
        if self.segments.len() > 16 {
            return invalid("resource path must contain at most 16 segments");
        }
        let mut kinds = BTreeSet::new();
        for segment in &self.segments {
            if !kinds.insert(segment.kind) {
                return invalid("resource path must not repeat a resource kind");
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CorrelationId(String);

impl CorrelationId {
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        if value.is_empty() || value.len() > MAX_ID_BYTES {
            return invalid(format!(
                "correlation id length must be in 1..={MAX_ID_BYTES} bytes"
            ));
        }
        if !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
        {
            return invalid(
                "correlation id must contain only ASCII letters, digits, '-', '_', '.', or ':'",
            );
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Serialize for CorrelationId {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for CorrelationId {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestContext {
    pub request_id: CorrelationId,
    pub operation_id: CorrelationId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<CorrelationId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline_unix_ms: Option<u64>,
}

impl RequestContext {
    pub fn validate(&self, mutation: bool) -> Result<()> {
        if mutation && self.idempotency_key.is_none() {
            return invalid("mutating requests require an idempotency key");
        }
        if self.deadline_unix_ms == Some(0) {
            return invalid("deadline_unix_ms must be greater than zero when present");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdempotencyBinding {
    pub key: CorrelationId,
    pub operation_sha256: String,
}

impl IdempotencyBinding {
    pub fn validate(&self) -> Result<()> {
        validate_sha256(&self.operation_sha256, "operation_sha256")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentMode {
    Embedded,
    LocalServer,
    Distributed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityStatus {
    Unavailable,
    Experimental,
    Available,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityDescriptor {
    pub name: CanonicalId,
    pub contract_version: u16,
    pub status: CapabilityStatus,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub limits: BTreeMap<CanonicalId, u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limitation: Option<String>,
}

impl CapabilityDescriptor {
    pub fn validate(&self) -> Result<()> {
        if self.contract_version == 0 {
            return invalid("capability contract version must be greater than zero");
        }
        if self
            .limitation
            .as_ref()
            .is_some_and(|value| value.is_empty() || value.len() > MAX_MESSAGE_BYTES)
        {
            return invalid(format!(
                "capability limitation length must be in 1..={MAX_MESSAGE_BYTES} bytes"
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceCapabilities {
    pub protocol: String,
    pub protocol_version: u16,
    pub implementation: CanonicalId,
    pub implementation_version: String,
    pub deployment_mode: DeploymentMode,
    pub instance: ResourceId,
    pub capabilities: Vec<CapabilityDescriptor>,
}

impl ServiceCapabilities {
    pub fn validate(&self) -> Result<()> {
        validate_protocol(&self.protocol, self.protocol_version)?;
        if self.instance.kind != ResourceKind::Instance {
            return invalid("service capability identity must be an instance resource");
        }
        if self.implementation_version.is_empty()
            || self.implementation_version.len() > MAX_ID_BYTES
            || !self.implementation_version.is_ascii()
        {
            return invalid(format!(
                "implementation version must be 1..={MAX_ID_BYTES} ASCII bytes"
            ));
        }
        if self.capabilities.len() > MAX_CAPABILITIES {
            return invalid(format!(
                "service may advertise at most {MAX_CAPABILITIES} capabilities"
            ));
        }
        let mut names = BTreeSet::new();
        for capability in &self.capabilities {
            capability.validate()?;
            if !names.insert(&capability.name) {
                return invalid("service capability names must be unique");
            }
        }
        if self
            .capabilities
            .windows(2)
            .any(|pair| pair[0].name > pair[1].name)
        {
            return invalid("service capabilities must be sorted by canonical name");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestEnvelope<T> {
    pub protocol: String,
    pub protocol_version: u16,
    pub context: RequestContext,
    pub resource: ResourcePath,
    pub payload: T,
}

impl<T> RequestEnvelope<T> {
    pub fn validate(&self, mutation: bool) -> Result<()> {
        validate_protocol(&self.protocol, self.protocol_version)?;
        self.context.validate(mutation)?;
        self.resource.validate()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidArgument,
    NotFound,
    AlreadyExists,
    Conflict,
    FailedPrecondition,
    Unauthenticated,
    PermissionDenied,
    ResourceExhausted,
    DeadlineExceeded,
    Cancelled,
    Unavailable,
    Corruption,
    UnsupportedVersion,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ErrorBody {
    pub code: ErrorCode,
    pub message: String,
    pub retryable: bool,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub details: BTreeMap<CanonicalId, String>,
}

impl ErrorBody {
    pub fn validate(&self) -> Result<()> {
        if self.message.is_empty() || self.message.len() > MAX_MESSAGE_BYTES {
            return invalid(format!(
                "error message length must be in 1..={MAX_MESSAGE_BYTES} bytes"
            ));
        }
        if self
            .details
            .values()
            .any(|value| value.len() > MAX_MESSAGE_BYTES)
        {
            return invalid(format!(
                "error detail values must be at most {MAX_MESSAGE_BYTES} bytes"
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ResponseOutcome<T> {
    Ok { payload: T },
    Error { error: ErrorBody },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResponseEnvelope<T> {
    pub protocol: String,
    pub protocol_version: u16,
    pub request_id: CorrelationId,
    pub operation_id: CorrelationId,
    pub outcome: ResponseOutcome<T>,
}

impl<T> ResponseEnvelope<T> {
    pub fn validate(&self) -> Result<()> {
        validate_protocol(&self.protocol, self.protocol_version)?;
        if let ResponseOutcome::Error { error } = &self.outcome {
            error.validate()?;
        }
        Ok(())
    }
}

/// Resource limits requested for one loopback transport session. These are
/// availability boundaries, not authentication or authorization policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionLimits {
    pub idle_timeout_ms: u64,
    pub absolute_timeout_ms: u64,
    pub max_open_transactions: u16,
}

impl SessionLimits {
    pub fn validate(&self) -> Result<()> {
        if !(MIN_LEASE_MS..=MAX_LEASE_MS).contains(&self.idle_timeout_ms)
            || !(MIN_LEASE_MS..=MAX_LEASE_MS).contains(&self.absolute_timeout_ms)
            || self.idle_timeout_ms > self.absolute_timeout_ms
        {
            return invalid("session timeouts are outside bounds or idle exceeds absolute");
        }
        if self.max_open_transactions == 0 || self.max_open_transactions > 256 {
            return invalid("max_open_transactions must be in 1..=256");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateSession {
    pub limits: SessionLimits,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionLease {
    pub session_id: CorrelationId,
    pub token: CorrelationId,
    pub issued_at_unix_ms: u64,
    pub idle_expires_at_unix_ms: u64,
    pub absolute_expires_at_unix_ms: u64,
    pub limits: SessionLimits,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenewSession {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CloseSession {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AbortTransaction {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionEndState {
    Closed,
    Expired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionTermination {
    pub session_id: CorrelationId,
    pub state: SessionEndState,
    pub ended_at_unix_ms: u64,
    pub affected_open_transactions: u16,
    pub idempotent_replay: bool,
}

impl SessionTermination {
    pub fn validate(&self) -> Result<()> {
        if self.ended_at_unix_ms == 0 {
            return invalid("session termination time must be greater than zero");
        }
        Ok(())
    }
}

impl SessionLease {
    pub fn validate(&self) -> Result<()> {
        self.limits.validate()?;
        if self.issued_at_unix_ms == 0
            || self.idle_expires_at_unix_ms <= self.issued_at_unix_ms
            || self.absolute_expires_at_unix_ms < self.idle_expires_at_unix_ms
        {
            return invalid("session lease timestamps are inconsistent");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BeginTransaction {
    pub scope: CanonicalId,
    pub timeout_ms: u64,
}

impl BeginTransaction {
    pub fn validate(&self) -> Result<()> {
        if !(MIN_LEASE_MS..=MAX_LEASE_MS).contains(&self.timeout_ms) {
            return invalid(format!(
                "transaction timeout must be in {MIN_LEASE_MS}..={MAX_LEASE_MS}"
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransactionState {
    Open,
    Committed,
    Aborted,
    Expired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransactionLease {
    pub transaction_id: CorrelationId,
    pub session_id: CorrelationId,
    pub scope: CanonicalId,
    pub read_cursor: u64,
    pub expires_at_unix_ms: u64,
    pub state: TransactionState,
}

impl TransactionLease {
    pub fn validate(&self) -> Result<()> {
        if self.expires_at_unix_ms == 0 {
            return invalid("transaction expiry must be greater than zero");
        }
        Ok(())
    }
}

/// First public mutation surface. Later F6 multi-model mutations extend this
/// tagged enum without exposing private `vyrm_core` representations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mutation", rename_all = "snake_case", deny_unknown_fields)]
pub enum TransactionMutation {
    AssertClaim {
        subject: CanonicalId,
        predicate: CanonicalId,
        object: String,
        valid_from: u64,
        tx_time: u64,
        producer: CanonicalId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        confidence: Option<f32>,
    },
}

impl TransactionMutation {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::AssertClaim {
                object,
                valid_from,
                tx_time,
                confidence,
                ..
            } => {
                if object.is_empty() || object.len() > MAX_MESSAGE_BYTES {
                    return invalid(format!(
                        "claim object length must be in 1..={MAX_MESSAGE_BYTES} bytes"
                    ));
                }
                if *valid_from == 0 || *tx_time == 0 {
                    return invalid("claim timestamps must be greater than zero");
                }
                if confidence
                    .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
                {
                    return invalid("claim confidence must be finite and in 0..=1");
                }
                Ok(())
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommitTransaction {
    pub operation_sha256: String,
    pub mutations: Vec<TransactionMutation>,
}

impl CommitTransaction {
    pub fn validate(&self) -> Result<()> {
        validate_sha256(&self.operation_sha256, "operation_sha256")?;
        if self.mutations.is_empty() || self.mutations.len() > MAX_TRANSACTION_CLAIMS {
            return invalid(format!(
                "transaction mutation count must be in 1..={MAX_TRANSACTION_CLAIMS}"
            ));
        }
        for mutation in &self.mutations {
            mutation.validate()?;
        }
        Ok(())
    }

    pub fn computed_operation_sha256(&self) -> String {
        transaction_operation_sha256(&self.mutations)
    }
}

/// SHA-256 over the stable typed JSON representation of the ordered mutation
/// list. Clients must use this function's cross-language equivalent rather
/// than hashing arbitrary input-object key order.
pub fn transaction_operation_sha256(mutations: &[TransactionMutation]) -> String {
    let bytes = serde_json::to_vec(mutations).expect("public transaction mutations serialize");
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(64);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in digest {
        output.push(HEX[usize::from(byte >> 4)] as char);
        output.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    output
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewTransaction {
    pub mutations: Vec<TransactionMutation>,
}

impl PreviewTransaction {
    pub fn validate(&self) -> Result<()> {
        if self.mutations.is_empty() || self.mutations.len() > MAX_TRANSACTION_CLAIMS {
            return invalid(format!(
                "transaction mutation count must be in 1..={MAX_TRANSACTION_CLAIMS}"
            ));
        }
        for mutation in &self.mutations {
            mutation.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransactionPreview {
    pub transaction_id: CorrelationId,
    pub read_cursor: u64,
    pub operation_sha256: String,
    pub mutations: Vec<TransactionMutation>,
}

impl TransactionPreview {
    pub fn validate(&self) -> Result<()> {
        validate_sha256(&self.operation_sha256, "operation_sha256")?;
        PreviewTransaction {
            mutations: self.mutations.clone(),
        }
        .validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommitReceipt {
    pub transaction_id: CorrelationId,
    pub operation_sha256: String,
    pub first_claim_sequence: u64,
    pub last_claim_sequence: u64,
    pub mutation_count: u64,
    pub idempotent_replay: bool,
}

impl CommitReceipt {
    pub fn validate(&self) -> Result<()> {
        validate_sha256(&self.operation_sha256, "operation_sha256")?;
        if self.first_claim_sequence == 0
            || self.last_claim_sequence < self.first_claim_sequence
            || self.mutation_count != self.last_claim_sequence - self.first_claim_sequence + 1
        {
            return invalid("commit receipt sequence interval is inconsistent");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Liveness {
    pub observed_at_unix_ms: u64,
}

impl Liveness {
    pub fn validate(&self) -> Result<()> {
        if self.observed_at_unix_ms == 0 {
            return invalid("liveness observation time must be greater than zero");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Readiness {
    pub observed_at_unix_ms: u64,
    pub claim_sequence: u64,
    pub runtime_cursor: u64,
    pub backend: CanonicalId,
}

impl Readiness {
    pub fn validate(&self) -> Result<()> {
        if self.observed_at_unix_ms == 0 {
            return invalid("readiness observation time must be greater than zero");
        }
        Ok(())
    }
}

pub fn validate_protocol(protocol: &str, version: u16) -> Result<()> {
    if protocol != PROTOCOL {
        return invalid(format!(
            "unsupported protocol {protocol:?}; expected {PROTOCOL:?}"
        ));
    }
    if version != PROTOCOL_VERSION {
        return invalid(format!(
            "unsupported protocol version {version}; expected {PROTOCOL_VERSION}"
        ));
    }
    Ok(())
}

fn validate_sha256(value: &str, field: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return invalid(format!("{field} must be lowercase SHA-256 hex"));
    }
    Ok(())
}

fn invalid<T>(message: impl Into<String>) -> Result<T> {
    Err(ContractError(message.into()))
}
