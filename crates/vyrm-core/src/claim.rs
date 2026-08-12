//! The claim: one bi-temporal assertion with provenance.

use crate::error::{Error, Result};
use crate::ident::{Predicate, Subject};
use serde::{Deserialize, Serialize};

/// Milliseconds since the Unix epoch. The kernel never reads a clock itself —
/// time is always supplied by the caller, so every operation is reproducible and
/// testable.
pub type Millis = u64;

/// Tier the claim currently belongs to. Unconditional at tier 0, and present
/// from the first revision so that promotion is a state transition rather than a
/// migration. See `SPEC.md` §9.2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    #[default]
    Local,
    Primary,
    Tenant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PromotionState {
    #[default]
    Unpromoted,
    Pending,
    Promoted,
    Denied,
}

/// Who produced this claim. Mandatory: a claim written by an executor on behalf
/// of a model must be attributable to both.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Producer {
    /// e.g. "agent:clyffy", "human:jessay", "tool:check-datafusion"
    pub actor: String,
    /// e.g. "claude-opus-5" when the actor wrote on a model's behalf.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_behalf_of: Option<String>,
    /// Session or run identifier, so a claim traces back to its context.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Claim {
    pub subject: Subject,
    pub predicate: Predicate,
    pub object: String,

    /// Start of the valid-time interval: when the claim began to hold in the
    /// modelled domain.
    pub valid_from: Millis,
    /// End of the valid-time interval, exclusive. `None` denotes an open
    /// interval.
    #[serde(default)]
    pub valid_to: Option<Millis>,
    /// Transaction time: the instant the kernel recorded this claim. Distinct
    /// from `valid_from`, which records when the claim began to hold.
    pub tx_time: Millis,

    pub producer: Producer,
    #[serde(default)]
    pub confidence: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supersedes: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,

    #[serde(default)]
    pub tier: Tier,
    #[serde(default)]
    pub promotion_state: PromotionState,
}

impl Claim {
    pub fn new(
        subject: Subject,
        predicate: Predicate,
        object: impl Into<String>,
        valid_from: Millis,
        tx_time: Millis,
        producer: Producer,
    ) -> Self {
        Self {
            subject,
            predicate,
            object: object.into(),
            valid_from,
            valid_to: None,
            tx_time,
            producer,
            confidence: None,
            supersedes: None,
            signature: None,
            tier: Tier::default(),
            promotion_state: PromotionState::default(),
        }
    }

    /// Closes this claim's valid-time interval. Retirement, not deletion: the
    /// claim remains readable for resolutions at instants before `at`.
    pub fn retire(&mut self, at: Millis) -> Result<()> {
        if at <= self.valid_from {
            return Err(Error::InvalidValidityWindow {
                valid_from: self.valid_from,
                valid_to: at,
            });
        }
        self.valid_to = Some(at);
        Ok(())
    }

    /// Was this claim valid at `as_of`?
    ///
    /// A key seek alone cannot answer this: it finds the newest claim with
    /// `valid_from <= as_of`, but that claim may have been retired without a
    /// successor. The window check is what makes retirement meaningful.
    pub fn valid_at(&self, as_of: Millis) -> bool {
        if as_of < self.valid_from {
            return false;
        }
        match self.valid_to {
            // Half-open [valid_from, valid_to): at the instant of retirement the
            // claim is already not valid, so a same-instant successor is
            // unambiguous.
            Some(to) => as_of < to,
            None => true,
        }
    }

    pub fn validate(&self) -> Result<()> {
        if let Some(to) = self.valid_to {
            if to <= self.valid_from {
                return Err(Error::InvalidValidityWindow {
                    valid_from: self.valid_from,
                    valid_to: to,
                });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn producer() -> Producer {
        Producer {
            actor: "agent:clyffy".into(),
            on_behalf_of: Some("claude-opus-5".into()),
            session: Some("s-1".into()),
        }
    }

    fn claim(valid_from: Millis) -> Claim {
        Claim::new(
            Subject::new("wp3").unwrap(),
            Predicate::new("status").unwrap(),
            "in_progress",
            valid_from,
            valid_from,
            producer(),
        )
    }

    #[test]
    fn open_window_is_valid_forever_after() {
        let c = claim(100);
        assert!(!c.valid_at(99));
        assert!(c.valid_at(100));
        assert!(c.valid_at(u64::MAX));
    }

    #[test]
    fn retired_window_is_half_open() {
        let mut c = claim(100);
        c.retire(200).unwrap();
        assert!(c.valid_at(100));
        assert!(c.valid_at(199));
        assert!(!c.valid_at(200), "retirement instant is already invalid");
        assert!(!c.valid_at(201));
    }

    #[test]
    fn retirement_cannot_invert_the_window() {
        let mut c = claim(100);
        assert!(c.retire(100).is_err());
        assert!(c.retire(99).is_err());
    }

    #[test]
    fn round_trips_through_json() {
        let c = claim(100);
        let text = serde_json::to_string(&c).unwrap();
        assert_eq!(serde_json::from_str::<Claim>(&text).unwrap(), c);
    }
}
