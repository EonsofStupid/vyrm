//! Persistent, journaled RRD transport-session and claim-transaction service.

use rrd_contract::{
    BeginTransaction, CanonicalId, CommitReceipt, CommitTransaction, CorrelationId, CreateSession,
    SessionLease, SessionLimits, TransactionLease, TransactionMutation, TransactionState,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use vyrm_core::{digest, Claim, Predicate, Producer, Subject};
use vyrm_store::{ControlTransition, Engine};

static ID_COUNTER: AtomicU64 = AtomicU64::new(1);

pub type Result<T> = std::result::Result<T, ServiceError>;

#[derive(Debug)]
pub enum ServiceError {
    Contract(String),
    Store(vyrm_store::Error),
    SessionNotFound,
    Unauthenticated,
    SessionExpired,
    TransactionNotFound,
    TransactionExpired,
    TransactionClosed,
    TransactionQuota,
    OperationDigestMismatch,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionState {
    session_id: CorrelationId,
    status: SessionStatus,
    issued_at_unix_ms: u64,
    idle_expires_at_unix_ms: u64,
    absolute_expires_at_unix_ms: u64,
    limits: SessionLimits,
    token_sha256: String,
    transactions: BTreeMap<CorrelationId, TransactionLease>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SessionStatus {
    Active,
    Expired,
}

pub struct RrdService<E> {
    engine: E,
    instance: CanonicalId,
}

impl<E: Engine> RrdService<E> {
    pub fn new(engine: E, instance: CanonicalId) -> Self {
        Self { engine, instance }
    }

    pub fn engine(&self) -> &E {
        &self.engine
    }

    pub fn create_session(
        &self,
        request: &CreateSession,
        now: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<SessionLease> {
        request
            .limits
            .validate()
            .map_err(|error| ServiceError::Contract(error.to_string()))?;
        let session_id = generated_id("session", now, request_id)?;
        let token = generated_id("token", now, operation_id)?;
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
            session_id: session_id.clone(),
            status: SessionStatus::Active,
            issued_at_unix_ms: now,
            idle_expires_at_unix_ms: idle_expires,
            absolute_expires_at_unix_ms: absolute_expires,
            limits: request.limits.clone(),
            token_sha256: digest::sha256_hex(token.as_str().as_bytes()),
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

    pub fn begin_transaction(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        request: &BeginTransaction,
        now: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<TransactionLease> {
        request
            .validate()
            .map_err(|error| ServiceError::Contract(error.to_string()))?;
        let (bytes, mut state) =
            self.authorize(session_id, token, now, request_id, operation_id)?;
        let open = state
            .transactions
            .values()
            .filter(|transaction| transaction.state == TransactionState::Open)
            .count();
        if open >= usize::from(state.limits.max_open_transactions) {
            return Err(ServiceError::TransactionQuota);
        }
        let transaction_id = generated_id("transaction", now, operation_id)?;
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
        state
            .transactions
            .insert(transaction_id, transaction.clone());
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
        let actual_digest =
            digest::sha256_hex(&serde_json::to_vec(&request.mutations).map_err(contract_json)?);
        if actual_digest != request.operation_sha256 {
            return Err(ServiceError::OperationDigestMismatch);
        }
        let (bytes, mut state) =
            self.authorize(session_id, token, now, request_id, operation_id)?;
        let transaction_state = state
            .transactions
            .get(transaction_id)
            .map(|transaction| transaction.state)
            .ok_or(ServiceError::TransactionNotFound)?;
        if transaction_state == TransactionState::Committed {
            let claims = public_claims(request, session_id)?;
            let accepted = self.engine.append_batch_idempotent(
                idempotency_key.as_str(),
                &request.operation_sha256,
                &claims,
            )?;
            touch_session(&mut state, now);
            self.replace_session(
                session_id,
                bytes,
                state,
                now,
                "transaction.replayed",
                request_id,
                operation_id,
            )?;
            return Ok(receipt(transaction_id, &accepted));
        }
        if transaction_state != TransactionState::Open {
            return Err(ServiceError::TransactionClosed);
        }
        if now
            >= state
                .transactions
                .get(transaction_id)
                .expect("transaction retained")
                .expires_at_unix_ms
        {
            state
                .transactions
                .get_mut(transaction_id)
                .expect("transaction retained")
                .state = TransactionState::Expired;
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
        let claims = public_claims(request, session_id)?;
        let accepted = self.engine.append_batch_idempotent(
            idempotency_key.as_str(),
            &request.operation_sha256,
            &claims,
        )?;
        state
            .transactions
            .get_mut(transaction_id)
            .expect("transaction retained")
            .state = TransactionState::Committed;
        touch_session(&mut state, now);
        self.replace_session(
            session_id,
            bytes,
            state,
            now,
            "transaction.committed",
            request_id,
            operation_id,
        )?;
        Ok(receipt(transaction_id, &accepted))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn abort_transaction(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        transaction_id: &CorrelationId,
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
        let action = match transaction.state {
            TransactionState::Open if now >= transaction.expires_at_unix_ms => {
                transaction.state = TransactionState::Expired;
                "transaction.expired"
            }
            TransactionState::Open => {
                transaction.state = TransactionState::Aborted;
                "transaction.aborted"
            }
            TransactionState::Aborted => "transaction.abort_replayed",
            TransactionState::Expired => return Err(ServiceError::TransactionExpired),
            TransactionState::Committed => return Err(ServiceError::TransactionClosed),
        };
        let transaction = transaction.clone();
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

    fn authorize(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        now: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<(Vec<u8>, SessionState)> {
        let key = session_key(&self.instance, session_id);
        let bytes = self
            .engine
            .control_record(&key)?
            .ok_or(ServiceError::SessionNotFound)?;
        let mut state: SessionState = serde_json::from_slice(&bytes).map_err(contract_json)?;
        if state.token_sha256 != digest::sha256_hex(token.as_str().as_bytes()) {
            return Err(ServiceError::Unauthenticated);
        }
        if state.status != SessionStatus::Active {
            return Err(ServiceError::SessionExpired);
        }
        if now >= state.idle_expires_at_unix_ms || now >= state.absolute_expires_at_unix_ms {
            state.status = SessionStatus::Expired;
            for transaction in state.transactions.values_mut() {
                if transaction.state == TransactionState::Open {
                    transaction.state = TransactionState::Expired;
                }
            }
            self.replace_session(
                session_id,
                bytes,
                state,
                now,
                "session.expired",
                request_id,
                operation_id,
            )?;
            return Err(ServiceError::SessionExpired);
        }
        Ok((bytes, state))
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
    ) -> Result<()> {
        self.engine.commit_control_transition(&ControlTransition {
            key: session_key(&self.instance, session_id),
            expected: Some(expected),
            replacement: Some(serde_json::to_vec(&state).map_err(contract_json)?),
            at: now,
            actor: "rrd-server".into(),
            action: action.into(),
            request_id: request_id.into(),
            operation_id: operation_id.into(),
        })?;
        Ok(())
    }
}

fn touch_session(state: &mut SessionState, now: u64) {
    state.idle_expires_at_unix_ms = now
        .saturating_add(state.limits.idle_timeout_ms)
        .min(state.absolute_expires_at_unix_ms);
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

fn generated_id(prefix: &str, now: u64, seed: &str) -> Result<CorrelationId> {
    let counter = ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    CorrelationId::new(format!(
        "{prefix}-{}",
        digest::sha256_hex(format!("{prefix}\0{now}\0{seed}\0{counter}").as_bytes())
    ))
    .map_err(|error| ServiceError::Contract(error.to_string()))
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
