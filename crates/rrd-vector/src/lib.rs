//! Exact semantic oracle and projection contracts for Rrd vector search.
//!
//! The exact path is truth. Approximate indexes may propose candidates later,
//! but must publish coverage/freshness evidence and are measured against this
//! crate before a planner may select them.

mod catalog;
mod collection;
mod compact;
mod contract;
mod exact;
mod filter;
mod hnsw;
mod plan;
mod quantization;
mod quantization_catalog;
mod quantized_segment;
mod runtime;
mod segment;
mod turbo_segment;
mod turboquant;

#[cfg(feature = "accelerator")]
mod accelerator;

#[cfg(feature = "accelerator")]
pub use accelerator::{
    build_dense_artifact, AcceleratedBuildPolicy, AcceleratorTarget, DenseArtifactBuilder,
    DenseBuildBackend, DenseBuildOutcome,
};

pub use catalog::{
    VectorArtifactCatalogEntry, VectorCatalog, VectorProjectionDescriptor,
    VECTOR_ARTIFACT_CATALOG_VERSION, VECTOR_ARTIFACT_RECORD_TYPE,
};
pub use collection::{
    CollectionCatalogue, CollectionDeletionReceipt, CollectionEntry, CollectionError,
    CollectionMutationContext, CollectionOperationReceipt, NamedVectorConfig,
    PayloadIndexDefinition, PayloadIndexEntry, PayloadIndexKind, PayloadIndexOperationReceipt,
    VectorCollectionDefinition, VectorCollectionRepository, VectorMemoryTier, VectorValueKind,
    VECTOR_COLLECTION_CATALOGUE_VERSION,
};
pub use compact::{
    CompactDenseSegment, DenseKernel, DenseMemoryPlacement, COMPACT_DENSE_FORMAT_VERSION,
};
pub use contract::{
    EmbeddingModelBinding, MultiVectorComparator, ScoreMetric, SearchHit, SearchMode,
    SearchRequest, VectorCandidate, VectorQuery, VectorVisibilityRequest,
};
pub use exact::{
    candidates_from_changes, materialize_visible, score_query, search_changes_exact, search_exact,
    search_exact_ref,
};
pub use filter::{FilterCondition, FilterExpression, FilterOperator};
pub use hnsw::{
    HnswConfig, HnswDescriptor, HnswIndex, HnswKernel, HnswMaintenanceKind, HNSW_FORMAT_VERSION,
};
pub use plan::{
    AccessPathKind, CandidatePath, PlanDecision, RejectedPath, SearchPlan, VectorPlanner,
    EXACT_SCAN_PROJECTION_ID,
};
pub use quantization::ScalarQuantizedVector;
pub use quantization_catalog::{
    QuantizationArtifactCatalogue, QuantizationArtifactEntry, QuantizationArtifactSnapshot,
    QuantizationArtifactState, QuantizationLifecycleAction, QuantizationLifecycleEvent,
    QUANTIZATION_ARTIFACT_RECORD_TYPE, QUANTIZATION_CATALOG_VERSION,
    QUANTIZATION_LIFECYCLE_RECORD_TYPE,
};
pub use quantized_segment::{
    ProductCompression, QuantizationMethod, QuantizedDescriptor, QuantizedKernel,
    QuantizedMemoryPlacement, QuantizedSegment, QuantizedSegmentConfig,
    QUANTIZED_SEGMENT_FORMAT_VERSION,
};
pub use runtime::{
    PreparedVectorSearch, SearchExecution, VectorArtifact, VectorArtifactKind, VectorRuntime,
};
pub use segment::{
    ImmutableVectorSegment, SegmentDescriptor, VectorSegmentConfig, VECTOR_SEGMENT_FORMAT_VERSION,
};
pub use turbo_segment::{
    TurboQuantDescriptor, TurboQuantSegment, TurboQuantSegmentConfig,
    TURBOQUANT_SEGMENT_FORMAT_VERSION,
};
pub use turboquant::{TurboQuantBits, TurboQuantVector, TURBOQUANT_FORMAT_VERSION};
