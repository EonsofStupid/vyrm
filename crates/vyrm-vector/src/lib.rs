//! Exact semantic oracle and projection contracts for Vyrm vector search.
//!
//! The exact path is truth. Approximate indexes may propose candidates later,
//! but must publish coverage/freshness evidence and are measured against this
//! crate before a planner may select them.

mod catalog;
mod compact;
mod contract;
mod exact;
mod filter;
mod hnsw;
mod plan;
mod quantization;
mod runtime;
mod segment;

#[cfg(feature = "accelerator")]
mod accelerator;

#[cfg(feature = "accelerator")]
pub use accelerator::{
    build_dense_artifact, AcceleratedBuildPolicy, AcceleratorTarget, DenseArtifactBuilder,
    DenseBuildBackend, DenseBuildOutcome,
};

pub use catalog::{VectorCatalog, VectorProjectionDescriptor};
pub use compact::{
    CompactDenseSegment, DenseKernel, DenseMemoryPlacement, COMPACT_DENSE_FORMAT_VERSION,
};
pub use contract::{
    EmbeddingModelBinding, MultiVectorComparator, ScoreMetric, SearchHit, SearchMode,
    SearchRequest, VectorCandidate, VectorQuery,
};
pub use exact::{candidates_from_changes, search_changes_exact, search_exact, search_exact_ref};
pub use filter::{FilterCondition, FilterExpression, FilterOperator};
pub use hnsw::{HnswConfig, HnswDescriptor, HnswIndex, HNSW_FORMAT_VERSION};
pub use plan::{
    AccessPathKind, CandidatePath, PlanDecision, RejectedPath, SearchPlan, VectorPlanner,
    EXACT_SCAN_PROJECTION_ID,
};
pub use quantization::ScalarQuantizedVector;
pub use runtime::{SearchExecution, VectorArtifact, VectorRuntime};
pub use segment::{
    ImmutableVectorSegment, SegmentDescriptor, VectorSegmentConfig, VECTOR_SEGMENT_FORMAT_VERSION,
};
