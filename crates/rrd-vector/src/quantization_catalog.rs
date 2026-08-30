//! Append-only lifecycle model for immutable quantization artifacts.

use crate::contract::invalid;
use crate::{VectorArtifact, VectorArtifactKind, VectorProjectionDescriptor};
use rrd_core::{
    digest, ObjectReference, ProjectionId, ProjectionState, Result, RuntimeRef, ScopeId,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const QUANTIZATION_CATALOG_VERSION: u16 = 1;
pub const QUANTIZATION_ARTIFACT_RECORD_TYPE: &str = "vector_quantization_artifact";
pub const QUANTIZATION_LIFECYCLE_RECORD_TYPE: &str = "vector_quantization_lifecycle";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuantizationArtifactState {
    Ready,
    Active,
    Retired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuantizationLifecycleAction {
    Build,
    Activate,
    Retire,
}

/// Immutable identity and object binding established by `build`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuantizationArtifactEntry {
    pub contract_version: u16,
    pub collection_id: ProjectionId,
    pub vector_name: ProjectionId,
    pub kind: VectorArtifactKind,
    pub descriptor: VectorProjectionDescriptor,
    pub object: ObjectReference,
    pub built_at: u64,
    pub entry_digest: String,
}

impl QuantizationArtifactEntry {
    pub fn record_reference(descriptor: &VectorProjectionDescriptor) -> Result<RuntimeRef> {
        RuntimeRef::new(
            QUANTIZATION_ARTIFACT_RECORD_TYPE,
            format!(
                "{}@{}",
                descriptor.stamp().id,
                descriptor.stamp().generation
            ),
        )
    }

    pub fn new(
        collection_id: ProjectionId,
        vector_name: ProjectionId,
        kind: VectorArtifactKind,
        descriptor: VectorProjectionDescriptor,
        object: ObjectReference,
        built_at: u64,
    ) -> Result<Self> {
        let mut entry = Self {
            contract_version: QUANTIZATION_CATALOG_VERSION,
            collection_id,
            vector_name,
            kind,
            descriptor,
            object,
            built_at,
            entry_digest: String::new(),
        };
        entry.validate_components()?;
        entry.entry_digest = digest::sha256_hex(&entry.identity_bytes()?);
        Ok(entry)
    }

    pub fn validate(&self) -> Result<()> {
        self.validate_components()?;
        if self.entry_digest != digest::sha256_hex(&self.identity_bytes()?) {
            return invalid("quantization artifact entry digest does not match its fields");
        }
        Ok(())
    }

    pub fn scope(&self) -> &ScopeId {
        self.descriptor.scope()
    }

    pub fn decode_artifact(&self, bytes: &[u8]) -> Result<VectorArtifact> {
        self.validate()?;
        if bytes.len() as u64 != self.object.length
            || digest::sha256_hex(bytes) != self.object.sha256
        {
            return invalid("quantization artifact bytes differ from the object reference");
        }
        let artifact = VectorArtifact::from_bytes(self.kind, bytes)?;
        if artifact.descriptor() != self.descriptor {
            return invalid("quantization artifact differs from its lifecycle descriptor");
        }
        Ok(artifact)
    }

    fn validate_components(&self) -> Result<()> {
        if self.contract_version != QUANTIZATION_CATALOG_VERSION || self.built_at == 0 {
            return invalid("quantization artifact contract version or build time is invalid");
        }
        self.descriptor.validate()?;
        if self.descriptor.stamp().state != ProjectionState::Ready
            || !matches!(
                self.kind,
                VectorArtifactKind::ScalarQuantized
                    | VectorArtifactKind::ProductQuantized
                    | VectorArtifactKind::BinaryQuantized
                    | VectorArtifactKind::TurboQuant
            )
        {
            return invalid("quantization lifecycle accepts only ready quantized artifacts");
        }
        let kind_matches = match (&self.descriptor, self.kind) {
            (
                VectorProjectionDescriptor::Quantized { descriptor },
                VectorArtifactKind::ScalarQuantized,
            ) => descriptor.method == crate::QuantizationMethod::Scalar,
            (
                VectorProjectionDescriptor::Quantized { descriptor },
                VectorArtifactKind::ProductQuantized,
            ) => matches!(descriptor.method, crate::QuantizationMethod::Product { .. }),
            (
                VectorProjectionDescriptor::Quantized { descriptor },
                VectorArtifactKind::BinaryQuantized,
            ) => descriptor.method == crate::QuantizationMethod::Binary,
            (VectorProjectionDescriptor::TurboQuant { .. }, VectorArtifactKind::TurboQuant) => true,
            _ => false,
        };
        if !kind_matches {
            return invalid("quantization artifact kind differs from its descriptor method");
        }
        self.object.validate()?;
        if self.object.media_type != self.kind.media_type()
            || self.object.subject.as_ref() != Some(&Self::record_reference(&self.descriptor)?)
        {
            return invalid("quantization object binding differs from the artifact identity");
        }
        Ok(())
    }

    fn identity_bytes(&self) -> Result<Vec<u8>> {
        serde_json::to_vec(&(
            self.contract_version,
            &self.collection_id,
            &self.vector_name,
            self.kind,
            &self.descriptor,
            &self.object,
            self.built_at,
        ))
        .map_err(|error| rrd_core::Error::InvalidRuntime {
            reason: format!("quantization artifact identity cannot be encoded: {error}"),
        })
    }
}

/// One append-only state transition. The digest chain makes deletion,
/// reordering, or substitution of lifecycle decisions detectable at reopen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuantizationLifecycleEvent {
    pub contract_version: u16,
    pub revision: u64,
    pub projection_id: ProjectionId,
    pub generation: u64,
    pub action: QuantizationLifecycleAction,
    pub at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_event_digest: Option<String>,
    pub event_digest: String,
}

impl QuantizationLifecycleEvent {
    pub fn record_reference(revision: u64) -> Result<RuntimeRef> {
        RuntimeRef::new(
            QUANTIZATION_LIFECYCLE_RECORD_TYPE,
            format!("revision-{revision:020}"),
        )
    }

    pub fn new(
        revision: u64,
        projection_id: ProjectionId,
        generation: u64,
        action: QuantizationLifecycleAction,
        at: u64,
        previous_event_digest: Option<String>,
    ) -> Result<Self> {
        let mut event = Self {
            contract_version: QUANTIZATION_CATALOG_VERSION,
            revision,
            projection_id,
            generation,
            action,
            at,
            previous_event_digest,
            event_digest: String::new(),
        };
        event.validate_components()?;
        event.event_digest = digest::sha256_hex(&event.identity_bytes()?);
        Ok(event)
    }

    pub fn validate(&self) -> Result<()> {
        self.validate_components()?;
        if self.event_digest != digest::sha256_hex(&self.identity_bytes()?) {
            return invalid("quantization lifecycle event digest does not match its fields");
        }
        Ok(())
    }

    fn validate_components(&self) -> Result<()> {
        if self.contract_version != QUANTIZATION_CATALOG_VERSION
            || self.revision == 0
            || self.generation == 0
            || self.at == 0
            || self
                .previous_event_digest
                .as_ref()
                .is_some_and(|digest| !valid_digest(digest))
        {
            return invalid("quantization lifecycle event coordinates are invalid");
        }
        Ok(())
    }

    fn identity_bytes(&self) -> Result<Vec<u8>> {
        serde_json::to_vec(&(
            self.contract_version,
            self.revision,
            &self.projection_id,
            self.generation,
            self.action,
            self.at,
            &self.previous_event_digest,
        ))
        .map_err(|error| rrd_core::Error::InvalidRuntime {
            reason: format!("quantization lifecycle event cannot be encoded: {error}"),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuantizationArtifactSnapshot {
    pub entry: QuantizationArtifactEntry,
    pub state: QuantizationArtifactState,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QuantizationArtifactCatalogue {
    pub revision: u64,
    pub last_event_digest: Option<String>,
    pub artifacts: BTreeMap<(ProjectionId, u64), QuantizationArtifactSnapshot>,
    pub active: BTreeMap<ProjectionId, u64>,
    built: BTreeSet<(ProjectionId, u64)>,
    last_activated: BTreeMap<ProjectionId, u64>,
}

impl QuantizationArtifactCatalogue {
    pub fn reconstruct(
        entries: impl IntoIterator<Item = QuantizationArtifactEntry>,
        mut events: Vec<QuantizationLifecycleEvent>,
    ) -> Result<Self> {
        let mut catalogue = Self::default();
        for entry in entries {
            entry.validate()?;
            let key = (
                entry.descriptor.stamp().id.clone(),
                entry.descriptor.stamp().generation,
            );
            if catalogue
                .artifacts
                .insert(
                    key,
                    QuantizationArtifactSnapshot {
                        entry,
                        state: QuantizationArtifactState::Ready,
                    },
                )
                .is_some()
            {
                return invalid("duplicate quantization artifact generation");
            }
        }
        events.sort_by_key(|event| event.revision);
        for event in events {
            catalogue.apply(&event)?;
        }
        if catalogue.built.len() != catalogue.artifacts.len() {
            return invalid("quantization artifact is missing its build lifecycle event");
        }
        Ok(catalogue)
    }

    pub fn next_event(
        &self,
        projection_id: ProjectionId,
        generation: u64,
        action: QuantizationLifecycleAction,
        at: u64,
    ) -> Result<QuantizationLifecycleEvent> {
        QuantizationLifecycleEvent::new(
            self.revision
                .checked_add(1)
                .ok_or_else(|| rrd_core::Error::InvalidRuntime {
                    reason: "quantization lifecycle revision overflowed".into(),
                })?,
            projection_id,
            generation,
            action,
            at,
            self.last_event_digest.clone(),
        )
    }

    pub fn apply(&mut self, event: &QuantizationLifecycleEvent) -> Result<()> {
        event.validate()?;
        if event.revision != self.revision + 1
            || event.previous_event_digest != self.last_event_digest
        {
            return invalid("quantization lifecycle revision or digest chain is discontinuous");
        }
        let key = (event.projection_id.clone(), event.generation);
        match event.action {
            QuantizationLifecycleAction::Build => {
                let artifact =
                    self.artifacts
                        .get(&key)
                        .ok_or_else(|| rrd_core::Error::InvalidRuntime {
                            reason: "quantization build event has no immutable artifact".into(),
                        })?;
                if artifact.state != QuantizationArtifactState::Ready {
                    return invalid("quantization artifact build is not in ready state");
                }
                let expected_generation = self
                    .built
                    .iter()
                    .filter(|(id, _)| id == &event.projection_id)
                    .map(|(_, generation)| *generation)
                    .max()
                    .unwrap_or(0)
                    + 1;
                if event.generation != expected_generation {
                    return invalid("quantization build generations are not contiguous");
                }
                if !self.built.insert(key) {
                    return invalid("quantization artifact has duplicate build lifecycle events");
                }
            }
            QuantizationLifecycleAction::Activate => {
                if !self.built.contains(&key) {
                    return invalid("quantization artifact must be built before activation");
                }
                let target =
                    self.artifacts
                        .get(&key)
                        .ok_or_else(|| rrd_core::Error::InvalidRuntime {
                            reason: "quantization activation target is absent".into(),
                        })?;
                if target.state != QuantizationArtifactState::Ready {
                    return invalid("only a ready quantization artifact can be activated");
                }
                if self
                    .last_activated
                    .get(&event.projection_id)
                    .is_some_and(|generation| *generation >= event.generation)
                {
                    return invalid("quantization activation cannot move generation backward");
                }
                if let Some(active_generation) = self.active.get(&event.projection_id).copied() {
                    self.artifacts
                        .get_mut(&(event.projection_id.clone(), active_generation))
                        .expect("active quantization generation must exist")
                        .state = QuantizationArtifactState::Retired;
                }
                self.artifacts
                    .get_mut(&key)
                    .expect("activation target was checked")
                    .state = QuantizationArtifactState::Active;
                self.active
                    .insert(event.projection_id.clone(), event.generation);
                self.last_activated
                    .insert(event.projection_id.clone(), event.generation);
            }
            QuantizationLifecycleAction::Retire => {
                if !self.built.contains(&key) {
                    return invalid("quantization artifact must be built before retirement");
                }
                let target = self.artifacts.get_mut(&key).ok_or_else(|| {
                    rrd_core::Error::InvalidRuntime {
                        reason: "quantization retirement target is absent".into(),
                    }
                })?;
                if target.state == QuantizationArtifactState::Retired {
                    return invalid("quantization artifact is already retired");
                }
                target.state = QuantizationArtifactState::Retired;
                if self.active.get(&event.projection_id) == Some(&event.generation) {
                    self.active.remove(&event.projection_id);
                }
            }
        }
        self.revision = event.revision;
        self.last_event_digest = Some(event.event_digest.clone());
        Ok(())
    }

    pub fn active_entries(&self) -> impl Iterator<Item = &QuantizationArtifactEntry> {
        self.active
            .iter()
            .map(|(id, generation)| &self.artifacts[&(id.clone(), *generation)].entry)
    }
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_event_chain_rejects_skips_and_backward_activation() {
        let id = ProjectionId::new("quant:scalar:body").unwrap();
        let mut catalogue = QuantizationArtifactCatalogue::default();
        let build = catalogue
            .next_event(id.clone(), 1, QuantizationLifecycleAction::Build, 1)
            .unwrap();
        assert!(catalogue.apply(&build).is_err());
        let skipped = QuantizationLifecycleEvent::new(
            2,
            id,
            1,
            QuantizationLifecycleAction::Activate,
            2,
            None,
        )
        .unwrap();
        assert!(catalogue.apply(&skipped).is_err());
    }
}
