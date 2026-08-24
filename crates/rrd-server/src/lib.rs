//! Persistent, journaled RRD transport-session and claim-transaction service.

mod http;

pub use http::{HttpError, RRD_MAX_BODY_BYTES, RrdHttpServer, load_or_create_token_key};

use hmac::{Hmac, KeyInit, Mac};
use rrd_contract::{
    AbortTransaction, BeginTransaction, CanonicalId, ChangeMutationSnapshot,
    ChangefeedFollowResult, ChangefeedPage, ChangefeedValidation, ClaimChangeSnapshot,
    ClaimPromotionSnapshot, ClaimTierSnapshot, CloseSession, CommitReceipt, CommitTransaction,
    CorrelationId, CreateSession, DataEventSchema, DataGeoPoint, DataGeoValue, DataObjectReceipt,
    DataProperties, DataPropertySchema, DataRecordSchema, DataReference, DataRelationSchema,
    DataSchemaRegistry, DataSeriesValue, DataValueType, DataVectorNormalization, DataVectorValue,
    ExecuteQuery, FollowChangefeed, PreviewTransaction, QueryExecutionSnapshot, QueryPlanCandidate,
    QueryPlanSnapshot, QueryResult, QueryRowSnapshot, QueryValue, ReadChangefeed, RenewSession,
    RuntimeChangeSnapshot, SearchVectors, SessionEndState, SessionLease, SessionLimits,
    SessionTermination, TransactionLease, TransactionMutation, TransactionPreview,
    TransactionState, VectorSearchHit, VectorSearchMetric, VectorSearchQuery, VectorSearchResult,
    transaction_operation_sha256,
};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::collections::BTreeMap;
use std::fmt;
use std::time::{Duration, Instant};
use vyrm_core::{
    Claim, EmbeddingProvenance, GeoPoint, GeoValue, ObjectReceipt, ObjectReference, Predicate,
    Producer, PromotionState, RuntimeCommit, RuntimeEvent, RuntimeEventSchema, RuntimeGeo,
    RuntimeMutation, RuntimeProperties, RuntimePropertySchema, RuntimeRecord, RuntimeRecordSchema,
    RuntimeRef, RuntimeRelation, RuntimeRelationSchema, RuntimeSchemaRegistry, RuntimeSeriesSample,
    RuntimeType, RuntimeValue, RuntimeValueType, RuntimeVector, ScopeId, SeriesValue, Subject,
    Tier, VectorNormalization, VectorValue, digest,
};
use vyrm_store::{ControlTransition, Engine};

const SESSION_STATE_FORMAT: u16 = 1;
const MAX_SESSION_RENEWALS: usize = 64;

pub type Result<T> = std::result::Result<T, ServiceError>;

#[derive(Debug)]
pub enum ServiceError {
    Contract(String),
    Estate(String),
    Store(vyrm_store::Error),
    SessionNotFound,
    Unauthenticated,
    SessionExpired,
    TransactionNotFound,
    TransactionExpired,
    TransactionClosed,
    TransactionQuota,
    RenewalQuota,
    IdempotencyConflict,
    CommitInProgress,
    WrongScope,
    OperationDigestMismatch,
    Query(String),
    Vector(String),
    Changefeed(String),
}

impl fmt::Display for ServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for ServiceError {}

impl From<vyrm_store::Error> for ServiceError {
    fn from(value: vyrm_store::Error) -> Self {
        Self::Store(value)
    }
}

impl From<rrd_estate::Error> for ServiceError {
    fn from(value: rrd_estate::Error) -> Self {
        Self::Estate(value.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionState {
    format_version: u16,
    session_id: CorrelationId,
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

pub struct RrdService<E> {
    engine: E,
    instance: CanonicalId,
    token_key: [u8; 32],
}

impl<E: Engine> RrdService<E> {
    pub fn new(engine: E, instance: CanonicalId, token_key: [u8; 32]) -> Self {
        Self {
            engine,
            instance,
            token_key,
        }
    }

    pub fn engine(&self) -> &E {
        &self.engine
    }

    #[allow(clippy::too_many_arguments)]
    pub fn read_estate(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        estate_id: CanonicalId,
        _request: &rrd_contract::ReadEstate,
        now: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<Option<rrd_contract::EstateSnapshot>> {
        self.authorize(session_id, token, now, request_id, operation_id)?;
        let repository = rrd_estate::EstateRepository::new(&self.engine, estate_id);
        repository
            .load()
            .map(|document| document.as_ref().map(rrd_estate::public_snapshot))
            .map_err(Into::into)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn execute_query(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        request: &ExecuteQuery,
        now: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<QueryResult> {
        request
            .validate()
            .map_err(|error| ServiceError::Query(error.to_string()))?;
        self.authorize(session_id, token, now, request_id, operation_id)?;
        let expected_scope = format!("instance:{}", self.instance);
        if request.scope != expected_scope {
            return Err(ServiceError::WrongScope);
        }
        let scope = ScopeId::new(request.scope.clone())
            .map_err(|error| ServiceError::Query(error.to_string()))?;
        let query = vyrm_ql::parse(&request.query)
            .map_err(|error| ServiceError::Query(error.to_string()))?;
        let parameters = request
            .parameters
            .iter()
            .map(|(name, value)| Ok((name.clone(), runtime_value(value)?)))
            .collect::<Result<vyrm_mx::Parameters>>()?;
        let catalog = vyrm_mx::Catalog::capture(&self.engine, &scope)
            .map_err(|error| ServiceError::Query(error.to_string()))?;
        let bound = vyrm_mx::bind(&query, &parameters, &catalog)
            .map_err(|error| ServiceError::Query(error.to_string()))?;
        let plan = vyrm_mx::plan(&bound).map_err(|error| ServiceError::Query(error.to_string()))?;
        let budget = vyrm_mx::ExecutionBudget {
            max_scanned_changes: usize::try_from(request.budget.max_scanned_changes)
                .map_err(|_| ServiceError::Query("query scan budget exceeds usize".into()))?,
            max_rows: usize::try_from(request.budget.max_rows)
                .map_err(|_| ServiceError::Query("query row budget exceeds usize".into()))?,
            max_output_bytes: usize::try_from(request.budget.max_output_bytes)
                .map_err(|_| ServiceError::Query("query output budget exceeds usize".into()))?,
            max_batch_rows: usize::try_from(request.budget.max_batch_rows)
                .map_err(|_| ServiceError::Query("query batch budget exceeds usize".into()))?,
        };
        let execution = vyrm_mx::execute(&self.engine, &plan, &budget)
            .map_err(|error| ServiceError::Query(error.to_string()))?;
        let rows = execution
            .batches
            .iter()
            .flat_map(|batch| batch.rows.iter())
            .map(|row| {
                Ok(QueryRowSnapshot {
                    identity: row.identity.clone(),
                    values: row
                        .values
                        .iter()
                        .map(|(name, value)| Ok((name.clone(), query_value(value)?)))
                        .collect::<Result<_>>()?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(QueryResult {
            canonical_query: query.canonical(),
            scope: request.scope.clone(),
            read_manifest_sha256: execution.read_manifest.clone(),
            known_at_cursor: execution.known_at_cursor,
            schema_revision: bound.schema_revision,
            plan: QueryPlanSnapshot {
                plan_sha256: plan.digest.clone(),
                exact: plan.explanation.contract.exact,
                deterministic_order: plan.explanation.contract.deterministic_order.clone(),
                authorization_boundary: plan.explanation.contract.authorization_boundary.clone(),
                candidates: plan
                    .explanation
                    .candidates
                    .iter()
                    .map(|candidate| QueryPlanCandidate {
                        name: candidate.name.clone(),
                        selected: candidate.selected,
                        exact: candidate.exact,
                        reason: candidate.reason.clone(),
                    })
                    .collect(),
            },
            execution: QueryExecutionSnapshot {
                scanned_changes: u64::try_from(execution.scanned_changes)
                    .map_err(|_| ServiceError::Query("scanned changes exceed u64".into()))?,
                stamp_validation: execution.stamp_validation,
                stamp_validation_max_changes: u64::try_from(execution.stamp_validation_max_changes)
                    .map_err(|_| ServiceError::Query("stamp evidence exceeds u64".into()))?,
                stamp_validation_proof_nodes: execution.stamp_validation_proof_nodes,
                returned_rows: u64::try_from(execution.returned_rows)
                    .map_err(|_| ServiceError::Query("returned rows exceed u64".into()))?,
                output_bytes: u64::try_from(execution.output_bytes)
                    .map_err(|_| ServiceError::Query("output bytes exceed u64".into()))?,
                truncated: execution.truncated,
            },
            rows,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn search_vectors(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        request: &SearchVectors,
        now: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<VectorSearchResult> {
        request
            .validate()
            .map_err(|error| ServiceError::Vector(error.to_string()))?;
        self.authorize(session_id, token, now, request_id, operation_id)?;
        let expected_scope = format!("instance:{}", self.instance);
        if request.scope != expected_scope {
            return Err(ServiceError::WrongScope);
        }
        let scope = ScopeId::new(request.scope.clone()).map_err(core_vector)?;
        let read = self.engine.runtime_read_stamp(&scope)?;
        let limit = usize::try_from(request.max_scanned_changes)
            .map_err(|_| ServiceError::Vector("vector scan budget exceeds usize".into()))?;
        let page = self.engine.runtime_read_changes(&read, 0, limit)?;
        if page.through_cursor < page.head_cursor {
            return Err(ServiceError::Vector(format!(
                "vector exact scan requires more than {} retained changes",
                request.max_scanned_changes
            )));
        }
        let candidates = vyrm_vector::candidates_from_changes(&page.changes, &scope);
        let runtime = vyrm_vector::VectorRuntime::new(candidates).map_err(core_vector)?;
        let query = match &request.query {
            VectorSearchQuery::Dense { values } => vyrm_vector::VectorQuery::Dense {
                values: values.clone(),
            },
            VectorSearchQuery::Sparse {
                dimensions,
                indices,
                values,
            } => vyrm_vector::VectorQuery::Sparse {
                dimensions: *dimensions,
                indices: indices.clone(),
                values: values.clone(),
            },
            VectorSearchQuery::MultiDense {
                dimensions,
                vectors,
                comparator: _,
            } => vyrm_vector::VectorQuery::MultiDense {
                dimensions: *dimensions,
                vectors: vectors.clone(),
                comparator: vyrm_vector::MultiVectorComparator::MaxSim,
            },
        };
        let search = vyrm_vector::SearchRequest {
            scope,
            read: read.clone(),
            valid_at: request.valid_at,
            field: request.field.as_str().into(),
            query,
            metric: match request.metric {
                VectorSearchMetric::Cosine => vyrm_vector::ScoreMetric::Cosine,
                VectorSearchMetric::Dot => vyrm_vector::ScoreMetric::Dot,
                VectorSearchMetric::Euclidean => vyrm_vector::ScoreMetric::Euclidean,
                VectorSearchMetric::Manhattan => vyrm_vector::ScoreMetric::Manhattan,
            },
            embedding_model: None,
            top_k: usize::try_from(request.top_k)
                .map_err(|_| ServiceError::Vector("vector top_k exceeds usize".into()))?,
            mode: vyrm_vector::SearchMode::Exact,
            filter: None,
        };
        let prepared = runtime.prepare_search(&search, 1).map_err(core_vector)?;
        let execution = runtime
            .execute_search(&search, &prepared)
            .map_err(core_vector)?;
        let access_path = match execution.plan.selected.kind {
            vyrm_vector::AccessPathKind::ExactScan => "exact_scan",
            vyrm_vector::AccessPathKind::ExactSegment => "exact_segment",
            vyrm_vector::AccessPathKind::Hnsw => "hnsw",
        };
        Ok(VectorSearchResult {
            scope: request.scope.clone(),
            read_manifest_sha256: read.manifest_id,
            known_at_cursor: read.commit_cursor,
            scanned_changes: page.validation.change_reads,
            plan_sha256: prepared.plan_digest().into(),
            access_path: CanonicalId::new(access_path)
                .map_err(|error| ServiceError::Vector(error.to_string()))?,
            exact: execution.plan.selected.kind != vyrm_vector::AccessPathKind::Hnsw,
            hits: execution
                .hits
                .into_iter()
                .map(|hit| {
                    Ok(VectorSearchHit {
                        reference: public_data_ref(&hit.reference)?,
                        subject: public_data_ref(&hit.subject)?,
                        source_cursor: hit.source_cursor,
                        score: hit.score,
                    })
                })
                .collect::<Result<_>>()?,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn read_changefeed(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        request: &ReadChangefeed,
        now: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<ChangefeedPage> {
        request
            .validate()
            .map_err(|error| ServiceError::Changefeed(error.to_string()))?;
        self.authorize(session_id, token, now, request_id, operation_id)?;
        let expected_scope = format!("instance:{}", self.instance);
        if request.scope != expected_scope {
            return Err(ServiceError::WrongScope);
        }
        let scope = ScopeId::new(request.scope.clone()).map_err(core_changefeed)?;
        let limit = usize::try_from(request.limit)
            .map_err(|_| ServiceError::Changefeed("changefeed limit exceeds usize".into()))?;
        let page = self
            .engine
            .runtime_changes_since(request.after_cursor, limit, Some(&scope))?;
        Ok(ChangefeedPage {
            requested_after_cursor: page.requested_after,
            through_cursor: page.through_cursor,
            head_cursor: page.head_cursor,
            has_more: page.has_more(),
            validation: ChangefeedValidation {
                method: page.validation.method,
                change_reads: page.validation.change_reads,
                proof_nodes: page.validation.proof_nodes,
            },
            changes: page
                .changes
                .iter()
                .map(public_runtime_change)
                .collect::<Result<_>>()?,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn follow_changefeed(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        request: &FollowChangefeed,
        now: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<ChangefeedFollowResult> {
        request
            .validate()
            .map_err(|error| ServiceError::Changefeed(error.to_string()))?;
        let started = Instant::now();
        let timeout = Duration::from_millis(request.wait_timeout_ms);
        loop {
            let page = self.read_changefeed(
                session_id,
                token,
                &request.read,
                now,
                request_id,
                operation_id,
            )?;
            let elapsed = started.elapsed();
            if !page.changes.is_empty() || elapsed >= timeout {
                return Ok(ChangefeedFollowResult {
                    timed_out: page.changes.is_empty(),
                    waited_ms: u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX),
                    page,
                });
            }
            std::thread::sleep(
                timeout
                    .saturating_sub(elapsed)
                    .min(Duration::from_millis(25)),
            );
        }
    }

    pub fn create_session(
        &self,
        request: &CreateSession,
        idempotency_key: &CorrelationId,
        now: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<SessionLease> {
        request
            .limits
            .validate()
            .map_err(|error| ServiceError::Contract(error.to_string()))?;
        let operation_sha256 = operation_digest(request)?;
        let session_id = self.keyed_id("session", &[idempotency_key.as_str()])?;
        let token = self.session_token(&session_id, 0)?;
        let key = session_key(&self.instance, &session_id);
        if let Some(bytes) = self.engine.control_record(&key)? {
            let state = decode_session(&bytes)?;
            return self.replay_created_session(state, idempotency_key, &operation_sha256);
        }
        let idle_expires = now
            .checked_add(request.limits.idle_timeout_ms)
            .ok_or_else(|| ServiceError::Contract("session idle expiry overflow".into()))?;
        let absolute_expires = now
            .checked_add(request.limits.absolute_timeout_ms)
            .ok_or_else(|| ServiceError::Contract("session absolute expiry overflow".into()))?;
        let lease = SessionLease {
            session_id: session_id.clone(),
            token: token.clone(),
            issued_at_unix_ms: now,
            idle_expires_at_unix_ms: idle_expires,
            absolute_expires_at_unix_ms: absolute_expires,
            limits: request.limits.clone(),
        };
        let state = SessionState {
            format_version: SESSION_STATE_FORMAT,
            session_id: session_id.clone(),
            status: SessionStatus::Active,
            issued_at_unix_ms: now,
            idle_expires_at_unix_ms: idle_expires,
            absolute_expires_at_unix_ms: absolute_expires,
            limits: request.limits.clone(),
            creation_idempotency_key: idempotency_key.clone(),
            creation_operation_sha256: operation_sha256,
            creation_idle_expires_at_unix_ms: idle_expires,
            token_sha256: digest::sha256_hex(token.as_str().as_bytes()),
            token_generation: 0,
            renewals: BTreeMap::new(),
            closure: None,
            transactions: BTreeMap::new(),
        };
        self.engine.commit_control_transition(&ControlTransition {
            key: session_key(&self.instance, &session_id),
            expected: None,
            replacement: Some(serde_json::to_vec(&state).map_err(contract_json)?),
            at: now,
            actor: "rrd-server".into(),
            action: "session.created".into(),
            request_id: request_id.into(),
            operation_id: operation_id.into(),
        })?;
        Ok(lease)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn begin_transaction(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        request: &BeginTransaction,
        idempotency_key: &CorrelationId,
        now: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<TransactionLease> {
        request
            .validate()
            .map_err(|error| ServiceError::Contract(error.to_string()))?;
        if !matches!(request.scope.as_str(), "claims" | "data") {
            return Err(ServiceError::WrongScope);
        }
        let (bytes, mut state) =
            self.authorize(session_id, token, now, request_id, operation_id)?;
        let operation_sha256 = operation_digest(request)?;
        let transaction_id = self.keyed_id(
            "transaction",
            &[session_id.as_str(), idempotency_key.as_str()],
        )?;
        if let Some(existing) = state.transactions.get(&transaction_id) {
            if existing.begin_idempotency_key != *idempotency_key
                || existing.begin_operation_sha256 != operation_sha256
            {
                return Err(ServiceError::IdempotencyConflict);
            }
            return Ok(existing.lease.clone());
        }
        let open = state
            .transactions
            .values()
            .filter(|transaction| transaction.lease.state == TransactionState::Open)
            .count();
        if open >= usize::from(state.limits.max_open_transactions) {
            return Err(ServiceError::TransactionQuota);
        }
        let expires_at = now
            .checked_add(request.timeout_ms)
            .ok_or_else(|| ServiceError::Contract("transaction expiry overflow".into()))?
            .min(state.absolute_expires_at_unix_ms);
        let transaction = TransactionLease {
            transaction_id: transaction_id.clone(),
            session_id: session_id.clone(),
            scope: request.scope.clone(),
            read_cursor: self.engine.runtime_cursor()?,
            expires_at_unix_ms: expires_at,
            state: TransactionState::Open,
        };
        state.transactions.insert(
            transaction_id,
            TransactionRecord {
                lease: transaction.clone(),
                begin_idempotency_key: idempotency_key.clone(),
                begin_operation_sha256: operation_sha256,
                commit_intent: None,
                commit_receipt: None,
                abort: None,
            },
        );
        touch_session(&mut state, now);
        self.replace_session(
            session_id,
            bytes,
            state,
            now,
            "transaction.began",
            request_id,
            operation_id,
        )?;
        Ok(transaction)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn commit_transaction(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        transaction_id: &CorrelationId,
        idempotency_key: &CorrelationId,
        request: &CommitTransaction,
        now: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<CommitReceipt> {
        request
            .validate()
            .map_err(|error| ServiceError::Contract(error.to_string()))?;
        let actual_digest = request.computed_operation_sha256();
        if actual_digest != request.operation_sha256 {
            return Err(ServiceError::OperationDigestMismatch);
        }
        let (mut bytes, mut state) = self.load_authenticated(session_id, token)?;
        let record = state
            .transactions
            .get(transaction_id)
            .ok_or(ServiceError::TransactionNotFound)?;
        let transaction_scope = record.lease.scope.clone();
        let read_cursor = record.lease.read_cursor;
        if record.lease.state == TransactionState::Committed {
            let intent = record
                .commit_intent
                .as_ref()
                .ok_or(ServiceError::TransactionClosed)?;
            require_same_commit(intent, idempotency_key, &request.operation_sha256)?;
            let mut stored = record
                .commit_receipt
                .clone()
                .ok_or(ServiceError::TransactionClosed)?;
            stored.idempotent_replay = true;
            return Ok(stored);
        }
        if record.lease.state != TransactionState::Open {
            return Err(ServiceError::TransactionClosed);
        }
        if let Some(intent) = &record.commit_intent {
            require_same_commit(intent, idempotency_key, &request.operation_sha256)?;
        } else {
            self.require_active_or_expire(
                session_id,
                bytes.clone(),
                &mut state,
                now,
                request_id,
                operation_id,
            )?;
            let record = state
                .transactions
                .get_mut(transaction_id)
                .expect("transaction retained");
            if now >= record.lease.expires_at_unix_ms {
                record.lease.state = TransactionState::Expired;
                touch_session(&mut state, now);
                self.replace_session(
                    session_id,
                    bytes,
                    state,
                    now,
                    "transaction.expired",
                    request_id,
                    operation_id,
                )?;
                return Err(ServiceError::TransactionExpired);
            }
            let runtime_identity = if transaction_scope.as_str() == "data" {
                let commit =
                    public_runtime_commit(request, session_id, &self.instance, read_cursor, now)?;
                Some((now, commit.digest()))
            } else if transaction_scope.as_str() == "claims" {
                None
            } else {
                return Err(ServiceError::WrongScope);
            };
            record.commit_intent = Some(CommitIntent {
                idempotency_key: idempotency_key.clone(),
                operation_sha256: request.operation_sha256.clone(),
                runtime_at_unix_ms: runtime_identity.as_ref().map(|value| value.0),
                runtime_commit_sha256: runtime_identity.map(|value| value.1),
            });
            bytes = self.replace_session(
                session_id,
                bytes,
                state.clone(),
                now,
                "transaction.commit_prepared",
                request_id,
                operation_id,
            )?;
        }
        let accepted_receipt = if transaction_scope.as_str() == "claims" {
            let claims = public_claims(request, session_id)?;
            let storage_key = claim_acceptance_key(&self.instance, session_id, idempotency_key);
            let accepted = self.engine.append_batch_idempotent(
                &storage_key,
                &request.operation_sha256,
                &claims,
            )?;
            receipt(transaction_id, &accepted)
        } else if transaction_scope.as_str() == "data" {
            let intent = state
                .transactions
                .get(transaction_id)
                .and_then(|record| record.commit_intent.as_ref())
                .ok_or(ServiceError::TransactionClosed)?;
            let runtime_at = intent.runtime_at_unix_ms.ok_or_else(|| {
                ServiceError::Contract("data transaction intent lacks runtime time".into())
            })?;
            let expected_commit = intent.runtime_commit_sha256.as_ref().ok_or_else(|| {
                ServiceError::Contract("data transaction intent lacks runtime identity".into())
            })?;
            let commit = public_runtime_commit(
                request,
                session_id,
                &self.instance,
                read_cursor,
                runtime_at,
            )?;
            if commit.digest() != *expected_commit {
                return Err(ServiceError::OperationDigestMismatch);
            }
            let (outcome, idempotent_replay) =
                if let Some(outcome) = self.engine.runtime_commit_outcome(expected_commit)? {
                    (outcome, true)
                } else {
                    match self.engine.commit_runtime(&commit) {
                        Ok(outcome) => (outcome, false),
                        Err(error) => match self.engine.runtime_commit_outcome(expected_commit)? {
                            Some(outcome) => (outcome, true),
                            None => return Err(error.into()),
                        },
                    }
                };
            runtime_receipt(
                transaction_id,
                &request.operation_sha256,
                &outcome,
                idempotent_replay,
            )
        } else {
            return Err(ServiceError::WrongScope);
        };
        let record = state
            .transactions
            .get_mut(transaction_id)
            .expect("transaction retained");
        record.lease.state = TransactionState::Committed;
        record.commit_receipt = Some(accepted_receipt.clone());
        touch_or_expire_after_commit(&mut state, now);
        self.replace_session(
            session_id,
            bytes,
            state,
            now,
            "transaction.committed",
            request_id,
            operation_id,
        )?;
        Ok(accepted_receipt)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn abort_transaction(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        transaction_id: &CorrelationId,
        request: &AbortTransaction,
        idempotency_key: &CorrelationId,
        now: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<TransactionLease> {
        let (bytes, mut state) =
            self.authorize(session_id, token, now, request_id, operation_id)?;
        let transaction = state
            .transactions
            .get_mut(transaction_id)
            .ok_or(ServiceError::TransactionNotFound)?;
        let operation_sha256 = operation_digest(request)?;
        let action = match transaction.lease.state {
            TransactionState::Open if transaction.commit_intent.is_some() => {
                return Err(ServiceError::CommitInProgress);
            }
            TransactionState::Open if now >= transaction.lease.expires_at_unix_ms => {
                transaction.lease.state = TransactionState::Expired;
                "transaction.expired"
            }
            TransactionState::Open => {
                transaction.lease.state = TransactionState::Aborted;
                transaction.abort = Some(AbortRecord {
                    idempotency_key: idempotency_key.clone(),
                    operation_sha256,
                });
                "transaction.aborted"
            }
            TransactionState::Aborted => {
                let accepted = transaction
                    .abort
                    .as_ref()
                    .ok_or(ServiceError::TransactionClosed)?;
                if accepted.idempotency_key != *idempotency_key
                    || accepted.operation_sha256 != operation_sha256
                {
                    return Err(ServiceError::IdempotencyConflict);
                }
                return Ok(transaction.lease.clone());
            }
            TransactionState::Expired => return Err(ServiceError::TransactionExpired),
            TransactionState::Committed => return Err(ServiceError::TransactionClosed),
        };
        let transaction = transaction.lease.clone();
        touch_session(&mut state, now);
        self.replace_session(
            session_id,
            bytes,
            state,
            now,
            action,
            request_id,
            operation_id,
        )?;
        if transaction.state == TransactionState::Expired {
            return Err(ServiceError::TransactionExpired);
        }
        Ok(transaction)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn renew_session(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        request: &RenewSession,
        idempotency_key: &CorrelationId,
        now: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<SessionLease> {
        let operation_sha256 = operation_digest(request)?;
        let key = session_key(&self.instance, session_id);
        let bytes = self
            .engine
            .control_record(&key)?
            .ok_or(ServiceError::SessionNotFound)?;
        let mut state = decode_session(&bytes)?;
        let presented_sha256 = digest::sha256_hex(token.as_str().as_bytes());
        if let Some(accepted) = state.renewals.get(idempotency_key) {
            if accepted.operation_sha256 != operation_sha256
                || accepted.previous_token_sha256 != presented_sha256
            {
                return Err(ServiceError::IdempotencyConflict);
            }
            return self.session_lease(
                &state,
                accepted.token_generation,
                accepted.idle_expires_at_unix_ms,
            );
        }
        if state.renewals.len() >= MAX_SESSION_RENEWALS {
            return Err(ServiceError::RenewalQuota);
        }
        if state.token_sha256 != presented_sha256 {
            return Err(ServiceError::Unauthenticated);
        }
        self.require_active_or_expire(
            session_id,
            bytes.clone(),
            &mut state,
            now,
            request_id,
            operation_id,
        )?;
        let previous_token_sha256 = state.token_sha256.clone();
        state.token_generation = state
            .token_generation
            .checked_add(1)
            .ok_or_else(|| ServiceError::Contract("session token generation overflow".into()))?;
        let token = self.session_token(session_id, state.token_generation)?;
        state.token_sha256 = digest::sha256_hex(token.as_str().as_bytes());
        touch_session(&mut state, now);
        state.renewals.insert(
            idempotency_key.clone(),
            RenewalRecord {
                operation_sha256,
                previous_token_sha256,
                token_generation: state.token_generation,
                idle_expires_at_unix_ms: state.idle_expires_at_unix_ms,
            },
        );
        let lease = lease_from_state(&state, token, state.idle_expires_at_unix_ms);
        self.replace_session(
            session_id,
            bytes,
            state,
            now,
            "session.renewed",
            request_id,
            operation_id,
        )?;
        Ok(lease)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn close_session(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        request: &CloseSession,
        idempotency_key: &CorrelationId,
        now: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<SessionTermination> {
        let operation_sha256 = operation_digest(request)?;
        let key = session_key(&self.instance, session_id);
        let bytes = self
            .engine
            .control_record(&key)?
            .ok_or(ServiceError::SessionNotFound)?;
        let mut state = decode_session(&bytes)?;
        let presented_sha256 = digest::sha256_hex(token.as_str().as_bytes());
        if let Some(closure) = &state.closure {
            if closure.idempotency_key != *idempotency_key
                || closure.operation_sha256 != operation_sha256
                || closure.previous_token_sha256 != presented_sha256
            {
                return Err(ServiceError::IdempotencyConflict);
            }
            return Ok(termination(state.session_id, closure, true));
        }
        if state.token_sha256 != presented_sha256 {
            return Err(ServiceError::Unauthenticated);
        }
        self.require_active_or_expire(
            session_id,
            bytes.clone(),
            &mut state,
            now,
            request_id,
            operation_id,
        )?;
        if state.transactions.values().any(|transaction| {
            transaction.lease.state == TransactionState::Open && transaction.commit_intent.is_some()
        }) {
            return Err(ServiceError::CommitInProgress);
        }
        let mut affected = 0_u16;
        for transaction in state.transactions.values_mut() {
            if transaction.lease.state == TransactionState::Open {
                transaction.lease.state = TransactionState::Aborted;
                affected = affected.saturating_add(1);
            }
        }
        state.status = SessionStatus::Closed;
        let closure = ClosureRecord {
            idempotency_key: idempotency_key.clone(),
            operation_sha256,
            previous_token_sha256: presented_sha256,
            ended_at_unix_ms: now,
            affected_open_transactions: affected,
        };
        state.closure = Some(closure.clone());
        self.replace_session(
            session_id,
            bytes,
            state,
            now,
            "session.closed",
            request_id,
            operation_id,
        )?;
        Ok(termination(session_id.clone(), &closure, false))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn preview_transaction(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        transaction_id: &CorrelationId,
        request: &PreviewTransaction,
        now: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<TransactionPreview> {
        request
            .validate()
            .map_err(|error| ServiceError::Contract(error.to_string()))?;
        let (bytes, mut state) =
            self.authorize(session_id, token, now, request_id, operation_id)?;
        let transaction = state
            .transactions
            .get_mut(transaction_id)
            .ok_or(ServiceError::TransactionNotFound)?;
        if transaction.lease.state != TransactionState::Open {
            return Err(ServiceError::TransactionClosed);
        }
        if transaction.commit_intent.is_some() {
            return Err(ServiceError::CommitInProgress);
        }
        if now >= transaction.lease.expires_at_unix_ms {
            transaction.lease.state = TransactionState::Expired;
            self.replace_session(
                session_id,
                bytes,
                state,
                now,
                "transaction.expired",
                request_id,
                operation_id,
            )?;
            return Err(ServiceError::TransactionExpired);
        }
        if !matches!(transaction.lease.scope.as_str(), "claims" | "data") {
            return Err(ServiceError::WrongScope);
        }
        let preview = TransactionPreview {
            transaction_id: transaction_id.clone(),
            read_cursor: transaction.lease.read_cursor,
            operation_sha256: transaction_operation_sha256(&request.mutations),
            mutations: request.mutations.clone(),
        };
        touch_session(&mut state, now);
        self.replace_session(
            session_id,
            bytes,
            state,
            now,
            "transaction.previewed",
            request_id,
            operation_id,
        )?;
        Ok(preview)
    }

    fn authorize(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        now: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<(Vec<u8>, SessionState)> {
        let (bytes, mut state) = self.load_authenticated(session_id, token)?;
        self.require_active_or_expire(
            session_id,
            bytes.clone(),
            &mut state,
            now,
            request_id,
            operation_id,
        )?;
        Ok((bytes, state))
    }

    fn load_authenticated(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
    ) -> Result<(Vec<u8>, SessionState)> {
        let bytes = self
            .engine
            .control_record(&session_key(&self.instance, session_id))?
            .ok_or(ServiceError::SessionNotFound)?;
        let state = decode_session(&bytes)?;
        if state.token_sha256 != digest::sha256_hex(token.as_str().as_bytes()) {
            return Err(ServiceError::Unauthenticated);
        }
        Ok((bytes, state))
    }

    #[allow(clippy::too_many_arguments)]
    fn require_active_or_expire(
        &self,
        session_id: &CorrelationId,
        expected: Vec<u8>,
        state: &mut SessionState,
        now: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<()> {
        if state.status != SessionStatus::Active {
            return Err(ServiceError::SessionExpired);
        }
        if now < state.idle_expires_at_unix_ms && now < state.absolute_expires_at_unix_ms {
            return Ok(());
        }
        state.status = SessionStatus::Expired;
        for transaction in state.transactions.values_mut() {
            if transaction.lease.state == TransactionState::Open
                && transaction.commit_intent.is_none()
            {
                transaction.lease.state = TransactionState::Expired;
            }
        }
        self.replace_session(
            session_id,
            expected,
            state.clone(),
            now,
            "session.expired",
            request_id,
            operation_id,
        )?;
        Err(ServiceError::SessionExpired)
    }

    fn replay_created_session(
        &self,
        state: SessionState,
        idempotency_key: &CorrelationId,
        operation_sha256: &str,
    ) -> Result<SessionLease> {
        if state.creation_idempotency_key != *idempotency_key
            || state.creation_operation_sha256 != operation_sha256
        {
            return Err(ServiceError::IdempotencyConflict);
        }
        self.session_lease(&state, 0, state.creation_idle_expires_at_unix_ms)
    }

    fn session_lease(
        &self,
        state: &SessionState,
        token_generation: u64,
        idle_expires_at_unix_ms: u64,
    ) -> Result<SessionLease> {
        let token = self.session_token(&state.session_id, token_generation)?;
        Ok(lease_from_state(state, token, idle_expires_at_unix_ms))
    }

    fn session_token(
        &self,
        session_id: &CorrelationId,
        token_generation: u64,
    ) -> Result<CorrelationId> {
        self.keyed_id(
            "token",
            &[session_id.as_str(), &token_generation.to_string()],
        )
    }

    fn keyed_id(&self, prefix: &str, parts: &[&str]) -> Result<CorrelationId> {
        let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(&self.token_key)
            .expect("HMAC accepts a 32-byte key");
        mac.update(prefix.as_bytes());
        mac.update(b"\0");
        mac.update(self.instance.as_str().as_bytes());
        for part in parts {
            mac.update(b"\0");
            mac.update(part.as_bytes());
        }
        let bytes = mac.finalize().into_bytes();
        CorrelationId::new(format!("{prefix}-{}", lower_hex(bytes.as_slice())))
            .map_err(|error| ServiceError::Contract(error.to_string()))
    }

    #[allow(clippy::too_many_arguments)]
    fn replace_session(
        &self,
        session_id: &CorrelationId,
        expected: Vec<u8>,
        state: SessionState,
        now: u64,
        action: &str,
        request_id: &str,
        operation_id: &str,
    ) -> Result<Vec<u8>> {
        let replacement = serde_json::to_vec(&state).map_err(contract_json)?;
        self.engine.commit_control_transition(&ControlTransition {
            key: session_key(&self.instance, session_id),
            expected: Some(expected),
            replacement: Some(replacement.clone()),
            at: now,
            actor: "rrd-server".into(),
            action: action.into(),
            request_id: request_id.into(),
            operation_id: operation_id.into(),
        })?;
        Ok(replacement)
    }
}

fn touch_session(state: &mut SessionState, now: u64) {
    state.idle_expires_at_unix_ms = now
        .saturating_add(state.limits.idle_timeout_ms)
        .min(state.absolute_expires_at_unix_ms);
}

fn touch_or_expire_after_commit(state: &mut SessionState, now: u64) {
    if state.status == SessionStatus::Active
        && now < state.idle_expires_at_unix_ms
        && now < state.absolute_expires_at_unix_ms
    {
        touch_session(state, now);
        return;
    }
    state.status = SessionStatus::Expired;
    for transaction in state.transactions.values_mut() {
        if transaction.lease.state == TransactionState::Open && transaction.commit_intent.is_none()
        {
            transaction.lease.state = TransactionState::Expired;
        }
    }
}

fn runtime_value(value: &QueryValue) -> Result<RuntimeValue> {
    Ok(match value {
        QueryValue::Null => RuntimeValue::Null,
        QueryValue::Bool(value) => RuntimeValue::Bool(*value),
        QueryValue::Integer(value) => RuntimeValue::Integer(*value),
        QueryValue::Unsigned(value) => RuntimeValue::Unsigned(*value),
        QueryValue::Decimal(value) => RuntimeValue::Decimal(value.clone()),
        QueryValue::String(value) => RuntimeValue::String(value.clone()),
        QueryValue::Digest(value) => RuntimeValue::Digest(value.clone()),
        QueryValue::List(values) => RuntimeValue::List(
            values
                .iter()
                .map(runtime_value)
                .collect::<Result<Vec<_>>>()?,
        ),
        QueryValue::Map(values) => RuntimeValue::Map(
            values
                .iter()
                .map(|(name, value)| Ok((name.clone(), runtime_value(value)?)))
                .collect::<Result<_>>()?,
        ),
    })
}

fn query_value(value: &RuntimeValue) -> Result<QueryValue> {
    Ok(match value {
        RuntimeValue::Null => QueryValue::Null,
        RuntimeValue::Bool(value) => QueryValue::Bool(*value),
        RuntimeValue::Integer(value) => QueryValue::Integer(*value),
        RuntimeValue::Unsigned(value) => QueryValue::Unsigned(*value),
        RuntimeValue::Decimal(value) => QueryValue::Decimal(value.clone()),
        RuntimeValue::String(value) => QueryValue::String(value.clone()),
        RuntimeValue::Digest(value) => QueryValue::Digest(value.clone()),
        RuntimeValue::List(values) => {
            QueryValue::List(values.iter().map(query_value).collect::<Result<Vec<_>>>()?)
        }
        RuntimeValue::Map(values) => QueryValue::Map(
            values
                .iter()
                .map(|(name, value)| Ok((name.clone(), query_value(value)?)))
                .collect::<Result<_>>()?,
        ),
    })
}

fn public_runtime_commit(
    request: &CommitTransaction,
    session_id: &CorrelationId,
    instance: &CanonicalId,
    expected_cursor: u64,
    at: u64,
) -> Result<RuntimeCommit> {
    let mutations = request
        .mutations
        .iter()
        .map(|mutation| public_runtime_mutation(mutation, session_id))
        .collect::<Result<Vec<_>>>()?;
    let commit = RuntimeCommit {
        scope: ScopeId::new(format!("instance:{instance}")).map_err(core_contract)?,
        at,
        actor: format!("session:{}", session_id.as_str()),
        expected_cursor,
        mutations,
    };
    commit.validate().map_err(core_contract)?;
    Ok(commit)
}

fn public_runtime_mutation(
    mutation: &TransactionMutation,
    session_id: &CorrelationId,
) -> Result<RuntimeMutation> {
    Ok(match mutation {
        TransactionMutation::AssertClaim { .. } => RuntimeMutation::Claim {
            claim: public_claim(mutation, session_id)?,
        },
        TransactionMutation::PutSchema { registry } => RuntimeMutation::Schema {
            registry: runtime_schema(registry)?,
        },
        TransactionMutation::PutRecord {
            reference,
            valid_from,
            valid_to,
            properties,
        } => RuntimeMutation::Record {
            record: RuntimeRecord {
                reference: runtime_ref(reference)?,
                valid_from: *valid_from,
                valid_to: *valid_to,
                properties: runtime_properties(properties)?,
            },
        },
        TransactionMutation::PutRelation {
            reference,
            from,
            to,
            valid_from,
            valid_to,
            properties,
        } => RuntimeMutation::Relation {
            relation: RuntimeRelation {
                reference: runtime_ref(reference)?,
                from: runtime_ref(from)?,
                to: runtime_ref(to)?,
                valid_from: *valid_from,
                valid_to: *valid_to,
                properties: runtime_properties(properties)?,
            },
        },
        TransactionMutation::AppendEvent {
            kind,
            subject,
            properties,
        } => RuntimeMutation::Event {
            event: RuntimeEvent {
                kind: RuntimeType::new(kind.as_str()).map_err(core_contract)?,
                subject: subject.as_ref().map(runtime_ref).transpose()?,
                properties: runtime_properties(properties)?,
            },
        },
        TransactionMutation::PutVector {
            reference,
            subject,
            field,
            valid_from,
            valid_to,
            value,
            provenance,
            properties,
        } => RuntimeMutation::Vector {
            vector: RuntimeVector {
                reference: runtime_ref(reference)?,
                subject: runtime_ref(subject)?,
                field: field.as_str().into(),
                valid_from: *valid_from,
                valid_to: *valid_to,
                value: runtime_vector_value(value),
                provenance: provenance
                    .as_ref()
                    .map(|value| -> Result<EmbeddingProvenance> {
                        Ok(EmbeddingProvenance {
                            source_digest: value.source_sha256.clone(),
                            model: value.model.clone(),
                            model_digest: value.model_sha256.clone(),
                            dimensions: value.dimensions,
                            normalization: match value.normalization {
                                DataVectorNormalization::None => VectorNormalization::None,
                                DataVectorNormalization::UnitL2 => VectorNormalization::UnitL2,
                            },
                            generation_parameters: runtime_properties(
                                &value.generation_parameters,
                            )?,
                        })
                    })
                    .transpose()?,
                properties: runtime_properties(properties)?,
            },
        },
        TransactionMutation::AppendSeriesSample {
            reference,
            series,
            observed_at,
            value,
            properties,
        } => RuntimeMutation::SeriesSample {
            sample: RuntimeSeriesSample {
                reference: runtime_ref(reference)?,
                series: runtime_ref(series)?,
                observed_at: *observed_at,
                value: match value {
                    DataSeriesValue::Integer(value) => SeriesValue::Integer(*value),
                    DataSeriesValue::Unsigned(value) => SeriesValue::Unsigned(*value),
                    DataSeriesValue::Decimal(value) => SeriesValue::Decimal(value.clone()),
                    DataSeriesValue::Bool(value) => SeriesValue::Bool(*value),
                    DataSeriesValue::String(value) => SeriesValue::String(value.clone()),
                },
                properties: runtime_properties(properties)?,
            },
        },
        TransactionMutation::PutGeo {
            reference,
            subject,
            field,
            valid_from,
            valid_to,
            value,
            properties,
        } => RuntimeMutation::Geo {
            geo: RuntimeGeo {
                reference: runtime_ref(reference)?,
                subject: runtime_ref(subject)?,
                field: field.as_str().into(),
                valid_from: *valid_from,
                valid_to: *valid_to,
                value: runtime_geo_value(value),
                properties: runtime_properties(properties)?,
            },
        },
        TransactionMutation::PublishObjectReference {
            reference,
            subject,
            sha256,
            length,
            media_type,
            receipt,
            properties,
        } => RuntimeMutation::Object {
            object: ObjectReference {
                reference: runtime_ref(reference)?,
                subject: subject.as_ref().map(runtime_ref).transpose()?,
                sha256: sha256.clone(),
                length: *length,
                media_type: media_type.clone(),
                receipt: ObjectReceipt {
                    backend: receipt.backend.clone(),
                    key: receipt.key.clone(),
                    version: receipt.version.clone(),
                    etag: receipt.etag.clone(),
                },
                properties: runtime_properties(properties)?,
            },
        },
    })
}

fn runtime_ref(reference: &DataReference) -> Result<RuntimeRef> {
    RuntimeRef::new(reference.kind.as_str(), reference.id.as_str()).map_err(core_contract)
}

fn runtime_properties(properties: &DataProperties) -> Result<RuntimeProperties> {
    properties
        .iter()
        .map(|(name, value)| Ok((name.clone(), runtime_value(value)?)))
        .collect()
}

fn runtime_schema(registry: &DataSchemaRegistry) -> Result<RuntimeSchemaRegistry> {
    Ok(RuntimeSchemaRegistry {
        revision: registry.revision,
        migration: registry.migration.clone(),
        records: registry
            .records
            .iter()
            .map(|(kind, schema)| {
                Ok((
                    RuntimeType::new(kind.as_str()).map_err(core_contract)?,
                    RuntimeRecordSchema {
                        properties: runtime_property_schemas(&schema.properties),
                        allow_additional_properties: schema.allow_additional_properties,
                        unique_properties: schema.unique_properties.clone(),
                    },
                ))
            })
            .collect::<Result<_>>()?,
        relations: registry
            .relations
            .iter()
            .map(|(kind, schema)| {
                Ok((
                    RuntimeType::new(kind.as_str()).map_err(core_contract)?,
                    RuntimeRelationSchema {
                        from: schema
                            .from
                            .iter()
                            .map(|kind| RuntimeType::new(kind.as_str()).map_err(core_contract))
                            .collect::<Result<_>>()?,
                        to: schema
                            .to
                            .iter()
                            .map(|kind| RuntimeType::new(kind.as_str()).map_err(core_contract))
                            .collect::<Result<_>>()?,
                        properties: runtime_property_schemas(&schema.properties),
                        allow_additional_properties: schema.allow_additional_properties,
                        unique_pair: schema.unique_pair,
                        max_outgoing: schema.max_outgoing,
                        max_incoming: schema.max_incoming,
                    },
                ))
            })
            .collect::<Result<_>>()?,
        events: registry
            .events
            .iter()
            .map(|(kind, schema)| {
                Ok((
                    RuntimeType::new(kind.as_str()).map_err(core_contract)?,
                    RuntimeEventSchema {
                        subject_required: schema.subject_required,
                        subject_types: schema
                            .subject_types
                            .iter()
                            .map(|kind| RuntimeType::new(kind.as_str()).map_err(core_contract))
                            .collect::<Result<_>>()?,
                        properties: runtime_property_schemas(&schema.properties),
                        allow_additional_properties: schema.allow_additional_properties,
                    },
                ))
            })
            .collect::<Result<_>>()?,
    })
}

fn runtime_property_schemas(
    properties: &BTreeMap<String, rrd_contract::DataPropertySchema>,
) -> BTreeMap<String, RuntimePropertySchema> {
    properties
        .iter()
        .map(|(name, schema)| {
            (
                name.clone(),
                RuntimePropertySchema {
                    value_type: match schema.value_type {
                        DataValueType::Null => RuntimeValueType::Null,
                        DataValueType::Bool => RuntimeValueType::Bool,
                        DataValueType::Integer => RuntimeValueType::Integer,
                        DataValueType::Unsigned => RuntimeValueType::Unsigned,
                        DataValueType::Decimal => RuntimeValueType::Decimal,
                        DataValueType::String => RuntimeValueType::String,
                        DataValueType::Digest => RuntimeValueType::Digest,
                        DataValueType::List => RuntimeValueType::List,
                        DataValueType::Map => RuntimeValueType::Map,
                    },
                    required: schema.required,
                },
            )
        })
        .collect()
}

fn runtime_vector_value(value: &DataVectorValue) -> VectorValue {
    match value {
        DataVectorValue::Dense { values } => VectorValue::Dense {
            values: values.clone(),
        },
        DataVectorValue::Sparse {
            dimensions,
            indices,
            values,
        } => VectorValue::Sparse {
            dimensions: *dimensions,
            indices: indices.clone(),
            values: values.clone(),
        },
        DataVectorValue::MultiDense {
            dimensions,
            vectors,
        } => VectorValue::MultiDense {
            dimensions: *dimensions,
            vectors: vectors.clone(),
        },
    }
}

fn runtime_geo_value(value: &DataGeoValue) -> GeoValue {
    let point = |value: &DataGeoPoint| GeoPoint {
        longitude: value.longitude,
        latitude: value.latitude,
    };
    match value {
        DataGeoValue::Point { point: value } => GeoValue::Point {
            point: point(value),
        },
        DataGeoValue::BoundingBox {
            southwest,
            northeast,
        } => GeoValue::BoundingBox {
            southwest: point(southwest),
            northeast: point(northeast),
        },
    }
}

fn core_contract(error: vyrm_core::Error) -> ServiceError {
    ServiceError::Contract(error.to_string())
}

fn core_vector(error: vyrm_core::Error) -> ServiceError {
    ServiceError::Vector(error.to_string())
}

fn core_changefeed(error: vyrm_core::Error) -> ServiceError {
    ServiceError::Changefeed(error.to_string())
}

fn public_data_ref(reference: &RuntimeRef) -> Result<DataReference> {
    Ok(DataReference {
        kind: CanonicalId::new(reference.kind.as_str())
            .map_err(|error| ServiceError::Vector(error.to_string()))?,
        id: CanonicalId::new(reference.id.as_str())
            .map_err(|error| ServiceError::Vector(error.to_string()))?,
    })
}

fn public_runtime_change(change: &vyrm_core::RuntimeChange) -> Result<RuntimeChangeSnapshot> {
    Ok(RuntimeChangeSnapshot {
        cursor: change.cursor,
        commit_sha256: change.commit_id.clone(),
        commit_ordinal: change.commit_ordinal,
        scope: change.scope.to_string(),
        at_unix_ms: change.at,
        actor: change.actor.clone(),
        mutation: match &change.mutation {
            RuntimeMutation::Claim { claim } => ChangeMutationSnapshot::Claim {
                claim: ClaimChangeSnapshot {
                    subject: claim.subject.as_str().into(),
                    predicate: claim.predicate.as_str().into(),
                    object: claim.object.clone(),
                    valid_from: claim.valid_from,
                    valid_to: claim.valid_to,
                    tx_time: claim.tx_time,
                    producer: claim.producer.actor.clone(),
                    on_behalf_of: claim.producer.on_behalf_of.clone(),
                    session: claim.producer.session.clone(),
                    confidence: claim.confidence,
                    supersedes_sha256: claim.supersedes.clone(),
                    signature: claim.signature.clone(),
                    tier: match claim.tier {
                        Tier::Local => ClaimTierSnapshot::Local,
                        Tier::Primary => ClaimTierSnapshot::Primary,
                        Tier::Tenant => ClaimTierSnapshot::Tenant,
                    },
                    promotion: match claim.promotion_state {
                        PromotionState::Unpromoted => ClaimPromotionSnapshot::Unpromoted,
                        PromotionState::Pending => ClaimPromotionSnapshot::Pending,
                        PromotionState::Promoted => ClaimPromotionSnapshot::Promoted,
                        PromotionState::Denied => ClaimPromotionSnapshot::Denied,
                    },
                },
            },
            mutation => ChangeMutationSnapshot::Data {
                mutation: public_data_mutation(mutation)?,
            },
        },
        previous_change_sha256: change.previous_digest.clone(),
        change_sha256: change.digest.clone(),
    })
}

fn public_data_mutation(mutation: &RuntimeMutation) -> Result<TransactionMutation> {
    Ok(match mutation {
        RuntimeMutation::Claim { .. } => {
            return Err(ServiceError::Changefeed(
                "claim must use the lossless claim snapshot".into(),
            ));
        }
        RuntimeMutation::Schema { registry } => TransactionMutation::PutSchema {
            registry: public_schema(registry)?,
        },
        RuntimeMutation::Record { record } => TransactionMutation::PutRecord {
            reference: public_change_ref(&record.reference)?,
            valid_from: record.valid_from,
            valid_to: record.valid_to,
            properties: public_properties(&record.properties)?,
        },
        RuntimeMutation::Relation { relation } => TransactionMutation::PutRelation {
            reference: public_change_ref(&relation.reference)?,
            from: public_change_ref(&relation.from)?,
            to: public_change_ref(&relation.to)?,
            valid_from: relation.valid_from,
            valid_to: relation.valid_to,
            properties: public_properties(&relation.properties)?,
        },
        RuntimeMutation::Event { event } => TransactionMutation::AppendEvent {
            kind: public_change_id(event.kind.as_str())?,
            subject: event.subject.as_ref().map(public_change_ref).transpose()?,
            properties: public_properties(&event.properties)?,
        },
        RuntimeMutation::Vector { vector } => TransactionMutation::PutVector {
            reference: public_change_ref(&vector.reference)?,
            subject: public_change_ref(&vector.subject)?,
            field: public_change_id(&vector.field)?,
            valid_from: vector.valid_from,
            valid_to: vector.valid_to,
            value: public_vector_value(&vector.value),
            provenance: vector
                .provenance
                .as_ref()
                .map(|value| -> Result<rrd_contract::DataEmbeddingProvenance> {
                    Ok(rrd_contract::DataEmbeddingProvenance {
                        source_sha256: value.source_digest.clone(),
                        model: value.model.clone(),
                        model_sha256: value.model_digest.clone(),
                        dimensions: value.dimensions,
                        normalization: match value.normalization {
                            VectorNormalization::None => DataVectorNormalization::None,
                            VectorNormalization::UnitL2 => DataVectorNormalization::UnitL2,
                        },
                        generation_parameters: public_properties(&value.generation_parameters)?,
                    })
                })
                .transpose()?,
            properties: public_properties(&vector.properties)?,
        },
        RuntimeMutation::SeriesSample { sample } => TransactionMutation::AppendSeriesSample {
            reference: public_change_ref(&sample.reference)?,
            series: public_change_ref(&sample.series)?,
            observed_at: sample.observed_at,
            value: match &sample.value {
                SeriesValue::Integer(value) => DataSeriesValue::Integer(*value),
                SeriesValue::Unsigned(value) => DataSeriesValue::Unsigned(*value),
                SeriesValue::Decimal(value) => DataSeriesValue::Decimal(value.clone()),
                SeriesValue::Bool(value) => DataSeriesValue::Bool(*value),
                SeriesValue::String(value) => DataSeriesValue::String(value.clone()),
            },
            properties: public_properties(&sample.properties)?,
        },
        RuntimeMutation::Geo { geo } => TransactionMutation::PutGeo {
            reference: public_change_ref(&geo.reference)?,
            subject: public_change_ref(&geo.subject)?,
            field: public_change_id(&geo.field)?,
            valid_from: geo.valid_from,
            valid_to: geo.valid_to,
            value: public_geo_value(&geo.value),
            properties: public_properties(&geo.properties)?,
        },
        RuntimeMutation::Object { object } => TransactionMutation::PublishObjectReference {
            reference: public_change_ref(&object.reference)?,
            subject: object.subject.as_ref().map(public_change_ref).transpose()?,
            sha256: object.sha256.clone(),
            length: object.length,
            media_type: object.media_type.clone(),
            receipt: DataObjectReceipt {
                backend: object.receipt.backend.clone(),
                key: object.receipt.key.clone(),
                version: object.receipt.version.clone(),
                etag: object.receipt.etag.clone(),
            },
            properties: public_properties(&object.properties)?,
        },
    })
}

fn public_change_id(value: &str) -> Result<CanonicalId> {
    CanonicalId::new(value).map_err(|error| ServiceError::Changefeed(error.to_string()))
}

fn public_change_ref(reference: &RuntimeRef) -> Result<DataReference> {
    Ok(DataReference {
        kind: public_change_id(reference.kind.as_str())?,
        id: public_change_id(reference.id.as_str())?,
    })
}

fn public_properties(properties: &RuntimeProperties) -> Result<DataProperties> {
    properties
        .iter()
        .map(|(name, value)| Ok((name.clone(), query_value(value)?)))
        .collect()
}

fn public_schema(registry: &RuntimeSchemaRegistry) -> Result<DataSchemaRegistry> {
    Ok(DataSchemaRegistry {
        revision: registry.revision,
        migration: registry.migration.clone(),
        records: registry
            .records
            .iter()
            .map(|(kind, schema)| {
                Ok((
                    public_change_id(kind.as_str())?,
                    DataRecordSchema {
                        properties: public_property_schemas(&schema.properties),
                        allow_additional_properties: schema.allow_additional_properties,
                        unique_properties: schema.unique_properties.clone(),
                    },
                ))
            })
            .collect::<Result<_>>()?,
        relations: registry
            .relations
            .iter()
            .map(|(kind, schema)| {
                Ok((
                    public_change_id(kind.as_str())?,
                    DataRelationSchema {
                        from: schema
                            .from
                            .iter()
                            .map(|kind| public_change_id(kind.as_str()))
                            .collect::<Result<_>>()?,
                        to: schema
                            .to
                            .iter()
                            .map(|kind| public_change_id(kind.as_str()))
                            .collect::<Result<_>>()?,
                        properties: public_property_schemas(&schema.properties),
                        allow_additional_properties: schema.allow_additional_properties,
                        unique_pair: schema.unique_pair,
                        max_outgoing: schema.max_outgoing,
                        max_incoming: schema.max_incoming,
                    },
                ))
            })
            .collect::<Result<_>>()?,
        events: registry
            .events
            .iter()
            .map(|(kind, schema)| {
                Ok((
                    public_change_id(kind.as_str())?,
                    DataEventSchema {
                        subject_required: schema.subject_required,
                        subject_types: schema
                            .subject_types
                            .iter()
                            .map(|kind| public_change_id(kind.as_str()))
                            .collect::<Result<_>>()?,
                        properties: public_property_schemas(&schema.properties),
                        allow_additional_properties: schema.allow_additional_properties,
                    },
                ))
            })
            .collect::<Result<_>>()?,
    })
}

fn public_property_schemas(
    properties: &BTreeMap<String, RuntimePropertySchema>,
) -> BTreeMap<String, DataPropertySchema> {
    properties
        .iter()
        .map(|(name, schema)| {
            (
                name.clone(),
                DataPropertySchema {
                    value_type: match schema.value_type {
                        RuntimeValueType::Null => DataValueType::Null,
                        RuntimeValueType::Bool => DataValueType::Bool,
                        RuntimeValueType::Integer => DataValueType::Integer,
                        RuntimeValueType::Unsigned => DataValueType::Unsigned,
                        RuntimeValueType::Decimal => DataValueType::Decimal,
                        RuntimeValueType::String => DataValueType::String,
                        RuntimeValueType::Digest => DataValueType::Digest,
                        RuntimeValueType::List => DataValueType::List,
                        RuntimeValueType::Map => DataValueType::Map,
                    },
                    required: schema.required,
                },
            )
        })
        .collect()
}

fn public_vector_value(value: &VectorValue) -> DataVectorValue {
    match value {
        VectorValue::Dense { values } => DataVectorValue::Dense {
            values: values.clone(),
        },
        VectorValue::Sparse {
            dimensions,
            indices,
            values,
        } => DataVectorValue::Sparse {
            dimensions: *dimensions,
            indices: indices.clone(),
            values: values.clone(),
        },
        VectorValue::MultiDense {
            dimensions,
            vectors,
        } => DataVectorValue::MultiDense {
            dimensions: *dimensions,
            vectors: vectors.clone(),
        },
    }
}

fn public_geo_value(value: &GeoValue) -> DataGeoValue {
    let point = |value: &GeoPoint| DataGeoPoint {
        longitude: value.longitude,
        latitude: value.latitude,
    };
    match value {
        GeoValue::Point { point: value } => DataGeoValue::Point {
            point: point(value),
        },
        GeoValue::BoundingBox {
            southwest,
            northeast,
        } => DataGeoValue::BoundingBox {
            southwest: point(southwest),
            northeast: point(northeast),
        },
    }
}

fn public_claim(mutation: &TransactionMutation, session_id: &CorrelationId) -> Result<Claim> {
    let TransactionMutation::AssertClaim {
        subject,
        predicate,
        object,
        valid_from,
        tx_time,
        producer,
        confidence,
    } = mutation
    else {
        return Err(ServiceError::Contract(
            "expected an assert_claim mutation".into(),
        ));
    };
    let mut claim = Claim::new(
        Subject::new(subject.as_str()).map_err(core_contract)?,
        Predicate::new(predicate.as_str()).map_err(core_contract)?,
        object,
        *valid_from,
        *tx_time,
        Producer {
            actor: producer.as_str().into(),
            on_behalf_of: None,
            session: Some(session_id.as_str().into()),
        },
    );
    claim.confidence = *confidence;
    Ok(claim)
}

fn public_claims(request: &CommitTransaction, session_id: &CorrelationId) -> Result<Vec<Claim>> {
    request
        .mutations
        .iter()
        .map(|mutation| match mutation {
            TransactionMutation::AssertClaim { .. } => public_claim(mutation, session_id),
            _ => Err(ServiceError::Contract(
                "the claims transaction scope accepts only assert_claim mutations".into(),
            )),
        })
        .collect()
}

fn receipt(
    transaction_id: &CorrelationId,
    accepted: &vyrm_store::IdempotentAppendOutcome,
) -> CommitReceipt {
    CommitReceipt {
        transaction_id: transaction_id.clone(),
        operation_sha256: accepted.operation_sha256.clone(),
        first_claim_sequence: accepted.append.first_sequence,
        last_claim_sequence: accepted.append.last_sequence,
        mutation_count: accepted.append.count as u64,
        runtime_commit_sha256: None,
        first_runtime_cursor: None,
        last_runtime_cursor: None,
        claim_mutation_count: None,
        idempotent_replay: accepted.idempotent_replay,
    }
}

fn runtime_receipt(
    transaction_id: &CorrelationId,
    operation_sha256: &str,
    accepted: &vyrm_core::RuntimeCommitOutcome,
    idempotent_replay: bool,
) -> CommitReceipt {
    let claim_count = accepted
        .last_claim_sequence
        .zip(accepted.first_claim_sequence)
        .map_or(0, |(last, first)| last - first + 1);
    CommitReceipt {
        transaction_id: transaction_id.clone(),
        operation_sha256: operation_sha256.into(),
        first_claim_sequence: accepted.first_claim_sequence.unwrap_or(0),
        last_claim_sequence: accepted.last_claim_sequence.unwrap_or(0),
        mutation_count: accepted.count as u64,
        runtime_commit_sha256: Some(accepted.commit_id.clone()),
        first_runtime_cursor: Some(accepted.first_cursor),
        last_runtime_cursor: Some(accepted.last_cursor),
        claim_mutation_count: Some(claim_count),
        idempotent_replay,
    }
}

fn operation_digest(value: &impl Serialize) -> Result<String> {
    Ok(digest::sha256_hex(
        &serde_json::to_vec(value).map_err(contract_json)?,
    ))
}

fn decode_session(bytes: &[u8]) -> Result<SessionState> {
    let state: SessionState = serde_json::from_slice(bytes).map_err(contract_json)?;
    if state.format_version != SESSION_STATE_FORMAT {
        return Err(ServiceError::Contract(format!(
            "unsupported session-state format {}; expected {SESSION_STATE_FORMAT}",
            state.format_version
        )));
    }
    Ok(state)
}

fn lease_from_state(
    state: &SessionState,
    token: CorrelationId,
    idle_expires_at_unix_ms: u64,
) -> SessionLease {
    SessionLease {
        session_id: state.session_id.clone(),
        token,
        issued_at_unix_ms: state.issued_at_unix_ms,
        idle_expires_at_unix_ms,
        absolute_expires_at_unix_ms: state.absolute_expires_at_unix_ms,
        limits: state.limits.clone(),
    }
}

fn termination(
    session_id: CorrelationId,
    closure: &ClosureRecord,
    idempotent_replay: bool,
) -> SessionTermination {
    SessionTermination {
        session_id,
        state: SessionEndState::Closed,
        ended_at_unix_ms: closure.ended_at_unix_ms,
        affected_open_transactions: closure.affected_open_transactions,
        idempotent_replay,
    }
}

fn require_same_commit(
    intent: &CommitIntent,
    idempotency_key: &CorrelationId,
    operation_sha256: &str,
) -> Result<()> {
    if intent.idempotency_key != *idempotency_key || intent.operation_sha256 != operation_sha256 {
        return Err(ServiceError::IdempotencyConflict);
    }
    Ok(())
}

fn claim_acceptance_key(
    instance: &CanonicalId,
    session_id: &CorrelationId,
    idempotency_key: &CorrelationId,
) -> String {
    format!(
        "rrd:{}",
        digest::sha256_hex(
            format!(
                "{}\0{}\0{}",
                instance.as_str(),
                session_id.as_str(),
                idempotency_key.as_str()
            )
            .as_bytes()
        )
    )
}

fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[usize::from(byte >> 4)] as char);
        output.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    output
}

fn session_key(instance: &CanonicalId, session: &CorrelationId) -> String {
    format!("server/state/{instance}/session/{}", session.as_str())
}

fn contract_json(error: serde_json::Error) -> ServiceError {
    ServiceError::Contract(error.to_string())
}

pub fn default_session_limits() -> SessionLimits {
    SessionLimits {
        idle_timeout_ms: 60_000,
        absolute_timeout_ms: 900_000,
        max_open_transactions: 8,
    }
}
