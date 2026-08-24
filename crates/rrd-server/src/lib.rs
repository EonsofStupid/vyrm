//! Persistent, journaled RRD transport-session and claim-transaction service.

mod http;

pub use http::{HttpError, RRD_MAX_BODY_BYTES, RrdHttpServer, load_or_create_token_key};

use hmac::{Hmac, KeyInit, Mac};
use rrd_contract::{
    AbortTransaction, BeginTransaction, CanonicalId, CloseSession, CommitReceipt,
    CommitTransaction, CorrelationId, CreateSession, ExecuteQuery, PreviewTransaction,
    QueryExecutionSnapshot, QueryPlanCandidate, QueryPlanSnapshot, QueryResult, QueryRowSnapshot,
    QueryValue, RenewSession, SessionEndState, SessionLease, SessionLimits, SessionTermination,
    TransactionLease, TransactionMutation, TransactionPreview, TransactionState,
    transaction_operation_sha256,
};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::collections::BTreeMap;
use std::fmt;
use vyrm_core::{Claim, Predicate, Producer, RuntimeValue, ScopeId, Subject, digest};
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
            if record.lease.scope.as_str() != "claims" {
                return Err(ServiceError::WrongScope);
            }
            record.commit_intent = Some(CommitIntent {
                idempotency_key: idempotency_key.clone(),
                operation_sha256: request.operation_sha256.clone(),
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
        let claims = public_claims(request, session_id)?;
        let storage_key = claim_acceptance_key(&self.instance, session_id, idempotency_key);
        let accepted = self.engine.append_batch_idempotent(
            &storage_key,
            &request.operation_sha256,
            &claims,
        )?;
        let accepted_receipt = receipt(transaction_id, &accepted);
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
        if transaction.lease.scope.as_str() != "claims" {
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

fn public_claims(request: &CommitTransaction, session_id: &CorrelationId) -> Result<Vec<Claim>> {
    request
        .mutations
        .iter()
        .map(|mutation| match mutation {
            TransactionMutation::AssertClaim {
                subject,
                predicate,
                object,
                valid_from,
                tx_time,
                producer,
                confidence,
            } => {
                let mut claim = Claim::new(
                    Subject::new(subject.as_str())
                        .map_err(|error| ServiceError::Contract(error.to_string()))?,
                    Predicate::new(predicate.as_str())
                        .map_err(|error| ServiceError::Contract(error.to_string()))?,
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
        idempotent_replay: accepted.idempotent_replay,
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
