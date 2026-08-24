use super::*;

impl AppState {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn audit_response(
        &self,
        action: SecurityAction,
        resource: &rrd_contract::ResourcePath,
        context: &RequestContext,
        principal_id: Option<CanonicalId>,
        request_body: &[u8],
        response: &HttpResponse,
        now: u64,
        attempt: u64,
    ) -> std::result::Result<(), ServiceError> {
        let status_code = response.status().as_u16();
        let decision = match status_code {
            200..=299 => AuditDecision::Allowed,
            401 | 403 => AuditDecision::Denied,
            _ => AuditDecision::Failed,
        };
        let response_sha256 = response
            .extensions()
            .get::<ResponseDigest>()
            .map(|digest| digest.0.clone())
            .unwrap_or_else(|| sha256_hex(status_code.to_string().as_bytes()));
        self.append_audit_record(
            action,
            resource,
            context,
            principal_id,
            request_body,
            now,
            attempt,
            AuditPhase::Completed,
            decision,
            status_code,
            response_sha256,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn audit_authorized(
        &self,
        action: SecurityAction,
        resource: &rrd_contract::ResourcePath,
        context: &RequestContext,
        principal_id: Option<CanonicalId>,
        request_body: &[u8],
        now: u64,
        attempt: u64,
    ) -> std::result::Result<(), ServiceError> {
        self.append_audit_record(
            action,
            resource,
            context,
            principal_id,
            request_body,
            now,
            attempt,
            AuditPhase::Authorized,
            AuditDecision::Allowed,
            100,
            sha256_hex(b"rrd-audit-completion-pending"),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn append_audit_record(
        &self,
        action: SecurityAction,
        resource: &rrd_contract::ResourcePath,
        context: &RequestContext,
        principal_id: Option<CanonicalId>,
        request_body: &[u8],
        now: u64,
        attempt: u64,
        phase: AuditPhase,
        decision: AuditDecision,
        status_code: u16,
        response_sha256: String,
    ) -> std::result::Result<(), ServiceError> {
        self.service.append_audit(AuditEvent {
            at_unix_ms: now,
            attempt,
            principal_id,
            action,
            resource: resource.clone(),
            request_id: context.request_id.as_str().into(),
            operation_id: context.operation_id.as_str().into(),
            phase,
            decision,
            status_code,
            request_sha256: sha256_hex(request_body),
            response_sha256,
        })
    }
}
