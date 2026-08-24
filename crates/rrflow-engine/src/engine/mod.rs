//! Authoritative RRFlow engine composition and operations.

use hmac::{Hmac, KeyInit, Mac};
use rrd_contract::{
    transaction_operation_sha256, AbortTransaction, AuditDecision, AuditPage, AuditPhase,
    AuditRecordSnapshot, BeginTransaction, CanonicalId, ChangeMutationSnapshot,
    ChangefeedFollowResult, ChangefeedPage, ChangefeedValidation, ClaimChangeSnapshot,
    ClaimPromotionSnapshot, ClaimTierSnapshot, CloseSession, CommitReceipt, CommitTransaction,
    CorrelationId, CreateInstanceBackup, CreateInstanceBackupResult, CreateSession,
    DataEmbeddingProvenance, DataEventSchema, DataGeoPoint, DataGeoValue, DataObjectReceipt,
    DataProperties, DataPropertySchema, DataRecordSchema, DataReference, DataRelationSchema,
    DataSchemaRegistry, DataSeriesValue, DataValueType, DataVectorNormalization, DataVectorValue,
    EnsureQueryIndex, EnsureQueryIndexResult, EnsureVectorCollection, EnsureVectorCollectionResult,
    ExecuteQuery, FollowChangefeed, InstanceBackupCatalogueSnapshot, InstanceBackupSnapshot,
    ListInstanceBackups, ListQueryIndexes, ListVectorCollections, LiveQueryDeltaResult,
    LiveQueryRowChange, LogicalArchiveSnapshot, NamedVectorDefinition, PollLiveQuery,
    PreviewTransaction, QueryExecutionSnapshot, QueryIndexCatalogueSnapshot, QueryIndexSnapshot,
    QueryIndexState, QueryPlanCandidate, QueryPlanSnapshot, QueryResult, QueryRowSnapshot,
    QueryValue, ReadAudit, ReadChangefeed, Readiness, RenewSession, RequestContext, ResourceId,
    ResourceKind, ResourcePath, RestoreInstanceBackup, RestoreInstanceBackupResult,
    RetrieveVectorPoints, RuntimeChangeSnapshot, ScrollVectorPoints, SearchVectors, SecurityAction,
    SessionEndState, SessionLease, SessionLimits, SessionTermination, TransactionLease,
    TransactionMutation, TransactionPreview, TransactionState, VectorCollectionCatalogueSnapshot,
    VectorCollectionSnapshot, VectorEmbeddingModel, VectorMemoryTier, VectorPayloadFilter,
    VectorPayloadOperator, VectorPointBatch, VectorPointPage, VectorPointSnapshot, VectorSearchHit,
    VectorSearchMetric, VectorSearchQuery, VectorSearchResult, VectorValueKind,
};
use rrd_store::{ControlTransition, Engine, PersistentEngine};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use vyrm_core::{
    digest, Claim, DataTransaction, EmbeddingProvenance, GeoPoint, GeoValue, ObjectReceipt,
    ObjectReference, Predicate, Producer, ProjectionId, ProjectionState, PromotionState, ReadStamp,
    RuntimeCommit, RuntimeEvent, RuntimeEventSchema, RuntimeGeo, RuntimeMutation,
    RuntimeProperties, RuntimePropertySchema, RuntimeRecord, RuntimeRecordSchema, RuntimeRef,
    RuntimeRelation, RuntimeRelationSchema, RuntimeSchemaRegistry, RuntimeSeriesSample,
    RuntimeType, RuntimeValue, RuntimeValueType, RuntimeVector, ScopeId, SeriesValue, Subject,
    Tier, VectorNormalization, VectorValue,
};
use vyrm_ql::{CursorExpr, Projection, TimeExpr};

mod backup;
mod changefeed;
mod control;
mod core;
mod error;
mod estate;
mod model;
mod query;
mod security;
mod session;
mod transaction;
mod vector;

use control::*;
pub use core::RrflowEngine;
pub use error::{Result, ServiceError, ServiceErrorKind};
use model::*;
pub use security::{AuditEvent, MAX_AUDIT_PAGE_RECORDS};
use session::*;

const SESSION_STATE_FORMAT: u16 = 2;
const MAX_SESSION_RENEWALS: usize = 64;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionState {
    format_version: u16,
    session_id: CorrelationId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    principal_id: Option<CanonicalId>,
    status: SessionStatus,
    issued_at_unix_ms: u64,
    idle_expires_at_unix_ms: u64,
    absolute_expires_at_unix_ms: u64,
    limits: SessionLimits,
    creation_idempotency_key: CorrelationId,
    creation_operation_sha256: String,
    creation_idle_expires_at_unix_ms: u64,
    token_sha256: String,
    token_generation: u64,
    renewals: BTreeMap<CorrelationId, RenewalRecord>,
    closure: Option<ClosureRecord>,
    transactions: BTreeMap<CorrelationId, TransactionRecord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SessionStatus {
    Active,
    Expired,
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RenewalRecord {
    operation_sha256: String,
    previous_token_sha256: String,
    token_generation: u64,
    idle_expires_at_unix_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClosureRecord {
    idempotency_key: CorrelationId,
    operation_sha256: String,
    previous_token_sha256: String,
    ended_at_unix_ms: u64,
    affected_open_transactions: u16,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TransactionRecord {
    lease: TransactionLease,
    read: ReadStamp,
    begin_idempotency_key: CorrelationId,
    begin_operation_sha256: String,
    commit_intent: Option<CommitIntent>,
    commit_receipt: Option<CommitReceipt>,
    abort: Option<AbortRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CommitIntent {
    idempotency_key: CorrelationId,
    operation_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    runtime_at_unix_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    runtime_commit_sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AbortRecord {
    idempotency_key: CorrelationId,
    operation_sha256: String,
}

#[cfg(test)]
mod tests;
