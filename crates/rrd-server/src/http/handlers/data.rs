use super::super::*;

impl AppState {
    pub(in crate::http) fn read_estate(
        &self,
        headers: &HeaderMap,
        body: &[u8],
        path: &str,
        now: u64,
    ) -> HttpResponse {
        let estate = estate_action(path, "read").expect("route checked");
        self.with_authenticated_envelope::<ReadEstate, _, _>(
            headers,
            body,
            now,
            RrdOperation::EstateRead,
            None,
            |envelope, session, token| {
                let estate = CanonicalId::new(estate).map_err(|error| {
                    ApiError::new(ErrorCode::InvalidArgument, error.to_string(), false)
                })?;
                let resource_estate = envelope
                    .resource
                    .segments
                    .iter()
                    .find(|segment| segment.kind == ResourceKind::Estate)
                    .map(|segment| &segment.id);
                if resource_estate != Some(&estate) {
                    return Err(ApiError::new(
                        ErrorCode::FailedPrecondition,
                        "request resource does not target the path estate",
                        false,
                    ));
                }
                self.service
                    .read_estate(
                        session,
                        token,
                        estate,
                        now,
                        envelope.context.request_id.as_str(),
                        envelope.context.operation_id.as_str(),
                    )
                    .map_err(api_error)?
                    .ok_or_else(|| {
                        ApiError::new(ErrorCode::NotFound, "estate authority not found", false)
                    })
            },
        )
    }

    pub(in crate::http) fn execute_query(
        &self,
        headers: &HeaderMap,
        body: &[u8],
        now: u64,
    ) -> HttpResponse {
        self.with_authenticated_envelope::<ExecuteQuery, _, _>(
            headers,
            body,
            now,
            RrdOperation::QueryExecute,
            None,
            |envelope, session, token| {
                self.service
                    .execute_query(
                        session,
                        token,
                        &envelope.payload,
                        now,
                        envelope.context.request_id.as_str(),
                        envelope.context.operation_id.as_str(),
                    )
                    .map_err(api_error)
            },
        )
    }

    pub(in crate::http) fn poll_live_query(
        &self,
        headers: &HeaderMap,
        body: &[u8],
        now: u64,
    ) -> HttpResponse {
        self.with_authenticated_envelope::<PollLiveQuery, _, _>(
            headers,
            body,
            now,
            RrdOperation::QueryLivePoll,
            None,
            |envelope, session, token| {
                if envelope.context.deadline_unix_ms.is_some_and(|deadline| {
                    now.checked_add(envelope.payload.wait_timeout_ms)
                        .is_none_or(|completion| completion > deadline)
                }) {
                    return Err(ApiError::new(
                        ErrorCode::DeadlineExceeded,
                        "live query wait exceeds the request deadline",
                        false,
                    ));
                }
                self.service
                    .poll_live_query(
                        session,
                        token,
                        &envelope.payload,
                        now,
                        envelope.context.request_id.as_str(),
                        envelope.context.operation_id.as_str(),
                    )
                    .map_err(api_error)
            },
        )
    }

    pub(in crate::http) fn ensure_query_index(
        &self,
        headers: &HeaderMap,
        body: &[u8],
        now: u64,
    ) -> HttpResponse {
        self.with_authenticated_envelope::<EnsureQueryIndex, _, _>(
            headers,
            body,
            now,
            RrdOperation::QueryIndexEnsure,
            None,
            |envelope, session, token| {
                let idempotency_key = envelope
                    .context
                    .idempotency_key
                    .as_ref()
                    .expect("mutating envelopes require idempotency");
                self.service
                    .ensure_query_index(
                        session,
                        token,
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

    pub(in crate::http) fn list_query_indexes(
        &self,
        headers: &HeaderMap,
        body: &[u8],
        now: u64,
    ) -> HttpResponse {
        self.with_authenticated_envelope::<ListQueryIndexes, _, _>(
            headers,
            body,
            now,
            RrdOperation::QueryIndexList,
            None,
            |envelope, session, token| {
                self.service
                    .list_query_indexes(
                        session,
                        token,
                        &envelope.payload,
                        now,
                        envelope.context.request_id.as_str(),
                        envelope.context.operation_id.as_str(),
                    )
                    .map_err(api_error)
            },
        )
    }
}
