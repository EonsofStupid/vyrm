//! Invocation records. `SPEC.md` §13.
//!
//! Stage 1 requires that every trigger be invoked explicitly and recorded, and
//! that no trigger be automated before its recorded invocations justify it. This
//! module is that record.
//!
//! Records are authoritative, not telemetry: they are the evidence from which
//! automation policy is later derived, so losing them would remove the basis for
//! the decision. The §13.1 effectiveness ledger extends this shape with token
//! counts rather than introducing a second log.

use serde::{Deserialize, Serialize};
use vyrm_core::Millis;

/// What caused an invocation. Only `Manual` occurs at stage 1; the remaining
/// variants exist so that promoting a trigger to automatic is a change of value
/// rather than a change of schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    Manual,
    Event,
    Interval,
    Threshold,
}

impl std::fmt::Display for Trigger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Trigger::Manual => "manual",
            Trigger::Event => "event",
            Trigger::Interval => "interval",
            Trigger::Threshold => "threshold",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Ok,
    Error,
}

/// One recorded invocation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Invocation {
    /// Monotonic ordinal, allocated inside the recording transaction.
    pub ordinal: u64,
    pub at: Millis,
    pub trigger: Trigger,
    pub command: String,
    pub arguments: Vec<String>,
    pub outcome: Outcome,
    pub duration_ms: u64,
    /// Failure reason, or a short result summary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl Invocation {
    /// Rendered as one line, for an operator reading the log directly.
    pub fn render(&self) -> String {
        format!(
            "{:>6}  {}  {:<9} {:<10} {:>7}ms  {}{}",
            self.ordinal,
            self.at,
            self.trigger.to_string(),
            self.command,
            self.duration_ms,
            match self.outcome {
                Outcome::Ok => "ok",
                Outcome::Error => "error",
            },
            self.detail
                .as_ref()
                .map(|d| format!("  {d}"))
                .unwrap_or_default(),
        )
    }
}

/// Key for one invocation record: `{at:020}\x00{ordinal:020}`.
///
/// Time leads so the log scans in chronological order. The ordinal disambiguates
/// invocations recorded within the same millisecond.
pub(crate) fn invocation_key(at: Millis, ordinal: u64) -> Vec<u8> {
    let mut key = format!("{at:020}").into_bytes();
    key.push(0x00);
    key.extend_from_slice(format!("{ordinal:020}").as_bytes());
    key
}

/// Lower bound for an invocation scan starting at `at`.
pub(crate) fn invocation_bound(at: Millis) -> Vec<u8> {
    format!("{at:020}").into_bytes()
}

/// Fields supplied when recording an invocation.
///
/// Grouped rather than passed positionally: the ordinal is allocated by the
/// store, so a caller supplies everything else as one value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvocationInput<'a> {
    pub at: Millis,
    pub trigger: Trigger,
    pub command: &'a str,
    pub arguments: &'a [String],
    pub outcome: Outcome,
    pub duration_ms: u64,
    pub detail: Option<String>,
}
