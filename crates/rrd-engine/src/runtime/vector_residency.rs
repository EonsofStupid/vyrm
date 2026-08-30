//! Process-local physical residency for immutable vector artifacts.
//!
//! Durable catalogue/object records remain the authority. This manager owns
//! only decoded serving bytes and makes collection `memory_tier` policy real:
//! pinned entries are retained within a hard admission bound, cached entries
//! use byte-bounded LRU eviction, and cold entries are transient (mmap when the
//! object authority exposes a verified local path).

use super::{VectorArtifactBinding, VectorArtifactResidencyKey};
use rrd_store::ImmutableObjectStore;
use rrd_vector::{VectorArtifact, VectorMemoryTier};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

pub const DEFAULT_VECTOR_PINNED_BYTES: u64 = 256 * 1024 * 1024;
pub const DEFAULT_VECTOR_CACHED_BYTES: u64 = 64 * 1024 * 1024;
const MAX_VECTOR_RESIDENCY_BYTES: u64 = 1024 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VectorResidencyLimits {
    pub pinned_bytes: u64,
    pub cached_bytes: u64,
}

impl VectorResidencyLimits {
    pub fn validate(self) -> Result<Self, VectorResidencyError> {
        if self.pinned_bytes > MAX_VECTOR_RESIDENCY_BYTES
            || self.cached_bytes > MAX_VECTOR_RESIDENCY_BYTES
        {
            return Err(VectorResidencyError::Configuration(
                "vector residency limits exceed one TiB".into(),
            ));
        }
        Ok(self)
    }
}

impl Default for VectorResidencyLimits {
    fn default() -> Self {
        Self {
            pinned_bytes: DEFAULT_VECTOR_PINNED_BYTES,
            cached_bytes: DEFAULT_VECTOR_CACHED_BYTES,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VectorResidencySource {
    PinnedHit,
    CachedHit,
    PinnedLoad,
    CachedLoad,
    ColdMapped,
    ColdOwned,
    CacheBypassMapped,
    CacheBypassOwned,
}

#[derive(Debug, Clone)]
pub struct VectorResidencyAcquisition {
    pub artifact: Arc<VectorArtifact>,
    pub source: VectorResidencySource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VectorResidencySnapshot {
    pub limits: VectorResidencyLimits,
    pub pinned_entries: usize,
    pub pinned_resident_bytes: u64,
    pub cached_entries: usize,
    pub cached_resident_bytes: u64,
    pub pinned_hits: u64,
    pub cached_hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub stale_reclaims: u64,
    pub tier_transitions: u64,
    pub cold_mmap_loads: u64,
    pub cold_owned_loads: u64,
    pub cache_bypasses: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VectorResidencyError {
    Configuration(String),
    Pressure {
        tier: VectorMemoryTier,
        required_bytes: u64,
        resident_bytes: u64,
        capacity_bytes: u64,
    },
    Artifact(String),
}

impl VectorResidencyError {
    pub const fn is_pressure(&self) -> bool {
        matches!(self, Self::Pressure { .. })
    }
}

impl fmt::Display for VectorResidencyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Configuration(reason) => write!(formatter, "invalid vector residency: {reason}"),
            Self::Pressure {
                tier,
                required_bytes,
                resident_bytes,
                capacity_bytes,
            } => write!(
                formatter,
                "vector {tier:?} residency pressure: required={required_bytes} resident={resident_bytes} capacity={capacity_bytes}"
            ),
            Self::Artifact(reason) => write!(formatter, "vector artifact load failed: {reason}"),
        }
    }
}

impl std::error::Error for VectorResidencyError {}

#[derive(Debug, Clone)]
struct ResidentArtifact {
    artifact: Arc<VectorArtifact>,
    accounted_bytes: u64,
    last_used: u64,
}

#[derive(Debug, Clone)]
pub struct VectorResidencyManager {
    limits: VectorResidencyLimits,
    pinned: BTreeMap<VectorArtifactResidencyKey, ResidentArtifact>,
    cached: BTreeMap<VectorArtifactResidencyKey, ResidentArtifact>,
    pinned_bytes: u64,
    cached_bytes: u64,
    clock: u64,
    pinned_hits: u64,
    cached_hits: u64,
    misses: u64,
    evictions: u64,
    stale_reclaims: u64,
    tier_transitions: u64,
    cold_mmap_loads: u64,
    cold_owned_loads: u64,
    cache_bypasses: u64,
}

impl VectorResidencyManager {
    pub fn new(limits: VectorResidencyLimits) -> Result<Self, VectorResidencyError> {
        Ok(Self {
            limits: limits.validate()?,
            pinned: BTreeMap::new(),
            cached: BTreeMap::new(),
            pinned_bytes: 0,
            cached_bytes: 0,
            clock: 0,
            pinned_hits: 0,
            cached_hits: 0,
            misses: 0,
            evictions: 0,
            stale_reclaims: 0,
            tier_transitions: 0,
            cold_mmap_loads: 0,
            cold_owned_loads: 0,
            cache_bypasses: 0,
        })
    }

    pub fn snapshot(&self) -> VectorResidencySnapshot {
        VectorResidencySnapshot {
            limits: self.limits,
            pinned_entries: self.pinned.len(),
            pinned_resident_bytes: self.pinned_bytes,
            cached_entries: self.cached.len(),
            cached_resident_bytes: self.cached_bytes,
            pinned_hits: self.pinned_hits,
            cached_hits: self.cached_hits,
            misses: self.misses,
            evictions: self.evictions,
            stale_reclaims: self.stale_reclaims,
            tier_transitions: self.tier_transitions,
            cold_mmap_loads: self.cold_mmap_loads,
            cold_owned_loads: self.cold_owned_loads,
            cache_bypasses: self.cache_bypasses,
        }
    }

    pub fn reconcile(&mut self, active: &BTreeSet<VectorArtifactResidencyKey>) {
        let mut reclaimed = 0u64;
        self.pinned.retain(|key, entry| {
            let keep = active.contains(key);
            if !keep {
                self.pinned_bytes = self.pinned_bytes.saturating_sub(entry.accounted_bytes);
                reclaimed = reclaimed.saturating_add(1);
            }
            keep
        });
        self.cached.retain(|key, entry| {
            let keep = active.contains(key);
            if !keep {
                self.cached_bytes = self.cached_bytes.saturating_sub(entry.accounted_bytes);
                reclaimed = reclaimed.saturating_add(1);
            }
            keep
        });
        self.stale_reclaims = self.stale_reclaims.saturating_add(reclaimed);
    }

    pub fn acquire(
        &mut self,
        tier: VectorMemoryTier,
        binding: &VectorArtifactBinding,
        objects: &impl ImmutableObjectStore,
    ) -> Result<VectorResidencyAcquisition, VectorResidencyError> {
        let key = binding.key();
        let accounted_bytes = binding.object.length;
        match tier {
            VectorMemoryTier::Pinned => self.acquire_pinned(key, accounted_bytes, binding, objects),
            VectorMemoryTier::Cached => self.acquire_cached(key, accounted_bytes, binding, objects),
            VectorMemoryTier::Cold => self.acquire_cold(&key, binding, objects),
        }
    }

    fn acquire_pinned(
        &mut self,
        key: VectorArtifactResidencyKey,
        bytes: u64,
        binding: &VectorArtifactBinding,
        objects: &impl ImmutableObjectStore,
    ) -> Result<VectorResidencyAcquisition, VectorResidencyError> {
        self.tick()?;
        if let Some(entry) = self.pinned.get_mut(&key) {
            entry.last_used = self.clock;
            self.pinned_hits = self.pinned_hits.saturating_add(1);
            return Ok(VectorResidencyAcquisition {
                artifact: entry.artifact.clone(),
                source: VectorResidencySource::PinnedHit,
            });
        }
        self.ensure_pinned_capacity(bytes)?;
        if let Some(mut entry) = self.cached.remove(&key) {
            self.cached_bytes = self.cached_bytes.saturating_sub(entry.accounted_bytes);
            entry.last_used = self.clock;
            let artifact = entry.artifact.clone();
            self.pinned_bytes = self.pinned_bytes.saturating_add(entry.accounted_bytes);
            self.pinned.insert(key, entry);
            self.tier_transitions = self.tier_transitions.saturating_add(1);
            return Ok(VectorResidencyAcquisition {
                artifact,
                source: VectorResidencySource::PinnedHit,
            });
        }
        self.misses = self.misses.saturating_add(1);
        let artifact = binding
            .decode_owned(objects)
            .map_err(|error| VectorResidencyError::Artifact(error.to_string()))?;
        self.pinned.insert(
            key,
            ResidentArtifact {
                artifact: artifact.clone(),
                accounted_bytes: bytes,
                last_used: self.clock,
            },
        );
        self.pinned_bytes = self.pinned_bytes.saturating_add(bytes);
        Ok(VectorResidencyAcquisition {
            artifact,
            source: VectorResidencySource::PinnedLoad,
        })
    }

    fn acquire_cached(
        &mut self,
        key: VectorArtifactResidencyKey,
        bytes: u64,
        binding: &VectorArtifactBinding,
        objects: &impl ImmutableObjectStore,
    ) -> Result<VectorResidencyAcquisition, VectorResidencyError> {
        self.tick()?;
        if let Some(entry) = self.cached.get_mut(&key) {
            entry.last_used = self.clock;
            self.cached_hits = self.cached_hits.saturating_add(1);
            return Ok(VectorResidencyAcquisition {
                artifact: entry.artifact.clone(),
                source: VectorResidencySource::CachedHit,
            });
        }
        let promoted = self.pinned.remove(&key);
        if let Some(entry) = &promoted {
            self.pinned_bytes = self.pinned_bytes.saturating_sub(entry.accounted_bytes);
            self.tier_transitions = self.tier_transitions.saturating_add(1);
        }
        if bytes > self.limits.cached_bytes {
            self.cache_bypasses = self.cache_bypasses.saturating_add(1);
            let (artifact, mapped) = self.load_transient(binding, objects)?;
            return Ok(VectorResidencyAcquisition {
                artifact,
                source: if mapped {
                    VectorResidencySource::CacheBypassMapped
                } else {
                    VectorResidencySource::CacheBypassOwned
                },
            });
        }
        self.evict_cached_for(bytes)?;
        if let Some(mut entry) = promoted {
            entry.last_used = self.clock;
            let artifact = entry.artifact.clone();
            self.cached_bytes = self.cached_bytes.saturating_add(entry.accounted_bytes);
            self.cached.insert(key, entry);
            return Ok(VectorResidencyAcquisition {
                artifact,
                source: VectorResidencySource::CachedHit,
            });
        }
        self.misses = self.misses.saturating_add(1);
        let artifact = binding
            .decode_owned(objects)
            .map_err(|error| VectorResidencyError::Artifact(error.to_string()))?;
        self.cached.insert(
            key,
            ResidentArtifact {
                artifact: artifact.clone(),
                accounted_bytes: bytes,
                last_used: self.clock,
            },
        );
        self.cached_bytes = self.cached_bytes.saturating_add(bytes);
        Ok(VectorResidencyAcquisition {
            artifact,
            source: VectorResidencySource::CachedLoad,
        })
    }

    fn acquire_cold(
        &mut self,
        key: &VectorArtifactResidencyKey,
        binding: &VectorArtifactBinding,
        objects: &impl ImmutableObjectStore,
    ) -> Result<VectorResidencyAcquisition, VectorResidencyError> {
        if let Some(entry) = self.pinned.remove(key) {
            self.pinned_bytes = self.pinned_bytes.saturating_sub(entry.accounted_bytes);
            self.tier_transitions = self.tier_transitions.saturating_add(1);
        }
        if let Some(entry) = self.cached.remove(key) {
            self.cached_bytes = self.cached_bytes.saturating_sub(entry.accounted_bytes);
            self.tier_transitions = self.tier_transitions.saturating_add(1);
        }
        self.misses = self.misses.saturating_add(1);
        let (artifact, mapped) = self.load_transient(binding, objects)?;
        if mapped {
            self.cold_mmap_loads = self.cold_mmap_loads.saturating_add(1);
        } else {
            self.cold_owned_loads = self.cold_owned_loads.saturating_add(1);
        }
        Ok(VectorResidencyAcquisition {
            artifact,
            source: if mapped {
                VectorResidencySource::ColdMapped
            } else {
                VectorResidencySource::ColdOwned
            },
        })
    }

    fn load_transient(
        &self,
        binding: &VectorArtifactBinding,
        objects: &impl ImmutableObjectStore,
    ) -> Result<(Arc<VectorArtifact>, bool), VectorResidencyError> {
        if let Some(artifact) = binding
            .decode_mapped(objects)
            .map_err(|error| VectorResidencyError::Artifact(error.to_string()))?
        {
            return Ok((artifact, true));
        }
        binding
            .decode_owned(objects)
            .map(|artifact| (artifact, false))
            .map_err(|error| VectorResidencyError::Artifact(error.to_string()))
    }

    fn ensure_pinned_capacity(&self, bytes: u64) -> Result<(), VectorResidencyError> {
        if bytes > self.limits.pinned_bytes.saturating_sub(self.pinned_bytes) {
            return Err(VectorResidencyError::Pressure {
                tier: VectorMemoryTier::Pinned,
                required_bytes: bytes,
                resident_bytes: self.pinned_bytes,
                capacity_bytes: self.limits.pinned_bytes,
            });
        }
        Ok(())
    }

    fn evict_cached_for(&mut self, bytes: u64) -> Result<(), VectorResidencyError> {
        while bytes > self.limits.cached_bytes.saturating_sub(self.cached_bytes) {
            let Some(key) = self
                .cached
                .iter()
                .min_by(|(left_key, left), (right_key, right)| {
                    left.last_used
                        .cmp(&right.last_used)
                        .then_with(|| left_key.cmp(right_key))
                })
                .map(|(key, _)| key.clone())
            else {
                return Err(VectorResidencyError::Pressure {
                    tier: VectorMemoryTier::Cached,
                    required_bytes: bytes,
                    resident_bytes: self.cached_bytes,
                    capacity_bytes: self.limits.cached_bytes,
                });
            };
            let removed = self
                .cached
                .remove(&key)
                .expect("LRU key must remain present");
            self.cached_bytes = self.cached_bytes.saturating_sub(removed.accounted_bytes);
            self.evictions = self.evictions.saturating_add(1);
        }
        Ok(())
    }

    fn tick(&mut self) -> Result<(), VectorResidencyError> {
        self.clock = self.clock.checked_add(1).ok_or_else(|| {
            VectorResidencyError::Configuration("residency LRU clock overflowed".into())
        })?;
        Ok(())
    }
}

impl Default for VectorResidencyManager {
    fn default() -> Self {
        Self::new(VectorResidencyLimits::default())
            .expect("default vector residency limits are valid")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rrd_core::{
        ObjectReference, ProjectionId, RuntimeProperties, RuntimeRef, RuntimeVector, ScopeId,
        VectorValue,
    };
    use rrd_store::LocalObjectStore;
    use rrd_vector::{
        CompactDenseSegment, DenseMemoryPlacement, ScoreMetric, VectorArtifactKind,
        VectorCandidate, VectorSegmentConfig,
    };
    use std::collections::BTreeSet;

    fn binding(
        objects: &LocalObjectStore,
        scope: &ScopeId,
        projection: &str,
        candidate_id: &str,
    ) -> VectorArtifactBinding {
        let candidate = VectorCandidate {
            scope: scope.clone(),
            source_cursor: 1,
            vector: RuntimeVector {
                reference: RuntimeRef::new("embedding", candidate_id).unwrap(),
                subject: RuntimeRef::new("document", candidate_id).unwrap(),
                collection: None,
                field: "body".into(),
                valid_from: 1,
                valid_to: None,
                value: VectorValue::Dense {
                    values: vec![1.0, 0.0],
                },
                provenance: None,
                properties: RuntimeProperties::new(),
            },
        };
        let artifact = VectorArtifact::from(
            CompactDenseSegment::build(
                VectorSegmentConfig {
                    id: ProjectionId::new(projection).unwrap(),
                    scope: scope.clone(),
                    field: "body".into(),
                    dimensions: 2,
                    metric: ScoreMetric::Dot,
                    embedding_model: None,
                    filter_properties: BTreeSet::new(),
                },
                1,
                1,
                [candidate],
            )
            .unwrap(),
        );
        let verified = objects.put(artifact.as_bytes()).unwrap();
        let object = ObjectReference::for_verified(
            format!("{projection}:bytes"),
            None,
            VectorArtifactKind::CompactDense.media_type(),
            verified.sha256,
            verified.length,
            verified.receipt,
        )
        .unwrap();
        VectorArtifactBinding {
            kind: VectorArtifactKind::CompactDense,
            descriptor: artifact.descriptor(),
            object,
        }
    }

    #[test]
    fn pinned_admission_is_bounded_and_tier_changes_are_physical() {
        let temporary = tempfile::tempdir().unwrap();
        let objects = LocalObjectStore::open(temporary.path()).unwrap();
        let scope = ScopeId::new("instance:residency-pinned").unwrap();
        let binding = binding(&objects, &scope, "vector:compact:pinned", "pinned");
        let bytes = binding.object.length;
        let mut manager = VectorResidencyManager::new(VectorResidencyLimits {
            pinned_bytes: bytes,
            cached_bytes: bytes,
        })
        .unwrap();

        let loaded = manager
            .acquire(VectorMemoryTier::Pinned, &binding, &objects)
            .unwrap();
        assert_eq!(loaded.source, VectorResidencySource::PinnedLoad);
        assert_eq!(
            manager
                .acquire(VectorMemoryTier::Pinned, &binding, &objects)
                .unwrap()
                .source,
            VectorResidencySource::PinnedHit
        );
        assert_eq!(manager.snapshot().pinned_resident_bytes, bytes);

        assert_eq!(
            manager
                .acquire(VectorMemoryTier::Cached, &binding, &objects)
                .unwrap()
                .source,
            VectorResidencySource::CachedHit
        );
        let cached = manager.snapshot();
        assert_eq!(cached.pinned_entries, 0);
        assert_eq!(cached.cached_entries, 1);

        let cold = manager
            .acquire(VectorMemoryTier::Cold, &binding, &objects)
            .unwrap();
        assert_eq!(cold.source, VectorResidencySource::ColdMapped);
        let VectorArtifact::CompactDense(segment) = cold.artifact.as_ref() else {
            panic!("fixture must remain compact dense")
        };
        assert_eq!(segment.memory_placement(), DenseMemoryPlacement::Mapped);
        let cold = manager.snapshot();
        assert_eq!(cold.cached_entries, 0);
        assert_eq!(cold.tier_transitions, 2);
        assert_eq!(cold.cold_mmap_loads, 1);

        manager
            .acquire(VectorMemoryTier::Pinned, &binding, &objects)
            .unwrap();
        manager.reconcile(&BTreeSet::new());
        assert_eq!(manager.snapshot().pinned_entries, 0);
        assert_eq!(manager.snapshot().stale_reclaims, 1);

        let mut pressured = VectorResidencyManager::new(VectorResidencyLimits {
            pinned_bytes: bytes - 1,
            cached_bytes: bytes,
        })
        .unwrap();
        let error = pressured
            .acquire(VectorMemoryTier::Pinned, &binding, &objects)
            .unwrap_err();
        assert!(matches!(
            error,
            VectorResidencyError::Pressure {
                tier: VectorMemoryTier::Pinned,
                required_bytes,
                resident_bytes: 0,
                capacity_bytes,
            } if required_bytes == bytes && capacity_bytes == bytes - 1
        ));
        assert_eq!(pressured.snapshot().pinned_entries, 0);
    }

    #[test]
    fn cached_residency_evicts_the_least_recently_used_artifact() {
        let temporary = tempfile::tempdir().unwrap();
        let objects = LocalObjectStore::open(temporary.path()).unwrap();
        let scope = ScopeId::new("instance:residency-cache").unwrap();
        let first = binding(&objects, &scope, "vector:compact:first", "first");
        let second = binding(&objects, &scope, "vector:compact:second", "second");
        let capacity = first.object.length.max(second.object.length);
        let mut manager = VectorResidencyManager::new(VectorResidencyLimits {
            pinned_bytes: capacity,
            cached_bytes: capacity,
        })
        .unwrap();

        assert_eq!(
            manager
                .acquire(VectorMemoryTier::Cached, &first, &objects)
                .unwrap()
                .source,
            VectorResidencySource::CachedLoad
        );
        assert_eq!(
            manager
                .acquire(VectorMemoryTier::Cached, &second, &objects)
                .unwrap()
                .source,
            VectorResidencySource::CachedLoad
        );
        assert_eq!(
            manager
                .acquire(VectorMemoryTier::Cached, &second, &objects)
                .unwrap()
                .source,
            VectorResidencySource::CachedHit
        );
        let snapshot = manager.snapshot();
        assert_eq!(snapshot.cached_entries, 1);
        assert_eq!(snapshot.evictions, 1);
        assert_eq!(snapshot.cached_hits, 1);
        assert!(snapshot.cached_resident_bytes <= capacity);

        manager
            .acquire(VectorMemoryTier::Cached, &first, &objects)
            .unwrap();
        let snapshot = manager.snapshot();
        assert_eq!(snapshot.evictions, 2);
        assert_eq!(snapshot.cached_entries, 1);
    }
}
