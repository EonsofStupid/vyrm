use super::super::*;

impl AppState {
    pub(in crate::http) fn read_changefeed(
        &self,
        headers: &HeaderMap,
        body: &[u8],
        now: u64,
    ) -> HttpResponse {
        self.with_authenticated_envelope::<ReadChangefeed, _, _>(
            headers,
            body,
            now,
            RrdOperation::ChangefeedRead,
            None,
            |envelope, session, token| {
                self.service
                    .read_changefeed(
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

    pub(in crate::http) fn follow_changefeed(
        &self,
        headers: &HeaderMap,
        body: &[u8],
        now: u64,
    ) -> HttpResponse {
        self.with_authenticated_envelope::<FollowChangefeed, _, _>(
            headers,
            body,
            now,
            RrdOperation::ChangefeedFollow,
            None,
            |envelope, session, token| {
                if envelope.context.deadline_unix_ms.is_some_and(|deadline| {
                    now.checked_add(envelope.payload.wait_timeout_ms)
                        .is_none_or(|completion| completion > deadline)
                }) {
                    return Err(ApiError::new(
                        ErrorCode::DeadlineExceeded,
                        "changefeed follow wait exceeds the request deadline",
                        false,
                    ));
                }
                self.service
                    .follow_changefeed(
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
