use std::fmt;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Corruption { offset: u64, reason: String },
    UnsupportedVersion { object: &'static str, version: u16 },
    InvalidBatch(String),
    InvalidManifest(String),
    TornTail { offset: u64 },
    PoisonedWriter,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "vyrmKV I/O: {error}"),
            Self::Corruption { offset, reason } => {
                write!(formatter, "vyrmKV corruption at byte {offset}: {reason}")
            }
            Self::UnsupportedVersion { object, version } => {
                write!(formatter, "unsupported {object} format version {version}")
            }
            Self::InvalidBatch(reason) => write!(formatter, "invalid WAL batch: {reason}"),
            Self::InvalidManifest(reason) => write!(formatter, "invalid manifest: {reason}"),
            Self::TornTail { offset } => write!(
                formatter,
                "WAL has an incomplete tail at byte {offset}; explicit repair is required"
            ),
            Self::PoisonedWriter => write!(
                formatter,
                "WAL writer is poisoned after a failed append and must be reopened"
            ),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for Error {
    fn from(value: serde_json::Error) -> Self {
        Self::InvalidManifest(value.to_string())
    }
}
