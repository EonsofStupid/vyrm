//! `vyrmMX`: deterministic binding, planning, and reference execution.
//!
//! The implementation begins from an exact stamped authoritative-log scan and
//! admits narrower authoritative paths only when the bound query proves them
//! equivalent. Faster projections may compete later, but must publish
//! freshness evidence and pass differential verification first.

mod catalog;
mod error;
mod execute;
mod index;
mod live;
mod plan;

pub use catalog::{Catalog, SchemaVersion};
pub use error::{Error, Result};
pub use execute::{execute, ExecutionBudget, QueryBatch, QueryExecution, QueryRow};
pub use index::{
    IndexArtifact, IndexArtifactPublication, IndexCatalogue, IndexCatalogueRepository,
    IndexDefinition, IndexEntry, IndexMutationContext, INDEX_ARTIFACT_CONTRACT_VERSION,
    INDEX_CATALOGUE_CONTRACT_VERSION,
};
pub use live::{poll_live_query, LiveQueryBudget, LiveQueryDelta, LiveRowChange};
pub use plan::{
    bind, plan, BoundFilter, BoundIndexCandidate, BoundQuery, CandidatePath, ExecutionContract,
    LogicalOperator, LogicalPlan, Parameters, PhysicalOperator, PhysicalPlan, PlanExplanation,
};
