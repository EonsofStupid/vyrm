use crate::RrdEngine;
use rrd_contract::{CanonicalId, CorrelationId, CreateSession, SessionLimits, MAX_LEASE_MS};
use rrd_core::digest;

#[derive(Clone, Copy)]
pub(super) enum AdapterCaller<'a> {
    Anonymous,
    ApiKey {
        principal_id: &'a CanonicalId,
        credential: &'a [u8],
    },
    Session {
        session_id: &'a CorrelationId,
        token: &'a CorrelationId,
    },
}

/// Internal lease plumbing for one high-level runtime adapter operation.
///
/// Session tokens never enter tool schemas or results. Stable identities are
/// domain-separated by operation name and caller idempotency key so retries
/// converge through the ordinary RRD session and operation repositories.
pub(super) struct AdapterSession {
    pub(super) session_id: CorrelationId,
    pub(super) token: CorrelationId,
    pub(super) request_id: CorrelationId,
    pub(super) operation_id: CorrelationId,
}

impl AdapterSession {
    pub(super) fn open(
        engine: &RrdEngine,
        caller: AdapterCaller<'_>,
        operation: &str,
        idempotency_key: &CorrelationId,
        at: u64,
    ) -> crate::Result<Self> {
        let identity = digest::sha256_hex(
            &serde_json::to_vec(&(operation, idempotency_key.as_str()))
                .expect("adapter identity fields serialize"),
        );
        let short = &identity[..32];
        let session_key = CorrelationId::new(format!("mcp-{operation}-session-{short}"))
            .map_err(|error| crate::ServiceError::Contract(error.to_string()))?;
        let request_id = CorrelationId::new(format!("mcp-{operation}-request-{short}"))
            .map_err(|error| crate::ServiceError::Contract(error.to_string()))?;
        let operation_id = CorrelationId::new(format!("mcp-{operation}-operation-{short}"))
            .map_err(|error| crate::ServiceError::Contract(error.to_string()))?;
        let request = CreateSession {
            limits: SessionLimits {
                idle_timeout_ms: MAX_LEASE_MS,
                absolute_timeout_ms: MAX_LEASE_MS,
                max_open_transactions: 1,
            },
        };
        let (session_id, token) = match caller {
            AdapterCaller::Anonymous => {
                let lease = engine.create_session(
                    &request,
                    &session_key,
                    at,
                    request_id.as_str(),
                    operation_id.as_str(),
                )?;
                (lease.session_id, lease.token)
            }
            AdapterCaller::ApiKey {
                principal_id,
                credential,
            } if engine.security_enforced()? => {
                let lease = engine.create_authenticated_session(
                    principal_id,
                    credential,
                    &request,
                    &session_key,
                    at,
                    request_id.as_str(),
                    operation_id.as_str(),
                )?;
                (lease.session_id, lease.token)
            }
            AdapterCaller::ApiKey { .. } => {
                let lease = engine.create_session(
                    &request,
                    &session_key,
                    at,
                    request_id.as_str(),
                    operation_id.as_str(),
                )?;
                (lease.session_id, lease.token)
            }
            AdapterCaller::Session { session_id, token } => (session_id.clone(), token.clone()),
        };
        Ok(Self {
            session_id,
            token,
            request_id,
            operation_id,
        })
    }

    /// Opens a bounded reusable session for an adapter read without exposing
    /// idempotency or lease fields in the model-facing schema. At most one
    /// session identity is created per operation and maximum-lease window.
    pub(super) fn open_read(
        engine: &RrdEngine,
        caller: AdapterCaller<'_>,
        operation: &str,
        at: u64,
    ) -> crate::Result<Self> {
        let window = at / MAX_LEASE_MS;
        let key = CorrelationId::new(format!("read-window-{window}"))
            .map_err(|error| crate::ServiceError::Contract(error.to_string()))?;
        Self::open(engine, caller, operation, &key, at)
    }
}
