use super::super::*;

impl AppState {
    pub(in crate::http) fn create_session(
        &self,
        headers: &HeaderMap,
        body: &[u8],
        now: u64,
    ) -> HttpResponse {
        self.with_envelope::<CreateSession, _, _>(
            headers,
            body,
            now,
            RrdOperation::SessionCreate,
            |envelope, identity| {
                let idempotency_key = required_idempotency(&envelope.context)?;
                match identity {
                    Some((principal_id, credential)) => self.service.create_authenticated_session(
                        &principal_id,
                        credential.as_bytes(),
                        &envelope.payload,
                        idempotency_key,
                        now,
                        envelope.context.request_id.as_str(),
                        envelope.context.operation_id.as_str(),
                    ),
                    None => self.service.create_session(
                        &envelope.payload,
                        idempotency_key,
                        now,
                        envelope.context.request_id.as_str(),
                        envelope.context.operation_id.as_str(),
                    ),
                }
                .map_err(api_error)
            },
        )
    }

    pub(in crate::http) fn renew_session(
        &self,
        headers: &HeaderMap,
        body: &[u8],
        path: &str,
        now: u64,
    ) -> HttpResponse {
        let path_id = session_action(path, "renew").expect("route checked");
        self.with_authenticated_envelope::<RenewSession, _, _>(
            headers,
            body,
            now,
            RrdOperation::SessionRenew,
            Some(path_id),
            |envelope, session, token| {
                let idempotency_key = required_idempotency(&envelope.context)?;
                self.service
                    .renew_session(
                        session,
                        token,
                        &envelope.payload,
                        idempotency_key,
                        now,
                        envelope.context.request_id.as_str(),
                        envelope.context.operation_id.as_str(),
                    )
                    .map_err(api_error)
            },
        )
    }

    pub(in crate::http) fn close_session(
        &self,
        headers: &HeaderMap,
        body: &[u8],
        path: &str,
        now: u64,
    ) -> HttpResponse {
        let path_id = session_id(path).expect("route checked");
        self.with_authenticated_envelope::<CloseSession, _, _>(
            headers,
            body,
            now,
            RrdOperation::SessionClose,
            Some(path_id),
            |envelope, session, token| {
                let idempotency_key = required_idempotency(&envelope.context)?;
                self.service
                    .close_session(
                        session,
                        token,
                        &envelope.payload,
                        idempotency_key,
                        now,
                        envelope.context.request_id.as_str(),
                        envelope.context.operation_id.as_str(),
                    )
                    .map_err(api_error)
            },
        )
    }

    pub(in crate::http) fn begin_transaction(
        &self,
        headers: &HeaderMap,
        body: &[u8],
        now: u64,
    ) -> HttpResponse {
        self.with_authenticated_envelope::<BeginTransaction, _, _>(
            headers,
            body,
            now,
            RrdOperation::TransactionBegin,
            None,
            |envelope, session, token| {
                self.service
                    .begin_transaction(session, token, &envelope.payload, &envelope.context, now)
                    .map_err(api_error)
            },
        )
    }

    pub(in crate::http) fn preview_transaction(
        &self,
        headers: &HeaderMap,
        body: &[u8],
        path: &str,
        now: u64,
    ) -> HttpResponse {
        let transaction = transaction_action(path, "preview").expect("route checked");
        self.with_authenticated_envelope::<PreviewTransaction, _, _>(
            headers,
            body,
            now,
            RrdOperation::TransactionPreview,
            None,
            |envelope, session, token| {
                let transaction = parse_correlation(transaction)?;
                self.service
                    .preview_transaction(
                        session,
                        token,
                        &transaction,
                        &envelope.payload,
                        now,
                        envelope.context.request_id.as_str(),
                        envelope.context.operation_id.as_str(),
                    )
                    .map_err(api_error)
            },
        )
    }

    pub(in crate::http) fn commit_transaction(
        &self,
        headers: &HeaderMap,
        body: &[u8],
        path: &str,
        now: u64,
    ) -> HttpResponse {
        let transaction = transaction_action(path, "commit").expect("route checked");
        self.with_authenticated_envelope::<CommitTransaction, _, _>(
            headers,
            body,
            now,
            RrdOperation::TransactionCommit,
            None,
            |envelope, session, token| {
                if envelope.context.deadline_unix_ms.is_none() {
                    return Err(ApiError::new(
                        ErrorCode::InvalidArgument,
                        "commit requires deadline_unix_ms",
                        false,
                    ));
                }
                let transaction = parse_correlation(transaction)?;
                let idempotency_key = required_idempotency(&envelope.context)?;
                self.service
                    .commit_transaction(
                        session,
                        token,
                        &transaction,
                        idempotency_key,
                        &envelope.payload,
                        now,
                        envelope.context.request_id.as_str(),
                        envelope.context.operation_id.as_str(),
                    )
                    .map_err(api_error)
            },
        )
    }

    pub(in crate::http) fn abort_transaction(
        &self,
        headers: &HeaderMap,
        body: &[u8],
        path: &str,
        now: u64,
    ) -> HttpResponse {
        let transaction = transaction_id(path).expect("route checked");
        self.with_authenticated_envelope::<AbortTransaction, _, _>(
            headers,
            body,
            now,
            RrdOperation::TransactionAbort,
            None,
            |envelope, session, token| {
                let transaction = parse_correlation(transaction)?;
                self.service
                    .abort_transaction(
                        session,
                        token,
                        &transaction,
                        &envelope.payload,
                        &envelope.context,
                        now,
                    )
                    .map_err(api_error)
            },
        )
    }
}
