//! Substrate adapter errors.

use std::fmt;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    /// Propagated from the substrate.
    Substrate(String),
    /// Claim encoding or decoding failed.
    Codec(String),
    /// Propagated from the kernel.
    Kernel(vyrm_core::Error),
    /// Sequence allocation overflowed. Reported rather than saturated, so that
    /// overflow cannot degrade into silent key reuse. `SPEC.md` §11 correction 2.
    SequenceOverflow,
    /// A recorded watermark was not a valid sequence value.
    CorruptWatermark(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Substrate(m) => write!(f, "substrate: {m}"),
            Error::Codec(m) => write!(f, "claim codec: {m}"),
            Error::Kernel(e) => write!(f, "kernel: {e}"),
            Error::SequenceOverflow => write!(f, "sequence allocation overflowed"),
            Error::CorruptWatermark(m) => write!(f, "corrupt sequence watermark: {m}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<fjall::Error> for Error {
    fn from(value: fjall::Error) -> Self {
        Error::Substrate(value.to_string())
    }
}

impl From<vyrm_core::Error> for Error {
    fn from(value: vyrm_core::Error) -> Self {
        Error::Kernel(value)
    }
}

impl From<serde_json::Error> for Error {
    fn from(value: serde_json::Error) -> Self {
        Error::Codec(value.to_string())
    }
}
