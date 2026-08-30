//! # RRD reasoning runtime
//!
//! The runtime experience (`PLAN.md` Step P). Everything below `rrflow` is a
//! library an agent must *choose* to call; this module is the layer that makes
//! choosing unnecessary. A harness lifecycle event arrives (session start, a
//! prompt, a tool call, a turn end), and the memory layer answers before the
//! model reasons — recall injected, application runs journaled, a quarantined
//! projection enforced as a gate rather than hoped about in prose.
//!
//! Three axes, never conflated (`PLAN.md` Step P): **harness** (the agent
//! runtime this module adapts to, [`registry`]), **provider** (the model
//! backend), and **billing mode** (subscription quota vs per-usage tokens).
//! An adapter's verification is a bi-temporal claim with an expiry, so the
//! registry audits itself through `resolve_as_of` rather than a scheduler.
//!
//! The 2026 field converged on the lesson this crate encodes: memory is
//! placed into context by the runtime, deterministically — not left to the
//! model's discipline. Writes ride Buffered durability and never block the
//! turn; recall is the only synchronous path.

pub mod adapters;
pub mod architecture;
mod attunement;
pub mod cluster_transfer;
pub mod context;
pub mod data_plane;
pub mod execution;
pub mod hook;
pub mod init;
pub mod instance;
pub mod lifecycle;
pub mod operator_knowledge;
pub mod policy;
pub mod preflight;
pub mod query;
pub mod reasoning;
pub mod registry;
pub mod routing;
pub mod stack;
mod tools;
pub mod trace;
pub mod vector_catalog;
pub mod vector_residency;
pub mod workflow;
pub mod workplan;

pub use adapters::{
    adapter_conformance_fixtures, adapter_coverage, classify_adapter_tool,
    normalize_adapter_fixture, normalize_hook_input, normalize_tool_proposal,
    AdapterCoverageEntryV1, AdapterCoveragePointV1, AdapterCoverageStateV1,
    AdapterCoverageVectorV1, AdapterFixtureDocumentV1, AdapterFixtureExpectedV1, AdapterFixtureV1,
    AdapterKindV1, AdapterMutationClassV1, AdapterToolClass, CanonicalAdapterEventV1,
    ADAPTER_CONFORMANCE_FORMAT_VERSION,
};
pub use architecture::{
    load_architecture_review, review_architecture, reviewed_golden_patterns, ArchitectureAnswerV1,
    ArchitectureAssessmentV1, ArchitectureBoundaryDecisionV1, ArchitectureBoundaryKindV1,
    ArchitectureEvidenceKindV1, ArchitectureEvidenceV1, ArchitectureImpactDispositionV1,
    ArchitectureImplicationV1, ArchitectureReview, BoundaryChoiceAnswerV1,
    CapabilityOwnershipAnswerV1, CrossBoundaryImplicationsAnswerV1, GoldenPatternSelectionV1,
    PathOwnershipAnswerV1, PatternChoiceAnswerV1, PublicContractAnswerV1, RejectedGoldenPatternV1,
    ReviewedGoldenPattern, SelectedGoldenPatternV1, VerificationMatrixAnswerV1,
    ARCHITECTURE_ASSESSMENT_FORMAT_VERSION, ARCHITECTURE_REVIEW_FORMAT,
    GOLDEN_PATTERN_REGISTRY_REVISION, GOLDEN_PATTERN_SELECTION_PROJECTION,
};
pub use attunement::{
    authorize_attuned_tool, complete_attuned_tool, consume_attuned_tool_authorization,
    load_attunement_receipt, record_attunement_receipt, require_attuned_tool_authorization,
    require_fresh_attunement, PlanningSourceFingerprint, ProjectAttunementReceipt,
};
pub use cluster_transfer::{
    execute_traced_artifact_transfer, record_artifact_transfer_observation,
    DurableArtifactTransferObserver,
};
pub use context::{
    classify_task, load_task_preflight_receipt, ContextSourceRoute, EvidenceDisposition,
    NamedReadStamp, TaskClassification, TaskPreflightReceipt, TASK_PREFLIGHT_RECEIPT_FORMAT,
    TASK_PREFLIGHT_RECEIPT_PROJECTION,
};
pub use data_plane::{
    execute_traced_embedding, execute_traced_vector_search, TracedEmbeddingExecution,
    TracedVectorSearch,
};
pub use execution::{
    load_exact_execution_observation, record_exact_execution_observation,
    ExactExecutionObservationV1, ExactExecutionRepositoryV1, ExactExecutionRequestV1,
    ExactExecutionStreamV1, EXACT_EXECUTION_CONTRACT, MAX_EXACT_EXECUTION_ARGV,
    MAX_EXACT_EXECUTION_ARG_BYTES, MAX_EXACT_EXECUTION_CHANGED_PATHS,
    MAX_EXACT_EXECUTION_OUTPUT_BYTES, MAX_EXACT_EXECUTION_TIMEOUT_MS,
};
pub use hook::{handle, HookContext, HookEvent, HookResponse};
pub use init::{init, InitReport, STORE_DIR};
pub use instance::{
    InstanceBinding, InstanceManifest, InstanceMode, ProjectAuthorityBinding, INSTANCE_FILE,
    INSTANCE_FORMAT, PROJECT_AUTHORITY_FORMAT,
};
pub use lifecycle::{
    append_lifecycle_event, authorize_lifecycle_tool, authorize_planned_lifecycle_tool,
    complete_lifecycle_tool, complete_planned_lifecycle_tool, consume_lifecycle_tool_authorization,
    load_active_lifecycle_tool_authorization, load_lifecycle_events, load_lifecycle_session,
    refresh_lifecycle_projection, LifecycleEnforcementLevelV1, LifecycleEventCommandV1,
    LifecycleEventEnvelopeV1, LifecycleEventTypeV1, LifecyclePayloadV1, LifecyclePhaseV1,
    LifecycleProjectionRefreshV1, LifecycleReadStampV1, LifecycleRiskV1,
    LifecycleSessionSnapshotV1, LifecycleSupervisorContextV1, LifecycleTaskKindV1,
    LifecycleToolAuthorizationV1, LifecycleToolCompletionV1, LifecycleToolRequestV1,
    LifecycleTraceContextV1, LifecycleTurnStatusV1, LIFECYCLE_RUNTIME_EVENT_TYPE,
    LIFECYCLE_RUNTIME_SESSION_TYPE,
};
pub use operator_knowledge::{
    execute_traced_operator_search, execute_traced_operator_sync, TracedOperatorSearch,
    TracedOperatorSync,
};
pub use policy::{evaluate_tool, ContractDifferential, ToolPolicy};
pub use preflight::{preflight, preflight_task, Preflight};
pub use query::{
    execute_traced_query, query_parameters_from_json, ExecutionBudget, Parameters,
    TracedQueryExecution,
};
pub use reasoning::{
    active_reasoning_run, reasoning_run, reasoning_runs, record_reasoning, REASONING_SCOPE,
};
pub use registry::{Harness, HookProtocol, Registry, Verification, VERIFICATION_TTL_MS};
pub use routing::{
    ensure_routing_fresh, load_project_artifacts, load_routing, reset_routing, RoutingReady,
};
pub use stack::{
    detect, package_run_event, package_run_event_argv, PackageManager, PackageRunEvent,
    StackProfile,
};
pub use tools::{
    is_runtime_tool, mcp_task_catalogue, runtime_tool_catalogue, runtime_tool_contract_catalogue,
    runtime_tool_operation, McpTaskCatalogue, McpTaskDisposition, McpTaskDomain,
    RuntimeToolDefinition, RuntimeToolLifecyclePolicy, RuntimeToolResult,
    MCP_TASK_CATALOGUE_VERSION,
};
pub use trace::{
    install_runtime_trace_contract, record_runtime_trace, DurableTraceSpan, TraceIdentity,
};
pub(crate) use vector_catalog::vector_artifact_catalog_entries_from_changes;
pub use vector_catalog::{
    build_traced_quantization_artifact, publish_traced_vector_artifact,
    publish_traced_vector_artifact_with_evidence, quantization_artifact_catalogue,
    reopen_vector_runtime, reopen_vector_runtime_metadata, transition_traced_quantization_artifact,
    vector_artifact_catalog_entries, QuantizationArtifactPublication,
    QuantizationArtifactTransition, VectorArtifactBinding, VectorArtifactPublication,
    VectorArtifactResidencyKey, VectorRuntimeManifest,
};
pub use vector_residency::{
    VectorResidencyAcquisition, VectorResidencyError, VectorResidencyLimits,
    VectorResidencyManager, VectorResidencySnapshot, VectorResidencySource,
    DEFAULT_VECTOR_CACHED_BYTES, DEFAULT_VECTOR_PINNED_BYTES,
};
pub use workflow::{
    resolve_package_argv, resolve_package_command, VerificationPolicy, WorkflowAuthorization,
    WorkflowCatalog, WorkflowDecision, WorkflowDifferential, WorkflowManifest, WorkflowObservation,
    WorkflowPreflight, WorkflowRule, WorkflowStatus, WORKFLOW_FILE, WORKFLOW_FORMAT,
};
pub use workplan::{
    activate_work_item, authorize_work_item_tool, complete_work_item_tool, install_work_plan,
    load_active_work_item_authorization, load_active_work_item_plan, load_work_item_verification,
    load_work_plan, read_work_plan, record_project_work_item_plan, record_work_item_plan,
    sync_project_work_plan, verify_recorded_work_item, verify_work_item, WorkItemExecutionMode,
    WorkItemPlanRecord, WorkItemStatus, WorkItemToolAuthorization, WorkItemVerification,
    WorkItemVerificationArtifact, WorkItemVerificationCheck, WorkPlanEventContext,
    WorkPlanOperation, WorkPlanSnapshot, WORK_PLAN_FILE,
};
