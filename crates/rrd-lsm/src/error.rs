use std::fmt;
use std::path::PathBuf;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    IoContext {
        operation: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },
    Corruption {
        offset: u64,
        reason: String,
    },
    UnsupportedVersion {
        object: &'static str,
        version: u16,
    },
    InvalidConfiguration(String),
    InvalidBatch(String),
    InvalidManifest(String),
    ManifestConflict {
        expected: Option<String>,
        actual: Option<String>,
    },
    InvalidSegment(String),
    TornTail {
        offset: u64,
    },
    DatabaseWriterLock {
        path: PathBuf,
    },
    RecoveryRequired {
        boundary: &'static str,
    },
    PoisonedWriter,
    InjectedFailure {
        mode: &'static str,
        boundary: &'static str,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "RRD LSM I/O: {error}"),
            Self::IoContext {
                operation,
                path,
                source,
            } => write!(
                formatter,
                "RRD LSM I/O while {operation} {}: {source}",
                path.display()
            ),
            Self::Corruption { offset, reason } => {
                write!(formatter, "RRD LSM corruption at byte {offset}: {reason}")
            }
            Self::UnsupportedVersion { object, version } => {
                write!(formatter, "unsupported {object} format version {version}")
            }
            Self::InvalidConfiguration(reason) => {
                write!(formatter, "invalid RRD LSM configuration: {reason}")
            }
            Self::InvalidBatch(reason) => write!(formatter, "invalid WAL batch: {reason}"),
            Self::InvalidManifest(reason) => write!(formatter, "invalid manifest: {reason}"),
            Self::ManifestConflict { expected, actual } => write!(
                formatter,
                "manifest compare-and-swap conflict: expected {expected:?}, actual {actual:?}"
            ),
            Self::InvalidSegment(reason) => write!(formatter, "invalid segment: {reason}"),
            Self::TornTail { offset } => write!(
                formatter,
                "WAL has an incomplete tail at byte {offset}; explicit repair is required"
            ),
            Self::DatabaseWriterLock { path } => write!(
                formatter,
                "RRD LSM database already has an active writer: {}",
                path.display()
            ),
            Self::RecoveryRequired { boundary } => write!(
                formatter,
                "RRD LSM writer crossed {boundary} without applying its durable WAL frame; reopen is required"
            ),
            Self::PoisonedWriter => write!(
                formatter,
                "WAL writer is poisoned after a failed append and must be reopened"
            ),
            Self::InjectedFailure { mode, boundary } => {
                write!(formatter, "injected {mode} failure at {boundary}")
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::IoContext { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl Error {
    pub(crate) fn io(
        operation: &'static str,
        path: impl Into<PathBuf>,
        source: std::io::Error,
    ) -> Self {
        Self::IoContext {
            operation,
            path: path.into(),
            source,
        }
    }
}

impl From<serde_json::Error> for Error {
    fn from(value: serde_json::Error) -> Self {
        Self::InvalidManifest(value.to_string())
    }
}
