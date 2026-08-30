use super::{
    require_time, validate_ascii_key, validate_error, validate_id_key, validate_sha256, Error,
    EstateDocument, MutationContext, Result,
};
use rrd_contract::CanonicalId;
use rrd_core::digest;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const ESTATE_AUTHORITY_FORMAT: u16 = 1;
pub const MAX_AUTHORITY_RESOURCES: usize = 16_384;
pub const MAX_AUTHORITY_RECEIPTS: usize = 65_536;
pub const MAX_AUTHORITY_IDEMPOTENCY_BINDINGS: usize = 16_384;
const MAX_SECRET_REFERENCES_PER_RESOURCE: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorityResourceKind {
    Organisation,
    Account,
    Entitlement,
    Project,
    Environment,
    Instance,
    Node,
    Shard,
    Job,
    Assignment,
    Health,
    SecretReference,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorityStatus {
    Pending,
    Ready,
    Degraded,
    Failed,
    Retired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorityReceiptBoundary {
    DesiredAccepted,
    Assigned,
    Applied,
    Observed,
    Completed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorityDesiredState {
    pub generation: u64,
    pub spec_sha256: String,
    pub updated_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorityObservedState {
    pub generation: u64,
    pub status: AuthorityStatus,
    pub observed_at: u64,
    pub evidence_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// One provider-neutral estate identity. Resource kinds retain strict parent
/// rules while sharing the same monotonic desired/observed state contract.
/// Secret references contain only a content-addressed locator digest in
/// `spec_sha256`; secret material and raw locators never enter RRD.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorityResource {
    pub id: CanonicalId,
    pub kind: AuthorityResourceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<CanonicalId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secondary_parent_id: Option<CanonicalId>,
    pub name: String,
    pub spec_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub desired: Option<AuthorityDesiredState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed: Option<AuthorityObservedState>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub secret_reference_ids: Vec<CanonicalId>,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorityReceipt {
    pub id: CanonicalId,
    pub resource_id: CanonicalId,
    pub operation_id: CanonicalId,
    pub lease_epoch: u64,
    pub boundary: AuthorityReceiptBoundary,
    pub at: u64,
    pub evidence_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorityIdempotencyBinding {
    pub operation_id: CanonicalId,
    pub request_sha256: String,
    pub bound_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EstateAuthorityState {
    pub format: u16,
    pub resources: BTreeMap<String, AuthorityResource>,
    pub receipts: BTreeMap<String, AuthorityReceipt>,
    pub idempotency: BTreeMap<String, AuthorityIdempotencyBinding>,
}

impl Default for EstateAuthorityState {
    fn default() -> Self {
        Self {
            format: ESTATE_AUTHORITY_FORMAT,
            resources: BTreeMap::new(),
            receipts: BTreeMap::new(),
            idempotency: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ApplyAuthority {
    pub context: MutationContext,
    pub idempotency_key: String,
    pub resources: Vec<AuthorityResource>,
    pub receipts: Vec<AuthorityReceipt>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorityMutationOutcome {
    pub document: EstateDocument,
    pub idempotent_replay: bool,
}

impl EstateAuthorityState {
    pub fn validate(&self) -> Result<()> {
        if self.format != ESTATE_AUTHORITY_FORMAT {
            return Err(Error::Invalid("unsupported estate authority format".into()));
        }
        if self.resources.len() > MAX_AUTHORITY_RESOURCES
            || self.receipts.len() > MAX_AUTHORITY_RECEIPTS
            || self.idempotency.len() > MAX_AUTHORITY_IDEMPOTENCY_BINDINGS
        {
            return Err(Error::Invalid(
                "estate authority cardinality exceeded".into(),
            ));
        }
        for (key, resource) in &self.resources {
            validate_id_key(key, &resource.id)?;
            validate_resource_fields(resource)?;
            validate_resource_relationships(resource, &self.resources)?;
        }
        for (key, receipt) in &self.receipts {
            validate_id_key(key, &receipt.id)?;
            require_time(receipt.at)?;
            if receipt.lease_epoch == 0 {
                return Err(Error::Invalid(
                    "authority receipt lease epoch must be non-zero".into(),
                ));
            }
            validate_sha256(&receipt.evidence_sha256, "authority receipt evidence")?;
            if !self.resources.contains_key(receipt.resource_id.as_str()) {
                return Err(Error::Invalid(format!(
                    "authority receipt {} names an unknown resource",
                    receipt.id
                )));
            }
        }
        for (key, binding) in &self.idempotency {
            validate_ascii_key(key, "authority idempotency key")?;
            validate_sha256(&binding.request_sha256, "authority idempotency request")?;
            require_time(binding.bound_at)?;
        }
        Ok(())
    }

    pub fn resource(&self, id: &CanonicalId) -> Option<&AuthorityResource> {
        self.resources.get(id.as_str())
    }

    pub fn catalogue_sha256(&self) -> String {
        digest::sha256_hex(&serde_json::to_vec(self).expect("validated authority state serializes"))
    }

    pub(crate) fn apply_batch(&mut self, request: &ApplyAuthority) -> Result<()> {
        if request.resources.is_empty() && request.receipts.is_empty() {
            return Err(Error::Invalid(
                "authority mutation must contain a resource or receipt".into(),
            ));
        }
        let mut resource_ids = BTreeSet::new();
        for resource in &request.resources {
            if !resource_ids.insert(resource.id.clone()) {
                return Err(Error::Invalid(
                    "authority mutation repeats a resource id".into(),
                ));
            }
            self.upsert_resource(resource.clone())?;
        }
        let mut receipt_ids = BTreeSet::new();
        for receipt in &request.receipts {
            if !receipt_ids.insert(receipt.id.clone()) {
                return Err(Error::Invalid(
                    "authority mutation repeats a receipt id".into(),
                ));
            }
            match self.receipts.get(receipt.id.as_str()) {
                Some(existing) if existing == receipt => {}
                Some(_) => {
                    return Err(Error::Invalid(
                        "authority receipt identity was rebound".into(),
                    ))
                }
                None => {
                    self.receipts
                        .insert(receipt.id.to_string(), receipt.clone());
                }
            }
        }
        self.validate()
    }

    fn upsert_resource(&mut self, resource: AuthorityResource) -> Result<()> {
        validate_resource_fields(&resource)?;
        if let Some(previous) = self.resources.get(resource.id.as_str()) {
            if previous.kind != resource.kind
                || previous.parent_id != resource.parent_id
                || previous.secondary_parent_id != resource.secondary_parent_id
                || previous.created_at != resource.created_at
            {
                return Err(Error::Invalid(
                    "authority resource identity or ownership was rebound".into(),
                ));
            }
            if resource.updated_at < previous.updated_at {
                return Err(Error::Invalid(
                    "authority resource update moved backwards in time".into(),
                ));
            }
            validate_state_advance(
                previous.desired.as_ref(),
                resource.desired.as_ref(),
                "desired",
            )?;
            validate_observation_advance(previous.observed.as_ref(), resource.observed.as_ref())?;
        } else if self.resources.len() == MAX_AUTHORITY_RESOURCES {
            return Err(Error::Invalid(
                "estate authority resource limit exceeded".into(),
            ));
        }
        self.resources.insert(resource.id.to_string(), resource);
        Ok(())
    }
}

pub(crate) fn authority_request_sha256(request: &ApplyAuthority) -> String {
    digest::sha256_hex(
        &serde_json::to_vec(&(
            &request.idempotency_key,
            &request.resources,
            &request.receipts,
        ))
        .expect("authority request fields serialize"),
    )
}

fn validate_resource_fields(resource: &AuthorityResource) -> Result<()> {
    validate_label(&resource.name)?;
    validate_sha256(&resource.spec_sha256, "authority resource specification")?;
    require_time(resource.created_at)?;
    require_time(resource.updated_at)?;
    if resource.updated_at < resource.created_at {
        return Err(Error::Invalid(
            "authority resource updated_at precedes created_at".into(),
        ));
    }
    if resource.secret_reference_ids.len() > MAX_SECRET_REFERENCES_PER_RESOURCE {
        return Err(Error::Invalid(
            "authority resource secret reference limit exceeded".into(),
        ));
    }
    let mut references = BTreeSet::new();
    if resource
        .secret_reference_ids
        .iter()
        .any(|id| !references.insert(id))
    {
        return Err(Error::Invalid(
            "authority resource repeats a secret reference".into(),
        ));
    }
    if let Some(desired) = &resource.desired {
        validate_desired(desired)?;
        if desired.updated_at > resource.updated_at {
            return Err(Error::Invalid(
                "authority desired state is newer than its resource".into(),
            ));
        }
    }
    if let Some(observed) = &resource.observed {
        validate_observed(observed)?;
        if observed.observed_at > resource.updated_at {
            return Err(Error::Invalid(
                "authority observation is newer than its resource".into(),
            ));
        }
        if resource
            .desired
            .as_ref()
            .is_some_and(|desired| observed.generation > desired.generation)
        {
            return Err(Error::Invalid(
                "authority observed generation exceeds desired generation".into(),
            ));
        }
    }
    if matches!(
        resource.kind,
        AuthorityResourceKind::Node
            | AuthorityResourceKind::Shard
            | AuthorityResourceKind::Job
            | AuthorityResourceKind::Assignment
    ) && resource.desired.is_none()
    {
        return Err(Error::Invalid(
            "operational authority resource has no desired state".into(),
        ));
    }
    if resource.kind == AuthorityResourceKind::Health && resource.observed.is_none() {
        return Err(Error::Invalid("health resource has no observation".into()));
    }
    if matches!(
        resource.kind,
        AuthorityResourceKind::Health | AuthorityResourceKind::SecretReference
    ) && resource.desired.is_some()
    {
        return Err(Error::Invalid(
            "health and secret-reference resources cannot carry desired state".into(),
        ));
    }
    Ok(())
}

fn validate_resource_relationships(
    resource: &AuthorityResource,
    resources: &BTreeMap<String, AuthorityResource>,
) -> Result<()> {
    let expected_parent = match resource.kind {
        AuthorityResourceKind::Organisation => None,
        AuthorityResourceKind::Account => Some(AuthorityResourceKind::Organisation),
        AuthorityResourceKind::Entitlement | AuthorityResourceKind::Project => {
            Some(AuthorityResourceKind::Account)
        }
        AuthorityResourceKind::Environment => Some(AuthorityResourceKind::Project),
        AuthorityResourceKind::Instance
        | AuthorityResourceKind::Node
        | AuthorityResourceKind::Job
        | AuthorityResourceKind::SecretReference => Some(AuthorityResourceKind::Environment),
        AuthorityResourceKind::Shard => Some(AuthorityResourceKind::Instance),
        AuthorityResourceKind::Assignment => Some(AuthorityResourceKind::Job),
        AuthorityResourceKind::Health => None,
    };
    match (expected_parent, resource.parent_id.as_ref()) {
        (None, None) if resource.kind == AuthorityResourceKind::Organisation => {}
        (Some(kind), Some(parent_id)) => require_kind(resources, parent_id, kind, "parent")?,
        (None, Some(parent_id)) if resource.kind == AuthorityResourceKind::Health => {
            let parent = resources.get(parent_id.as_str()).ok_or_else(|| {
                Error::Invalid(format!("health {} has an unknown parent", resource.id))
            })?;
            if matches!(
                parent.kind,
                AuthorityResourceKind::Health | AuthorityResourceKind::SecretReference
            ) {
                return Err(Error::Invalid(
                    "health must describe a non-health estate resource".into(),
                ));
            }
        }
        _ => {
            return Err(Error::Invalid(format!(
                "authority resource {} has an invalid parent",
                resource.id
            )))
        }
    }
    match resource.kind {
        AuthorityResourceKind::Assignment => {
            let node = resource
                .secondary_parent_id
                .as_ref()
                .ok_or_else(|| Error::Invalid("assignment has no node secondary parent".into()))?;
            require_kind(
                resources,
                node,
                AuthorityResourceKind::Node,
                "secondary parent",
            )?;
        }
        _ if resource.secondary_parent_id.is_some() => {
            return Err(Error::Invalid(
                "only assignments may have a secondary parent".into(),
            ))
        }
        _ => {}
    }
    for secret_id in &resource.secret_reference_ids {
        require_kind(
            resources,
            secret_id,
            AuthorityResourceKind::SecretReference,
            "secret reference",
        )?;
    }
    if matches!(
        resource.kind,
        AuthorityResourceKind::Health | AuthorityResourceKind::SecretReference
    ) && !resource.secret_reference_ids.is_empty()
    {
        return Err(Error::Invalid(
            "health and secret-reference resources cannot reference secrets".into(),
        ));
    }
    Ok(())
}

fn require_kind(
    resources: &BTreeMap<String, AuthorityResource>,
    id: &CanonicalId,
    expected: AuthorityResourceKind,
    relationship: &str,
) -> Result<()> {
    let resource = resources
        .get(id.as_str())
        .ok_or_else(|| Error::Invalid(format!("authority {relationship} {id} is unknown")))?;
    if resource.kind != expected {
        return Err(Error::Invalid(format!(
            "authority {relationship} {id} has kind {:?}, expected {expected:?}",
            resource.kind
        )));
    }
    Ok(())
}

fn validate_desired(state: &AuthorityDesiredState) -> Result<()> {
    if state.generation == 0 {
        return Err(Error::Invalid(
            "authority desired generation must be non-zero".into(),
        ));
    }
    validate_sha256(&state.spec_sha256, "authority desired specification")?;
    require_time(state.updated_at)
}

fn validate_observed(state: &AuthorityObservedState) -> Result<()> {
    validate_sha256(&state.evidence_sha256, "authority observation evidence")?;
    require_time(state.observed_at)?;
    validate_error(state.error.as_deref())
}

fn validate_state_advance(
    previous: Option<&AuthorityDesiredState>,
    next: Option<&AuthorityDesiredState>,
    name: &str,
) -> Result<()> {
    match (previous, next) {
        (Some(_), None) => Err(Error::Invalid(format!(
            "authority {name} state cannot be removed"
        ))),
        (Some(previous), Some(next)) if next.generation < previous.generation => Err(
            Error::Invalid(format!("authority {name} generation moved backwards")),
        ),
        (Some(previous), Some(next))
            if next.generation == previous.generation && next != previous =>
        {
            Err(Error::Invalid(format!(
                "authority {name} generation was rebound"
            )))
        }
        _ => Ok(()),
    }
}

fn validate_observation_advance(
    previous: Option<&AuthorityObservedState>,
    next: Option<&AuthorityObservedState>,
) -> Result<()> {
    match (previous, next) {
        (Some(_), None) => Err(Error::Invalid(
            "authority observed state cannot be removed".into(),
        )),
        (Some(previous), Some(next)) if next.generation < previous.generation => Err(
            Error::Invalid("authority observed generation moved backwards".into()),
        ),
        (Some(previous), Some(next))
            if next.generation == previous.generation
                && next.observed_at <= previous.observed_at
                && next != previous =>
        {
            Err(Error::Invalid(
                "authority observation did not advance time or generation".into(),
            ))
        }
        _ => Ok(()),
    }
}

fn validate_label(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 256
        || value
            .bytes()
            .any(|byte| byte == 0 || (!byte.is_ascii_graphic() && byte != b' '))
    {
        return Err(Error::Invalid("authority resource name is invalid".into()));
    }
    Ok(())
}
