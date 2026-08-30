//! Runtime ownership of the persisted source-routing projection.
//!
//! `rrd-graph` supplies the derivation; this module composes it with the
//! storage port and gives lifecycle callers a single freshness barrier. A
//! mutation is allowed only after this barrier has attuned to the current
//! root, strictly refreshed every indexable file, and persisted the resulting
//! generation.

use rrd_graph::{Index, ProjectAttunement, ProjectProfile, ProjectTopology, Refresh};
use rrd_store::Engine;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const ROUTING_PROJECTION: &str = "routing-index-v1";
const ROUTING_FORMAT: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutingOrigin {
    Built,
    Refreshed,
}

/// Evidence produced by a successful freshness barrier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutingReady {
    pub origin: RoutingOrigin,
    pub files: usize,
    pub symbols: usize,
    pub generation: u64,
    pub refresh: Refresh,
    pub topology_sha256: String,
    pub profile_sha256: String,
}

impl RoutingReady {
    pub fn render(&self) -> String {
        let origin = match self.origin {
            RoutingOrigin::Built => "built",
            RoutingOrigin::Refreshed => "refreshed",
        };
        format!(
            "{origin} generation {}; {} file(s), {} symbol(s); topology={} profile={}; {}",
            self.generation,
            self.files,
            self.symbols,
            &self.topology_sha256[..12],
            &self.profile_sha256[..12],
            self.refresh.render()
        )
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredRouting {
    format: u32,
    root: PathBuf,
    index: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    topology: Option<ProjectTopology>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    profile: Option<ProjectProfile>,
}

struct DecodedRouting {
    index: Index,
    topology: Option<ProjectTopology>,
    profile: Option<ProjectProfile>,
}

fn canonical_root(root: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let root = std::fs::canonicalize(root)
        .map_err(|error| format!("cannot establish routing root {}: {error}", root.display()))?;
    if !root.is_dir() {
        return Err(format!("routing root {} is not a directory", root.display()).into());
    }
    Ok(root)
}

fn encode(
    root: &Path,
    index: &Index,
    topology: &ProjectTopology,
    profile: &ProjectProfile,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let stored = StoredRouting {
        format: ROUTING_FORMAT,
        root: root.to_path_buf(),
        index: serde_json::from_slice(&index.to_bytes())?,
        topology: Some(topology.clone()),
        profile: Some(profile.clone()),
    };
    Ok(serde_json::to_vec(&stored)?)
}

fn decode(
    bytes: &[u8],
    expected_root: &Path,
) -> Result<DecodedRouting, Box<dyn std::error::Error>> {
    let stored: StoredRouting = serde_json::from_slice(bytes).map_err(|error| {
        format!(
            "routing projection is unreadable: {error}; recover with `rrflow reset-routing --root {}`",
            expected_root.display()
        )
    })?;
    if stored.format != ROUTING_FORMAT {
        return Err(format!(
            "routing projection format {} is unsupported (expected {}); recover with `rrflow reset-routing --root {}`",
            stored.format,
            ROUTING_FORMAT,
            expected_root.display()
        )
        .into());
    }
    if stored.root != expected_root {
        return Err(format!(
            "routing projection belongs to {}, not {}; use a project-local database or rebind explicitly with `rrflow reset-routing --root {}`",
            stored.root.display(),
            expected_root.display(),
            expected_root.display()
        )
        .into());
    }
    match (&stored.topology, &stored.profile) {
        (Some(topology), Some(profile)) if profile.verify_digest(topology) => {}
        (None, None) => {}
        _ => {
            return Err(format!(
                "routing topology/profile artifacts failed digest verification; recover with `rrflow reset-routing --root {}`",
                expected_root.display()
            )
            .into())
        }
    }
    let bytes = serde_json::to_vec(&stored.index)?;
    let index = Index::from_bytes(&bytes).map_err(|error| -> Box<dyn std::error::Error> {
        format!(
            "routing index is unreadable: {error}; recover with `rrflow reset-routing --root {}`",
            expected_root.display()
        )
        .into()
    })?;
    Ok(DecodedRouting {
        index,
        topology: stored.topology,
        profile: stored.profile,
    })
}

/// Establishes and persists routing freshness for `root`.
#[tracing::instrument(level = "debug", skip_all)]
pub fn ensure_routing_fresh<E: Engine>(
    store: &E,
    root: &Path,
) -> Result<RoutingReady, Box<dyn std::error::Error>> {
    let root = canonical_root(root)?;
    let attunement = ProjectAttunement::materialize(&root)?;
    let stored = store.get_projection(ROUTING_PROJECTION)?;
    let (mut index, prior_topology, prior_profile, origin) = match stored {
        Some(bytes) => {
            let decoded = decode(&bytes, &root)?;
            (
                decoded.index,
                decoded.topology,
                decoded.profile,
                RoutingOrigin::Refreshed,
            )
        }
        None => (Index::default(), None, None, RoutingOrigin::Built),
    };
    let refresh = index.refresh_strict(&attunement.routing_profile)?;
    let artifact_changed = prior_topology.as_ref().map(|value| &value.digest)
        != Some(&attunement.topology.digest)
        || prior_profile.as_ref().map(|value| &value.digest) != Some(&attunement.profile.digest);

    // A touched-but-identical file updates the stat cache even though the
    // content generation is unchanged, so that state is worth persisting too.
    if origin == RoutingOrigin::Built
        || artifact_changed
        || !refresh.is_noop()
        || refresh.read_but_identical > 0
    {
        store.put_projection(
            ROUTING_PROJECTION,
            &encode(&root, &index, &attunement.topology, &attunement.profile)?,
        )?;
    }

    Ok(RoutingReady {
        origin,
        files: index.file_count(),
        symbols: index.symbol_count(),
        generation: index.generation(),
        refresh,
        topology_sha256: attunement.topology.digest,
        profile_sha256: attunement.profile.digest,
    })
}

/// Loads a persisted routing index only after verifying its root binding.
pub fn load_routing<E: Engine>(
    store: &E,
    root: &Path,
) -> Result<Option<Index>, Box<dyn std::error::Error>> {
    let root = canonical_root(root)?;
    store
        .get_projection(ROUTING_PROJECTION)?
        .map(|bytes| decode(&bytes, &root).map(|decoded| decoded.index))
        .transpose()
}

/// Loads the materialized project topology/profile only after root and digest
/// verification. A legacy routing artifact is hydrated by the next freshness
/// barrier rather than becoming a second schema version.
pub fn load_project_artifacts<E: Engine>(
    store: &E,
    root: &Path,
) -> Result<Option<(ProjectTopology, ProjectProfile)>, Box<dyn std::error::Error>> {
    let root = canonical_root(root)?;
    let Some(bytes) = store.get_projection(ROUTING_PROJECTION)? else {
        return Ok(None);
    };
    let decoded = decode(&bytes, &root)?;
    match (decoded.topology, decoded.profile) {
        (Some(topology), Some(profile)) => Ok(Some((topology, profile))),
        (None, None) => Err(
            "routing projection predates materialized topology; run RRFlow preflight to hydrate it"
                .into(),
        ),
        _ => Err("routing projection contains partial project artifacts".into()),
    }
}

/// Explicit recovery and root-rebinding path. Unlike the freshness barrier,
/// this intentionally discards the prior derived state and rebuilds it.
#[tracing::instrument(level = "debug", skip_all)]
pub fn reset_routing<E: Engine>(
    store: &E,
    root: &Path,
) -> Result<RoutingReady, Box<dyn std::error::Error>> {
    let root = canonical_root(root)?;
    let attunement = ProjectAttunement::materialize(&root)?;
    let mut index = Index::default();
    let refresh = index.refresh_strict(&attunement.routing_profile)?;
    store.put_projection(
        ROUTING_PROJECTION,
        &encode(&root, &index, &attunement.topology, &attunement.profile)?,
    )?;
    Ok(RoutingReady {
        origin: RoutingOrigin::Built,
        files: index.file_count(),
        symbols: index.symbol_count(),
        generation: index.generation(),
        refresh,
        topology_sha256: attunement.topology.digest,
        profile_sha256: attunement.profile.digest,
    })
}
