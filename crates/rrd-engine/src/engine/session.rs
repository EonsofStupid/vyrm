use super::*;

impl RrdEngine {
    pub fn create_session(
        &self,
        request: &CreateSession,
        idempotency_key: &CorrelationId,
        now: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<SessionLease> {
        if self.security_enforced()? {
            return Err(ServiceError::Unauthenticated);
        }
        self.create_session_bound(
            request,
            idempotency_key,
            None,
            None,
            now,
            request_id,
            operation_id,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_authenticated_session(
        &self,
        principal_id: &CanonicalId,
        credential: &[u8],
        request: &CreateSession,
        idempotency_key: &CorrelationId,
        now: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<SessionLease> {
        let authorization =
            rrd_security::SecurityRepository::new(&self.storage, self.instance.clone())
                .authenticate_and_authorize(
                    principal_id,
                    credential,
                    SecurityAction::SessionCreate,
                    &self.instance_resource(),
                    now,
                )?;
        self.create_session_bound(
            request,
            idempotency_key,
            Some(principal_id.clone()),
            Some(authorization.credential_revision),
            now,
            request_id,
            operation_id,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_jwt_authenticated_session(
        &self,
        jwt: &str,
        signing_key: &[u8],
        request: &CreateSession,
        idempotency_key: &CorrelationId,
        now: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<SessionLease> {
        let repository =
            rrd_security::SecurityRepository::new(&self.storage, self.instance.clone());
        let principal_id = repository.authenticate_jwt(jwt, signing_key, now)?;
        let authorization = repository.authorize_principal(
            &principal_id,
            SecurityAction::SessionCreate,
            &self.instance_resource(),
            now,
        )?;
        self.create_session_bound(
            request,
            idempotency_key,
            Some(principal_id),
            Some(authorization.credential_revision),
            now,
            request_id,
            operation_id,
        )
    }

    /// Creates a session from a third-party identity only after the adapter
    /// has cryptographically verified its assertion. RRD resolves the exact
    /// persisted issuer/subject/audience binding and still applies the current
    /// SessionCreate policy.
    #[allow(clippy::too_many_arguments)]
    pub fn create_verified_identity_session(
        &self,
        issuer: &str,
        subject: &str,
        audience: &str,
        request: &CreateSession,
        idempotency_key: &CorrelationId,
        now: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<SessionLease> {
        let repository =
            rrd_security::SecurityRepository::new(&self.storage, self.instance.clone());
        let principal_id = repository.resolve_verified_identity(issuer, subject, audience, now)?;
        let authorization = repository.authorize_principal(
            &principal_id,
            SecurityAction::SessionCreate,
            &self.instance_resource(),
            now,
        )?;
        self.create_session_bound(
            request,
            idempotency_key,
            Some(principal_id),
            Some(authorization.credential_revision),
            now,
            request_id,
            operation_id,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn create_session_bound(
        &self,
        request: &CreateSession,
        idempotency_key: &CorrelationId,
        principal_id: Option<CanonicalId>,
        principal_credential_revision: Option<u64>,
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
        if let Some(bytes) = self.storage.control_record(&key)? {
            let state = decode_session(&bytes)?;
            if state.principal_id != principal_id
                || state.principal_credential_revision != principal_credential_revision
            {
                return Err(ServiceError::IdempotencyConflict);
            }
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
            principal_id,
            principal_credential_revision,
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
        self.storage.commit_control_transition(&ControlTransition {
            key: session_key(&self.instance, &session_id),
            expected: None,
            replacement: Some(serde_json::to_vec(&state).map_err(contract_json)?),
            at: now,
            actor: "rrd-engine".into(),
            action: "session.created".into(),
            request_id: request_id.into(),
            operation_id: operation_id.into(),
        })?;
        Ok(lease)
    }

    pub(in crate::engine) fn invocation_session_principal(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        operation: RrdOperation,
        context: &RequestContext,
        now: u64,
    ) -> Result<(Option<CanonicalId>, Option<u64>)> {
        let key = session_key(&self.instance, session_id);
        let bytes = self
            .storage
            .control_record(&key)?
            .ok_or(ServiceError::SessionNotFound)?;
        let mut state = decode_session(&bytes)?;
        let presented_sha256 = digest::sha256_hex(token.as_str().as_bytes());
        let idempotency_key = context.idempotency_key.as_ref();
        let authenticated_replay = match operation {
            RrdOperation::SessionRenew => idempotency_key
                .and_then(|key| state.renewals.get(key))
                .is_some_and(|renewal| renewal.previous_token_sha256 == presented_sha256),
            RrdOperation::SessionClose => state.closure.as_ref().is_some_and(|closure| {
                Some(&closure.idempotency_key) == idempotency_key
                    && closure.previous_token_sha256 == presented_sha256
            }),
            _ => false,
        };
        if authenticated_replay {
            return Ok((state.principal_id, state.principal_credential_revision));
        }
        if state.token_sha256 != presented_sha256 {
            return Err(ServiceError::Unauthenticated);
        }
        self.require_active_or_expire(
            session_id,
            bytes,
            &mut state,
            now,
            context.request_id.as_str(),
            context.operation_id.as_str(),
        )?;
        Ok((state.principal_id, state.principal_credential_revision))
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
            .storage
            .control_record(&key)?
            .ok_or(ServiceError::SessionNotFound)?;
        let mut state = decode_session(&bytes)?;
        self.authorize_session_policy(&state, SecurityAction::SessionRenew, now)?;
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
            .storage
            .control_record(&key)?
            .ok_or(ServiceError::SessionNotFound)?;
        let mut state = decode_session(&bytes)?;
        self.authorize_session_policy(&state, SecurityAction::SessionClose, now)?;
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
    pub(in crate::engine) fn authorize(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        action: SecurityAction,
        now: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<(Vec<u8>, SessionState)> {
        let (bytes, state, _) = self.authorize_resource(
            session_id,
            token,
            action,
            &self.instance_resource(),
            now,
            request_id,
            operation_id,
        )?;
        Ok((bytes, state))
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::engine) fn authorize_resource(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        action: SecurityAction,
        resource: &ResourcePath,
        now: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<(Vec<u8>, SessionState, Option<rrd_security::Authorization>)> {
        let mut principal_id = None;
        let result = (|| {
            let (bytes, mut state) = self.load_authenticated(session_id, token)?;
            principal_id.clone_from(&state.principal_id);
            self.require_active_or_expire(
                session_id,
                bytes.clone(),
                &mut state,
                now,
                request_id,
                operation_id,
            )?;
            let authorization =
                self.compile_session_authorization(&state, action, resource, now)?;
            Ok((bytes, state, authorization))
        })();
        self.record_direct_authorization(
            principal_id,
            action,
            resource,
            now,
            request_id,
            operation_id,
            result.as_ref().err(),
        )?;
        result
    }

    pub(in crate::engine) fn load_authenticated(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
    ) -> Result<(Vec<u8>, SessionState)> {
        let bytes = self
            .storage
            .control_record(&session_key(&self.instance, session_id))?
            .ok_or(ServiceError::SessionNotFound)?;
        let state = decode_session(&bytes)?;
        if state.token_sha256 != digest::sha256_hex(token.as_str().as_bytes()) {
            return Err(ServiceError::Unauthenticated);
        }
        Ok((bytes, state))
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::engine) fn require_active_or_expire(
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

    pub(in crate::engine) fn replay_created_session(
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

    pub(in crate::engine) fn session_lease(
        &self,
        state: &SessionState,
        token_generation: u64,
        idle_expires_at_unix_ms: u64,
    ) -> Result<SessionLease> {
        let token = self.session_token(&state.session_id, token_generation)?;
        Ok(lease_from_state(state, token, idle_expires_at_unix_ms))
    }

    pub(in crate::engine) fn session_token(
        &self,
        session_id: &CorrelationId,
        token_generation: u64,
    ) -> Result<CorrelationId> {
        self.keyed_id(
            "token",
            &[session_id.as_str(), &token_generation.to_string()],
        )
    }

    pub(in crate::engine) fn keyed_id(
        &self,
        prefix: &str,
        parts: &[&str],
    ) -> Result<CorrelationId> {
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
    pub(in crate::engine) fn replace_session(
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
        self.storage.commit_control_transition(&ControlTransition {
            key: session_key(&self.instance, session_id),
            expected: Some(expected),
            replacement: Some(replacement.clone()),
            at: now,
            actor: "rrd-engine".into(),
            action: action.into(),
            request_id: request_id.into(),
            operation_id: operation_id.into(),
        })?;
        Ok(replacement)
    }
}

pub(in crate::engine) fn touch_session(state: &mut SessionState, now: u64) {
    state.idle_expires_at_unix_ms = now
        .saturating_add(state.limits.idle_timeout_ms)
        .min(state.absolute_expires_at_unix_ms);
}

pub(in crate::engine) fn decode_session(bytes: &[u8]) -> Result<SessionState> {
    let state: SessionState = serde_json::from_slice(bytes).map_err(contract_json)?;
    if state.format_version != SESSION_STATE_FORMAT {
        return Err(ServiceError::Contract(format!(
            "unsupported session-state format {}; expected {SESSION_STATE_FORMAT}",
            state.format_version
        )));
    }
    Ok(state)
}

pub(in crate::engine) fn lease_from_state(
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

pub(in crate::engine) fn termination(
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

pub(in crate::engine) fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[usize::from(byte >> 4)] as char);
        output.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    output
}

pub(in crate::engine) fn session_key(instance: &CanonicalId, session: &CorrelationId) -> String {
    format!("server/state/{instance}/session/{}", session.as_str())
}
