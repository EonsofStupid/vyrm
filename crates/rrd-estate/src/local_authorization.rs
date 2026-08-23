use rrd_contract::CanonicalId;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub const LOCAL_OPERATOR_POLICY_FORMAT: u16 = 1;
const MAX_POLICY_BYTES: u64 = 1024 * 1024;
const MAX_ESTATE_GRANTS: usize = 1024;
const OPERATOR_KEY_BYTES: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalEstatePermission {
    Create,
    SetDesired,
    ScheduleBackup,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalOperatorPolicy {
    pub format: u16,
    pub operator_id: CanonicalId,
    pub key_sha256: String,
    pub not_before_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub estates: BTreeMap<String, BTreeSet<LocalEstatePermission>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalOperatorAuthorization {
    pub operator_id: CanonicalId,
    pub estate_id: CanonicalId,
    pub permission: LocalEstatePermission,
}

impl LocalOperatorPolicy {
    pub fn load_json(path: &Path) -> Result<Self, String> {
        let metadata = std::fs::metadata(path)
            .map_err(|error| format!("cannot inspect local operator policy: {error}"))?;
        if !metadata.is_file() {
            return Err("local operator policy is not a regular file".into());
        }
        validate_private_metadata(&metadata, "policy")?;
        if metadata.len() > MAX_POLICY_BYTES {
            return Err("local operator policy exceeds one MiB".into());
        }
        let bytes = std::fs::read(path)
            .map_err(|error| format!("cannot read local operator policy: {error}"))?;
        let policy: Self = serde_json::from_slice(&bytes)
            .map_err(|error| format!("cannot decode local operator policy: {error}"))?;
        policy.validate()?;
        Ok(policy)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.format != LOCAL_OPERATOR_POLICY_FORMAT
            || self.estates.is_empty()
            || self.estates.len() > MAX_ESTATE_GRANTS
        {
            return Err("unsupported or empty/oversized local operator policy".into());
        }
        validate_sha256(&self.key_sha256)?;
        if self.not_before_unix_ms >= self.expires_at_unix_ms {
            return Err("local operator policy validity window is invalid".into());
        }
        for (estate, permissions) in &self.estates {
            let canonical = CanonicalId::new(estate.clone()).map_err(|error| error.to_string())?;
            if canonical.as_str() != estate || permissions.is_empty() {
                return Err("local operator estate grant is invalid".into());
            }
        }
        Ok(())
    }

    pub fn authorize_key_file(
        &self,
        key_path: &Path,
        estate_id: CanonicalId,
        permission: LocalEstatePermission,
        at_unix_ms: u64,
    ) -> Result<LocalOperatorAuthorization, String> {
        if at_unix_ms < self.not_before_unix_ms || at_unix_ms >= self.expires_at_unix_ms {
            return Err("local operator policy is outside its validity window".into());
        }
        let permissions = self
            .estates
            .get(estate_id.as_str())
            .ok_or("local operator policy does not grant this estate")?;
        if !permissions.contains(&permission) {
            return Err("local operator policy does not grant this action".into());
        }
        let metadata = std::fs::metadata(key_path)
            .map_err(|error| format!("cannot inspect local operator key: {error}"))?;
        if !metadata.is_file() {
            return Err("local operator key is not a regular file".into());
        }
        validate_private_metadata(&metadata, "key")?;
        let key = std::fs::read(key_path)
            .map_err(|error| format!("cannot read local operator key: {error}"))?;
        if key.len() != OPERATOR_KEY_BYTES {
            return Err("local operator key must contain exactly 32 bytes".into());
        }
        if sha256_hex(&key) != self.key_sha256 {
            return Err("local operator key does not match the policy digest".into());
        }
        Ok(LocalOperatorAuthorization {
            operator_id: self.operator_id.clone(),
            estate_id,
            permission,
        })
    }
}

#[cfg(unix)]
fn validate_private_metadata(metadata: &std::fs::Metadata, kind: &str) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;
    if metadata.mode() & 0o077 != 0 {
        Err(format!(
            "local operator {kind} must not grant group/world permissions"
        ))
    } else {
        Ok(())
    }
}

#[cfg(not(unix))]
fn validate_private_metadata(_metadata: &std::fs::Metadata, _kind: &str) -> Result<(), String> {
    Ok(())
}

fn validate_sha256(value: &str) -> Result<(), String> {
    if value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        Ok(())
    } else {
        Err("local operator key SHA-256 is invalid".into())
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}
