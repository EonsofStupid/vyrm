use super::super::*;

impl AppState {
    pub(in crate::http) fn read_audit(
        &self,
        headers: &HeaderMap,
        body: &[u8],
        now: u64,
    ) -> HttpResponse {
        self.with_authenticated_envelope::<ReadAudit, _, _>(
            headers,
            body,
            now,
            RrdOperation::AuditRead,
            None,
            |envelope, session, token| {
                self.service
                    .read_audit(
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
