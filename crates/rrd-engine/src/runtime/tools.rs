//! Engine-owned execution boundary for cooperative runtime tools.
//!
//! MCP and future provider adapters translate transport envelopes only. Tool
//! semantics, persistence access, recall observation, lifecycle handling, and
//! the authoritative invocation ledger remain inside RRD.

use super::{
    active_reasoning_run, append_lifecycle_event, ensure_routing_fresh, execute_traced_query,
    handle, load_routing, preflight, query_parameters_from_json, reasoning_run, record_reasoning,
    ExecutionBudget, HookContext, HookEvent, InstanceBinding, REASONING_SCOPE,
};
use crate::{
    load_or_create_token_key, product_capability_catalogue, Invocation, InvocationCompletion,
    InvocationCredential, RrdEngine, RrdOperation, ServiceError, ServiceErrorKind,
};
use rrd_contract::{
    endpoint_catalogue, transaction_operation_sha256, AuditDecision, BeginTransaction, CanonicalId,
    CommitTransaction, CorrelationId, CreateInstanceBackup, EnsureQueryIndex,
    EnsureVectorCollection, FollowChangefeed, ForwardRollbackReceipt, ForwardRollbackRequest,
    LifecycleEventCommandV1, ListInstanceBackups, ListQueryIndexes, ListVectorCollections,
    PollLiveQuery, ReadAudit, ReadChangefeed, RequestContext, RestoreInstanceBackup,
    RetrieveVectorPoints, RuntimeToolCatalogue, RuntimeToolDescriptor, RuntimeToolInvocation,
    RuntimeToolInvocationResult, ScrollVectorPoints, SearchVectors, SecurityAction,
    TransactionMutation, MAX_LEASE_MS, MIN_LEASE_MS, PROTOCOL, PROTOCOL_VERSION,
    RUNTIME_TOOL_CATALOGUE_VERSION,
};
pub use rrd_contract::{RuntimeToolAuthorization, RuntimeToolLifecyclePolicy};
use rrd_core::{
    digest, Claim, ClaimReader, ClaimSource, Predicate, Producer, Reader, ReasoningPayload,
    RecallQuery, ScopeId, Subject,
};
use rrd_store::{
    Effectiveness, Engine, InvocationInput as OperatorInvocationInput, Outcome as OperatorOutcome,
    RecallOutcome, Trigger as OperatorTrigger,
};
use schemars::{schema_for, JsonSchema};
use serde_json::{json, Value};
use std::path::Path;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

mod adapter;
mod lifecycle;
mod workplan;

use adapter::{AdapterCaller, AdapterSession};
use lifecycle::RuntimeToolLifecycle;

const LOCAL_TOKEN_KEY_FILE: &str = "RRD.SECRET";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeToolResult {
    pub text: String,
    pub detail: Option<String>,
}

/// One engine-owned cooperative tool contract.
///
/// Adapters serialize this catalogue directly. Keeping the schema beside the
/// executable dispatch prevents MCP, Connectome, and future provider adapters
/// from advertising a tool that the RRD engine cannot execute.
#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeToolDefinition {
    pub name: &'static str,
    /// Existing engine capability implemented by this adapter. `None` means
    /// the runtime tool is itself the authoritative capability row.
    pub capability_id: Option<&'static str>,
    pub description: &'static str,
    pub input_schema: Value,
    pub mutation: bool,
    pub authorization: RuntimeToolAuthorization,
    pub action: SecurityAction,
    pub lifecycle: RuntimeToolLifecyclePolicy,
}

#[derive(JsonSchema)]
#[allow(dead_code)]
struct ServiceStatusArguments {
    /// Observation time; defaults to the adapter invocation time.
    at: Option<u64>,
}

#[derive(Debug, Clone, serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct LifecycleApplyArguments {
    /// Strict provider-neutral lifecycle command to append to RRD.
    command: LifecycleEventCommandV1,
    /// Durable observation time; defaults to the adapter invocation time.
    recorded_at_unix_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, serde::Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
enum HookApplyEvent {
    SessionStart,
    UserPromptSubmit,
    PreToolUse,
    PostToolUse,
    Stop,
    PreCompact,
}

impl HookApplyEvent {
    fn runtime_event(self) -> HookEvent {
        match self {
            Self::SessionStart => HookEvent::SessionStart,
            Self::UserPromptSubmit => HookEvent::UserPromptSubmit,
            Self::PreToolUse => HookEvent::PreToolUse,
            Self::PostToolUse => HookEvent::PostToolUse,
            Self::Stop => HookEvent::Stop,
            Self::PreCompact => HookEvent::PreCompact,
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct HookApplyArguments {
    event: HookApplyEvent,
    input: Value,
    at: Option<u64>,
    budget: Option<usize>,
}

#[derive(Debug, Clone, serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct DataCommitArguments {
    /// Stable caller identity for retrying this exact ordered mutation set.
    idempotency_key: CorrelationId,
    /// Ordered multi-model mutations committed under one RRD read stamp.
    mutations: Vec<TransactionMutation>,
    /// Transaction timeout; defaults to 60 seconds.
    timeout_ms: Option<u64>,
    /// Logical operation time; defaults to adapter invocation time.
    at: Option<u64>,
}

#[derive(Debug, Clone, serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct DataRollbackArguments {
    /// Stable caller identity for retrying this exact rollback.
    idempotency_key: CorrelationId,
    /// Modeled valid-time instant whose structural state will be restored.
    target_valid_at: u64,
    /// Transaction-time prefix that defined the selected historical state.
    target_known_at_cursor: u64,
    /// New valid-time instant at which compensating versions become effective.
    effective_at: u64,
    /// Human operator reason; persisted by digest in immutable rollback evidence.
    reason: String,
    /// Transaction timeout; defaults to 60 seconds.
    timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct QueryIndexEnsureArguments {
    /// Stable caller identity for retrying this exact index definition.
    idempotency_key: CorrelationId,
    #[serde(flatten)]
    request: EnsureQueryIndex,
    /// Logical operation time; defaults to adapter invocation time.
    at: Option<u64>,
}

#[derive(Debug, Clone, serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct QueryIndexListArguments {
    #[serde(flatten)]
    request: ListQueryIndexes,
    /// Logical observation time; defaults to adapter invocation time.
    at: Option<u64>,
}

#[derive(Debug, Clone, serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct LiveQueryPollArguments {
    #[serde(flatten)]
    request: PollLiveQuery,
    /// Logical observation time; defaults to adapter invocation time.
    at: Option<u64>,
}

#[derive(Debug, Clone, serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ChangefeedReadArguments {
    #[serde(flatten)]
    request: ReadChangefeed,
    /// Logical observation time; defaults to adapter invocation time.
    at: Option<u64>,
}

#[derive(Debug, Clone, serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ChangefeedFollowArguments {
    #[serde(flatten)]
    request: FollowChangefeed,
    /// Logical observation time; defaults to adapter invocation time.
    at: Option<u64>,
}

#[derive(Debug, Clone, serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct VectorCollectionEnsureArguments {
    /// Stable caller identity for retrying this exact collection definition.
    idempotency_key: CorrelationId,
    #[serde(flatten)]
    request: EnsureVectorCollection,
    /// Logical operation time; defaults to adapter invocation time.
    at: Option<u64>,
}

#[derive(Debug, Clone, serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct VectorCollectionListArguments {
    #[serde(flatten)]
    request: ListVectorCollections,
    /// Logical observation time; defaults to adapter invocation time.
    at: Option<u64>,
}

#[derive(Debug, Clone, serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct VectorPointsRetrieveArguments {
    #[serde(flatten)]
    request: RetrieveVectorPoints,
    /// Logical observation time; defaults to adapter invocation time.
    at: Option<u64>,
}

#[derive(Debug, Clone, serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct VectorPointsScrollArguments {
    #[serde(flatten)]
    request: ScrollVectorPoints,
    /// Logical observation time; defaults to adapter invocation time.
    at: Option<u64>,
}

#[derive(Debug, Clone, serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct VectorSearchArguments {
    #[serde(flatten)]
    request: SearchVectors,
    /// Logical observation time; defaults to adapter invocation time.
    at: Option<u64>,
}

#[derive(Debug, Clone, serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct BackupCreateArguments {
    /// Stable caller identity for retrying this exact logical backup.
    idempotency_key: CorrelationId,
    #[serde(flatten)]
    request: CreateInstanceBackup,
    /// Logical operation time; defaults to adapter invocation time.
    at: Option<u64>,
}

#[derive(Debug, Clone, serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct BackupListArguments {
    #[serde(flatten)]
    request: ListInstanceBackups,
    /// Logical observation time; defaults to adapter invocation time.
    at: Option<u64>,
}

#[derive(Debug, Clone, Copy, serde::Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum RestoreConfirmation {
    RestoreToNewRoot,
}

#[derive(Debug, Clone, serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RestoreArguments {
    /// Stable caller identity for retrying this exact restore.
    idempotency_key: CorrelationId,
    /// Required acknowledgement that RRD creates an isolated restore root and
    /// never overwrites the active database.
    confirmation: RestoreConfirmation,
    #[serde(flatten)]
    request: RestoreInstanceBackup,
    /// Logical operation time; defaults to adapter invocation time.
    at: Option<u64>,
}

#[derive(Debug, Clone, serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct EstateReadArguments {
    estate_id: CanonicalId,
    /// Logical observation time; defaults to adapter invocation time.
    at: Option<u64>,
}

#[derive(Debug, Clone, serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct AuditReadArguments {
    #[serde(flatten)]
    request: ReadAudit,
    /// Logical observation time; defaults to adapter invocation time.
    at: Option<u64>,
}

struct ExecutedTool {
    text: String,
    effectiveness: Option<Effectiveness>,
    detail: Option<String>,
}

impl RrdEngine {
    /// Opens the one embedded RRD authority bound to a discovered instance.
    pub fn open_bound(binding: &InstanceBinding) -> crate::Result<Self> {
        binding
            .require_runtime_ready()
            .map_err(|error| ServiceError::Contract(error.to_string()))?;
        let instance = CanonicalId::new(binding.manifest.id.clone())
            .map_err(|error| ServiceError::Contract(error.to_string()))?;
        let database = binding.expected_store();
        // Establish the authenticated native storage identity before placing
        // the engine-owned token file inside it. Creating the token first
        // would make a new non-empty directory look like a legacy substrate.
        let mut engine = Self::open(&database, instance, [0_u8; 32])?;
        let token_key = load_or_create_token_key(&database.join(LOCAL_TOKEN_KEY_FILE))
            .map_err(|error| ServiceError::Storage(error.to_string()))?;
        engine.token_key = token_key;
        engine.bind_project_authority(binding, wall_clock_millis())?;
        Ok(engine)
    }

    /// Opens RRD with operator-selected token material while retaining the same
    /// immutable project-authority binding as the default embedded opener.
    pub fn open_bound_with_token_key(
        binding: &InstanceBinding,
        instance: CanonicalId,
        token_key: [u8; 32],
        at: u64,
    ) -> crate::Result<Self> {
        binding
            .require_runtime_ready()
            .map_err(|error| ServiceError::Contract(error.to_string()))?;
        binding
            .verify_store_path(&binding.expected_store())
            .map_err(|error| ServiceError::Contract(error.to_string()))?;
        let engine = Self::open(&binding.expected_store(), instance, token_key)?;
        engine.bind_project_authority(binding, at)?;
        Ok(engine)
    }

    /// Executes one named runtime tool and records its outcome atomically in
    /// the same engine authority used by every other embedded or daemon face.
    pub fn call_runtime_tool(
        &self,
        root: &Path,
        name: &str,
        args: &Value,
        at: u64,
    ) -> crate::Result<RuntimeToolResult> {
        self.call_runtime_tool_inner(root, name, args, at, AdapterCaller::Anonymous)
    }

    /// Executes one validated public runtime-tool invocation while preserving
    /// the transport caller through policy, nested engine sessions, and audit.
    pub fn invoke_runtime_tool(
        &self,
        root: &Path,
        request: &RuntimeToolInvocation,
        invocation: Invocation,
        credential: InvocationCredential<'_>,
    ) -> crate::Result<RuntimeToolInvocationResult> {
        request
            .validate()
            .map_err(|error| ServiceError::Contract(error.to_string()))?;
        let expected_request_sha256 = digest::sha256_hex(
            &serde_json::to_vec(request)
                .map_err(|error| ServiceError::Contract(error.to_string()))?,
        );
        if invocation.request_sha256 != expected_request_sha256 {
            return Err(ServiceError::Contract(
                "runtime invocation request digest does not match its payload".into(),
            ));
        }
        let name = request.tool.as_str();
        let definition = runtime_tool_catalogue()
            .into_iter()
            .find(|definition| definition.name == name)
            .ok_or_else(|| ServiceError::Contract(format!("unknown runtime tool {name:?}")))?;
        let operation = runtime_tool_operation_for_action(definition.action)?;
        let caller = adapter_caller(credential);
        let authorized = if definition.authorization == RuntimeToolAuthorization::Public
            && matches!(credential, InvocationCredential::Anonymous)
        {
            None
        } else {
            Some(self.begin_invocation(invocation.clone(), operation, credential)?)
        };
        let result = self.call_runtime_tool_inner(
            root,
            name,
            &request.arguments,
            invocation.observed_at_unix_ms,
            caller,
        );
        let (decision, status_code, response_sha256) = runtime_tool_completion(&result);
        let completion = InvocationCompletion {
            decision,
            status_code,
            response_sha256,
        };
        match authorized {
            Some(authorized) => self.complete_invocation(&authorized, completion)?,
            None => self.record_public_invocation(invocation, operation, completion)?,
        }
        let result = result?;
        let result = RuntimeToolInvocationResult {
            catalogue_version: RUNTIME_TOOL_CATALOGUE_VERSION,
            tool: request.tool.clone(),
            arguments_sha256: request.arguments_sha256.clone(),
            content_sha256: digest::sha256_hex(result.text.as_bytes()),
            content: result.text,
            detail: result.detail,
        };
        result
            .validate()
            .map_err(|error| ServiceError::Contract(error.to_string()))?;
        Ok(result)
    }

    fn call_runtime_tool_inner(
        &self,
        root: &Path,
        name: &str,
        args: &Value,
        at: u64,
        caller: AdapterCaller<'_>,
    ) -> crate::Result<RuntimeToolResult> {
        let definition = runtime_tool_catalogue()
            .into_iter()
            .find(|definition| definition.name == name)
            .ok_or_else(|| ServiceError::Contract(format!("unknown runtime tool {name:?}")))?;
        if definition.authorization == RuntimeToolAuthorization::Governed
            && self.security_enforced()?
            && matches!(caller, AdapterCaller::Anonymous)
        {
            return Err(ServiceError::Unauthenticated);
        }

        let (execution_args, lifecycle) =
            RuntimeToolLifecycle::prepare(name, args, definition.lifecycle)?;
        lifecycle.authorize(&self.storage, root, at)?;

        let started = Instant::now();
        let mut result = execute_runtime_tool(self, root, name, &execution_args, at, caller);
        if definition.lifecycle == RuntimeToolLifecyclePolicy::PlannedMutation {
            let response = match &result {
                Ok(_) => json!({"success": true}),
                Err(error) => json!({"success": false, "error": error.to_string()}),
            };
            if let Err(error) = lifecycle.complete(&self.storage, root, response, at) {
                result = Err(format!(
                    "runtime tool execution could not close its lifecycle observation: {error}"
                )
                .into());
            }
        }
        let duration_ms = started.elapsed().as_millis() as u64;
        let (outcome, detail, effectiveness) = match &result {
            Ok(result) => (
                OperatorOutcome::Ok,
                result.detail.clone(),
                result.effectiveness.clone(),
            ),
            Err(error) => (OperatorOutcome::Error, Some(error.to_string()), None),
        };
        let arguments = invocation_arguments(name, args);
        self.record_operator_invocation(OperatorInvocationInput {
            at,
            trigger: OperatorTrigger::Manual,
            command: &format!("mcp:{name}"),
            arguments: &arguments,
            outcome,
            duration_ms,
            detail,
            effectiveness,
        })
        .map_err(|error| ServiceError::Runtime(error.to_string()))?;

        result
            .map(|result| RuntimeToolResult {
                text: result.text,
                detail: result.detail,
            })
            .map_err(|error| ServiceError::Runtime(error.to_string()))
    }
}

fn wall_clock_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .max(1) as u64
}

fn adapter_caller(credential: InvocationCredential<'_>) -> AdapterCaller<'_> {
    match credential {
        InvocationCredential::Anonymous => AdapterCaller::Anonymous,
        InvocationCredential::ApiKey {
            principal_id,
            credential,
        } => AdapterCaller::ApiKey {
            principal_id,
            credential,
        },
        InvocationCredential::Jwt { token, signing_key } => {
            AdapterCaller::Jwt { token, signing_key }
        }
        InvocationCredential::Session { session_id, token } => {
            AdapterCaller::Session { session_id, token }
        }
    }
}

/// Resolves a public tool name to the exact engine operation used for policy
/// and audit. Transports use this only to attribute malformed envelopes after
/// the tool name is known; execution resolves the same catalogue again.
pub fn runtime_tool_operation(name: &str) -> crate::Result<RrdOperation> {
    let definition = runtime_tool_catalogue()
        .into_iter()
        .find(|definition| definition.name == name)
        .ok_or_else(|| ServiceError::Contract(format!("unknown runtime tool {name:?}")))?;
    runtime_tool_operation_for_action(definition.action)
}

fn runtime_tool_operation_for_action(action: SecurityAction) -> crate::Result<RrdOperation> {
    let operation = match action {
        SecurityAction::ServiceInspect => RrdOperation::ServiceInspect,
        SecurityAction::QueryExecute => RrdOperation::QueryExecute,
        SecurityAction::QueryLivePoll => RrdOperation::QueryLivePoll,
        SecurityAction::QueryIndexEnsure => RrdOperation::QueryIndexEnsure,
        SecurityAction::QueryIndexList => RrdOperation::QueryIndexList,
        SecurityAction::TransactionCommit => RrdOperation::TransactionCommit,
        SecurityAction::ChangefeedRead => RrdOperation::ChangefeedRead,
        SecurityAction::ChangefeedFollow => RrdOperation::ChangefeedFollow,
        SecurityAction::VectorCollectionEnsure => RrdOperation::VectorCollectionEnsure,
        SecurityAction::VectorCollectionList => RrdOperation::VectorCollectionList,
        SecurityAction::VectorPointRetrieve => RrdOperation::VectorPointRetrieve,
        SecurityAction::VectorPointScroll => RrdOperation::VectorPointScroll,
        SecurityAction::VectorSearch => RrdOperation::VectorSearch,
        SecurityAction::BackupCreate => RrdOperation::BackupCreate,
        SecurityAction::BackupList => RrdOperation::BackupList,
        SecurityAction::RestoreCreate => RrdOperation::RestoreCreate,
        SecurityAction::EstateRead => RrdOperation::EstateRead,
        SecurityAction::AuditRead => RrdOperation::AuditRead,
        SecurityAction::MemoryContextRead => RrdOperation::MemoryContextRead,
        SecurityAction::MemoryInspect => RrdOperation::MemoryInspect,
        SecurityAction::MemoryRecall => RrdOperation::MemoryRecall,
        SecurityAction::MemoryRetire => RrdOperation::MemoryRetire,
        SecurityAction::MemoryWrite => RrdOperation::MemoryWrite,
        SecurityAction::LifecycleApply => RrdOperation::LifecycleApply,
        SecurityAction::ProjectAttune => RrdOperation::ProjectAttune,
        SecurityAction::ProjectRoute => RrdOperation::ProjectRoute,
        SecurityAction::ReasoningRead => RrdOperation::ReasoningRead,
        SecurityAction::ReasoningWrite => RrdOperation::ReasoningWrite,
        SecurityAction::WorkPlanRead => RrdOperation::WorkPlanRead,
        SecurityAction::WorkPlanControl => RrdOperation::WorkPlanControl,
        SecurityAction::WorkPlanVerifyExecute => RrdOperation::WorkPlanVerifyExecute,
        SecurityAction::UnknownRequest
        | SecurityAction::SessionCreate
        | SecurityAction::SessionRenew
        | SecurityAction::SessionClose
        | SecurityAction::TransactionBegin
        | SecurityAction::TransactionPreview
        | SecurityAction::TransactionAbort
        | SecurityAction::SubscriptionOpen
        | SecurityAction::SubscriptionConnect
        | SecurityAction::SubscriptionAck
        | SecurityAction::SubscriptionClose
        | SecurityAction::AuditExport
        | SecurityAction::DiagnosticsRead
        | SecurityAction::EstateAdmin
        | SecurityAction::SecurityAdmin
        | SecurityAction::RuntimeToolCatalogueRead => {
            return Err(ServiceError::Contract(format!(
                "security action {action:?} is not a runtime-tool operation"
            )));
        }
    };
    Ok(operation)
}

fn runtime_tool_completion(
    result: &crate::Result<RuntimeToolResult>,
) -> (AuditDecision, u16, String) {
    match result {
        Ok(result) => (
            AuditDecision::Allowed,
            200,
            digest::sha256_hex(result.text.as_bytes()),
        ),
        Err(error) => {
            let (decision, status) = match error.kind() {
                ServiceErrorKind::Unauthenticated => (AuditDecision::Denied, 401),
                ServiceErrorKind::PermissionDenied => (AuditDecision::Denied, 403),
                ServiceErrorKind::InvalidArgument => (AuditDecision::Failed, 400),
                ServiceErrorKind::NotFound => (AuditDecision::Failed, 404),
                ServiceErrorKind::Conflict => (AuditDecision::Failed, 409),
                ServiceErrorKind::FailedPrecondition => (AuditDecision::Failed, 412),
                ServiceErrorKind::ResourceExhausted => (AuditDecision::Failed, 429),
                ServiceErrorKind::DeadlineExceeded => (AuditDecision::Failed, 504),
                ServiceErrorKind::Internal => (AuditDecision::Failed, 500),
            };
            (
                decision,
                status,
                digest::sha256_hex(error.to_string().as_bytes()),
            )
        }
    }
}

pub fn is_runtime_tool(name: &str) -> bool {
    runtime_tool_catalogue()
        .iter()
        .any(|definition| definition.name == name)
}

/// Returns the complete executable cooperative-tool catalogue in stable name
/// order. This is the sole source used by MCP discovery and execution gates.
pub fn runtime_tool_catalogue() -> Vec<RuntimeToolDefinition> {
    let mut tools = vec![
        tool(
            "rrflow_context",
            "Load a bounded consolidated view of current project memory with provenance",
            json!({"type":"object","properties":{"subjects":{"type":"array","items":{"type":"string"}},"at":{"type":"integer"},"budget":{"type":"integer"},"max_subjects":{"type":"integer"}}}),
            false,
            RuntimeToolLifecyclePolicy::ReadOnly,
        ),
        tool(
            "rrflow_forget",
            "Retire one current memory fact without erasing its bitemporal history",
            json!({"type":"object","required":["subject","predicate"],"properties":{"subject":{"type":"string"},"predicate":{"type":"string"},"at":{"type":"integer"}}}),
            true,
            RuntimeToolLifecyclePolicy::ControlTransition,
        ),
        typed_tool::<HookApplyArguments>(
            "rrflow_hook",
            None,
            "Translate one supported provider hook payload through RRFlow's current hook adapter",
            true,
            RuntimeToolAuthorization::Governed,
            RuntimeToolLifecyclePolicy::ControlTransition,
        ),
        tool(
            "rrflow_inspect",
            "Inspect memory history, metadata, and provenance for a subject and optional predicate",
            json!({"type":"object","required":["subject"],"properties":{"subject":{"type":"string"},"predicate":{"type":"string"},"limit":{"type":"integer"}}}),
            false,
            RuntimeToolLifecyclePolicy::ReadOnly,
        ),
        typed_tool::<LifecycleApplyArguments>(
            "rrflow_lifecycle",
            None,
            "Append one strict provider-neutral lifecycle transition to the authoritative RRD event chain",
            true,
            RuntimeToolAuthorization::Governed,
            RuntimeToolLifecyclePolicy::ControlTransition,
        ),
        tool(
            "rrflow_preflight",
            "Attune the project, refresh routing, and inject current memory before reasoning",
            json!({"type":"object","properties":{"at":{"type":"integer"},"budget":{"type":"integer"},"harness":{"type":"string"}}}),
            true,
            RuntimeToolLifecyclePolicy::ControlTransition,
        ),
        tool(
            "rrflow_query",
            "Execute a project-scoped RRFlowQL read with durable parse, plan, and execution evidence",
            json!({"type":"object","required":["ql"],"properties":{"ql":{"type":"string"},"scope":{"type":"string"},"parameters":{"type":"object"},"at":{"type":"integer"},"max_scanned_changes":{"type":"integer"},"max_rows":{"type":"integer"},"max_output_bytes":{"type":"integer"},"max_batch_rows":{"type":"integer"}}}),
            false,
            RuntimeToolLifecyclePolicy::ReadOnly,
        ),
        tool(
            "rrflow_reasoning_record",
            "Append one typed goal, plan, attempt, observation, decision, verification, or outcome transition",
            json!({"type":"object","required":["run_id","payload"],"properties":{"run_id":{"type":"string"},"actor":{"type":"string"},"at":{"type":"integer"},"payload":{"type":"object"}}}),
            true,
            RuntimeToolLifecyclePolicy::ControlTransition,
        ),
        tool(
            "rrflow_reasoning_show",
            "Show a specified reasoning run or the active reasoning run",
            json!({"type":"object","properties":{"run_id":{"type":"string"}}}),
            false,
            RuntimeToolLifecyclePolicy::ReadOnly,
        ),
        tool(
            "rrflow_recall",
            "Recall current claims for exact subjects with provenance and a bounded token estimate",
            json!({"type":"object","required":["subjects"],"properties":{"subjects":{"type":"array","items":{"type":"string"}},"at":{"type":"integer"},"budget":{"type":"integer"}}}),
            false,
            RuntimeToolLifecyclePolicy::ReadOnly,
        ),
        tool(
            "rrflow_remember",
            "Persist one bitemporal fact or observation with explicit provenance",
            json!({"type":"object","required":["subject","predicate","object"],"properties":{"subject":{"type":"string"},"predicate":{"type":"string"},"object":{"type":"string"},"actor":{"type":"string"},"on_behalf_of":{"type":"string"},"session":{"type":"string"},"valid_from":{"type":"integer"},"at":{"type":"integer"},"confidence":{"type":"number","minimum":0,"maximum":1}}}),
            true,
            RuntimeToolLifecyclePolicy::ControlTransition,
        ),
        tool(
            "rrflow_route",
            "Refresh the project index and route a symbol or query to complete source files",
            json!({"type":"object","required":["query"],"properties":{"query":{"type":"string"},"limit":{"type":"integer"}}}),
            true,
            RuntimeToolLifecyclePolicy::ControlTransition,
        ),
        typed_tool::<ServiceStatusArguments>(
            "rrflow_service_status",
            None,
            "Inspect RRD readiness, security state, public endpoints, executable MCP tools, and the shared product capability catalogue",
            false,
            RuntimeToolAuthorization::Public,
            RuntimeToolLifecyclePolicy::ReadOnly,
        ),
        typed_tool::<DataCommitArguments>(
            "rrflow_data_commit",
            Some("transaction-commit"),
            "Atomically commit ordered document, relational, graph, event, vector, time-series, geo, object-reference, and claim mutations through the one RRD transaction authority",
            true,
            RuntimeToolAuthorization::Governed,
            RuntimeToolLifecyclePolicy::PlannedMutation,
        ),
        typed_tool::<DataRollbackArguments>(
            "rrflow_data_rollback",
            Some("historical-rollback"),
            "Restore a retained historical record/relation state by atomically appending compensating versions and immutable rollback audit evidence",
            true,
            RuntimeToolAuthorization::Governed,
            RuntimeToolLifecyclePolicy::PlannedMutation,
        ),
        typed_tool::<QueryIndexEnsureArguments>(
            "rrflow_query_index_ensure",
            Some("query-index-ensure"),
            "Create or rebuild one RRD query index through the shared catalogue, read-stamp, idempotency, security, and projection lifecycle",
            true,
            RuntimeToolAuthorization::Governed,
            RuntimeToolLifecyclePolicy::PlannedMutation,
        ),
        typed_tool::<QueryIndexListArguments>(
            "rrflow_query_index_list",
            Some("query-index-list"),
            "List the authoritative RRD query-index catalogue and projection state for one exact instance scope",
            false,
            RuntimeToolAuthorization::Governed,
            RuntimeToolLifecyclePolicy::ReadOnly,
        ),
        typed_tool::<LiveQueryPollArguments>(
            "rrflow_live_query_poll",
            Some("query-live-poll"),
            "Poll one bounded RRFlowQL live view for added, updated, and removed rows after an exact runtime cursor",
            false,
            RuntimeToolAuthorization::Governed,
            RuntimeToolLifecyclePolicy::ReadOnly,
        ),
        typed_tool::<ChangefeedReadArguments>(
            "rrflow_changefeed_read",
            Some("changefeed-read"),
            "Read one bounded validated page from RRD's retained ordered multi-model changefeed after an exact cursor",
            false,
            RuntimeToolAuthorization::Governed,
            RuntimeToolLifecyclePolicy::ReadOnly,
        ),
        typed_tool::<ChangefeedFollowArguments>(
            "rrflow_changefeed_follow",
            Some("changefeed-follow"),
            "Wait within the public timeout bound for the next validated retained RRD changefeed page",
            false,
            RuntimeToolAuthorization::Governed,
            RuntimeToolLifecyclePolicy::ReadOnly,
        ),
        typed_tool::<VectorCollectionEnsureArguments>(
            "rrflow_vector_collection_ensure",
            Some("vector-collection-ensure"),
            "Create or converge one named-vector collection through RRD's shared catalogue, security, idempotency, and read-stamp authority",
            true,
            RuntimeToolAuthorization::Governed,
            RuntimeToolLifecyclePolicy::PlannedMutation,
        ),
        typed_tool::<VectorCollectionListArguments>(
            "rrflow_vector_collection_list",
            Some("vector-collection-list"),
            "List RRD's authoritative named-vector collection definitions, generations, memory tiers, metrics, and model bindings",
            false,
            RuntimeToolAuthorization::Governed,
            RuntimeToolLifecyclePolicy::ReadOnly,
        ),
        typed_tool::<VectorPointsRetrieveArguments>(
            "rrflow_vector_points_retrieve",
            Some("vector-point-retrieve"),
            "Retrieve exact visible vector points from one named collection and vector address without leaking same-field rows from another address",
            false,
            RuntimeToolAuthorization::Governed,
            RuntimeToolLifecyclePolicy::ReadOnly,
        ),
        typed_tool::<VectorPointsScrollArguments>(
            "rrflow_vector_points_scroll",
            Some("vector-point-scroll"),
            "Scroll one deterministic bounded page of visible vector points from an exact named collection and vector address",
            false,
            RuntimeToolAuthorization::Governed,
            RuntimeToolLifecyclePolicy::ReadOnly,
        ),
        typed_tool::<VectorSearchArguments>(
            "rrflow_vector_search",
            Some("vector-search"),
            "Run bounded exact vector search through RRD's named-collection, read-stamp, payload-filter, metric, and security authority",
            false,
            RuntimeToolAuthorization::Governed,
            RuntimeToolLifecyclePolicy::ReadOnly,
        ),
        typed_tool::<BackupCreateArguments>(
            "rrflow_backup_create",
            Some("backup-create"),
            "Create and catalogue one authenticated logical backup through RRD's governed idempotent archive authority",
            true,
            RuntimeToolAuthorization::Governed,
            RuntimeToolLifecyclePolicy::PlannedMutation,
        ),
        typed_tool::<BackupListArguments>(
            "rrflow_backup_list",
            Some("backup-list"),
            "List RRD's logical backup catalogue with optional full archive verification",
            false,
            RuntimeToolAuthorization::Governed,
            RuntimeToolLifecyclePolicy::ReadOnly,
        ),
        typed_tool::<RestoreArguments>(
            "rrflow_restore",
            Some("restore-create"),
            "Restore one catalogued authenticated logical backup to a new isolated root after explicit acknowledgement",
            true,
            RuntimeToolAuthorization::Governed,
            RuntimeToolLifecyclePolicy::PlannedMutation,
        ),
        typed_tool::<EstateReadArguments>(
            "rrflow_estate_read",
            Some("estate-read"),
            "Read one persistent RRD estate's desired, observed, activity, lease, operation, and receipt state",
            false,
            RuntimeToolAuthorization::Governed,
            RuntimeToolLifecyclePolicy::ReadOnly,
        ),
        typed_tool::<AuditReadArguments>(
            "rrflow_audit_read",
            Some("audit-read"),
            "Read one bounded page from RRD's comprehensive persistent security audit journal",
            false,
            RuntimeToolAuthorization::Governed,
            RuntimeToolLifecyclePolicy::ReadOnly,
        ),
    ];
    tools.extend(workplan::definitions());
    tools.sort_by_key(|definition| definition.name);
    tools
}

/// Returns the transport-safe representation of the exact executable runtime
/// catalogue. Server, clients, MCP, and Connectome must serialize this value
/// rather than maintaining a second registry.
pub fn runtime_tool_contract_catalogue() -> RuntimeToolCatalogue {
    let catalogue = RuntimeToolCatalogue {
        protocol: PROTOCOL.into(),
        protocol_version: PROTOCOL_VERSION,
        catalogue_version: RUNTIME_TOOL_CATALOGUE_VERSION,
        tools: runtime_tool_catalogue()
            .into_iter()
            .map(|definition| RuntimeToolDescriptor {
                name: CanonicalId::new(definition.name)
                    .expect("static runtime tool name must be canonical"),
                capability_id: definition.capability_id.map(|capability| {
                    CanonicalId::new(capability)
                        .expect("static runtime capability identity must be canonical")
                }),
                description: definition.description.into(),
                input_schema: definition.input_schema,
                mutation: definition.mutation,
                authorization: definition.authorization,
                action: definition.action,
                lifecycle: definition.lifecycle,
            })
            .collect(),
    };
    catalogue
        .validate()
        .expect("engine runtime tool catalogue must satisfy the public contract");
    catalogue
}

fn tool(
    name: &'static str,
    description: &'static str,
    mut input_schema: Value,
    mutation: bool,
    lifecycle: RuntimeToolLifecyclePolicy,
) -> RuntimeToolDefinition {
    if lifecycle == RuntimeToolLifecyclePolicy::PlannedMutation {
        lifecycle::add_coordinates_schema(&mut input_schema);
    }
    RuntimeToolDefinition {
        name,
        capability_id: None,
        description,
        input_schema,
        mutation,
        authorization: RuntimeToolAuthorization::Governed,
        action: runtime_tool_action(name),
        lifecycle,
    }
}

fn typed_tool<T: JsonSchema>(
    name: &'static str,
    capability_id: Option<&'static str>,
    description: &'static str,
    mutation: bool,
    authorization: RuntimeToolAuthorization,
    lifecycle: RuntimeToolLifecyclePolicy,
) -> RuntimeToolDefinition {
    let mut input_schema =
        serde_json::to_value(schema_for!(T)).expect("generated runtime tool schema must serialize");
    if lifecycle == RuntimeToolLifecyclePolicy::PlannedMutation {
        lifecycle::add_coordinates_schema(&mut input_schema);
    }
    RuntimeToolDefinition {
        name,
        capability_id,
        description,
        input_schema,
        mutation,
        authorization,
        action: runtime_tool_action(name),
        lifecycle,
    }
}

fn runtime_tool_action(name: &str) -> SecurityAction {
    if let Some(operation) = workplan::operation(name) {
        return match operation {
            rrd_contract::WorkPlanOperation::Status => SecurityAction::WorkPlanRead,
            rrd_contract::WorkPlanOperation::Verify => SecurityAction::WorkPlanVerifyExecute,
            _ => SecurityAction::WorkPlanControl,
        };
    }
    match name {
        "rrflow_context" => SecurityAction::MemoryContextRead,
        "rrflow_forget" => SecurityAction::MemoryRetire,
        "rrflow_hook" => SecurityAction::LifecycleApply,
        "rrflow_inspect" => SecurityAction::MemoryInspect,
        "rrflow_lifecycle" => SecurityAction::LifecycleApply,
        "rrflow_preflight" => SecurityAction::ProjectAttune,
        "rrflow_query" => SecurityAction::QueryExecute,
        "rrflow_reasoning_record" => SecurityAction::ReasoningWrite,
        "rrflow_reasoning_show" => SecurityAction::ReasoningRead,
        "rrflow_recall" => SecurityAction::MemoryRecall,
        "rrflow_remember" => SecurityAction::MemoryWrite,
        "rrflow_route" => SecurityAction::ProjectRoute,
        "rrflow_service_status" => SecurityAction::ServiceInspect,
        "rrflow_data_commit" => SecurityAction::TransactionCommit,
        "rrflow_data_rollback" => SecurityAction::TransactionCommit,
        "rrflow_query_index_ensure" => SecurityAction::QueryIndexEnsure,
        "rrflow_query_index_list" => SecurityAction::QueryIndexList,
        "rrflow_live_query_poll" => SecurityAction::QueryLivePoll,
        "rrflow_changefeed_read" => SecurityAction::ChangefeedRead,
        "rrflow_changefeed_follow" => SecurityAction::ChangefeedFollow,
        "rrflow_vector_collection_ensure" => SecurityAction::VectorCollectionEnsure,
        "rrflow_vector_collection_list" => SecurityAction::VectorCollectionList,
        "rrflow_vector_points_retrieve" => SecurityAction::VectorPointRetrieve,
        "rrflow_vector_points_scroll" => SecurityAction::VectorPointScroll,
        "rrflow_vector_search" => SecurityAction::VectorSearch,
        "rrflow_backup_create" => SecurityAction::BackupCreate,
        "rrflow_backup_list" => SecurityAction::BackupList,
        "rrflow_restore" => SecurityAction::RestoreCreate,
        "rrflow_estate_read" => SecurityAction::EstateRead,
        "rrflow_audit_read" => SecurityAction::AuditRead,
        _ => panic!("runtime tool {name:?} has no security action"),
    }
}

fn execute_runtime_tool(
    engine: &RrdEngine,
    root: &Path,
    name: &str,
    args: &Value,
    invocation_at: u64,
    caller: AdapterCaller<'_>,
) -> Result<ExecutedTool, Box<dyn std::error::Error>> {
    if workplan::operation(name).is_some() {
        return workplan::execute(engine, root, name, args, invocation_at);
    }
    let store = &engine.storage;
    match name {
        "rrflow_data_commit" => execute_data_commit(engine, args, invocation_at, caller),
        "rrflow_data_rollback" => execute_data_rollback(engine, args, invocation_at, caller),
        "rrflow_query_index_ensure" => {
            execute_query_index_ensure(engine, args, invocation_at, caller)
        }
        "rrflow_query_index_list" => execute_query_index_list(engine, args, invocation_at, caller),
        "rrflow_live_query_poll" => execute_live_query_poll(engine, args, invocation_at, caller),
        "rrflow_changefeed_read" => execute_changefeed_read(engine, args, invocation_at, caller),
        "rrflow_changefeed_follow" => {
            execute_changefeed_follow(engine, args, invocation_at, caller)
        }
        "rrflow_vector_collection_ensure" => {
            execute_vector_collection_ensure(engine, args, invocation_at, caller)
        }
        "rrflow_vector_collection_list" => {
            execute_vector_collection_list(engine, args, invocation_at, caller)
        }
        "rrflow_vector_points_retrieve" => {
            execute_vector_points_retrieve(engine, args, invocation_at, caller)
        }
        "rrflow_vector_points_scroll" => {
            execute_vector_points_scroll(engine, args, invocation_at, caller)
        }
        "rrflow_vector_search" => execute_vector_search(engine, args, invocation_at, caller),
        "rrflow_backup_create" => execute_backup_create(engine, args, invocation_at, caller),
        "rrflow_backup_list" => execute_backup_list(engine, args, invocation_at, caller),
        "rrflow_restore" => execute_restore(engine, args, invocation_at, caller),
        "rrflow_estate_read" => execute_estate_read(engine, args, invocation_at, caller),
        "rrflow_audit_read" => execute_audit_read(engine, args, invocation_at, caller),
        "rrflow_service_status" => {
            let at = arg_u64(args, "at").unwrap_or(invocation_at);
            let tools = runtime_tool_catalogue();
            let endpoints = endpoint_catalogue();
            let capabilities = product_capability_catalogue();
            Ok(ExecutedTool {
                text: serde_json::to_string_pretty(&json!({
                    "observed_at": at,
                    "instance_id": engine.instance_id(),
                    "readiness": engine.readiness(at)?,
                    "security_enforced": engine.security_enforced()?,
                    "endpoint_count": endpoints.endpoints.len(),
                    "endpoints": endpoints,
                    "executable_mcp_tool_count": tools.len(),
                    "executable_mcp_tools": tools.iter().map(|tool| tool.name).collect::<Vec<_>>(),
                    "product_capabilities": capabilities,
                }))?,
                effectiveness: None,
                detail: Some(format!(
                    "{} endpoints; {} executable MCP tools",
                    endpoints.endpoints.len(),
                    tools.len()
                )),
            })
        }
        "rrflow_context" => {
            let reader = reader(args)?;
            let at = arg_u64(args, "at").unwrap_or(invocation_at);
            let max_subjects = arg_u64(args, "max_subjects").unwrap_or(64).clamp(1, 1_024) as usize;
            let subjects = match args.get("subjects") {
                Some(value) => value
                    .as_array()
                    .ok_or("subjects must be an array")?
                    .iter()
                    .map(|value| Subject::new(value.as_str().unwrap_or_default()))
                    .collect::<rrd_core::Result<Vec<_>>>()?,
                None => store.subjects()?.into_iter().take(max_subjects).collect(),
            };
            let query = RecallQuery {
                subjects,
                predicates: None,
                as_of: at,
            };
            let set = rrd_core::recall(
                store,
                &query,
                arg_u64(args, "budget").unwrap_or(4_000) as usize,
            )?;
            for claim in &set.claims {
                store.observe(&reader, &claim.subject, &claim.predicate, at)?;
            }
            Ok(ExecutedTool {
                text: serde_json::to_string_pretty(&json!({
                    "as_of": at,
                    "subjects_considered": query.subjects.len(),
                    "memory": set,
                }))?,
                effectiveness: None,
                detail: Some(format!("{} subject(s) considered", query.subjects.len())),
            })
        }
        "rrflow_forget" => {
            let subject = Subject::new(arg_str(args, "subject")?)?;
            let predicate = Predicate::new(arg_str(args, "predicate")?)?;
            let at = arg_u64(args, "at").unwrap_or(invocation_at);
            let current = store
                .current(&subject, &predicate, at)?
                .ok_or("no current memory fact matches subject and predicate")?;
            let mut retirement = current.clone();
            retirement.retire(at)?;
            retirement.tx_time = at;
            retirement.supersedes = Some(current.digest());
            let outcome = store.append_batch(std::slice::from_ref(&retirement))?;
            Ok(ExecutedTool {
                text: serde_json::to_string_pretty(&json!({
                    "retired": retirement,
                    "append": outcome,
                }))?,
                effectiveness: None,
                detail: Some(format!("retired at {at}")),
            })
        }
        "rrflow_inspect" => {
            let subject = Subject::new(arg_str(args, "subject")?)?;
            let limit = arg_u64(args, "limit").unwrap_or(100).clamp(1, 4_096) as usize;
            let mut claims = match args.get("predicate").and_then(Value::as_str) {
                Some(value) => store.all_versions(&subject, &Predicate::new(value)?)?,
                None => store.subject_versions(&subject)?,
            };
            let truncated = claims.len() > limit;
            claims.truncate(limit);
            Ok(ExecutedTool {
                text: serde_json::to_string_pretty(&json!({
                    "subject": subject,
                    "claims": claims,
                    "truncated": truncated,
                }))?,
                effectiveness: None,
                detail: None,
            })
        }
        "rrflow_preflight" => {
            let reader = reader(args)?;
            let at = arg_u64(args, "at").unwrap_or(invocation_at);
            let budget = arg_u64(args, "budget").unwrap_or(1_500) as usize;
            let harness = args.get("harness").and_then(Value::as_str);
            let flight = preflight(store, root, harness, &reader, at, budget)?;
            Ok(ExecutedTool {
                text: flight.context,
                effectiveness: Some(flight.effectiveness),
                detail: (!flight.warnings.is_empty())
                    .then(|| format!("{} warning(s)", flight.warnings.len())),
            })
        }
        "rrflow_recall" => {
            let reader = reader(args)?;
            let subjects = args
                .get("subjects")
                .and_then(Value::as_array)
                .ok_or("subjects must be an array")?
                .iter()
                .map(|value| Subject::new(value.as_str().unwrap_or_default()))
                .collect::<rrd_core::Result<Vec<_>>>()?;
            let query = RecallQuery {
                subjects,
                predicates: None,
                as_of: arg_u64(args, "at").unwrap_or(invocation_at),
            };
            let set = rrd_core::recall(
                store,
                &query,
                arg_u64(args, "budget").unwrap_or(1_500) as usize,
            )?;
            for claim in &set.claims {
                store.observe(&reader, &claim.subject, &claim.predicate, query.as_of)?;
            }
            Ok(ExecutedTool {
                text: serde_json::to_string_pretty(&set)?,
                effectiveness: Some(Effectiveness {
                    query: query
                        .subjects
                        .iter()
                        .map(|subject| subject.as_str())
                        .collect::<Vec<_>>()
                        .join(","),
                    claims_returned: set.claims.len(),
                    tokens_emitted: set.token_estimate as u64,
                    baseline_tokens: None,
                    baseline_mode: None,
                    provider: "mcp".into(),
                    outcome: RecallOutcome::Unknown,
                }),
                detail: None,
            })
        }
        "rrflow_remember" => {
            let at = arg_u64(args, "at").unwrap_or(invocation_at);
            let valid_from = arg_u64(args, "valid_from").unwrap_or(at);
            let mut claim = Claim::new(
                Subject::new(arg_str(args, "subject")?)?,
                Predicate::new(arg_str(args, "predicate")?)?,
                arg_str(args, "object")?,
                valid_from,
                at,
                Producer {
                    actor: args
                        .get("actor")
                        .and_then(Value::as_str)
                        .unwrap_or("agent:mcp")
                        .to_owned(),
                    on_behalf_of: args
                        .get("on_behalf_of")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    session: args
                        .get("session")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                },
            );
            claim.confidence = args
                .get("confidence")
                .and_then(Value::as_f64)
                .map(|value| value as f32);
            if claim
                .confidence
                .is_some_and(|confidence| !(0.0..=1.0).contains(&confidence))
            {
                return Err("confidence must be between zero and one".into());
            }
            let outcome = store.assert(&claim)?;
            Ok(ExecutedTool {
                text: serde_json::to_string_pretty(&json!({
                    "claim_id": claim.digest(),
                    "claim": claim,
                    "append": outcome,
                }))?,
                effectiveness: None,
                detail: Some("memory persisted".into()),
            })
        }
        "rrflow_route" => {
            let ready = ensure_routing_fresh(store, root)?;
            let index =
                load_routing(store, root)?.ok_or("routing projection absent after refresh")?;
            let routed = index.route(
                arg_str(args, "query")?,
                arg_u64(args, "limit").unwrap_or(5) as usize,
            );
            Ok(ExecutedTool {
                text: serde_json::to_string_pretty(
                    &json!({"freshness": ready.render(), "files": routed}),
                )?,
                effectiveness: None,
                detail: Some(ready.render()),
            })
        }
        "rrflow_query" => {
            let reader = reader(args)?;
            let scope = ScopeId::new(
                args.get("scope")
                    .and_then(Value::as_str)
                    .unwrap_or(REASONING_SCOPE),
            )?;
            let empty_parameters = json!({});
            let parameters =
                query_parameters_from_json(args.get("parameters").unwrap_or(&empty_parameters))?;
            let budget = ExecutionBudget {
                max_scanned_changes: arg_u64(args, "max_scanned_changes")
                    .unwrap_or(100_000)
                    .clamp(1, 1_000_000) as usize,
                max_rows: arg_u64(args, "max_rows")
                    .unwrap_or(10_000)
                    .clamp(1, 100_000) as usize,
                max_output_bytes: arg_u64(args, "max_output_bytes")
                    .unwrap_or(8 * 1024 * 1024)
                    .clamp(1, 64 * 1024 * 1024) as usize,
                max_batch_rows: arg_u64(args, "max_batch_rows")
                    .unwrap_or(256)
                    .clamp(1, 4_096) as usize,
                ..ExecutionBudget::default()
            };
            let result = execute_traced_query(
                store,
                scope,
                arg_str(args, "ql")?,
                &parameters,
                &budget,
                reader.as_str(),
                arg_u64(args, "at").unwrap_or(invocation_at),
            )?;
            Ok(ExecutedTool {
                text: serde_json::to_string_pretty(&result)?,
                effectiveness: None,
                detail: Some(format!("plan={}", result.plan.digest)),
            })
        }
        "rrflow_reasoning_record" => {
            let actor = args
                .get("actor")
                .and_then(Value::as_str)
                .unwrap_or("agent:mcp");
            let payload: ReasoningPayload =
                serde_json::from_value(args.get("payload").cloned().ok_or("payload is required")?)?;
            let event = record_reasoning(
                store,
                arg_str(args, "run_id")?,
                arg_u64(args, "at").unwrap_or(invocation_at),
                actor,
                payload,
            )?;
            Ok(ExecutedTool {
                text: serde_json::to_string_pretty(&event)?,
                effectiveness: None,
                detail: Some(format!(
                    "recorded {} #{}",
                    event.payload.name(),
                    event.ordinal
                )),
            })
        }
        "rrflow_reasoning_show" => {
            let run = match args.get("run_id").and_then(Value::as_str) {
                Some(id) => reasoning_run(store, id)?,
                None => active_reasoning_run(store)?,
            };
            let value =
                run.map(|run| json!({"run_id":run.id(),"state":run.state(),"events":run.events()}));
            Ok(ExecutedTool {
                text: serde_json::to_string_pretty(&value)?,
                effectiveness: None,
                detail: None,
            })
        }
        "rrflow_lifecycle" => {
            let request: LifecycleApplyArguments = serde_json::from_value(args.clone())?;
            let recorded_at = request.recorded_at_unix_ms.unwrap_or(invocation_at);
            let snapshot = append_lifecycle_event(store, request.command, recorded_at)?;
            Ok(ExecutedTool {
                text: serde_json::to_string_pretty(&snapshot)?,
                effectiveness: None,
                detail: Some(format!(
                    "canonical lifecycle {} event {} state {}",
                    snapshot.session_id, snapshot.event_count, snapshot.state_sha256
                )),
            })
        }
        "rrflow_hook" => {
            let request: HookApplyArguments = serde_json::from_value(args.clone())?;
            let reader = reader(args)?;
            let context = HookContext {
                store,
                root,
                harness: Some("mcp"),
                reader: &reader,
                now: request.at.unwrap_or(invocation_at),
                budget: request.budget.unwrap_or(1_500),
            };
            let response = handle(&context, request.event.runtime_event(), &request.input)?;
            Ok(ExecutedTool {
                text: response.stdout,
                effectiveness: response.effectiveness,
                detail: response.detail,
            })
        }
        _ => unreachable!("runtime tool name was validated"),
    }
}

fn execute_data_commit(
    engine: &RrdEngine,
    args: &Value,
    invocation_at: u64,
    caller: AdapterCaller<'_>,
) -> Result<ExecutedTool, Box<dyn std::error::Error>> {
    let request: DataCommitArguments = serde_json::from_value(args.clone())?;
    let at = request.at.unwrap_or(invocation_at);
    if at == 0 {
        return Err("data commit time must be greater than zero".into());
    }
    let timeout_ms = request.timeout_ms.unwrap_or(60_000);
    if !(MIN_LEASE_MS..=MAX_LEASE_MS).contains(&timeout_ms) {
        return Err(
            format!("transaction timeout must be in {MIN_LEASE_MS}..={MAX_LEASE_MS}").into(),
        );
    }
    let session = AdapterSession::open(engine, caller, "data", &request.idempotency_key, at)?;
    let identity = digest::sha256_hex(&serde_json::to_vec(&(
        "data",
        request.idempotency_key.as_str(),
    ))?);
    let short = &identity[..32];
    let begin_key = CorrelationId::new(format!("mcp-data-begin-{short}"))?;
    let deadline_unix_ms = at
        .checked_add(timeout_ms)
        .ok_or("data commit deadline overflow")?;
    let transaction = engine.begin_transaction(
        &session.session_id,
        &session.token,
        &BeginTransaction {
            scope: CanonicalId::new("data")?,
            timeout_ms,
        },
        &RequestContext {
            request_id: session.request_id.clone(),
            operation_id: session.operation_id.clone(),
            idempotency_key: Some(begin_key),
            deadline_unix_ms: Some(deadline_unix_ms),
        },
        at,
    )?;
    let operation_sha256 = transaction_operation_sha256(&request.mutations);
    let receipt = engine.commit_transaction(
        &session.session_id,
        &session.token,
        &transaction.transaction_id,
        &request.idempotency_key,
        &CommitTransaction {
            operation_sha256,
            mutations: request.mutations,
        },
        at,
        session.request_id.as_str(),
        session.operation_id.as_str(),
    )?;
    Ok(ExecutedTool {
        detail: Some(format!(
            "committed {} mutation(s) at runtime cursors {:?}..={:?}",
            receipt.mutation_count, receipt.first_runtime_cursor, receipt.last_runtime_cursor
        )),
        text: serde_json::to_string_pretty(&receipt)?,
        effectiveness: None,
    })
}

fn execute_data_rollback(
    engine: &RrdEngine,
    args: &Value,
    _invocation_at: u64,
    caller: AdapterCaller<'_>,
) -> Result<ExecutedTool, Box<dyn std::error::Error>> {
    let arguments: DataRollbackArguments = serde_json::from_value(args.clone())?;
    let request = ForwardRollbackRequest {
        target_valid_at: arguments.target_valid_at,
        target_known_at_cursor: arguments.target_known_at_cursor,
        effective_at: arguments.effective_at,
        reason: arguments.reason,
    };
    request.validate()?;
    let timeout_ms = arguments.timeout_ms.unwrap_or(60_000);
    if !(MIN_LEASE_MS..=MAX_LEASE_MS).contains(&timeout_ms) {
        return Err(
            format!("transaction timeout must be in {MIN_LEASE_MS}..={MAX_LEASE_MS}").into(),
        );
    }
    let at = request.effective_at;
    let session = AdapterSession::open(
        engine,
        caller,
        "data-rollback",
        &arguments.idempotency_key,
        at,
    )?;
    let identity = digest::sha256_hex(&serde_json::to_vec(&(
        "data-rollback",
        arguments.idempotency_key.as_str(),
    ))?);
    let begin_key = CorrelationId::new(format!("mcp-rollback-begin-{}", &identity[..32]))?;
    let deadline_unix_ms = at
        .checked_add(timeout_ms)
        .ok_or("rollback transaction deadline overflow")?;
    let transaction = engine.begin_transaction(
        &session.session_id,
        &session.token,
        &BeginTransaction {
            scope: CanonicalId::new("data")?,
            timeout_ms,
        },
        &RequestContext {
            request_id: session.request_id.clone(),
            operation_id: session.operation_id.clone(),
            idempotency_key: Some(begin_key),
            deadline_unix_ms: Some(deadline_unix_ms),
        },
        at,
    )?;
    let plan = engine.plan_forward_rollback(
        &request,
        transaction.read_cursor,
        &arguments.idempotency_key,
    )?;
    let commit = engine.commit_transaction(
        &session.session_id,
        &session.token,
        &transaction.transaction_id,
        &arguments.idempotency_key,
        &CommitTransaction {
            operation_sha256: plan.operation_sha256,
            mutations: plan.mutations,
        },
        at,
        session.request_id.as_str(),
        session.operation_id.as_str(),
    )?;
    let receipt = ForwardRollbackReceipt {
        request: plan.request,
        read_cursor: plan.read_cursor,
        target_state_sha256: plan.target_state_sha256,
        counts: plan.counts,
        commit,
    };
    receipt.validate()?;
    Ok(ExecutedTool {
        detail: Some(format!(
            "forward rollback restored {} and retired {} structural version(s)",
            receipt.counts.restored_records + receipt.counts.restored_relations,
            receipt.counts.retired_records + receipt.counts.retired_relations,
        )),
        text: serde_json::to_string_pretty(&receipt)?,
        effectiveness: None,
    })
}

fn execute_query_index_ensure(
    engine: &RrdEngine,
    args: &Value,
    invocation_at: u64,
    caller: AdapterCaller<'_>,
) -> Result<ExecutedTool, Box<dyn std::error::Error>> {
    let arguments: QueryIndexEnsureArguments = serde_json::from_value(args.clone())?;
    let at = arguments.at.unwrap_or(invocation_at);
    if at == 0 {
        return Err("query index ensure time must be greater than zero".into());
    }
    let session = AdapterSession::open(
        engine,
        caller,
        "query-index-ensure",
        &arguments.idempotency_key,
        at,
    )?;
    let result = engine.ensure_query_index(
        &session.session_id,
        &session.token,
        &arguments.idempotency_key,
        &arguments.request,
        at,
        session.request_id.as_str(),
        session.operation_id.as_str(),
    )?;
    Ok(ExecutedTool {
        detail: Some(format!(
            "index {} {:?} at catalogue revision {}",
            result.index.index_id, result.index.state, result.catalogue_revision
        )),
        text: serde_json::to_string_pretty(&result)?,
        effectiveness: None,
    })
}

fn execute_query_index_list(
    engine: &RrdEngine,
    args: &Value,
    invocation_at: u64,
    caller: AdapterCaller<'_>,
) -> Result<ExecutedTool, Box<dyn std::error::Error>> {
    let arguments: QueryIndexListArguments = serde_json::from_value(args.clone())?;
    let at = arguments.at.unwrap_or(invocation_at);
    if at == 0 {
        return Err("query index list time must be greater than zero".into());
    }
    let session = AdapterSession::open_read(engine, caller, "query-index-list", at)?;
    let catalogue = engine.list_query_indexes(
        &session.session_id,
        &session.token,
        &arguments.request,
        at,
        session.request_id.as_str(),
        session.operation_id.as_str(),
    )?;
    Ok(ExecutedTool {
        detail: Some(format!(
            "{} index(es) at catalogue revision {}",
            catalogue.indexes.len(),
            catalogue.revision
        )),
        text: serde_json::to_string_pretty(&catalogue)?,
        effectiveness: None,
    })
}

fn execute_live_query_poll(
    engine: &RrdEngine,
    args: &Value,
    invocation_at: u64,
    caller: AdapterCaller<'_>,
) -> Result<ExecutedTool, Box<dyn std::error::Error>> {
    let arguments: LiveQueryPollArguments = serde_json::from_value(args.clone())?;
    let at = arguments.at.unwrap_or(invocation_at);
    if at == 0 {
        return Err("live query poll time must be greater than zero".into());
    }
    let session = AdapterSession::open_read(engine, caller, "live-query-poll", at)?;
    let delta = engine.poll_live_query(
        &session.session_id,
        &session.token,
        &arguments.request,
        at,
        session.request_id.as_str(),
        session.operation_id.as_str(),
    )?;
    Ok(ExecutedTool {
        detail: Some(format!(
            "live delta {}..={} ({} added, {} updated, {} removed; timed_out={})",
            delta.from_cursor,
            delta.through_cursor,
            delta.added.len(),
            delta.updated.len(),
            delta.removed.len(),
            delta.timed_out
        )),
        text: serde_json::to_string_pretty(&delta)?,
        effectiveness: None,
    })
}

fn execute_changefeed_read(
    engine: &RrdEngine,
    args: &Value,
    invocation_at: u64,
    caller: AdapterCaller<'_>,
) -> Result<ExecutedTool, Box<dyn std::error::Error>> {
    let arguments: ChangefeedReadArguments = serde_json::from_value(args.clone())?;
    let at = arguments.at.unwrap_or(invocation_at);
    if at == 0 {
        return Err("changefeed read time must be greater than zero".into());
    }
    let session = AdapterSession::open_read(engine, caller, "changefeed-read", at)?;
    let page = engine.read_changefeed(
        &session.session_id,
        &session.token,
        &arguments.request,
        at,
        session.request_id.as_str(),
        session.operation_id.as_str(),
    )?;
    Ok(ExecutedTool {
        detail: Some(format!(
            "{} change(s) through cursor {} (head {}; has_more={})",
            page.changes.len(),
            page.through_cursor,
            page.head_cursor,
            page.has_more
        )),
        text: serde_json::to_string_pretty(&page)?,
        effectiveness: None,
    })
}

fn execute_changefeed_follow(
    engine: &RrdEngine,
    args: &Value,
    invocation_at: u64,
    caller: AdapterCaller<'_>,
) -> Result<ExecutedTool, Box<dyn std::error::Error>> {
    let arguments: ChangefeedFollowArguments = serde_json::from_value(args.clone())?;
    let at = arguments.at.unwrap_or(invocation_at);
    if at == 0 {
        return Err("changefeed follow time must be greater than zero".into());
    }
    let session = AdapterSession::open_read(engine, caller, "changefeed-follow", at)?;
    let followed = engine.follow_changefeed(
        &session.session_id,
        &session.token,
        &arguments.request,
        at,
        session.request_id.as_str(),
        session.operation_id.as_str(),
    )?;
    Ok(ExecutedTool {
        detail: Some(format!(
            "{} change(s) through cursor {} after {} ms (timed_out={})",
            followed.page.changes.len(),
            followed.page.through_cursor,
            followed.waited_ms,
            followed.timed_out
        )),
        text: serde_json::to_string_pretty(&followed)?,
        effectiveness: None,
    })
}

fn execute_vector_collection_ensure(
    engine: &RrdEngine,
    args: &Value,
    invocation_at: u64,
    caller: AdapterCaller<'_>,
) -> Result<ExecutedTool, Box<dyn std::error::Error>> {
    let arguments: VectorCollectionEnsureArguments = serde_json::from_value(args.clone())?;
    let at = arguments.at.unwrap_or(invocation_at);
    if at == 0 {
        return Err("vector collection ensure time must be greater than zero".into());
    }
    let session = AdapterSession::open(
        engine,
        caller,
        "vector-collection-ensure",
        &arguments.idempotency_key,
        at,
    )?;
    let result = engine.ensure_vector_collection(
        &session.session_id,
        &session.token,
        &arguments.request,
        &RequestContext {
            request_id: session.request_id,
            operation_id: session.operation_id,
            idempotency_key: Some(arguments.idempotency_key),
            deadline_unix_ms: None,
        },
        at,
    )?;
    Ok(ExecutedTool {
        detail: Some(format!(
            "collection {} generation {} at catalogue revision {}",
            result.collection.collection_id,
            result.collection.generation,
            result.catalogue_revision
        )),
        text: serde_json::to_string_pretty(&result)?,
        effectiveness: None,
    })
}

fn execute_vector_collection_list(
    engine: &RrdEngine,
    args: &Value,
    invocation_at: u64,
    caller: AdapterCaller<'_>,
) -> Result<ExecutedTool, Box<dyn std::error::Error>> {
    let arguments: VectorCollectionListArguments = serde_json::from_value(args.clone())?;
    let at = arguments.at.unwrap_or(invocation_at);
    if at == 0 {
        return Err("vector collection list time must be greater than zero".into());
    }
    let session = AdapterSession::open_read(engine, caller, "vector-collection-list", at)?;
    let catalogue = engine.list_vector_collections(
        &session.session_id,
        &session.token,
        &arguments.request,
        at,
        session.request_id.as_str(),
        session.operation_id.as_str(),
    )?;
    Ok(ExecutedTool {
        detail: Some(format!(
            "{} collection(s) at catalogue revision {}",
            catalogue.collections.len(),
            catalogue.revision
        )),
        text: serde_json::to_string_pretty(&catalogue)?,
        effectiveness: None,
    })
}

fn execute_vector_points_retrieve(
    engine: &RrdEngine,
    args: &Value,
    invocation_at: u64,
    caller: AdapterCaller<'_>,
) -> Result<ExecutedTool, Box<dyn std::error::Error>> {
    let arguments: VectorPointsRetrieveArguments = serde_json::from_value(args.clone())?;
    let at = arguments.at.unwrap_or(invocation_at);
    if at == 0 {
        return Err("vector point retrieve time must be greater than zero".into());
    }
    let session = AdapterSession::open_read(engine, caller, "vector-points-retrieve", at)?;
    let batch = engine.retrieve_vector_points(
        &session.session_id,
        &session.token,
        &arguments.request,
        at,
        session.request_id.as_str(),
        session.operation_id.as_str(),
    )?;
    Ok(ExecutedTool {
        detail: Some(format!(
            "retrieved {} point(s); {} requested reference(s) missing",
            batch.points.len(),
            batch.missing.len()
        )),
        text: serde_json::to_string_pretty(&batch)?,
        effectiveness: None,
    })
}

fn execute_vector_points_scroll(
    engine: &RrdEngine,
    args: &Value,
    invocation_at: u64,
    caller: AdapterCaller<'_>,
) -> Result<ExecutedTool, Box<dyn std::error::Error>> {
    let arguments: VectorPointsScrollArguments = serde_json::from_value(args.clone())?;
    let at = arguments.at.unwrap_or(invocation_at);
    if at == 0 {
        return Err("vector point scroll time must be greater than zero".into());
    }
    let session = AdapterSession::open_read(engine, caller, "vector-points-scroll", at)?;
    let page = engine.scroll_vector_points(
        &session.session_id,
        &session.token,
        &arguments.request,
        at,
        session.request_id.as_str(),
        session.operation_id.as_str(),
    )?;
    Ok(ExecutedTool {
        detail: Some(format!(
            "scrolled {} point(s) (truncated={})",
            page.points.len(),
            page.truncated
        )),
        text: serde_json::to_string_pretty(&page)?,
        effectiveness: None,
    })
}

fn execute_vector_search(
    engine: &RrdEngine,
    args: &Value,
    invocation_at: u64,
    caller: AdapterCaller<'_>,
) -> Result<ExecutedTool, Box<dyn std::error::Error>> {
    let arguments: VectorSearchArguments = serde_json::from_value(args.clone())?;
    let at = arguments.at.unwrap_or(invocation_at);
    if at == 0 {
        return Err("vector search time must be greater than zero".into());
    }
    let session = AdapterSession::open_read(engine, caller, "vector-search", at)?;
    let result = engine.search_vectors(
        &session.session_id,
        &session.token,
        &arguments.request,
        at,
        session.request_id.as_str(),
        session.operation_id.as_str(),
    )?;
    Ok(ExecutedTool {
        detail: Some(format!(
            "{} hit(s) via {} (exact={})",
            result.hits.len(),
            result.access_path,
            result.exact
        )),
        text: serde_json::to_string_pretty(&result)?,
        effectiveness: None,
    })
}

fn execute_backup_create(
    engine: &RrdEngine,
    args: &Value,
    invocation_at: u64,
    caller: AdapterCaller<'_>,
) -> Result<ExecutedTool, Box<dyn std::error::Error>> {
    let arguments: BackupCreateArguments = serde_json::from_value(args.clone())?;
    let at = arguments.at.unwrap_or(invocation_at);
    if at == 0 {
        return Err("backup create time must be greater than zero".into());
    }
    let session = AdapterSession::open(
        engine,
        caller,
        "backup-create",
        &arguments.idempotency_key,
        at,
    )?;
    let result = engine.create_instance_backup(
        &session.session_id,
        &session.token,
        &arguments.idempotency_key,
        &arguments.request,
        at,
        session.request_id.as_str(),
        session.operation_id.as_str(),
    )?;
    Ok(ExecutedTool {
        detail: Some(format!(
            "backup {} at catalogue revision {} (application_complete={})",
            result.backup.backup_sha256,
            result.catalogue_revision,
            result.backup.application_complete
        )),
        text: serde_json::to_string_pretty(&result)?,
        effectiveness: None,
    })
}

fn execute_backup_list(
    engine: &RrdEngine,
    args: &Value,
    invocation_at: u64,
    caller: AdapterCaller<'_>,
) -> Result<ExecutedTool, Box<dyn std::error::Error>> {
    let arguments: BackupListArguments = serde_json::from_value(args.clone())?;
    let at = arguments.at.unwrap_or(invocation_at);
    if at == 0 {
        return Err("backup list time must be greater than zero".into());
    }
    let session = AdapterSession::open_read(engine, caller, "backup-list", at)?;
    let catalogue = engine.list_instance_backups(
        &session.session_id,
        &session.token,
        &arguments.request,
        at,
        session.request_id.as_str(),
        session.operation_id.as_str(),
    )?;
    Ok(ExecutedTool {
        detail: Some(format!(
            "{} backup(s) at catalogue revision {} (verified={})",
            catalogue.backups.len(),
            catalogue.revision,
            catalogue.archives_verified
        )),
        text: serde_json::to_string_pretty(&catalogue)?,
        effectiveness: None,
    })
}

fn execute_restore(
    engine: &RrdEngine,
    args: &Value,
    invocation_at: u64,
    caller: AdapterCaller<'_>,
) -> Result<ExecutedTool, Box<dyn std::error::Error>> {
    let arguments: RestoreArguments = serde_json::from_value(args.clone())?;
    let RestoreConfirmation::RestoreToNewRoot = arguments.confirmation;
    let at = arguments.at.unwrap_or(invocation_at);
    if at == 0 {
        return Err("restore time must be greater than zero".into());
    }
    let session = AdapterSession::open(engine, caller, "restore", &arguments.idempotency_key, at)?;
    let result = engine.restore_instance_backup(
        &session.session_id,
        &session.token,
        &arguments.idempotency_key,
        &arguments.request,
        at,
        session.request_id.as_str(),
        session.operation_id.as_str(),
    )?;
    Ok(ExecutedTool {
        detail: Some(format!(
            "restored backup {} to isolated root {} (reopened={})",
            result.backup_sha256, result.restore_id, result.reopened
        )),
        text: serde_json::to_string_pretty(&result)?,
        effectiveness: None,
    })
}

fn execute_estate_read(
    engine: &RrdEngine,
    args: &Value,
    invocation_at: u64,
    caller: AdapterCaller<'_>,
) -> Result<ExecutedTool, Box<dyn std::error::Error>> {
    let arguments: EstateReadArguments = serde_json::from_value(args.clone())?;
    let at = arguments.at.unwrap_or(invocation_at);
    if at == 0 {
        return Err("estate read time must be greater than zero".into());
    }
    let session = AdapterSession::open_read(engine, caller, "estate-read", at)?;
    let estate = engine.read_estate(
        &session.session_id,
        &session.token,
        arguments.estate_id,
        at,
        session.request_id.as_str(),
        session.operation_id.as_str(),
    )?;
    Ok(ExecutedTool {
        detail: Some(match &estate {
            Some(estate) => format!(
                "estate {} revision {} with {} instance(s) and {} operation(s)",
                estate.id,
                estate.revision,
                estate.instances.len(),
                estate.operations.len()
            ),
            None => "estate is not present".into(),
        }),
        text: serde_json::to_string_pretty(&estate)?,
        effectiveness: None,
    })
}

fn execute_audit_read(
    engine: &RrdEngine,
    args: &Value,
    invocation_at: u64,
    caller: AdapterCaller<'_>,
) -> Result<ExecutedTool, Box<dyn std::error::Error>> {
    let arguments: AuditReadArguments = serde_json::from_value(args.clone())?;
    let at = arguments.at.unwrap_or(invocation_at);
    if at == 0 {
        return Err("audit read time must be greater than zero".into());
    }
    let session = AdapterSession::open_read(engine, caller, "audit-read", at)?;
    let page = engine.read_audit(
        &session.session_id,
        &session.token,
        &arguments.request,
        at,
        session.request_id.as_str(),
        session.operation_id.as_str(),
    )?;
    Ok(ExecutedTool {
        detail: Some(format!(
            "{} audit record(s) through sequence {}",
            page.records.len(),
            page.through_sequence
        )),
        text: serde_json::to_string_pretty(&page)?,
        effectiveness: None,
    })
}

fn invocation_arguments(name: &str, args: &Value) -> Vec<String> {
    if name != "rrflow_query" {
        return vec![args.to_string()];
    }
    let query = args.get("ql").and_then(Value::as_str).unwrap_or_default();
    let parameters = args.get("parameters").cloned().unwrap_or_else(|| json!({}));
    let parameter_bytes = serde_json::to_vec(&parameters).unwrap_or_default();
    vec![
        format!(
            "scope={}",
            args.get("scope")
                .and_then(Value::as_str)
                .unwrap_or(REASONING_SCOPE)
        ),
        format!("query_digest={}", digest::sha256_hex(query.as_bytes())),
        format!("parameter_digest={}", digest::sha256_hex(&parameter_bytes)),
    ]
}

fn reader(args: &Value) -> Result<Reader, Box<dyn std::error::Error>> {
    Ok(Reader::new(
        args.get("reader")
            .and_then(Value::as_str)
            .unwrap_or("agent:mcp"),
    )?)
}

fn arg_str<'a>(args: &'a Value, name: &str) -> Result<&'a str, Box<dyn std::error::Error>> {
    args.get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{name} is required").into())
}

fn arg_u64(args: &Value, name: &str) -> Option<u64> {
    args.get(name).and_then(Value::as_u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_runtime_definition_derives_matching_operation_policy() {
        for definition in runtime_tool_catalogue() {
            let operation = runtime_tool_operation_for_action(definition.action).unwrap();
            assert_eq!(operation.action(), definition.action, "{}", definition.name);
            assert_eq!(
                operation.mutates(),
                definition.mutation,
                "{}",
                definition.name
            );
        }
    }
}
