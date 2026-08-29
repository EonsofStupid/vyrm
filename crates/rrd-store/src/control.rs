//! Authoritative, replayable control-plane state transitions.

use crate::{Error, Result};
use rrd_core::digest;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const MAX_CONTROL_BATCH_TRANSITIONS: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlTransition {
    pub key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected: Option<Vec<u8>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replacement: Option<Vec<u8>>,
    pub at: u64,
    pub actor: String,
    pub action: String,
    pub request_id: String,
    pub operation_id: String,
}

impl ControlTransition {
    pub fn validate(&self) -> Result<()> {
        validate_control_key(&self.key)?;
        for (name, value) in [
            ("actor", self.actor.as_str()),
            ("action", self.action.as_str()),
            ("request_id", self.request_id.as_str()),
            ("operation_id", self.operation_id.as_str()),
        ] {
            if value.is_empty() || value.len() > 256 || !value.is_ascii() {
                return Err(Error::Substrate(format!(
                    "invalid control transition {name}"
                )));
            }
        }
        if self.at == 0 {
            return Err(Error::Substrate(
                "control transition time must be non-zero".into(),
            ));
        }
        if self
            .expected
            .as_ref()
            .is_some_and(|value| value.len() > 1024 * 1024)
            || self
                .replacement
                .as_ref()
                .is_some_and(|value| value.len() > 1024 * 1024)
        {
            return Err(Error::Substrate("control state exceeds one MiB".into()));
        }
        Ok(())
    }
}

pub(crate) fn validate_control_batch(transitions: &[ControlTransition]) -> Result<()> {
    if transitions.is_empty() || transitions.len() > MAX_CONTROL_BATCH_TRANSITIONS {
        return Err(Error::Substrate(
            "control batch size must be in 1..=64".into(),
        ));
    }
    let mut keys = BTreeSet::new();
    for transition in transitions {
        transition.validate()?;
        if !keys.insert(transition.key.as_str()) {
            return Err(Error::Substrate(
                "control batch cannot address one key more than once".into(),
            ));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlJournalEntry {
    pub sequence: u64,
    pub key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replacement: Option<Vec<u8>>,
    pub at: u64,
    pub actor: String,
    pub action: String,
    pub request_id: String,
    pub operation_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_digest: Option<String>,
    pub digest: String,
}

impl ControlJournalEntry {
    pub(crate) fn committed(
        sequence: u64,
        transition: &ControlTransition,
        previous_digest: Option<String>,
    ) -> Self {
        let mut entry = Self {
            sequence,
            key: transition.key.clone(),
            before_sha256: transition.expected.as_deref().map(digest::sha256_hex),
            replacement: transition.replacement.clone(),
            at: transition.at,
            actor: transition.actor.clone(),
            action: transition.action.clone(),
            request_id: transition.request_id.clone(),
            operation_id: transition.operation_id.clone(),
            previous_digest,
            digest: String::new(),
        };
        entry.digest = digest::sha256_hex(&entry.canonical_bytes());
        entry
    }

    pub fn verify(&self) -> bool {
        digest::sha256_hex(&self.canonical_bytes()) == self.digest
    }

    fn canonical_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(&(
            self.sequence,
            &self.key,
            &self.before_sha256,
            &self.replacement,
            self.at,
            &self.actor,
            &self.action,
            &self.request_id,
            &self.operation_id,
            &self.previous_digest,
        ))
        .expect("control journal fields serialize")
    }
}

pub(crate) fn verify_control_tail(
    sequence: u64,
    advertised_digest: Option<&str>,
    entry: Option<&ControlJournalEntry>,
) -> Result<()> {
    match (sequence, advertised_digest, entry) {
        (0, None, None) => Ok(()),
        (sequence, Some(advertised), Some(entry))
            if entry.sequence == sequence && entry.verify() && entry.digest == advertised =>
        {
            Ok(())
        }
        _ => Err(Error::Substrate(
            "control journal tail is missing, corrupt, or inconsistent".into(),
        )),
    }
}

pub(crate) fn verify_control_page(
    after: u64,
    anchor_digest: Option<String>,
    entries: &[ControlJournalEntry],
) -> Result<()> {
    let mut previous = anchor_digest;
    for (offset, entry) in entries.iter().enumerate() {
        let expected_sequence = after
            .checked_add(offset as u64 + 1)
            .ok_or(Error::SequenceOverflow)?;
        if entry.sequence != expected_sequence
            || !entry.verify()
            || entry.previous_digest != previous
        {
            return Err(Error::Substrate(
                "control journal page is missing, reordered, or corrupt".into(),
            ));
        }
        previous = Some(entry.digest.clone());
    }
    Ok(())
}

pub(crate) fn validate_control_key(key: &str) -> Result<()> {
    if !key.starts_with("server/state/")
        || key.len() > 256
        || !key.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_' | b'.' | b':')
        })
    {
        return Err(Error::Substrate("invalid server control-state key".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(sequence: u64, previous_digest: Option<String>) -> ControlJournalEntry {
        ControlJournalEntry::committed(
            sequence,
            &ControlTransition {
                key: "server/state/test/session-1".into(),
                expected: None,
                replacement: Some(format!("state-{sequence}").into_bytes()),
                at: sequence,
                actor: "test".into(),
                action: "test.transition".into(),
                request_id: format!("request-{sequence}"),
                operation_id: format!("operation-{sequence}"),
            },
            previous_digest,
        )
    }

    #[test]
    fn page_verification_rejects_gaps_reordering_and_tampering() {
        let first = entry(1, None);
        let second = entry(2, Some(first.digest.clone()));
        let third = entry(3, Some(second.digest.clone()));
        verify_control_page(0, None, &[first.clone(), second.clone(), third.clone()]).unwrap();
        verify_control_page(
            1,
            Some(first.digest.clone()),
            &[second.clone(), third.clone()],
        )
        .unwrap();

        assert!(verify_control_page(0, None, &[first.clone(), third.clone()]).is_err());
        assert!(verify_control_page(0, None, &[second.clone(), first.clone()]).is_err());
        assert!(
            verify_control_page(1, Some("0".repeat(64)), std::slice::from_ref(&second)).is_err()
        );
        let mut tampered = second;
        tampered.action = "tampered".into();
        assert!(verify_control_page(1, Some(first.digest), &[tampered]).is_err());
    }

    #[test]
    fn tail_verification_rejects_a_forked_or_missing_tail() {
        let first = entry(1, None);
        assert!(verify_control_tail(0, None, None).is_ok());
        assert!(verify_control_tail(1, Some(&first.digest), Some(&first)).is_ok());
        assert!(verify_control_tail(1, Some(&"0".repeat(64)), Some(&first)).is_err());
        assert!(verify_control_tail(1, Some(&first.digest), None).is_err());
    }
}
