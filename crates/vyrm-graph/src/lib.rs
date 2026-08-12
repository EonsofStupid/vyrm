//! # vyrm-graph
//!
//! The routing projection. `SPEC.md` §8.2.
//!
//! A projection derived from source: what exists, where it is declared, and what
//! reaches it. Queries route to a ranked **file list** with justification, and
//! the consuming rule is that those files are read in full.
//!
//! ## Why file lists
//!
//! Fragments are why an agent authors a new document beside an existing one
//! rather than extending it: it never saw the whole of what already existed.
//! Complete reads remove that failure mode, but cost tokens, so the value rests
//! entirely on routing precision — see `route::Index::route`.
//!
//! ## Attunement
//!
//! Deployment into a project must not require configuration. [`profile::Profile`]
//! derives the project's shape from its own manifests and file extensions, and
//! records the evidence for each conclusion.

pub mod profile;
pub mod route;
pub mod symbols;

pub use profile::{Language, Profile};
pub use route::{attune_and_index, Grounding, Index, IndexedFile, Justification, Refresh, RoutedFile};
pub use symbols::{Occurrence, Role};
