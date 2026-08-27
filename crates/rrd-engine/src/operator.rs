//! Engine-owned embedded and offline operator operations.
//!
//! This module is private: outward adapters receive [`crate::RrdEngine`] and
//! engine-owned value types, never a second storage-opening handle or an
//! `rrd_store::Engine` escape.

use crate::{
    InstanceBinding, LifecycleSupervisorContextV1, LifecycleToolAuthorizationV1, RrdEngine,
    ServiceError,
};
use rrd_store::Engine as _;
use serde_json::Value;
use std::path::{Path, PathBuf};

pub use rrd_core::{
    digest, Claim, ClaimReader, Millis, Predicate, Producer, Reader, ReasoningPayload, RecallQuery,
    RecallSet, ScopeId, Subject,
};
pub use rrd_store::{
    BackupCatalogue, BackupEntry, Effectiveness, FormatMigrationLedger, GroundingReport,
    Invocation as OperatorInvocation, InvocationInput as OperatorInvocationInput,
    LogicalArchiveInventory, LogicalRestoreReport, MigrationReport, Outcome, RecallOutcome,
    RemovalReport, Trigger,
};

pub type CoreResult<T> = rrd_core::Result<T>;
pub type OperatorResult<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// One bounded lifecycle hook request admitted by the engine authority.
pub struct RuntimeHookRequest<'a> {
    pub root: &'a Path,
    pub harness: Option<&'a str>,
    pub reader: &'a Reader,
    pub now: Millis,
    pub budget: usize,
    pub event: crate::HookEvent,
    pub input: &'a Value,
}

impl RrdEngine {
    /// Opens the canonical project-bound engine authority identified by an RRD
    /// store path. Only `<project>/.rrflow/rrd` is accepted.
    pub fn open_project_store(path: &Path) -> crate::Result<Self> {
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .map_err(|error| ServiceError::Storage(error.to_string()))?
                .join(path)
        };
        let state_root = absolute
            .parent()
            .ok_or_else(|| ServiceError::Contract("RRD store path has no .rrflow parent".into()))?;
        if absolute.file_name().and_then(|value| value.to_str()) != Some("rrd")
            || state_root.file_name().and_then(|value| value.to_str()) != Some(".rrflow")
        {
            return Err(ServiceError::Contract(
                "embedded RRD must use the canonical <project>/.rrflow/rrd path".into(),
            ));
        }
        let project_root = state_root
            .parent()
            .ok_or_else(|| ServiceError::Contract("RRD store path has no project root".into()))?;
        let binding = InstanceBinding::discover(project_root)
            .map_err(|error| ServiceError::Contract(error.to_string()))?;
        binding
            .verify_store_path(&absolute)
            .map_err(|error| ServiceError::Contract(error.to_string()))?;
        Self::open_bound(&binding)
    }

    pub fn path(&self) -> &Path {
        self.storage.path()
    }

    pub fn backend_name(&self) -> &'static str {
        self.storage.backend().as_str()
    }

    pub fn record_operator_invocation(
        &self,
        input: OperatorInvocationInput<'_>,
    ) -> OperatorResult<OperatorInvocation> {
        Ok(self.storage.record_invocation(input)?)
    }

    pub fn set_recall_outcome(
        &self,
        ordinal: u64,
        outcome: RecallOutcome,
    ) -> OperatorResult<OperatorInvocation> {
        Ok(self.storage.set_recall_outcome(ordinal, outcome)?)
    }

    pub fn invocations_since(&self, since: Millis) -> OperatorResult<Vec<OperatorInvocation>> {
        Ok(self.storage.invocations_since(since)?)
    }

    pub fn invocation_count(&self) -> OperatorResult<u64> {
        Ok(self.storage.invocation_count()?)
    }

    pub fn access_count(&self) -> OperatorResult<usize> {
        Ok(self.storage.access_count()?)
    }

    pub fn sequence(&self) -> OperatorResult<u64> {
        Ok(self.storage.sequence()?)
    }

    pub fn observe(
        &self,
        reader: &Reader,
        subject: &Subject,
        predicate: &Predicate,
        at: Millis,
    ) -> OperatorResult<()> {
        Ok(self.storage.observe(reader, subject, predicate, at)?)
    }

    pub fn recall(&self, query: &RecallQuery, token_budget: usize) -> OperatorResult<RecallSet> {
        Ok(rrd_core::recall(&self.storage, query, token_budget)?)
    }

    pub fn assert_claim(&self, claim: &Claim) -> OperatorResult<rrd_store::AppendOutcome> {
        Ok(self.storage.assert(claim)?)
    }

    pub fn claim_as_of(
        &self,
        subject: &Subject,
        predicate: &Predicate,
        at: Millis,
    ) -> OperatorResult<Option<Claim>> {
        Ok(self.storage.as_of(subject, predicate, at)?)
    }

    pub fn claim_history(
        &self,
        subject: &Subject,
        predicate: &Predicate,
    ) -> OperatorResult<Vec<Claim>> {
        Ok(self.storage.history(subject, predicate)?)
    }

    pub fn rebuild_current(&self) -> OperatorResult<rrd_store::RebuildOutcome> {
        Ok(self.storage.rebuild_current()?)
    }

    pub fn ground_current(&self, at: Millis) -> OperatorResult<GroundingReport> {
        Ok(self.storage.ground_current(at)?)
    }

    pub fn reset_current(&self) -> OperatorResult<rrd_store::RebuildOutcome> {
        Ok(self.storage.reset_current()?)
    }

    pub fn removal_report(
        &self,
        since: Millis,
        evaluated_at: Millis,
    ) -> OperatorResult<RemovalReport> {
        Ok(self.storage.removal_report(since, evaluated_at)?)
    }

    pub fn export_logical_archive(
        &self,
        archive: &Path,
    ) -> OperatorResult<LogicalArchiveInventory> {
        Ok(rrd_store::export_logical_archive(&self.storage, archive)?)
    }

    pub fn create_logical_backup(
        &self,
        catalogue: &Path,
        label: &str,
        now: Millis,
    ) -> OperatorResult<BackupEntry> {
        Ok(rrd_store::create_logical_backup(
            &self.storage,
            catalogue,
            label,
            now,
        )?)
    }

    pub fn verify_project_store(&self, root: &Path) -> OperatorResult<()> {
        let binding = InstanceBinding::discover(root)?;
        binding.require_runtime_ready()?;
        binding.verify_store_path(self.path())?;
        if binding.manifest.id != self.instance_id().as_str() {
            return Err("engine instance identity does not match the project manifest".into());
        }
        Ok(())
    }

    pub fn runtime_preflight(
        &self,
        root: &Path,
        harness: Option<&str>,
        reader: &Reader,
        now: Millis,
        budget: usize,
    ) -> OperatorResult<crate::Preflight> {
        crate::preflight(&self.storage, root, harness, reader, now, budget)
    }

    pub fn handle_runtime_hook(
        &self,
        request: RuntimeHookRequest<'_>,
    ) -> OperatorResult<crate::HookResponse> {
        crate::handle(
            &crate::HookContext {
                store: &self.storage,
                root: request.root,
                harness: request.harness,
                reader: request.reader,
                now: request.now,
                budget: request.budget,
            },
            request.event,
            request.input,
        )
    }

    pub fn consume_lifecycle_authorization(
        &self,
        context: &LifecycleSupervisorContextV1,
        authorization: &LifecycleToolAuthorizationV1,
        now: Millis,
    ) -> OperatorResult<()> {
        crate::consume_lifecycle_tool_authorization(&self.storage, context, authorization, now)
    }

    pub fn consume_attuned_authorization(
        &self,
        root: &Path,
        tool_sha256: &str,
        now: Millis,
        actor: &str,
    ) -> OperatorResult<crate::ProjectAttunementReceipt> {
        crate::consume_attuned_tool_authorization(&self.storage, root, tool_sha256, now, actor)
    }

    pub fn execute_operator_query(
        &self,
        scope: ScopeId,
        source: &str,
        parameters: &crate::Parameters,
        budget: &crate::ExecutionBudget,
        actor: &str,
        at: Millis,
    ) -> OperatorResult<crate::TracedQueryExecution> {
        crate::execute_traced_query(&self.storage, scope, source, parameters, budget, actor, at)
    }

    pub fn initialize_runtime(
        &self,
        root: &Path,
        harness: &crate::Harness,
        at: Millis,
    ) -> OperatorResult<crate::InitReport> {
        crate::init(&self.storage, root, harness, at)
    }

    pub fn record_reasoning_event(
        &self,
        run: &str,
        now: Millis,
        actor: &str,
        payload: ReasoningPayload,
    ) -> OperatorResult<rrd_core::ReasoningEvent> {
        crate::record_reasoning(&self.storage, run, now, actor, payload)
    }

    pub fn reasoning_run_by_id(&self, run: &str) -> OperatorResult<Option<rrd_core::ReasoningRun>> {
        crate::reasoning_run(&self.storage, run)
    }

    pub fn active_reasoning(&self) -> OperatorResult<Option<rrd_core::ReasoningRun>> {
        crate::active_reasoning_run(&self.storage)
    }

    pub fn record_harness_verification(
        &self,
        registry: &crate::Registry,
        name: &str,
        now: Millis,
        evidence: &str,
        actor: &str,
    ) -> OperatorResult<Claim> {
        Ok(registry.record_verification(&self.storage, name, now, evidence, actor)?)
    }

    pub fn harness_verification(
        &self,
        registry: &crate::Registry,
        harness: &crate::Harness,
        now: Millis,
    ) -> OperatorResult<crate::Verification> {
        Ok(registry.verification(&self.storage, harness, now)?)
    }

    pub fn reset_project_routing(&self, root: &Path) -> OperatorResult<crate::RoutingReady> {
        crate::reset_routing(&self.storage, root)
    }

    pub fn install_operator_work_plan(
        &self,
        definition: rrd_contract::WorkPlanDefinition,
        now: Millis,
        actor: &str,
        correlation_id: &str,
    ) -> OperatorResult<crate::WorkPlanSnapshot> {
        crate::install_work_plan(&self.storage, definition, now, actor, correlation_id)
    }

    pub fn operator_work_plan(
        &self,
        plan: &str,
    ) -> OperatorResult<Option<crate::WorkPlanSnapshot>> {
        crate::load_work_plan(&self.storage, plan)
    }

    pub fn activate_operator_work_item(
        &self,
        plan: &str,
        item: &str,
        now: Millis,
        actor: &str,
        correlation_id: &str,
    ) -> OperatorResult<crate::WorkPlanSnapshot> {
        crate::activate_work_item(&self.storage, plan, item, now, actor, correlation_id)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_operator_work_item_plan(
        &self,
        root: &Path,
        plan: &str,
        item: &str,
        execution_mode: crate::WorkItemExecutionMode,
        payload: &[u8],
        commands: Vec<Vec<String>>,
        now: Millis,
        actor: &str,
        correlation_id: &str,
    ) -> OperatorResult<crate::WorkPlanSnapshot> {
        crate::record_project_work_item_plan(
            &self.storage,
            root,
            plan,
            item,
            execution_mode,
            payload,
            commands,
            now,
            actor,
            correlation_id,
        )
    }

    pub fn verify_operator_work_item(
        &self,
        root: &Path,
        plan: &str,
        now: Millis,
        actor: &str,
        correlation_id: &str,
    ) -> OperatorResult<crate::WorkPlanSnapshot> {
        crate::verify_recorded_work_item(&self.storage, root, plan, now, actor, correlation_id)
    }

    pub fn migrate_storage(db: &Path, now: Millis) -> OperatorResult<MigrationReport> {
        Ok(rrd_store::migrate_fjall_to_native(db, now)?)
    }

    pub fn storage_migration_status(db: &Path) -> OperatorResult<Option<MigrationReport>> {
        Ok(rrd_store::migration_status(db)?)
    }

    pub fn rollback_storage_migration(db: &Path) -> OperatorResult<MigrationReport> {
        Ok(rrd_store::rollback_fjall_migration(db)?)
    }

    pub fn inspect_logical_archive(archive: &Path) -> OperatorResult<LogicalArchiveInventory> {
        Ok(rrd_store::inspect_logical_archive(archive)?)
    }

    pub fn restore_logical_archive(
        archive: &Path,
        db: &Path,
        now: Millis,
    ) -> OperatorResult<LogicalRestoreReport> {
        Ok(rrd_store::restore_logical_archive_to_new_root(
            archive, db, now,
        )?)
    }

    pub fn verify_backup_catalogue(catalogue: &Path) -> OperatorResult<BackupCatalogue> {
        Ok(rrd_store::verify_backup_catalogue(catalogue)?)
    }

    pub fn restore_catalogued_backup(
        catalogue: &Path,
        backup_id: &str,
        db: &Path,
        now: Millis,
    ) -> OperatorResult<LogicalRestoreReport> {
        Ok(rrd_store::restore_catalogued_backup(
            catalogue, backup_id, db, now,
        )?)
    }

    pub fn migrate_native_format(db: &Path, now: Millis) -> OperatorResult<FormatMigrationLedger> {
        Ok(rrd_store::migrate_native_format(db, now)?)
    }

    pub fn native_format_migration_status(
        db: &Path,
    ) -> OperatorResult<Option<FormatMigrationLedger>> {
        Ok(rrd_store::native_format_migration_status(db)?)
    }

    pub fn canonical_project_root_for_store(path: &Path) -> OperatorResult<PathBuf> {
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()?.join(path)
        };
        let state = absolute
            .parent()
            .ok_or("RRD store has no state directory")?;
        if absolute.file_name().and_then(|value| value.to_str()) != Some("rrd")
            || state.file_name().and_then(|value| value.to_str()) != Some(".rrflow")
        {
            return Err("RRD store must be <project>/.rrflow/rrd".into());
        }
        Ok(std::fs::canonicalize(
            state.parent().ok_or("RRD store has no project root")?,
        )?)
    }
}
