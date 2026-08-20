use crate::contract::invalid;
use crate::{
    CandidatePath, HnswDescriptor, SegmentDescriptor, VectorArtifact, VectorArtifactKind,
    EXACT_SCAN_PROJECTION_ID,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use vyrm_core::{
    digest, ObjectReference, ProjectionId, ProjectionStamp, ProjectionState, Result, RuntimeRef,
    ScopeId,
};

pub const VECTOR_ARTIFACT_CATALOG_VERSION: u16 = 1;
pub const VECTOR_ARTIFACT_RECORD_TYPE: &str = "vector_artifact";

/// One immutable, reconstructable vector projection publication.
///
/// The descriptor says what may be served. The object reference says exactly
/// which verified bytes implement it. Both enter one authoritative runtime
/// transaction, so a catalog entry can never point at a partially published
/// file or silently change representation after restart.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VectorArtifactCatalogEntry {
    pub contract_version: u16,
    pub catalog_revision: u64,
    pub kind: VectorArtifactKind,
    pub descriptor: VectorProjectionDescriptor,
    pub object: ObjectReference,
    pub published_at: u64,
    pub entry_digest: String,
}

impl VectorArtifactCatalogEntry {
    pub fn record_reference(descriptor: &VectorProjectionDescriptor) -> Result<RuntimeRef> {
        RuntimeRef::new(
            VECTOR_ARTIFACT_RECORD_TYPE,
            format!(
                "{}@{}",
                descriptor.stamp().id,
                descriptor.stamp().generation
            ),
        )
    }

    pub fn new(
        catalog_revision: u64,
        kind: VectorArtifactKind,
        descriptor: VectorProjectionDescriptor,
        object: ObjectReference,
        published_at: u64,
    ) -> Result<Self> {
        let mut entry = Self {
            contract_version: VECTOR_ARTIFACT_CATALOG_VERSION,
            catalog_revision,
            kind,
            descriptor,
            object,
            published_at,
            entry_digest: String::new(),
        };
        entry.validate_components()?;
        entry.entry_digest = digest::sha256_hex(&entry.identity_bytes()?);
        Ok(entry)
    }

    pub fn validate(&self) -> Result<()> {
        self.validate_components()?;
        let expected = digest::sha256_hex(&self.identity_bytes()?);
        if self.entry_digest != expected {
            return invalid("vector artifact catalog entry digest does not match its fields");
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
            return invalid("vector artifact object bytes differ from the catalog reference");
        }
        let artifact = VectorArtifact::from_bytes(self.kind, bytes)?;
        if artifact.descriptor() != self.descriptor {
            return invalid("decoded vector artifact differs from its catalog descriptor");
        }
        Ok(artifact)
    }

    fn validate_components(&self) -> Result<()> {
        if self.contract_version != VECTOR_ARTIFACT_CATALOG_VERSION || self.catalog_revision == 0 {
            return invalid("vector artifact catalog version and revision must be valid");
        }
        self.descriptor.validate()?;
        if self.descriptor.stamp().state != ProjectionState::Ready {
            return invalid("cataloged vector artifact must be ready");
        }
        let kind_matches = matches!(
            (&self.descriptor, self.kind),
            (
                VectorProjectionDescriptor::ExactSegment { .. },
                VectorArtifactKind::ExactSegment | VectorArtifactKind::CompactDense
            ) | (
                VectorProjectionDescriptor::Hnsw { .. },
                VectorArtifactKind::Hnsw
            )
        );
        if !kind_matches {
            return invalid("vector artifact codec kind differs from its projection descriptor");
        }
        self.object.validate()?;
        if self.object.media_type != self.kind.media_type() {
            return invalid("vector artifact object media type differs from its codec kind");
        }
        if self.object.subject.as_ref() != Some(&Self::record_reference(&self.descriptor)?) {
            return invalid("vector artifact object is not attached to its catalog record");
        }
        Ok(())
    }

    fn identity_bytes(&self) -> Result<Vec<u8>> {
        serde_json::to_vec(&(
            self.contract_version,
            self.catalog_revision,
            self.kind,
            &self.descriptor,
            &self.object,
            self.published_at,
        ))
        .map_err(|error| vyrm_core::Error::InvalidRuntime {
            reason: format!("vector artifact catalog identity cannot be encoded: {error}"),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum VectorProjectionDescriptor {
    ExactSegment { descriptor: SegmentDescriptor },
    Hnsw { descriptor: HnswDescriptor },
}

impl VectorProjectionDescriptor {
    pub fn stamp(&self) -> &ProjectionStamp {
        match self {
            Self::ExactSegment { descriptor } => &descriptor.stamp,
            Self::Hnsw { descriptor } => &descriptor.stamp,
        }
    }

    pub fn scope(&self) -> &vyrm_core::ScopeId {
        match self {
            Self::ExactSegment { descriptor } => &descriptor.scope,
            Self::Hnsw { descriptor } => &descriptor.scope,
        }
    }

    fn stamp_mut(&mut self) -> &mut ProjectionStamp {
        match self {
            Self::ExactSegment { descriptor } => &mut descriptor.stamp,
            Self::Hnsw { descriptor } => &mut descriptor.stamp,
        }
    }

    pub fn validate(&self) -> Result<()> {
        match self {
            Self::ExactSegment { descriptor } => descriptor.validate(),
            Self::Hnsw { descriptor } => descriptor.validate(),
        }
    }

    pub fn candidate_path(&self, estimated_cost: u64) -> CandidatePath {
        match self {
            Self::ExactSegment { descriptor } => descriptor.candidate_path(estimated_cost),
            Self::Hnsw { descriptor } => descriptor.candidate_path(estimated_cost),
        }
    }

    fn same_kind(&self, other: &Self) -> bool {
        matches!(
            (self, other),
            (Self::ExactSegment { .. }, Self::ExactSegment { .. })
                | (Self::Hnsw { .. }, Self::Hnsw { .. })
        )
    }
}

impl From<SegmentDescriptor> for VectorProjectionDescriptor {
    fn from(descriptor: SegmentDescriptor) -> Self {
        Self::ExactSegment { descriptor }
    }
}

impl From<HnswDescriptor> for VectorProjectionDescriptor {
    fn from(descriptor: HnswDescriptor) -> Self {
        Self::Hnsw { descriptor }
    }
}

/// Compare-and-swap catalog for every rebuildable vector projection.
///
/// Publication moves the previous generation to `retired`. Callers may only
/// reclaim its artifact after proving no leased snapshot protects that exact
/// `(projection id, generation)` pair.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VectorCatalog {
    pub revision: u64,
    pub entries: BTreeMap<ProjectionId, VectorProjectionDescriptor>,
    #[serde(default)]
    pub retired: Vec<VectorProjectionDescriptor>,
}

impl VectorCatalog {
    pub fn publish(
        &mut self,
        expected_revision: u64,
        descriptor: impl Into<VectorProjectionDescriptor>,
    ) -> Result<u64> {
        self.expect_revision(expected_revision)?;
        let descriptor = descriptor.into();
        descriptor.validate()?;
        if descriptor.stamp().id.as_str() == EXACT_SCAN_PROJECTION_ID {
            return invalid("vector projection uses the reserved exact-scan identity");
        }
        if descriptor.stamp().state != ProjectionState::Ready {
            return invalid("only a ready vector projection can be published");
        }
        let id = descriptor.stamp().id.clone();
        match self.entries.get(&id) {
            Some(previous) => {
                if !previous.same_kind(&descriptor) {
                    return invalid("vector projection kind cannot change within one identity");
                }
                if descriptor.stamp().generation != previous.stamp().generation.saturating_add(1) {
                    return invalid("vector projection generation must advance exactly once");
                }
                if descriptor.stamp().source_cursor < previous.stamp().source_cursor {
                    return invalid("vector projection source coverage cannot move backward");
                }
            }
            None if descriptor.stamp().generation != 1 => {
                return invalid("first vector projection generation must be one");
            }
            None => {}
        }
        if let Some(mut previous) = self.entries.remove(&id) {
            previous.stamp_mut().state = ProjectionState::Retiring;
            self.retired.push(previous);
            self.retired.sort_by(|left, right| {
                left.stamp()
                    .id
                    .cmp(&right.stamp().id)
                    .then_with(|| left.stamp().generation.cmp(&right.stamp().generation))
            });
        }
        self.entries.insert(id, descriptor);
        self.bump_revision()
    }

    pub fn quarantine(
        &mut self,
        expected_revision: u64,
        id: &ProjectionId,
        generation: u64,
    ) -> Result<u64> {
        self.expect_revision(expected_revision)?;
        let descriptor =
            self.entries
                .get_mut(id)
                .ok_or_else(|| vyrm_core::Error::InvalidRuntime {
                    reason: format!("vector projection {id} is absent"),
                })?;
        if descriptor.stamp().generation != generation {
            return invalid("vector quarantine generation differs from catalog");
        }
        if descriptor.stamp().state != ProjectionState::Ready {
            return invalid("only a ready vector generation can be quarantined");
        }
        descriptor.stamp_mut().state = ProjectionState::Quarantined;
        self.bump_revision()
    }

    pub fn reclaim_retired(&mut self, protected: &BTreeSet<(ProjectionId, u64)>) -> Vec<String> {
        let mut reclaimed = Vec::new();
        self.retired.retain(|descriptor| {
            if protected.contains(&(descriptor.stamp().id.clone(), descriptor.stamp().generation)) {
                true
            } else {
                reclaimed.push(descriptor.stamp().artifact_digest.clone());
                false
            }
        });
        reclaimed.sort();
        reclaimed
    }

    fn expect_revision(&self, expected_revision: u64) -> Result<()> {
        if self.revision != expected_revision {
            return invalid(format!(
                "vector catalog conflict: expected revision {expected_revision}, actual {}",
                self.revision
            ));
        }
        Ok(())
    }

    fn bump_revision(&mut self) -> Result<u64> {
        self.revision =
            self.revision
                .checked_add(1)
                .ok_or_else(|| vyrm_core::Error::InvalidRuntime {
                    reason: "vector catalog revision overflowed".into(),
                })?;
        Ok(self.revision)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ScoreMetric, VectorSegmentConfig};
    use std::collections::BTreeSet;
    use vyrm_core::{ScopeId, DATA_RUNTIME_CONTRACT_VERSION};

    fn segment(id: &str, generation: u64, cursor: u64) -> SegmentDescriptor {
        let scope = ScopeId::new("instance:catalog").unwrap();
        let config = VectorSegmentConfig {
            id: ProjectionId::new(id).unwrap(),
            scope: scope.clone(),
            field: "body".into(),
            dimensions: 2,
            metric: ScoreMetric::Dot,
            embedding_model: None,
            filter_properties: BTreeSet::new(),
        };
        SegmentDescriptor {
            stamp: ProjectionStamp {
                contract_version: DATA_RUNTIME_CONTRACT_VERSION,
                id: config.id.clone(),
                generation,
                source_cursor: cursor,
                config_digest: config.digest().unwrap(),
                artifact_digest: format!("{generation:064x}"),
                state: ProjectionState::Ready,
            },
            scope,
            field: "body".into(),
            dimensions: 2,
            metric: ScoreMetric::Dot,
            embedding_model: None,
            filter_properties: BTreeSet::new(),
            minimum_cursor: 0,
            candidate_versions: cursor as usize,
        }
    }

    #[test]
    fn catalog_enforces_cas_generation_retirement_and_quarantine() {
        let id = ProjectionId::new("vector:body").unwrap();
        let mut catalog = VectorCatalog::default();
        assert_eq!(catalog.publish(0, segment(id.as_str(), 1, 1)).unwrap(), 1);
        assert!(catalog.publish(0, segment(id.as_str(), 2, 2)).is_err());
        assert_eq!(catalog.publish(1, segment(id.as_str(), 2, 2)).unwrap(), 2);
        assert_eq!(catalog.retired.len(), 1);
        let protected = BTreeSet::from([(id.clone(), 1)]);
        assert!(catalog.reclaim_retired(&protected).is_empty());
        assert_eq!(catalog.reclaim_retired(&BTreeSet::new()).len(), 1);
        assert_eq!(catalog.quarantine(2, &id, 2).unwrap(), 3);
        assert_eq!(
            catalog.entries[&id].stamp().state,
            ProjectionState::Quarantined
        );
    }
}
