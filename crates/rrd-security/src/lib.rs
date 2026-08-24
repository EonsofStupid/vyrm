//! Persistent identity, deny-by-default authorization, and audit for RRD.
//!
//! This crate owns policy truth. RRD services enforce it; RRO provisions it;
//! Connectome only renders and administers it through authorized APIs.

use rrd_contract::{CanonicalId, ResourcePath};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use vyrm_core::digest;
use rrd_store::{ControlJournalEntry, ControlTransition, Engine};

pub const SECURITY_FORMAT: u16 = 1;
pub const MAX_PRINCIPALS: usize = 4_096;
pub const MAX_GRANTS_PER_PRINCIPAL: usize = 256;
pub const MAX_AUDIT_PAGE: usize = 1_024;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Store(rrd_store::Error),
    Invalid(String),
    AlreadyInitialized,
    NotInitialized,
    PrincipalNotFound,
    Unauthenticated,
    PermissionDenied,
    IdempotencyConflict,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(error) => write!(formatter, "security storage failed: {error}"),
            Self::Invalid(message) => write!(formatter, "invalid security state: {message}"),
            Self::AlreadyInitialized => formatter.write_str("security state already initialized"),
            Self::NotInitialized => formatter.write_str("security state is not initialized"),
            Self::PrincipalNotFound => formatter.write_str("security principal not found"),
            Self::Unauthenticated => formatter.write_str("principal credential was rejected"),
            Self::PermissionDenied => formatter.write_str("policy denied the requested action"),
            Self::IdempotencyConflict => formatter.write_str("security identity was rebound"),
        }
    }
}

impl std::error::Error for Error {}

impl From<rrd_store::Error> for Error {
    fn from(value: rrd_store::Error) -> Self {
        Self::Store(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalKind {
    User,
    Service,
    Node,
}

pub use rrd_contract::SecurityAction as Action;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceGrant {
    pub action: Action,
    /// Exact canonical resource prefix. An empty path grants no resource and is
    /// rejected; wildcard strings are never interpreted.
    pub resource_prefix: ResourcePath,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Principal {
    pub id: CanonicalId,
    pub kind: PrincipalKind,
    pub credential_sha256: String,
    pub not_before_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub disabled: bool,
    pub grants: Vec<ResourceGrant>,
}

impl Principal {
    pub fn validate(&self) -> Result<()> {
        validate_sha256(&self.credential_sha256)?;
        if self.not_before_unix_ms == 0
            || self.not_before_unix_ms >= self.expires_at_unix_ms
            || self.grants.is_empty()
            || self.grants.len() > MAX_GRANTS_PER_PRINCIPAL
        {
            return Err(Error::Invalid(
                "principal validity or grant bounds are invalid".into(),
            ));
        }
        let mut identities = BTreeSet::new();
        for grant in &self.grants {
            grant
                .resource_prefix
                .validate()
                .map_err(|error| Error::Invalid(error.to_string()))?;
            let identity =
                serde_json::to_vec(grant).map_err(|error| Error::Invalid(error.to_string()))?;
            if !identities.insert(identity) {
                return Err(Error::Invalid("principal grants must be unique".into()));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityState {
    pub format_version: u16,
    pub revision: u64,
    pub principals: BTreeMap<CanonicalId, Principal>,
}

impl SecurityState {
    pub fn validate(&self) -> Result<()> {
        if self.format_version != SECURITY_FORMAT
            || self.revision == 0
            || self.principals.is_empty()
            || self.principals.len() > MAX_PRINCIPALS
        {
            return Err(Error::Invalid(
                "unsupported, empty, or oversized security state".into(),
            ));
        }
        for (id, principal) in &self.principals {
            principal.validate()?;
            if id != &principal.id {
                return Err(Error::Invalid("principal map identity differs".into()));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Authorization {
    pub principal_id: CanonicalId,
    pub principal_kind: PrincipalKind,
    pub action: Action,
    pub resource: ResourcePath,
    pub policy_revision: u64,
}

pub use rrd_contract::{AuditDecision, AuditPhase};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditRecord {
    pub audit_id: CanonicalId,
    pub at_unix_ms: u64,
    pub principal_id: Option<CanonicalId>,
    pub action: Action,
    pub resource: ResourcePath,
    pub request_id: String,
    pub operation_id: String,
    pub phase: AuditPhase,
    pub decision: AuditDecision,
    pub status_code: u16,
    pub request_sha256: String,
    pub response_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditJournalPage {
    pub through_sequence: u64,
    pub records: Vec<(u64, AuditRecord)>,
}

impl AuditRecord {
    pub fn validate(&self) -> Result<()> {
        self.resource
            .validate()
            .map_err(|error| Error::Invalid(error.to_string()))?;
        validate_sha256(&self.request_sha256)?;
        validate_sha256(&self.response_sha256)?;
        if self.at_unix_ms == 0
            || self.request_id.is_empty()
            || self.operation_id.is_empty()
            || self.request_id.len() > 256
            || self.operation_id.len() > 256
            || !self.request_id.is_ascii()
            || !self.operation_id.is_ascii()
            || !(100..=599).contains(&self.status_code)
            || (self.phase == AuditPhase::Authorized
                && (self.decision != AuditDecision::Allowed || self.status_code != 100))
            || (self.phase == AuditPhase::Completed && self.status_code < 200)
        {
            return Err(Error::Invalid("audit coordinates are invalid".into()));
        }
        Ok(())
    }
}

pub struct SecurityRepository<'a, E> {
    engine: &'a E,
    instance: CanonicalId,
}

impl<'a, E: Engine> SecurityRepository<'a, E> {
    pub fn new(engine: &'a E, instance: CanonicalId) -> Self {
        Self { engine, instance }
    }

    pub fn load(&self) -> Result<Option<SecurityState>> {
        self.engine
            .control_record(&security_key(&self.instance))?
            .map(|bytes| decode_state(&bytes))
            .transpose()
    }

    pub fn initialize(
        &self,
        state: SecurityState,
        at: u64,
        actor: &str,
        request_id: &str,
        operation_id: &str,
    ) -> Result<()> {
        state.validate()?;
        if self.load()?.is_some() {
            return Err(Error::AlreadyInitialized);
        }
        self.engine.commit_control_transition(&ControlTransition {
            key: security_key(&self.instance),
            expected: None,
            replacement: Some(serde_json::to_vec(&state).map_err(json_error)?),
            at,
            actor: actor.into(),
            action: "security.initialized".into(),
            request_id: request_id.into(),
            operation_id: operation_id.into(),
        })?;
        Ok(())
    }

    pub fn authenticate_and_authorize(
        &self,
        principal_id: &CanonicalId,
        credential: &[u8],
        action: Action,
        resource: &ResourcePath,
        at: u64,
    ) -> Result<Authorization> {
        let state = self.load()?.ok_or(Error::NotInitialized)?;
        let principal = state
            .principals
            .get(principal_id)
            .ok_or(Error::PrincipalNotFound)?;
        let supplied = digest::sha256_hex(credential);
        if !constant_time_equal(supplied.as_bytes(), principal.credential_sha256.as_bytes()) {
            return Err(Error::Unauthenticated);
        }
        authorize(&state, principal, action, resource, at)
    }

    /// Re-evaluates current policy for a principal whose credential was
    /// authenticated when its short-lived RRD session was created.
    pub fn authorize_principal(
        &self,
        principal_id: &CanonicalId,
        action: Action,
        resource: &ResourcePath,
        at: u64,
    ) -> Result<Authorization> {
        let state = self.load()?.ok_or(Error::NotInitialized)?;
        let principal = state
            .principals
            .get(principal_id)
            .ok_or(Error::PrincipalNotFound)?;
        authorize(&state, principal, action, resource, at)
    }

    pub fn is_initialized(&self) -> Result<bool> {
        Ok(self.load()?.is_some())
    }

    pub fn append_audit(&self, record: &AuditRecord) -> Result<()> {
        record.validate()?;
        let bytes = serde_json::to_vec(record).map_err(json_error)?;
        let key = audit_key(&self.instance, &record.audit_id);
        if let Some(existing) = self.engine.control_record(&key)? {
            return if existing == bytes {
                Ok(())
            } else {
                Err(Error::IdempotencyConflict)
            };
        }
        self.engine.commit_control_transition(&ControlTransition {
            key,
            expected: None,
            replacement: Some(bytes),
            at: record.at_unix_ms,
            actor: record
                .principal_id
                .as_ref()
                .map_or("anonymous", CanonicalId::as_str)
                .into(),
            action: "security.audit".into(),
            request_id: record.request_id.clone(),
            operation_id: record.operation_id.clone(),
        })?;
        Ok(())
    }

    pub fn audit_since(&self, after: u64, limit: usize) -> Result<AuditJournalPage> {
        if limit == 0 || limit > MAX_AUDIT_PAGE {
            return Err(Error::Invalid("audit page limit is outside bounds".into()));
        }
        let mut cursor = after;
        let mut records = Vec::new();
        while records.len() < limit {
            let entries = self.engine.control_journal_since(cursor, limit)?;
            if entries.is_empty() {
                break;
            }
            for entry in &entries {
                cursor = entry.sequence;
                if entry.action == "security.audit" {
                    records.push((entry.sequence, decode_audit(entry)?));
                    if records.len() == limit {
                        break;
                    }
                }
            }
            if entries.len() < limit {
                break;
            }
        }
        Ok(AuditJournalPage {
            through_sequence: cursor,
            records,
        })
    }
}

fn authorize(
    state: &SecurityState,
    principal: &Principal,
    action: Action,
    resource: &ResourcePath,
    at: u64,
) -> Result<Authorization> {
    if principal.disabled || at < principal.not_before_unix_ms || at >= principal.expires_at_unix_ms
    {
        return Err(Error::PermissionDenied);
    }
    resource
        .validate()
        .map_err(|error| Error::Invalid(error.to_string()))?;
    if !principal.grants.iter().any(|grant| {
        grant.action == action && resource_has_prefix(resource, &grant.resource_prefix)
    }) {
        return Err(Error::PermissionDenied);
    }
    Ok(Authorization {
        principal_id: principal.id.clone(),
        principal_kind: principal.kind,
        action,
        resource: resource.clone(),
        policy_revision: state.revision,
    })
}

fn resource_has_prefix(resource: &ResourcePath, prefix: &ResourcePath) -> bool {
    prefix.segments.len() <= resource.segments.len()
        && resource.segments[..prefix.segments.len()] == prefix.segments
}

fn decode_state(bytes: &[u8]) -> Result<SecurityState> {
    let state: SecurityState = serde_json::from_slice(bytes).map_err(json_error)?;
    state.validate()?;
    Ok(state)
}

fn decode_audit(entry: &ControlJournalEntry) -> Result<AuditRecord> {
    let bytes = entry
        .replacement
        .as_deref()
        .ok_or_else(|| Error::Invalid("audit journal entry deleted its record".into()))?;
    let record: AuditRecord = serde_json::from_slice(bytes).map_err(json_error)?;
    record.validate()?;
    Ok(record)
}

fn security_key(instance: &CanonicalId) -> String {
    format!("server/state/{instance}/security/policy")
}

fn audit_key(instance: &CanonicalId, audit_id: &CanonicalId) -> String {
    format!("server/state/{instance}/audit/{audit_id}")
}

fn validate_sha256(value: &str) -> Result<()> {
    if value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        Ok(())
    } else {
        Err(Error::Invalid(
            "SHA-256 must be lowercase hexadecimal".into(),
        ))
    }
}

fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

fn json_error(error: serde_json::Error) -> Error {
    Error::Invalid(error.to_string())
}
