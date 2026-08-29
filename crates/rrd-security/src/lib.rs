//! Persistent identity, deny-by-default authorization, and audit for RRD.
//!
//! This crate owns policy truth. RRD services enforce it; RRO provisions it;
//! Connectome only renders and administers it through authorized APIs.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use hmac::{Hmac, KeyInit, Mac};
use rrd_contract::{CanonicalId, ResourcePath};
use rrd_core::{digest, RuntimeValue};
use rrd_store::{ControlJournalEntry, ControlTransition, Engine};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

pub const SECURITY_FORMAT: u16 = 1;
pub const MAX_PRINCIPALS: usize = 4_096;
pub const MAX_ROLES: usize = 1_024;
pub const MAX_GRANTS_PER_PRINCIPAL: usize = 256;
pub const MAX_ROLE_DEPTH: usize = 16;
pub const MAX_IDENTITY_BINDINGS: usize = 4_096;
pub const MAX_JWT_ISSUERS: usize = 32;
pub const MAX_JWT_BYTES: usize = 16 * 1024;
pub const MAX_JWT_LIFETIME_MS: u64 = 60 * 60 * 1_000;
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
    /// Optional data-plane restriction compiled into query predicates and
    /// projection before physical planning. A caller that cannot enforce this
    /// context must deny the operation rather than treating it as a broad
    /// resource grant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_policy: Option<DataPolicy>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyPredicate {
    pub field: String,
    pub value: RuntimeValue,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataPolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenant: Option<PolicyPredicate>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rows: Vec<PolicyPredicate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_fields: Option<BTreeSet<String>>,
}

impl DataPolicy {
    pub fn validate(&self) -> Result<()> {
        let mut fields = BTreeSet::new();
        for predicate in self.tenant.iter().chain(&self.rows) {
            validate_policy_field(&predicate.field)?;
            validate_policy_value(&predicate.value)?;
            if !fields.insert(predicate.field.as_str()) {
                return Err(Error::Invalid(
                    "data policy predicate fields must be unique".into(),
                ));
            }
        }
        if let Some(allowed) = &self.allowed_fields {
            if allowed.is_empty() || allowed.len() > 256 {
                return Err(Error::Invalid(
                    "field policy must contain 1..=256 fields".into(),
                ));
            }
            for field in allowed {
                validate_policy_field(field)?;
            }
        }
        if self.tenant.is_none() && self.rows.is_empty() && self.allowed_fields.is_none() {
            return Err(Error::Invalid("empty data policy is not permitted".into()));
        }
        Ok(())
    }

    pub fn predicates(&self) -> impl Iterator<Item = &PolicyPredicate> {
        self.tenant.iter().chain(&self.rows)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Role {
    pub id: CanonicalId,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub inherits: BTreeSet<CanonicalId>,
    pub grants: Vec<ResourceGrant>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JwtIssuer {
    pub id: CanonicalId,
    pub issuer: String,
    pub audience: String,
    pub key_id: CanonicalId,
    pub signing_key_sha256: String,
    pub not_before_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub disabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentityBinding {
    pub id: CanonicalId,
    pub issuer: String,
    pub subject: String,
    pub audience: String,
    pub principal_id: CanonicalId,
    pub disabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Principal {
    pub id: CanonicalId,
    pub kind: PrincipalKind,
    pub credential_sha256: String,
    #[serde(default = "initial_credential_revision")]
    pub credential_revision: u64,
    pub not_before_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub disabled: bool,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub role_ids: BTreeSet<CanonicalId>,
    pub grants: Vec<ResourceGrant>,
}

impl Principal {
    pub fn validate(&self) -> Result<()> {
        validate_sha256(&self.credential_sha256)?;
        if self.credential_revision == 0
            || self.not_before_unix_ms == 0
            || self.not_before_unix_ms >= self.expires_at_unix_ms
            || (self.grants.is_empty() && self.role_ids.is_empty())
            || self.grants.len() > MAX_GRANTS_PER_PRINCIPAL
        {
            return Err(Error::Invalid(
                "principal validity or grant bounds are invalid".into(),
            ));
        }
        let mut identities = BTreeSet::new();
        for grant in &self.grants {
            validate_grant(grant)?;
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
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub roles: BTreeMap<CanonicalId, Role>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub identity_bindings: BTreeMap<CanonicalId, IdentityBinding>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub jwt_issuers: BTreeMap<CanonicalId, JwtIssuer>,
}

impl SecurityState {
    pub fn validate(&self) -> Result<()> {
        if self.format_version != SECURITY_FORMAT
            || self.revision == 0
            || self.principals.is_empty()
            || self.principals.len() > MAX_PRINCIPALS
            || self.roles.len() > MAX_ROLES
            || self.identity_bindings.len() > MAX_IDENTITY_BINDINGS
            || self.jwt_issuers.len() > MAX_JWT_ISSUERS
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
            if principal
                .role_ids
                .iter()
                .any(|role| !self.roles.contains_key(role))
            {
                return Err(Error::Invalid(
                    "principal references an unknown role".into(),
                ));
            }
        }
        validate_roles(self)?;
        validate_identity_bindings(self)?;
        validate_jwt_issuers(self)?;
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
    pub credential_revision: u64,
    pub data_policy: Option<DataPolicy>,
    pub authorization_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JwtCredential {
    pub token: String,
    pub expires_at_unix_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JwtIssueRequest {
    pub issuer_id: CanonicalId,
    pub token_id: CanonicalId,
    pub issued_at_unix_ms: u64,
    pub not_before_unix_ms: u64,
    pub expires_at_unix_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct JwtHeader {
    alg: String,
    typ: String,
    kid: CanonicalId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct JwtClaims {
    iss: String,
    sub: CanonicalId,
    aud: String,
    iat: u64,
    nbf: u64,
    exp: u64,
    jti: CanonicalId,
    policy_revision: u64,
    credential_revision: u64,
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

    /// Replaces the complete security authority with one compare-and-swap
    /// transition. Exact replay is a no-op; revision gaps, stale writers, and
    /// payload substitution fail closed.
    pub fn replace(
        &self,
        expected_revision: u64,
        replacement: SecurityState,
        at: u64,
        actor: &str,
        request_id: &str,
        operation_id: &str,
    ) -> Result<()> {
        replacement.validate()?;
        if replacement.revision != expected_revision.saturating_add(1) {
            return Err(Error::Invalid(
                "security replacement must advance exactly one revision".into(),
            ));
        }
        let key = security_key(&self.instance);
        let current_bytes = self
            .engine
            .control_record(&key)?
            .ok_or(Error::NotInitialized)?;
        let current = decode_state(&current_bytes)?;
        if current == replacement {
            return Ok(());
        }
        if current.revision != expected_revision {
            return Err(Error::IdempotencyConflict);
        }
        self.engine.commit_control_transition(&ControlTransition {
            key,
            expected: Some(current_bytes),
            replacement: Some(serde_json::to_vec(&replacement).map_err(json_error)?),
            at,
            actor: actor.into(),
            action: "security.replaced".into(),
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
        self.authenticate_principal(principal_id, credential)?;
        let state = self.load()?.ok_or(Error::NotInitialized)?;
        compile_authorization(&state, principal_id, action, resource, at)
    }

    /// Authenticates identity without granting an action. This split allows
    /// callers to retain a proven principal on a subsequent policy denial
    /// while never attributing a failed credential attempt to that principal.
    pub fn authenticate_principal(
        &self,
        principal_id: &CanonicalId,
        credential: &[u8],
    ) -> Result<()> {
        let state = self.load()?.ok_or(Error::NotInitialized)?;
        let principal = state
            .principals
            .get(principal_id)
            .ok_or(Error::PrincipalNotFound)?;
        let supplied = digest::sha256_hex(credential);
        if !constant_time_equal(supplied.as_bytes(), principal.credential_sha256.as_bytes()) {
            return Err(Error::Unauthenticated);
        }
        Ok(())
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
        compile_authorization(&state, principal_id, action, resource, at)
    }

    /// Resolves an identity assertion only after a transport adapter has
    /// cryptographically verified the third-party token. The exact
    /// issuer/subject/audience tuple is persisted here; unbound assertions are
    /// never treated as application principals.
    pub fn resolve_verified_identity(
        &self,
        issuer: &str,
        subject: &str,
        audience: &str,
        at: u64,
    ) -> Result<CanonicalId> {
        let state = self.load()?.ok_or(Error::NotInitialized)?;
        let binding = state
            .identity_bindings
            .values()
            .find(|binding| {
                !binding.disabled
                    && binding.issuer == issuer
                    && binding.subject == subject
                    && binding.audience == audience
            })
            .ok_or(Error::Unauthenticated)?;
        let principal = state
            .principals
            .get(&binding.principal_id)
            .ok_or(Error::PrincipalNotFound)?;
        require_active_principal(principal, at)?;
        Ok(principal.id.clone())
    }

    pub fn issue_jwt(
        &self,
        principal_id: &CanonicalId,
        credential: &[u8],
        signing_key: &[u8],
        request: &JwtIssueRequest,
    ) -> Result<JwtCredential> {
        self.authenticate_principal(principal_id, credential)?;
        let state = self.load()?.ok_or(Error::NotInitialized)?;
        let principal = state
            .principals
            .get(principal_id)
            .ok_or(Error::PrincipalNotFound)?;
        require_active_principal(principal, request.issued_at_unix_ms)?;
        let issuer = state
            .jwt_issuers
            .get(&request.issuer_id)
            .ok_or(Error::Unauthenticated)?;
        validate_jwt_window(issuer, request)?;
        if digest::sha256_hex(signing_key) != issuer.signing_key_sha256 {
            return Err(Error::Unauthenticated);
        }
        let header = JwtHeader {
            alg: "HS256".into(),
            typ: "JWT".into(),
            kid: issuer.key_id.clone(),
        };
        let claims = JwtClaims {
            iss: issuer.issuer.clone(),
            sub: principal.id.clone(),
            aud: issuer.audience.clone(),
            iat: request.issued_at_unix_ms,
            nbf: request.not_before_unix_ms,
            exp: request.expires_at_unix_ms,
            jti: request.token_id.clone(),
            policy_revision: state.revision,
            credential_revision: principal.credential_revision,
        };
        let header = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header).map_err(json_error)?);
        let claims = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).map_err(json_error)?);
        let signed = format!("{header}.{claims}");
        let signature = jwt_signature(signing_key, signed.as_bytes())?;
        let token = format!("{signed}.{}", URL_SAFE_NO_PAD.encode(signature));
        if token.len() > MAX_JWT_BYTES {
            return Err(Error::Invalid("issued JWT exceeds its byte bound".into()));
        }
        Ok(JwtCredential {
            token,
            expires_at_unix_ms: request.expires_at_unix_ms,
        })
    }

    /// Validates signature and registered claims against the current
    /// authority. Credential rotation or principal/issuer revocation rejects
    /// an otherwise cryptographically valid older token.
    pub fn authenticate_jwt(
        &self,
        token: &str,
        signing_key: &[u8],
        at: u64,
    ) -> Result<CanonicalId> {
        if token.is_empty() || token.len() > MAX_JWT_BYTES {
            return Err(Error::Unauthenticated);
        }
        let mut segments = token.split('.');
        let header_segment = segments.next().ok_or(Error::Unauthenticated)?;
        let claims_segment = segments.next().ok_or(Error::Unauthenticated)?;
        let signature_segment = segments.next().ok_or(Error::Unauthenticated)?;
        if segments.next().is_some() {
            return Err(Error::Unauthenticated);
        }
        let header: JwtHeader = decode_jwt_segment(header_segment)?;
        let claims: JwtClaims = decode_jwt_segment(claims_segment)?;
        if header.alg != "HS256" || header.typ != "JWT" {
            return Err(Error::Unauthenticated);
        }
        let state = self.load()?.ok_or(Error::NotInitialized)?;
        let issuer = state
            .jwt_issuers
            .values()
            .find(|issuer| issuer.key_id == header.kid)
            .ok_or(Error::Unauthenticated)?;
        if issuer.disabled
            || issuer.issuer != claims.iss
            || issuer.audience != claims.aud
            || at < issuer.not_before_unix_ms
            || at >= issuer.expires_at_unix_ms
            || digest::sha256_hex(signing_key) != issuer.signing_key_sha256
        {
            return Err(Error::Unauthenticated);
        }
        let signature = URL_SAFE_NO_PAD
            .decode(signature_segment)
            .map_err(|_| Error::Unauthenticated)?;
        let signed = format!("{header_segment}.{claims_segment}");
        verify_jwt_signature(signing_key, signed.as_bytes(), &signature)?;
        if claims.iat == 0
            || claims.nbf < claims.iat
            || claims.exp <= claims.nbf
            || claims.exp.saturating_sub(claims.iat) > MAX_JWT_LIFETIME_MS
            || at < claims.nbf
            || at >= claims.exp
            || claims.policy_revision > state.revision
        {
            return Err(Error::Unauthenticated);
        }
        let principal = state
            .principals
            .get(&claims.sub)
            .ok_or(Error::Unauthenticated)?;
        require_active_principal(principal, at).map_err(|_| Error::Unauthenticated)?;
        if claims.credential_revision != principal.credential_revision {
            return Err(Error::Unauthenticated);
        }
        Ok(principal.id.clone())
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

fn compile_authorization(
    state: &SecurityState,
    principal_id: &CanonicalId,
    action: Action,
    resource: &ResourcePath,
    at: u64,
) -> Result<Authorization> {
    let principal = state
        .principals
        .get(principal_id)
        .ok_or(Error::PrincipalNotFound)?;
    require_active_principal(principal, at)?;
    resource
        .validate()
        .map_err(|error| Error::Invalid(error.to_string()))?;
    let mut grants = principal.grants.iter().collect::<Vec<_>>();
    let mut visited = BTreeSet::new();
    for role in &principal.role_ids {
        collect_role_grants(state, role, &mut visited, &mut grants)?;
    }
    let specificity = grants
        .iter()
        .filter(|grant| {
            grant.action == action && resource_has_prefix(resource, &grant.resource_prefix)
        })
        .map(|grant| grant.resource_prefix.segments.len())
        .max()
        .ok_or(Error::PermissionDenied)?;
    let selected = grants
        .into_iter()
        .filter(|grant| {
            grant.action == action
                && grant.resource_prefix.segments.len() == specificity
                && resource_has_prefix(resource, &grant.resource_prefix)
        })
        .collect::<Vec<_>>();
    let data_policy = selected[0].data_policy.clone();
    if selected
        .iter()
        .any(|grant| grant.data_policy != data_policy)
    {
        return Err(Error::Invalid(
            "equally specific grants have ambiguous data policies".into(),
        ));
    }
    let authorization_sha256 = digest::sha256_hex(
        &serde_json::to_vec(&(
            state.revision,
            principal.id.as_str(),
            principal.credential_revision,
            action,
            resource,
            &data_policy,
        ))
        .map_err(json_error)?,
    );
    Ok(Authorization {
        principal_id: principal.id.clone(),
        principal_kind: principal.kind,
        action,
        resource: resource.clone(),
        policy_revision: state.revision,
        credential_revision: principal.credential_revision,
        data_policy,
        authorization_sha256,
    })
}

fn require_active_principal(principal: &Principal, at: u64) -> Result<()> {
    if principal.disabled || at < principal.not_before_unix_ms || at >= principal.expires_at_unix_ms
    {
        return Err(Error::PermissionDenied);
    }
    Ok(())
}

fn collect_role_grants<'a>(
    state: &'a SecurityState,
    role_id: &CanonicalId,
    visited: &mut BTreeSet<CanonicalId>,
    grants: &mut Vec<&'a ResourceGrant>,
) -> Result<()> {
    if !visited.insert(role_id.clone()) {
        return Ok(());
    }
    let role = state
        .roles
        .get(role_id)
        .ok_or_else(|| Error::Invalid("principal references an unknown role".into()))?;
    grants.extend(&role.grants);
    for inherited in &role.inherits {
        collect_role_grants(state, inherited, visited, grants)?;
    }
    Ok(())
}

fn resource_has_prefix(resource: &ResourcePath, prefix: &ResourcePath) -> bool {
    prefix.segments.len() <= resource.segments.len()
        && resource.segments[..prefix.segments.len()] == prefix.segments
}

fn validate_grant(grant: &ResourceGrant) -> Result<()> {
    grant
        .resource_prefix
        .validate()
        .map_err(|error| Error::Invalid(error.to_string()))?;
    if let Some(policy) = &grant.data_policy {
        policy.validate()?;
    }
    Ok(())
}

fn validate_roles(state: &SecurityState) -> Result<()> {
    for (id, role) in &state.roles {
        if id != &role.id
            || (role.inherits.is_empty() && role.grants.is_empty())
            || role.grants.len() > MAX_GRANTS_PER_PRINCIPAL
        {
            return Err(Error::Invalid(
                "role identity or grant bounds are invalid".into(),
            ));
        }
        let mut grants = BTreeSet::new();
        for grant in &role.grants {
            validate_grant(grant)?;
            let identity = serde_json::to_vec(grant).map_err(json_error)?;
            if !grants.insert(identity) {
                return Err(Error::Invalid("role grants must be unique".into()));
            }
        }
        if role
            .inherits
            .iter()
            .any(|inherited| inherited == id || !state.roles.contains_key(inherited))
        {
            return Err(Error::Invalid("role inheritance target is invalid".into()));
        }
        let mut visiting = BTreeSet::new();
        validate_role_depth(state, id, &mut visiting, 0)?;
    }
    Ok(())
}

fn validate_role_depth(
    state: &SecurityState,
    role_id: &CanonicalId,
    visiting: &mut BTreeSet<CanonicalId>,
    depth: usize,
) -> Result<()> {
    if depth >= MAX_ROLE_DEPTH || !visiting.insert(role_id.clone()) {
        return Err(Error::Invalid(
            "role inheritance is cyclic or exceeds its depth bound".into(),
        ));
    }
    let role = state
        .roles
        .get(role_id)
        .ok_or_else(|| Error::Invalid("role inheritance target is missing".into()))?;
    for inherited in &role.inherits {
        validate_role_depth(state, inherited, visiting, depth + 1)?;
    }
    visiting.remove(role_id);
    Ok(())
}

fn validate_identity_bindings(state: &SecurityState) -> Result<()> {
    let mut tuples = BTreeSet::new();
    for (id, binding) in &state.identity_bindings {
        if id != &binding.id
            || !state.principals.contains_key(&binding.principal_id)
            || !bounded_identity(&binding.issuer)
            || !bounded_identity(&binding.subject)
            || !bounded_identity(&binding.audience)
            || !tuples.insert((
                binding.issuer.as_str(),
                binding.subject.as_str(),
                binding.audience.as_str(),
            ))
        {
            return Err(Error::Invalid(
                "third-party identity binding is invalid".into(),
            ));
        }
    }
    Ok(())
}

fn validate_jwt_issuers(state: &SecurityState) -> Result<()> {
    let mut key_ids = BTreeSet::new();
    for (id, issuer) in &state.jwt_issuers {
        validate_sha256(&issuer.signing_key_sha256)?;
        if id != &issuer.id
            || !bounded_identity(&issuer.issuer)
            || !bounded_identity(&issuer.audience)
            || issuer.not_before_unix_ms == 0
            || issuer.not_before_unix_ms >= issuer.expires_at_unix_ms
            || !key_ids.insert(&issuer.key_id)
        {
            return Err(Error::Invalid("JWT issuer is invalid".into()));
        }
    }
    Ok(())
}

fn validate_jwt_window(issuer: &JwtIssuer, request: &JwtIssueRequest) -> Result<()> {
    if issuer.disabled
        || request.issued_at_unix_ms < issuer.not_before_unix_ms
        || request.issued_at_unix_ms >= issuer.expires_at_unix_ms
        || request.not_before_unix_ms < request.issued_at_unix_ms
        || request.expires_at_unix_ms <= request.not_before_unix_ms
        || request.expires_at_unix_ms > issuer.expires_at_unix_ms
        || request
            .expires_at_unix_ms
            .saturating_sub(request.issued_at_unix_ms)
            > MAX_JWT_LIFETIME_MS
    {
        return Err(Error::Invalid("JWT validity window is invalid".into()));
    }
    Ok(())
}

fn validate_policy_field(field: &str) -> Result<()> {
    if field.is_empty()
        || field.len() > 256
        || !field
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '.'))
    {
        return Err(Error::Invalid("security policy field is invalid".into()));
    }
    Ok(())
}

fn validate_policy_value(value: &RuntimeValue) -> Result<()> {
    if matches!(
        value,
        RuntimeValue::Null
            | RuntimeValue::Bool(_)
            | RuntimeValue::Integer(_)
            | RuntimeValue::Unsigned(_)
            | RuntimeValue::String(_)
    ) {
        Ok(())
    } else {
        Err(Error::Invalid(
            "security predicates require scalar query values".into(),
        ))
    }
}

fn bounded_identity(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 1_024 && !value.as_bytes().contains(&0)
}

fn initial_credential_revision() -> u64 {
    1
}

fn decode_jwt_segment<T>(segment: &str) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let bytes = URL_SAFE_NO_PAD
        .decode(segment)
        .map_err(|_| Error::Unauthenticated)?;
    serde_json::from_slice(&bytes).map_err(|_| Error::Unauthenticated)
}

fn jwt_signature(key: &[u8], value: &[u8]) -> Result<Vec<u8>> {
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(key)
        .map_err(|_| Error::Invalid("JWT signing key is invalid".into()))?;
    mac.update(value);
    Ok(mac.finalize().into_bytes().to_vec())
}

fn verify_jwt_signature(key: &[u8], value: &[u8], signature: &[u8]) -> Result<()> {
    let expected = jwt_signature(key, value)?;
    if constant_time_equal(&expected, signature) {
        Ok(())
    } else {
        Err(Error::Unauthenticated)
    }
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
