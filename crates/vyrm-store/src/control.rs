//! Authoritative, replayable control-plane state transitions.

use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use vyrm_core::digest;

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
