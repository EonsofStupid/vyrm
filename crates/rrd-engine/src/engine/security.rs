use super::*;

pub const MAX_AUDIT_PAGE_RECORDS: u64 = rrd_security::MAX_AUDIT_PAGE as u64;

pub(in crate::engine) struct AuditEvent {
    pub at_unix_ms: u64,
    pub attempt: u64,
    pub principal_id: Option<CanonicalId>,
    pub action: SecurityAction,
    pub resource: ResourcePath,
    pub request_id: String,
    pub operation_id: String,
    pub phase: AuditPhase,
    pub decision: AuditDecision,
    pub status_code: u16,
    pub request_sha256: String,
    pub response_sha256: String,
}

impl RrdEngine {
    pub(in crate::engine) fn instance_resource(&self) -> ResourcePath {
        ResourcePath {
            segments: vec![ResourceId::new(
                ResourceKind::Instance,
                self.instance.as_str().to_owned(),
            )
            .expect("engine instance identity is canonical")],
        }
    }

    pub(in crate::engine) fn authorize_session_policy(
        &self,
        state: &SessionState,
        action: SecurityAction,
        now: u64,
    ) -> Result<()> {
        let repository =
            rrd_security::SecurityRepository::new(&self.storage, self.instance.clone());
        if !repository.is_initialized()? {
            return Ok(());
        }
        let principal = state
            .principal_id
            .as_ref()
            .ok_or(ServiceError::Unauthenticated)?;
        repository.authorize_principal(principal, action, &self.instance_resource(), now)?;
        Ok(())
    }

    pub fn security_enforced(&self) -> Result<bool> {
        rrd_security::SecurityRepository::new(&self.storage, self.instance.clone())
            .is_initialized()
            .map_err(Into::into)
    }

    pub(in crate::engine) fn authenticate_principal(
        &self,
        principal_id: &CanonicalId,
        credential: &[u8],
    ) -> Result<()> {
        rrd_security::SecurityRepository::new(&self.storage, self.instance.clone())
            .authenticate_principal(principal_id, credential)?;
        Ok(())
    }

    pub(in crate::engine) fn authorize_principal(
        &self,
        principal_id: &CanonicalId,
        action: SecurityAction,
        resource: &ResourcePath,
        now: u64,
    ) -> Result<()> {
        rrd_security::SecurityRepository::new(&self.storage, self.instance.clone())
            .authorize_principal(principal_id, action, resource, now)?;
        Ok(())
    }

    pub(in crate::engine) fn append_audit(&self, event: AuditEvent) -> Result<()> {
        let identity = digest::sha256_hex(
            serde_json::to_vec(&(
                event.request_id.as_str(),
                event.operation_id.as_str(),
                event.action,
                event.phase,
                event.at_unix_ms,
                event.attempt,
                event.request_sha256.as_str(),
                event.response_sha256.as_str(),
            ))
            .map_err(contract_json)?
            .as_slice(),
        );
        rrd_security::SecurityRepository::new(&self.storage, self.instance.clone()).append_audit(
            &rrd_security::AuditRecord {
                audit_id: CanonicalId::new(format!("audit-{identity}"))
                    .map_err(|error| ServiceError::Contract(error.to_string()))?,
                at_unix_ms: event.at_unix_ms,
                principal_id: event.principal_id,
                action: event.action,
                resource: event.resource,
                request_id: event.request_id,
                operation_id: event.operation_id,
                phase: event.phase,
                decision: event.decision,
                status_code: event.status_code,
                request_sha256: event.request_sha256,
                response_sha256: event.response_sha256,
            },
        )?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn read_audit(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        request: &ReadAudit,
        now: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<AuditPage> {
        request
            .validate()
            .map_err(|error| ServiceError::Contract(error.to_string()))?;
        self.authorize(
            session_id,
            token,
            SecurityAction::AuditRead,
            now,
            request_id,
            operation_id,
        )?;
        let page = rrd_security::SecurityRepository::new(&self.storage, self.instance.clone())
            .audit_since(request.after_sequence, usize::from(request.limit))
            .map_err(|error| ServiceError::Contract(error.to_string()))?;
        Ok(AuditPage {
            requested_after_sequence: request.after_sequence,
            through_sequence: page.through_sequence,
            records: page
                .records
                .into_iter()
                .map(|(sequence, record)| AuditRecordSnapshot {
                    sequence,
                    audit_id: record.audit_id,
                    at_unix_ms: record.at_unix_ms,
                    principal_id: record.principal_id,
                    action: record.action,
                    resource: record.resource,
                    request_id: record.request_id,
                    operation_id: record.operation_id,
                    phase: record.phase,
                    decision: record.decision,
                    status_code: record.status_code,
                    request_sha256: record.request_sha256,
                    response_sha256: record.response_sha256,
                })
                .collect(),
        })
    }
}
