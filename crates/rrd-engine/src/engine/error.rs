use super::*;

pub type Result<T> = std::result::Result<T, ServiceError>;

#[derive(Debug)]
pub enum ServiceError {
    Contract(String),
    Estate(String),
    Storage(String),
    StorageConflict(String),
    SessionNotFound,
    Unauthenticated,
    SessionExpired,
    TransactionNotFound,
    TransactionExpired,
    TransactionClosed,
    TransactionQuota,
    RenewalQuota,
    IdempotencyConflict,
    CommitInProgress,
    WrongScope,
    OperationDigestMismatch,
    Query(String),
    Vector(String),
    Changefeed(String),
    Subscription(String),
    SubscriptionNotFound,
    SubscriptionExpired {
        requested: u64,
        retention_floor: u64,
        head: u64,
    },
    SubscriptionLeaseExpired,
    SubscriptionClosed,
    SubscriptionConnectionReplaced,
    SubscriptionBackpressure,
    Backup(String),
    Runtime(String),
    PermissionDenied,
    DeadlineExceeded,
    ProjectBindingMismatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceErrorKind {
    InvalidArgument,
    NotFound,
    Unauthenticated,
    PermissionDenied,
    Conflict,
    FailedPrecondition,
    ResourceExhausted,
    DeadlineExceeded,
    Internal,
}

impl ServiceError {
    pub fn kind(&self) -> ServiceErrorKind {
        match self {
            Self::Contract(_)
            | Self::Changefeed(_)
            | Self::Subscription(_)
            | Self::Query(_)
            | Self::Runtime(_)
            | Self::Vector(_)
            | Self::OperationDigestMismatch
            | Self::WrongScope => ServiceErrorKind::InvalidArgument,
            Self::SessionNotFound | Self::TransactionNotFound | Self::SubscriptionNotFound => {
                ServiceErrorKind::NotFound
            }
            Self::Unauthenticated => ServiceErrorKind::Unauthenticated,
            Self::PermissionDenied => ServiceErrorKind::PermissionDenied,
            Self::IdempotencyConflict => ServiceErrorKind::Conflict,
            Self::SessionExpired
            | Self::TransactionExpired
            | Self::TransactionClosed
            | Self::CommitInProgress
            | Self::SubscriptionExpired { .. }
            | Self::SubscriptionLeaseExpired
            | Self::SubscriptionClosed
            | Self::SubscriptionConnectionReplaced
            | Self::ProjectBindingMismatch => ServiceErrorKind::FailedPrecondition,
            Self::TransactionQuota | Self::RenewalQuota | Self::SubscriptionBackpressure => {
                ServiceErrorKind::ResourceExhausted
            }
            Self::DeadlineExceeded => ServiceErrorKind::DeadlineExceeded,
            Self::StorageConflict(_) => ServiceErrorKind::Conflict,
            Self::Storage(_) | Self::Backup(_) | Self::Estate(_) => ServiceErrorKind::Internal,
        }
    }

    pub fn retryable(&self) -> bool {
        matches!(self, Self::StorageConflict(_))
    }
}

impl fmt::Display for ServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for ServiceError {}

impl From<rrd_store::Error> for ServiceError {
    fn from(value: rrd_store::Error) -> Self {
        let message = value.to_string();
        match value {
            rrd_store::Error::RuntimeConflict { .. }
            | rrd_store::Error::RuntimeSchemaConflict { .. }
            | rrd_store::Error::IdempotencyConflict(_)
            | rrd_store::Error::ControlConflict(_)
            | rrd_store::Error::ReadStampMismatch(_) => Self::StorageConflict(message),
            _ => Self::Storage(message),
        }
    }
}

impl From<rrd_estate::Error> for ServiceError {
    fn from(value: rrd_estate::Error) -> Self {
        Self::Estate(value.to_string())
    }
}

impl From<rrd_security::Error> for ServiceError {
    fn from(value: rrd_security::Error) -> Self {
        match value {
            rrd_security::Error::Unauthenticated | rrd_security::Error::PrincipalNotFound => {
                Self::Unauthenticated
            }
            rrd_security::Error::PermissionDenied | rrd_security::Error::NotInitialized => {
                Self::PermissionDenied
            }
            rrd_security::Error::Invalid(message) => Self::Contract(message),
            rrd_security::Error::IdempotencyConflict | rrd_security::Error::AlreadyInitialized => {
                Self::IdempotencyConflict
            }
            rrd_security::Error::Store(error) => Self::from(error),
        }
    }
}
