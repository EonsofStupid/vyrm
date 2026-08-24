use super::super::*;

impl AppState {
    pub(in crate::http) fn create_instance_backup(
        &self,
        headers: &HeaderMap,
        body: &[u8],
        now: u64,
    ) -> HttpResponse {
        self.with_authenticated_envelope::<CreateInstanceBackup, _, _>(
            headers,
            body,
            now,
            true,
            SecurityAction::BackupCreate,
            None,
            |envelope, session, token| {
                self.service
                    .create_instance_backup(
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

    pub(in crate::http) fn list_instance_backups(
        &self,
        headers: &HeaderMap,
        body: &[u8],
        now: u64,
    ) -> HttpResponse {
        self.with_authenticated_envelope::<ListInstanceBackups, _, _>(
            headers,
            body,
            now,
            false,
            SecurityAction::BackupList,
            None,
            |envelope, session, token| {
                self.service
                    .list_instance_backups(
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

    pub(in crate::http) fn restore_instance_backup(
        &self,
        headers: &HeaderMap,
        body: &[u8],
        now: u64,
    ) -> HttpResponse {
        self.with_authenticated_envelope::<RestoreInstanceBackup, _, _>(
            headers,
            body,
            now,
            true,
            SecurityAction::RestoreCreate,
            None,
            |envelope, session, token| {
                self.service
                    .restore_instance_backup(
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
