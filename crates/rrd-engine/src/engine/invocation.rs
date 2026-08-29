use super::security::AuditEvent;
use super::*;

/// Canonical operation identity presented by every embedded or transported
/// RRD call. Adapters select an operation; only the engine maps it to policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RrdOperation {
    ServiceInspect,
    UnknownRequest,
    SessionCreate,
    SessionRenew,
    SessionClose,
    TransactionBegin,
    TransactionPreview,
    TransactionCommit,
    TransactionAbort,
    EstateRead,
    QueryExecute,
    QueryLivePoll,
    QueryIndexEnsure,
    QueryIndexList,
    BackupCreate,
    BackupList,
    RestoreCreate,
    VectorSearch,
    VectorCollectionEnsure,
    VectorCollectionList,
    VectorPointScroll,
    VectorPointRetrieve,
    ChangefeedRead,
    ChangefeedFollow,
    SubscriptionOpen,
    SubscriptionConnect,
    SubscriptionAck,
    SubscriptionClose,
    AuditRead,
    DiagnosticsRead,
    MemoryContextRead,
    MemoryInspect,
    MemoryRecall,
    MemoryRetire,
    MemoryWrite,
    LifecycleApply,
    ProjectAttune,
    ProjectRoute,
    ReasoningRead,
    ReasoningWrite,
    WorkPlanRead,
    WorkPlanControl,
    WorkPlanVerifyExecute,
    RuntimeToolCatalogueRead,
}

impl RrdOperation {
    pub fn mutates(self) -> bool {
        matches!(
            self,
            Self::SessionCreate
                | Self::SessionRenew
                | Self::SessionClose
                | Self::TransactionBegin
                | Self::TransactionPreview
                | Self::TransactionCommit
                | Self::TransactionAbort
                | Self::SubscriptionOpen
                | Self::SubscriptionConnect
                | Self::SubscriptionAck
                | Self::SubscriptionClose
                | Self::QueryIndexEnsure
                | Self::BackupCreate
                | Self::RestoreCreate
                | Self::VectorCollectionEnsure
                | Self::MemoryRetire
                | Self::MemoryWrite
                | Self::LifecycleApply
                | Self::ProjectAttune
                | Self::ProjectRoute
                | Self::ReasoningWrite
                | Self::WorkPlanControl
                | Self::WorkPlanVerifyExecute
        )
    }

    pub(crate) fn action(self) -> SecurityAction {
        match self {
            Self::ServiceInspect => SecurityAction::ServiceInspect,
            Self::UnknownRequest => SecurityAction::UnknownRequest,
            Self::SessionCreate => SecurityAction::SessionCreate,
            Self::SessionRenew => SecurityAction::SessionRenew,
            Self::SessionClose => SecurityAction::SessionClose,
            Self::TransactionBegin => SecurityAction::TransactionBegin,
            Self::TransactionPreview => SecurityAction::TransactionPreview,
            Self::TransactionCommit => SecurityAction::TransactionCommit,
            Self::TransactionAbort => SecurityAction::TransactionAbort,
            Self::EstateRead => SecurityAction::EstateRead,
            Self::QueryExecute => SecurityAction::QueryExecute,
            Self::QueryLivePoll => SecurityAction::QueryLivePoll,
            Self::QueryIndexEnsure => SecurityAction::QueryIndexEnsure,
            Self::QueryIndexList => SecurityAction::QueryIndexList,
            Self::BackupCreate => SecurityAction::BackupCreate,
            Self::BackupList => SecurityAction::BackupList,
            Self::RestoreCreate => SecurityAction::RestoreCreate,
            Self::VectorSearch => SecurityAction::VectorSearch,
            Self::VectorCollectionEnsure => SecurityAction::VectorCollectionEnsure,
            Self::VectorCollectionList => SecurityAction::VectorCollectionList,
            Self::VectorPointScroll => SecurityAction::VectorPointScroll,
            Self::VectorPointRetrieve => SecurityAction::VectorPointRetrieve,
            Self::ChangefeedRead => SecurityAction::ChangefeedRead,
            Self::ChangefeedFollow => SecurityAction::ChangefeedFollow,
            Self::SubscriptionOpen => SecurityAction::SubscriptionOpen,
            Self::SubscriptionConnect => SecurityAction::SubscriptionConnect,
            Self::SubscriptionAck => SecurityAction::SubscriptionAck,
            Self::SubscriptionClose => SecurityAction::SubscriptionClose,
            Self::AuditRead => SecurityAction::AuditRead,
            Self::DiagnosticsRead => SecurityAction::DiagnosticsRead,
            Self::MemoryContextRead => SecurityAction::MemoryContextRead,
            Self::MemoryInspect => SecurityAction::MemoryInspect,
            Self::MemoryRecall => SecurityAction::MemoryRecall,
            Self::MemoryRetire => SecurityAction::MemoryRetire,
            Self::MemoryWrite => SecurityAction::MemoryWrite,
            Self::LifecycleApply => SecurityAction::LifecycleApply,
            Self::ProjectAttune => SecurityAction::ProjectAttune,
            Self::ProjectRoute => SecurityAction::ProjectRoute,
            Self::ReasoningRead => SecurityAction::ReasoningRead,
            Self::ReasoningWrite => SecurityAction::ReasoningWrite,
            Self::WorkPlanRead => SecurityAction::WorkPlanRead,
            Self::WorkPlanControl => SecurityAction::WorkPlanControl,
            Self::WorkPlanVerifyExecute => SecurityAction::WorkPlanVerifyExecute,
            Self::RuntimeToolCatalogueRead => SecurityAction::RuntimeToolCatalogueRead,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    pub context: RequestContext,
    pub resource: ResourcePath,
    pub observed_at_unix_ms: u64,
    pub attempt: u64,
    pub request_sha256: String,
}

#[derive(Clone, Copy)]
pub enum InvocationCredential<'a> {
    Anonymous,
    ApiKey {
        principal_id: &'a CanonicalId,
        credential: &'a [u8],
    },
    Jwt {
        token: &'a str,
        signing_key: &'a [u8],
    },
    Session {
        session_id: &'a CorrelationId,
        token: &'a CorrelationId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizedInvocation {
    invocation: Invocation,
    operation: RrdOperation,
    principal_id: Option<CanonicalId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvocationCompletion {
    pub decision: AuditDecision,
    pub status_code: u16,
    pub response_sha256: String,
}

impl RrdEngine {
    pub fn begin_invocation(
        &self,
        invocation: Invocation,
        operation: RrdOperation,
        credential: InvocationCredential<'_>,
    ) -> Result<AuthorizedInvocation> {
        self.validate_invocation(&invocation, operation)?;
        let repository =
            rrd_security::SecurityRepository::new(&self.storage, self.instance.clone());
        let security_enforced = repository.is_initialized()?;
        let mut authenticated_principal = None;
        let authorization = match credential {
            InvocationCredential::Anonymous if security_enforced => {
                Err(ServiceError::Unauthenticated)
            }
            InvocationCredential::Anonymous => Ok(None),
            InvocationCredential::ApiKey {
                principal_id,
                credential,
            } if security_enforced => (|| {
                self.authenticate_principal(principal_id, credential)?;
                authenticated_principal = Some(principal_id.clone());
                self.authorize_principal(
                    principal_id,
                    operation.action(),
                    &invocation.resource,
                    invocation.observed_at_unix_ms,
                )?;
                Ok(Some(principal_id.clone()))
            })(),
            InvocationCredential::ApiKey { .. } => Ok(None),
            InvocationCredential::Jwt { token, signing_key } if security_enforced => (|| {
                let principal_id = repository.authenticate_jwt(
                    token,
                    signing_key,
                    invocation.observed_at_unix_ms,
                )?;
                authenticated_principal = Some(principal_id.clone());
                self.authorize_principal(
                    &principal_id,
                    operation.action(),
                    &invocation.resource,
                    invocation.observed_at_unix_ms,
                )?;
                Ok(Some(principal_id))
            })(
            ),
            InvocationCredential::Jwt { .. } => Err(ServiceError::Unauthenticated),
            InvocationCredential::Session { session_id, token } => (|| {
                let (principal, credential_revision) = self.invocation_session_principal(
                    session_id,
                    token,
                    operation,
                    &invocation.context,
                    invocation.observed_at_unix_ms,
                )?;
                authenticated_principal = principal.clone();
                if security_enforced {
                    let principal_id = principal.as_ref().ok_or(ServiceError::Unauthenticated)?;
                    let authorization = self.authorize_principal(
                        principal_id,
                        operation.action(),
                        &invocation.resource,
                        invocation.observed_at_unix_ms,
                    )?;
                    if credential_revision != Some(authorization.credential_revision) {
                        return Err(ServiceError::Unauthenticated);
                    }
                }
                Ok(principal)
            })(),
        };

        let principal_id = match authorization {
            Ok(principal_id) => principal_id,
            Err(error) => {
                let (decision, status_code) = audit_failure(&error);
                self.append_invocation_audit(
                    &invocation,
                    operation,
                    authenticated_principal,
                    AuditPhase::Completed,
                    decision,
                    status_code,
                    digest::sha256_hex(error.to_string().as_bytes()),
                )?;
                return Err(error);
            }
        };

        self.append_invocation_audit(
            &invocation,
            operation,
            principal_id.clone(),
            AuditPhase::Authorized,
            AuditDecision::Allowed,
            100,
            digest::sha256_hex(b"rrd-audit-completion-pending"),
        )?;
        Ok(AuthorizedInvocation {
            invocation,
            operation,
            principal_id,
        })
    }

    pub fn complete_invocation(
        &self,
        authorized: &AuthorizedInvocation,
        completion: InvocationCompletion,
    ) -> Result<()> {
        validate_sha256(&completion.response_sha256)?;
        self.append_invocation_audit(
            &authorized.invocation,
            authorized.operation,
            authorized.principal_id.clone(),
            AuditPhase::Completed,
            completion.decision,
            completion.status_code,
            completion.response_sha256,
        )
    }

    /// Records a request rejected before authorization could complete, such as
    /// a malformed transport envelope or missing credential. The operation
    /// still determines the policy/audit action; the adapter cannot supply one.
    pub fn record_invocation_failure(
        &self,
        invocation: Invocation,
        operation: RrdOperation,
        principal_id: Option<CanonicalId>,
        completion: InvocationCompletion,
    ) -> Result<()> {
        self.validate_invocation_common(&invocation)?;
        if completion.decision == AuditDecision::Allowed {
            return Err(ServiceError::Contract(
                "a rejected invocation cannot have an allowed decision".into(),
            ));
        }
        validate_sha256(&completion.response_sha256)?;
        self.append_invocation_audit(
            &invocation,
            operation,
            principal_id,
            AuditPhase::Completed,
            completion.decision,
            completion.status_code,
            completion.response_sha256,
        )
    }

    /// Records an intentionally anonymous public endpoint, such as liveness
    /// or capability inspection. These endpoints never receive application
    /// mutation authority.
    pub fn record_public_invocation(
        &self,
        invocation: Invocation,
        operation: RrdOperation,
        completion: InvocationCompletion,
    ) -> Result<()> {
        if operation.mutates() {
            return Err(ServiceError::Contract(
                "a mutating operation cannot use the public invocation path".into(),
            ));
        }
        self.validate_invocation_common(&invocation)?;
        validate_sha256(&completion.response_sha256)?;
        self.append_invocation_audit(
            &invocation,
            operation,
            None,
            AuditPhase::Completed,
            completion.decision,
            completion.status_code,
            completion.response_sha256,
        )
    }

    fn validate_invocation(&self, invocation: &Invocation, operation: RrdOperation) -> Result<()> {
        self.validate_invocation_common(invocation)?;
        invocation
            .context
            .validate(operation.mutates())
            .map_err(|error| ServiceError::Contract(error.to_string()))?;
        Ok(())
    }

    fn validate_invocation_common(&self, invocation: &Invocation) -> Result<()> {
        invocation
            .context
            .validate(false)
            .map_err(|error| ServiceError::Contract(error.to_string()))?;
        invocation
            .resource
            .validate()
            .map_err(|error| ServiceError::Contract(error.to_string()))?;
        let target = invocation
            .resource
            .segments
            .iter()
            .find(|segment| segment.kind == ResourceKind::Instance);
        if target.is_none_or(|target| target.id != self.instance) {
            return Err(ServiceError::WrongScope);
        }
        if invocation
            .context
            .deadline_unix_ms
            .is_some_and(|deadline| deadline <= invocation.observed_at_unix_ms)
        {
            return Err(ServiceError::DeadlineExceeded);
        }
        validate_sha256(&invocation.request_sha256)
    }

    #[allow(clippy::too_many_arguments)]
    fn append_invocation_audit(
        &self,
        invocation: &Invocation,
        operation: RrdOperation,
        principal_id: Option<CanonicalId>,
        phase: AuditPhase,
        decision: AuditDecision,
        status_code: u16,
        response_sha256: String,
    ) -> Result<()> {
        self.append_audit(AuditEvent {
            at_unix_ms: invocation.observed_at_unix_ms,
            attempt: invocation.attempt,
            principal_id,
            action: operation.action(),
            resource: invocation.resource.clone(),
            request_id: invocation.context.request_id.as_str().into(),
            operation_id: invocation.context.operation_id.as_str().into(),
            phase,
            decision,
            status_code,
            request_sha256: invocation.request_sha256.clone(),
            response_sha256,
        })
    }
}

fn validate_sha256(value: &str) -> Result<()> {
    if value.len() == 64
        && value
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        Ok(())
    } else {
        Err(ServiceError::Contract(
            "invocation digest must be lowercase SHA-256 hex".into(),
        ))
    }
}

fn audit_failure(error: &ServiceError) -> (AuditDecision, u16) {
    match error.kind() {
        ServiceErrorKind::Unauthenticated => (AuditDecision::Denied, 401),
        ServiceErrorKind::PermissionDenied => (AuditDecision::Denied, 403),
        ServiceErrorKind::InvalidArgument => (AuditDecision::Failed, 400),
        ServiceErrorKind::NotFound => (AuditDecision::Failed, 404),
        ServiceErrorKind::Conflict => (AuditDecision::Failed, 409),
        ServiceErrorKind::FailedPrecondition => (AuditDecision::Failed, 412),
        ServiceErrorKind::ResourceExhausted => (AuditDecision::Failed, 429),
        ServiceErrorKind::DeadlineExceeded => (AuditDecision::Failed, 504),
        ServiceErrorKind::Internal => (AuditDecision::Failed, 500),
    }
}
