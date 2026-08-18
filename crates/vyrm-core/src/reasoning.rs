//! Typed, replayable contract for an externally inspectable reasoning run.
//!
//! The contract does not pretend to expose a model's hidden chain of thought.
//! It records the operational reasoning evidence a runtime can enforce:
//! goal, hypothesis/plan, attempts, observations, decisions, verification, and
//! outcome. Events are immutable and hash-chained; state is always derived by
//! replay so a corrupt or out-of-order ledger fails closed.

use crate::Millis;
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    /// Stable locator such as a repository path, command, trace, or artifact.
    pub source: String,
    /// SHA-256 of the observed content. Large evidence stays out of the event.
    pub digest: String,
    pub summary: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Passed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Check {
    pub name: String,
    pub status: CheckStatus,
    #[serde(default)]
    pub evidence: Vec<Evidence>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionKind {
    Continue,
    Verify,
    Stop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunOutcome {
    Succeeded,
    Failed,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReasoningPayload {
    Goal {
        statement: String,
        acceptance: Vec<String>,
    },
    Plan {
        hypothesis: String,
        steps: Vec<String>,
    },
    Attempt {
        summary: String,
        #[serde(default)]
        actions: Vec<String>,
    },
    Observation {
        summary: String,
        evidence: Vec<Evidence>,
    },
    Decision {
        decision: DecisionKind,
        rationale: String,
    },
    Verification {
        checks: Vec<Check>,
    },
    Outcome {
        outcome: RunOutcome,
        summary: String,
    },
}

impl ReasoningPayload {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Goal { .. } => "goal",
            Self::Plan { .. } => "plan",
            Self::Attempt { .. } => "attempt",
            Self::Observation { .. } => "observation",
            Self::Decision { .. } => "decision",
            Self::Verification { .. } => "verification",
            Self::Outcome { .. } => "outcome",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReasoningEvent {
    pub run_id: String,
    pub ordinal: u64,
    pub at: Millis,
    pub actor: String,
    pub payload: ReasoningPayload,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_digest: Option<String>,
    pub digest: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningState {
    Empty,
    NeedsPlan,
    NeedsAttempt,
    NeedsObservation,
    NeedsDecision,
    NeedsVerification,
    NeedsPostVerificationDecision,
    NeedsOutcome,
    Complete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractError(pub String);

impl fmt::Display for ContractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ContractError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReasoningRun {
    id: String,
    events: Vec<ReasoningEvent>,
    state: ReasoningState,
    last_verification_passed: Option<bool>,
}

impl ReasoningRun {
    pub fn empty(id: impl Into<String>) -> Result<Self, ContractError> {
        let id = id.into();
        validate_text("run_id", &id)?;
        if id.as_bytes().contains(&0) {
            return Err(ContractError("run_id must not contain NUL".into()));
        }
        Ok(Self {
            id,
            events: Vec::new(),
            state: ReasoningState::Empty,
            last_verification_passed: None,
        })
    }

    pub fn replay(events: Vec<ReasoningEvent>) -> Result<Self, ContractError> {
        let first = events
            .first()
            .ok_or_else(|| ContractError("cannot replay an empty reasoning run".into()))?;
        let mut run = Self::empty(first.run_id.clone())?;
        for stored in events {
            let expected = run.append(stored.at, stored.actor.clone(), stored.payload.clone())?;
            if expected != stored {
                return Err(ContractError(format!(
                    "reasoning event {} failed hash-chain or ordinal verification",
                    stored.ordinal
                )));
            }
        }
        Ok(run)
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn state(&self) -> ReasoningState {
        self.state
    }

    pub fn events(&self) -> &[ReasoningEvent] {
        &self.events
    }

    pub fn is_complete(&self) -> bool {
        self.state == ReasoningState::Complete
    }

    pub fn append(
        &mut self,
        at: Millis,
        actor: impl Into<String>,
        payload: ReasoningPayload,
    ) -> Result<ReasoningEvent, ContractError> {
        let actor = actor.into();
        validate_text("actor", &actor)?;
        if self.events.last().is_some_and(|event| at < event.at) {
            return Err(ContractError("reasoning event time must be monotonic".into()));
        }
        validate_payload(&payload)?;
        let next = transition(self.state, self.last_verification_passed, &payload)?;
        let ordinal = self.events.len() as u64 + 1;
        let previous_digest = self.events.last().map(|event| event.digest.clone());
        let digest = event_digest(
            &self.id,
            ordinal,
            at,
            &actor,
            &payload,
            previous_digest.as_deref(),
        );
        let event = ReasoningEvent {
            run_id: self.id.clone(),
            ordinal,
            at,
            actor,
            payload,
            previous_digest,
            digest,
        };
        if let ReasoningPayload::Verification { checks } = &event.payload {
            self.last_verification_passed =
                Some(checks.iter().all(|check| check.status == CheckStatus::Passed));
        }
        self.state = next;
        self.events.push(event.clone());
        Ok(event)
    }
}

fn transition(
    state: ReasoningState,
    last_verification_passed: Option<bool>,
    payload: &ReasoningPayload,
) -> Result<ReasoningState, ContractError> {
    use ReasoningPayload as P;
    use ReasoningState as S;
    let next = match (state, payload) {
        (S::Empty, P::Goal { .. }) => S::NeedsPlan,
        (S::NeedsPlan, P::Plan { .. }) => S::NeedsAttempt,
        (S::NeedsAttempt, P::Attempt { .. }) => S::NeedsObservation,
        (S::NeedsObservation, P::Observation { .. }) => S::NeedsDecision,
        (S::NeedsDecision, P::Decision { decision: DecisionKind::Continue, .. }) => S::NeedsAttempt,
        (S::NeedsDecision, P::Decision { decision: DecisionKind::Verify | DecisionKind::Stop, .. }) => S::NeedsVerification,
        (S::NeedsVerification, P::Verification { checks }) => {
            if checks.iter().all(|check| check.status == CheckStatus::Passed) {
                S::NeedsOutcome
            } else {
                S::NeedsPostVerificationDecision
            }
        }
        (S::NeedsPostVerificationDecision, P::Decision { decision: DecisionKind::Continue, .. }) => S::NeedsAttempt,
        (S::NeedsPostVerificationDecision, P::Decision { decision: DecisionKind::Stop, .. }) => S::NeedsOutcome,
        (S::NeedsOutcome, P::Outcome { outcome, .. }) => {
            match (last_verification_passed, outcome) {
                (Some(true), RunOutcome::Succeeded) => {}
                (Some(false), RunOutcome::Failed | RunOutcome::Blocked) => {}
                (Some(true), RunOutcome::Failed | RunOutcome::Blocked) => {}
                _ => return Err(ContractError("outcome contradicts verification evidence".into())),
            }
            S::Complete
        }
        _ => {
            return Err(ContractError(format!(
                "{} is invalid while reasoning run is {:?}",
                payload.name(),
                state
            )))
        }
    };
    Ok(next)
}

fn validate_text(field: &str, value: &str) -> Result<(), ContractError> {
    if value.trim().is_empty() {
        Err(ContractError(format!("{field} must not be empty")))
    } else {
        Ok(())
    }
}

fn validate_evidence(evidence: &Evidence) -> Result<(), ContractError> {
    validate_text("evidence.source", &evidence.source)?;
    validate_text("evidence.summary", &evidence.summary)?;
    if evidence.digest.len() != 64 || !evidence.digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(ContractError("evidence.digest must be a SHA-256 hex digest".into()));
    }
    Ok(())
}

fn validate_payload(payload: &ReasoningPayload) -> Result<(), ContractError> {
    use ReasoningPayload as P;
    match payload {
        P::Goal { statement, acceptance } => {
            validate_text("goal.statement", statement)?;
            if acceptance.is_empty() { return Err(ContractError("goal.acceptance must not be empty".into())); }
            for value in acceptance { validate_text("goal.acceptance", value)?; }
        }
        P::Plan { hypothesis, steps } => {
            validate_text("plan.hypothesis", hypothesis)?;
            if steps.is_empty() { return Err(ContractError("plan.steps must not be empty".into())); }
            for value in steps { validate_text("plan.steps", value)?; }
        }
        P::Attempt { summary, actions } => {
            validate_text("attempt.summary", summary)?;
            for value in actions { validate_text("attempt.actions", value)?; }
        }
        P::Observation { summary, evidence } => {
            validate_text("observation.summary", summary)?;
            if evidence.is_empty() { return Err(ContractError("observation.evidence must not be empty".into())); }
            for value in evidence { validate_evidence(value)?; }
        }
        P::Decision { rationale, .. } => validate_text("decision.rationale", rationale)?,
        P::Verification { checks } => {
            if checks.is_empty() { return Err(ContractError("verification.checks must not be empty".into())); }
            for check in checks {
                validate_text("verification.check.name", &check.name)?;
                if check.evidence.is_empty() {
                    return Err(ContractError(
                        "every verification check must cite content-addressed evidence".into(),
                    ));
                }
                for evidence in &check.evidence { validate_evidence(evidence)?; }
            }
        }
        P::Outcome { summary, .. } => validate_text("outcome.summary", summary)?,
    }
    Ok(())
}

fn event_digest(
    run_id: &str,
    ordinal: u64,
    at: Millis,
    actor: &str,
    payload: &ReasoningPayload,
    previous: Option<&str>,
) -> String {
    fn text(out: &mut Vec<u8>, value: &str) {
        out.extend_from_slice(&(value.len() as u64).to_be_bytes());
        out.extend_from_slice(value.as_bytes());
    }
    fn strings(out: &mut Vec<u8>, values: &[String]) {
        out.extend_from_slice(&(values.len() as u64).to_be_bytes());
        for value in values { text(out, value); }
    }
    fn evidence(out: &mut Vec<u8>, values: &[Evidence]) {
        out.extend_from_slice(&(values.len() as u64).to_be_bytes());
        for value in values {
            text(out, &value.source);
            text(out, &value.digest);
            text(out, &value.summary);
        }
    }
    let mut bytes = b"vyrm-reasoning-event-v1\0".to_vec();
    text(&mut bytes, run_id);
    bytes.extend_from_slice(&ordinal.to_be_bytes());
    bytes.extend_from_slice(&at.to_be_bytes());
    text(&mut bytes, actor);
    match payload {
        ReasoningPayload::Goal { statement, acceptance } => { bytes.push(0); text(&mut bytes, statement); strings(&mut bytes, acceptance); }
        ReasoningPayload::Plan { hypothesis, steps } => { bytes.push(1); text(&mut bytes, hypothesis); strings(&mut bytes, steps); }
        ReasoningPayload::Attempt { summary, actions } => { bytes.push(2); text(&mut bytes, summary); strings(&mut bytes, actions); }
        ReasoningPayload::Observation { summary, evidence: values } => { bytes.push(3); text(&mut bytes, summary); evidence(&mut bytes, values); }
        ReasoningPayload::Decision { decision, rationale } => { bytes.push(4); bytes.push(*decision as u8); text(&mut bytes, rationale); }
        ReasoningPayload::Verification { checks } => {
            bytes.push(5);
            bytes.extend_from_slice(&(checks.len() as u64).to_be_bytes());
            for check in checks { text(&mut bytes, &check.name); bytes.push(check.status as u8); evidence(&mut bytes, &check.evidence); }
        }
        ReasoningPayload::Outcome { outcome, summary } => { bytes.push(6); bytes.push(*outcome as u8); text(&mut bytes, summary); }
    }
    bytes.push(u8::from(previous.is_some()));
    if let Some(previous) = previous { text(&mut bytes, previous); }
    crate::digest::sha256_hex(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proof(summary: &str) -> Evidence {
        Evidence { source: "cargo test".into(), digest: "a".repeat(64), summary: summary.into() }
    }

    #[test]
    fn a_complete_run_is_typed_ordered_and_hash_chained() {
        let mut run = ReasoningRun::empty("run-1").unwrap();
        run.append(1, "agent", ReasoningPayload::Goal { statement: "fix it".into(), acceptance: vec!["tests pass".into()] }).unwrap();
        run.append(2, "agent", ReasoningPayload::Plan { hypothesis: "bug is temporal".into(), steps: vec!["test".into()] }).unwrap();
        run.append(3, "agent", ReasoningPayload::Attempt { summary: "patched".into(), actions: vec!["edit temporal.rs".into()] }).unwrap();
        run.append(4, "tool", ReasoningPayload::Observation { summary: "suite passed".into(), evidence: vec![proof("142 passed")] }).unwrap();
        run.append(5, "agent", ReasoningPayload::Decision { decision: DecisionKind::Verify, rationale: "candidate is ready".into() }).unwrap();
        run.append(6, "tool", ReasoningPayload::Verification { checks: vec![Check { name: "workspace tests".into(), status: CheckStatus::Passed, evidence: vec![proof("green")] }] }).unwrap();
        run.append(7, "agent", ReasoningPayload::Outcome { outcome: RunOutcome::Succeeded, summary: "acceptance met".into() }).unwrap();
        assert!(run.is_complete());
        assert_eq!(run.events()[1].previous_digest.as_deref(), Some(run.events()[0].digest.as_str()));
        assert_eq!(ReasoningRun::replay(run.events().to_vec()).unwrap(), run);
    }

    #[test]
    fn skipped_stages_and_success_without_passing_verification_are_rejected() {
        let mut run = ReasoningRun::empty("run-2").unwrap();
        assert!(run.append(1, "agent", ReasoningPayload::Attempt { summary: "guess".into(), actions: vec![] }).is_err());
        run.append(1, "agent", ReasoningPayload::Goal { statement: "fix".into(), acceptance: vec!["works".into()] }).unwrap();
        assert!(run.append(2, "agent", ReasoningPayload::Outcome { outcome: RunOutcome::Succeeded, summary: "trust me".into() }).is_err());
    }

    #[test]
    fn verification_without_content_addressed_evidence_is_rejected() {
        let payload = ReasoningPayload::Verification {
            checks: vec![Check {
                name: "tests".into(),
                status: CheckStatus::Passed,
                evidence: Vec::new(),
            }],
        };
        assert!(validate_payload(&payload).is_err());
    }

    #[test]
    fn failed_verification_requires_an_explicit_continue_or_stop_decision() {
        let mut run = ReasoningRun::empty("run-3").unwrap();
        run.append(1, "a", ReasoningPayload::Goal { statement: "fix".into(), acceptance: vec!["green".into()] }).unwrap();
        run.append(2, "a", ReasoningPayload::Plan { hypothesis: "x".into(), steps: vec!["y".into()] }).unwrap();
        run.append(3, "a", ReasoningPayload::Attempt { summary: "try".into(), actions: vec![] }).unwrap();
        run.append(4, "t", ReasoningPayload::Observation { summary: "red".into(), evidence: vec![proof("red")] }).unwrap();
        run.append(5, "a", ReasoningPayload::Decision { decision: DecisionKind::Verify, rationale: "check".into() }).unwrap();
        run.append(6, "t", ReasoningPayload::Verification { checks: vec![Check { name: "test".into(), status: CheckStatus::Failed, evidence: vec![proof("red")] }] }).unwrap();
        assert_eq!(run.state(), ReasoningState::NeedsPostVerificationDecision);
        assert!(run.append(7, "a", ReasoningPayload::Outcome { outcome: RunOutcome::Failed, summary: "failed".into() }).is_err());
    }
}
