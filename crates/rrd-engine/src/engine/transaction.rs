use super::*;

impl RrdEngine {
    pub fn begin_transaction(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        request: &BeginTransaction,
        context: &RequestContext,
        now: u64,
    ) -> Result<TransactionLease> {
        context
            .validate(true)
            .map_err(|error| ServiceError::Contract(error.to_string()))?;
        let idempotency_key = context.idempotency_key.as_ref().ok_or_else(|| {
            ServiceError::Contract("transaction begin requires idempotency".into())
        })?;
        request
            .validate()
            .map_err(|error| ServiceError::Contract(error.to_string()))?;
        if !matches!(request.scope.as_str(), "claims" | "data") {
            return Err(ServiceError::WrongScope);
        }
        let (bytes, mut state) = self.load_authenticated(session_id, token)?;
        self.authorize_session_policy(&state, SecurityAction::TransactionBegin, now)?;
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
        // Exact idempotent replays above remain observable after the session
        // lease expires. Only creation of a new transaction requires a live
        // session. This ordering is required for crash/reopen recovery: a
        // client can recover the stable transaction identity and replay the
        // commit without creating a second transaction.
        self.require_active_or_expire(
            session_id,
            bytes.clone(),
            &mut state,
            now,
            context.request_id.as_str(),
            context.operation_id.as_str(),
        )?;
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
        let scope = ScopeId::new(format!("instance:{}", self.instance))
            .map_err(|error| ServiceError::Contract(error.to_string()))?;
        let read = self.storage.runtime_read_stamp(&scope)?;
        let transaction = TransactionLease {
            transaction_id: transaction_id.clone(),
            session_id: session_id.clone(),
            scope: request.scope.clone(),
            read_cursor: read.commit_cursor,
            expires_at_unix_ms: expires_at,
            state: TransactionState::Open,
        };
        state.transactions.insert(
            transaction_id,
            TransactionRecord {
                lease: transaction.clone(),
                read,
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
            context.request_id.as_str(),
            context.operation_id.as_str(),
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
        self.authorize_session_policy(&state, SecurityAction::TransactionCommit, now)?;
        self.validate_collection_vector_mutations(request)?;
        let record = state
            .transactions
            .get(transaction_id)
            .ok_or(ServiceError::TransactionNotFound)?;
        let transaction_scope = record.lease.scope.clone();
        let read = record.read.clone();
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
            let runtime_identity = if matches!(transaction_scope.as_str(), "claims" | "data") {
                let commit = public_runtime_commit(
                    request,
                    session_id,
                    &self.instance,
                    read.commit_cursor,
                    now,
                )?;
                (now, commit.digest())
            } else {
                return Err(ServiceError::WrongScope);
            };
            record.commit_intent = Some(CommitIntent {
                idempotency_key: idempotency_key.clone(),
                operation_sha256: request.operation_sha256.clone(),
                runtime_at_unix_ms: Some(runtime_identity.0),
                runtime_commit_sha256: Some(runtime_identity.1),
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
        let accepted_receipt = if matches!(transaction_scope.as_str(), "claims" | "data") {
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
                read.commit_cursor,
                runtime_at,
            )?;
            if commit.digest() != *expected_commit {
                return Err(ServiceError::OperationDigestMismatch);
            }
            let (outcome, idempotent_replay) =
                if let Some(outcome) = self.storage.runtime_commit_outcome(expected_commit)? {
                    (outcome, true)
                } else {
                    let transaction = DataTransaction::new(read.clone(), commit.clone())
                        .map_err(|error| ServiceError::Contract(error.to_string()))?;
                    match self.storage.commit_data_transaction(&transaction) {
                        Ok(outcome) => (outcome, false),
                        Err(error) => match self.storage.runtime_commit_outcome(expected_commit)? {
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

    pub fn abort_transaction(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        transaction_id: &CorrelationId,
        request: &AbortTransaction,
        context: &RequestContext,
        now: u64,
    ) -> Result<TransactionLease> {
        context
            .validate(true)
            .map_err(|error| ServiceError::Contract(error.to_string()))?;
        let idempotency_key = context.idempotency_key.as_ref().ok_or_else(|| {
            ServiceError::Contract("transaction abort requires idempotency".into())
        })?;
        let (bytes, mut state) = self.authorize(
            session_id,
            token,
            SecurityAction::TransactionAbort,
            now,
            context.request_id.as_str(),
            context.operation_id.as_str(),
        )?;
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
            context.request_id.as_str(),
            context.operation_id.as_str(),
        )?;
        if transaction.state == TransactionState::Expired {
            return Err(ServiceError::TransactionExpired);
        }
        Ok(transaction)
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
        let (bytes, mut state) = self.authorize(
            session_id,
            token,
            SecurityAction::TransactionPreview,
            now,
            request_id,
            operation_id,
        )?;
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
