//! Runtime ownership of the persisted source-routing projection.
//!
//! `vyrm-graph` supplies the derivation; this module composes it with the
//! storage port and gives lifecycle callers a single freshness barrier. A
//! mutation is allowed only after this barrier has attuned to the current
//! root, strictly refreshed every indexable file, and persisted the resulting
//! generation.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use vyrm_graph::{Index, Profile, Refresh};
use vyrm_store::Engine;

pub const ROUTING_PROJECTION: &str = "routing-index-v1";
const ROUTING_FORMAT: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutingOrigin {
    Built,
    Refreshed,
}

/// Evidence produced by a successful freshness barrier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoutingReady {
    pub origin: RoutingOrigin,
    pub files: usize,
    pub symbols: usize,
    pub generation: u64,
    pub refresh: Refresh,
}

impl RoutingReady {
    pub fn render(&self) -> String {
        let origin = match self.origin {
            RoutingOrigin::Built => "built",
            RoutingOrigin::Refreshed => "refreshed",
        };
        format!(
            "{origin} generation {}; {} file(s), {} symbol(s); {}",
            self.generation,
            self.files,
            self.symbols,
            self.refresh.render()
        )
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredRouting {
    format: u32,
    root: PathBuf,
    index: serde_json::Value,
}

fn canonical_root(root: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let root = std::fs::canonicalize(root)
        .map_err(|error| format!("cannot establish routing root {}: {error}", root.display()))?;
    if !root.is_dir() {
        return Err(format!("routing root {} is not a directory", root.display()).into());
    }
    Ok(root)
}

fn encode(root: &Path, index: &Index) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let stored = StoredRouting {
        format: ROUTING_FORMAT,
        root: root.to_path_buf(),
        index: serde_json::from_slice(&index.to_bytes())?,
    };
    Ok(serde_json::to_vec(&stored)?)
}

fn decode(bytes: &[u8], expected_root: &Path) -> Result<Index, Box<dyn std::error::Error>> {
    let stored: StoredRouting = serde_json::from_slice(bytes).map_err(|error| {
        format!(
            "routing projection is unreadable: {error}; recover with `vyrm reset-routing --root {}`",
            expected_root.display()
        )
    })?;
    if stored.format != ROUTING_FORMAT {
        return Err(format!(
            "routing projection format {} is unsupported (expected {}); recover with `vyrm reset-routing --root {}`",
            stored.format,
            ROUTING_FORMAT,
            expected_root.display()
        )
        .into());
    }
    if stored.root != expected_root {
        return Err(format!(
            "routing projection belongs to {}, not {}; use a project-local database or rebind explicitly with `vyrm reset-routing --root {}`",
            stored.root.display(),
            expected_root.display(),
            expected_root.display()
        )
        .into());
    }
    let bytes = serde_json::to_vec(&stored.index)?;
    Index::from_bytes(&bytes).map_err(|error| {
        format!(
            "routing index is unreadable: {error}; recover with `vyrm reset-routing --root {}`",
            expected_root.display()
        )
        .into()
    })
}

/// Establishes and persists routing freshness for `root`.
#[tracing::instrument(level = "debug", skip_all)]
pub fn ensure_routing_fresh<E: Engine>(
    store: &E,
    root: &Path,
) -> Result<RoutingReady, Box<dyn std::error::Error>> {
    let root = canonical_root(root)?;
    let profile = Profile::attune(&root)?;
    let stored = store.get_projection(ROUTING_PROJECTION)?;
    let (mut index, origin) = match stored {
        Some(bytes) => (decode(&bytes, &root)?, RoutingOrigin::Refreshed),
        None => (Index::default(), RoutingOrigin::Built),
    };
    let refresh = index.refresh_strict(&profile)?;

    // A touched-but-identical file updates the stat cache even though the
    // content generation is unchanged, so that state is worth persisting too.
    if origin == RoutingOrigin::Built || !refresh.is_noop() || refresh.read_but_identical > 0 {
        store.put_projection(ROUTING_PROJECTION, &encode(&root, &index)?)?;
    }

    Ok(RoutingReady {
        origin,
        files: index.file_count(),
        symbols: index.symbol_count(),
        generation: index.generation(),
        refresh,
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
        .map(|bytes| decode(&bytes, &root))
        .transpose()
}

/// Explicit recovery and root-rebinding path. Unlike the freshness barrier,
/// this intentionally discards the prior derived state and rebuilds it.
#[tracing::instrument(level = "debug", skip_all)]
pub fn reset_routing<E: Engine>(
    store: &E,
    root: &Path,
) -> Result<RoutingReady, Box<dyn std::error::Error>> {
    let root = canonical_root(root)?;
    let profile = Profile::attune(&root)?;
    let mut index = Index::default();
    let refresh = index.refresh_strict(&profile)?;
    store.put_projection(ROUTING_PROJECTION, &encode(&root, &index)?)?;
    Ok(RoutingReady {
        origin: RoutingOrigin::Built,
        files: index.file_count(),
        symbols: index.symbol_count(),
        generation: index.generation(),
        refresh,
    })
}
