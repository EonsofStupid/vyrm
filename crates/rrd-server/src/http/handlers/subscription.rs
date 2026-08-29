use super::super::*;

impl AppState {
    pub(in crate::http) fn open_subscription(
        &self,
        headers: &HeaderMap,
        body: &[u8],
        now: u64,
    ) -> HttpResponse {
        self.with_authenticated_envelope::<OpenSubscription, _, _>(
            headers,
            body,
            now,
            RrdOperation::SubscriptionOpen,
            None,
            |envelope, session, token| {
                self.service
                    .open_subscription(
                        session,
                        token,
                        required_idempotency(&envelope.context)?,
                        &envelope.payload,
                        now,
                        envelope.context.request_id.as_str(),
                        envelope.context.operation_id.as_str(),
                    )
                    .map_err(api_error)
            },
        )
    }

    pub(in crate::http) fn close_subscription(
        &self,
        headers: &HeaderMap,
        body: &[u8],
        now: u64,
    ) -> HttpResponse {
        self.with_authenticated_envelope::<CloseSubscription, _, _>(
            headers,
            body,
            now,
            RrdOperation::SubscriptionClose,
            None,
            |envelope, session, token| {
                self.service
                    .close_subscription(
                        session,
                        token,
                        required_idempotency(&envelope.context)?,
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
