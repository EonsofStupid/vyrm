//! RRFlowQL syntax, deterministic binding, planning, and execution.
//!
//! The implementation begins from an exact stamped authoritative-log scan and
//! admits narrower authoritative paths only when the bound query proves them
//! equivalent. Faster projections may compete later, but must publish
//! freshness evidence and pass differential verification first.

mod arrow;
mod bm25;
mod catalog;
mod error;
mod execute;
mod fusion;
mod index;
mod live;
mod plan;
mod syntax;

pub use arrow::{record_batch_to_rows, rows_to_record_batch};
pub use bm25::{
    Bm25Analyzer, Bm25Artifact, Bm25Config, Bm25Document, Bm25Hit, Bm25Posting,
    BM25_ARTIFACT_CONTRACT_VERSION,
};
pub use catalog::{Catalog, SchemaVersion, SourceWatermarks};
pub use error::{Error, Result};
pub use execute::{execute, ExecutionBudget, QueryBatch, QueryExecution, QueryRow};
pub use fusion::execute_snapshot;
pub use index::{
    IndexArtifact, IndexArtifactPublication, IndexCatalogue, IndexCatalogueRepository,
    IndexDefinition, IndexEntry, IndexKind, IndexMutationContext, IndexOperationReceipt,
    INDEX_ARTIFACT_CONTRACT_VERSION, INDEX_CATALOGUE_CONTRACT_VERSION,
};
pub use live::{poll_live_query, LiveQueryBudget, LiveQueryDelta, LiveRowChange};
pub use plan::{
    bind, plan, BoundFilter, BoundIndexCandidate, BoundQuery, CandidatePath, ExecutionContract,
    LogicalOperator, LogicalPlan, Parameters, PhysicalOperator, PhysicalPlan, PlanExplanation,
    QueryFieldTypes,
};
pub use syntax::*;
