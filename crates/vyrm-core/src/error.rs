//! Kernel errors. No storage-engine or transport error types appear here.

use std::fmt;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    EmptyIdentifier {
        kind: &'static str,
    },
    SeparatorInIdentifier {
        kind: &'static str,
        value: String,
    },
    /// A claim's validity window is empty or inverted.
    InvalidValidityWindow {
        valid_from: u64,
        valid_to: u64,
    },
    /// A key could not be parsed back into its fields.
    MalformedKey {
        reason: &'static str,
    },
    /// Sequence allocation overflowed. Reported rather than saturated, so that
    /// overflow cannot degrade into silent key reuse.
    SequenceOverflow,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::EmptyIdentifier { kind } => write!(f, "{kind} must not be empty"),
            Error::SeparatorInIdentifier { kind, value } => {
                write!(f, "{kind} must not contain the 0x00 separator: {value:?}")
            }
            Error::InvalidValidityWindow {
                valid_from,
                valid_to,
            } => write!(
                f,
                "validity window is empty or inverted: valid_from={valid_from} valid_to={valid_to}"
            ),
            Error::MalformedKey { reason } => write!(f, "malformed claim key: {reason}"),
            Error::SequenceOverflow => write!(f, "sequence allocation overflowed"),
        }
    }
}

impl std::error::Error for Error {}
