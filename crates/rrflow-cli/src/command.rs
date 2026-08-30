//! Command definitions and execution.
//!
//! Execution is separated from `main` so that every command is exercised by
//! integration tests through the same path the operator uses, rather than
//! through a parallel test-only entry point.

use crate::workplan::WorkPlanAction;
use clap::{Args, Parser, Subcommand, ValueEnum};
use rrd_engine::{
    digest, Claim, CoreResult, Effectiveness, GroundingReport, Millis, Outcome, Predicate,
    Producer, Reader, ReasoningPayload, RecallOutcome, RecallQuery, RrdEngine, ScopeId, Subject,
    Trigger,
};

/// What a command produced: the operator-facing text, the `SPEC.md` §13.1
/// effectiveness fields for recall-carrying commands, and a detail line for
/// the invocation record.
pub struct Execution {
    pub text: String,
    pub effectiveness: Option<Effectiveness>,
    pub detail: Option<String>,
    /// Process success is explicit so diagnostic commands can emit their full
    /// machine-readable report while still failing CI on a blocked topology.
    pub success: bool,
}

impl From<String> for Execution {
    fn from(text: String) -> Self {
        Execution {
            text,
            effectiveness: None,
            detail: None,
            success: true,
        }
    }
}

#[derive(Parser, Debug)]
#[command(
    name = "rrflow",
    about = "RRFlow operator and development surface",
    long_about = "Every invocation is recorded (SPEC.md §13). No trigger may be \
                  automated before its recorded invocations justify it."
)]
pub struct Cli {
    /// Database directory.
    #[arg(long, short = 'd', env = "RRFLOW_DB")]
    pub db: Option<std::path::PathBuf>,

    /// Emit JSON instead of rendered text.
    #[arg(long, global = true)]
    pub json: bool,

    /// Identity recorded as the reader of any claim this command reads.
    #[arg(long, global = true, default_value = "operator:cli")]
    pub reader: String,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Command {
    /// Inspect whether this checkout can run the canonical RRFlow development
    /// topology without split storage authority or manual hidden state.
    Dev {
        #[command(subcommand)]
        action: DevAction,
    },
    /// Inspect and advance the RRD-enforced project work plan.
    WorkPlan {
        #[command(subcommand)]
        action: WorkPlanAction,
    },
    /// List or invoke the generated runtime catalogue through one explicit
    /// embedded engine or authenticated daemon authority.
    Runtime {
        #[command(flatten)]
        authority: RuntimeAuthorityArgs,
        #[command(subcommand)]
        action: RuntimeAction,
    },
    /// Record a claim.
    Assert {
        #[arg(long)]
        subject: String,
        #[arg(long)]
        predicate: String,
        #[arg(long)]
        object: String,
        /// Start of the valid-time interval. Defaults to now.
        #[arg(long)]
        valid_from: Option<Millis>,
        /// Actor recorded as the producer.
        #[arg(long, default_value = "operator:cli")]
        actor: String,
        /// Model the actor acted on behalf of.
        #[arg(long)]
        on_behalf_of: Option<String>,
    },
    /// Resolve the claim in force at an instant.
    AsOf {
        #[arg(long)]
        subject: String,
        #[arg(long)]
        predicate: String,
        /// Instant to resolve at. Defaults to now.
        #[arg(long)]
        at: Option<Millis>,
    },
    /// Every recorded version of a subject and predicate, newest first.
    History {
        #[arg(long)]
        subject: String,
        #[arg(long)]
        predicate: String,
    },
    /// Store counters and watermarks.
    Status,
    /// Derive removal candidates. Analysis only; nothing is removed.
    Gc {
        /// Inclusive lower bound of the interval considered.
        #[arg(long, default_value_t = 0)]
        since: Millis,
    },
    /// Recorded invocations, chronologically.
    Invocations {
        #[arg(long, default_value_t = 0)]
        since: Millis,
    },
    /// Resolve the claims in force for a subject set into a recall set
    /// (`SPEC.md` §10). Semantic content with provenance; rendering to a
    /// prompt belongs to the consuming adapter.
    Recall {
        /// Subject to recall. Repeatable.
        #[arg(long = "subject", required = true)]
        subjects: Vec<String>,
        /// Narrow to these predicates. Repeatable; absent recalls all.
        #[arg(long = "predicate")]
        predicates: Vec<String>,
        /// Instant to resolve at. Defaults to now.
        #[arg(long)]
        at: Option<Millis>,
        /// Token budget for the recall set.
        #[arg(long, default_value_t = 1500)]
        budget: usize,
        /// Consumer the recall set is destined for, recorded in the ledger.
        #[arg(long, default_value = "frontier:claude")]
        provider: String,
    },
    /// Judge a recorded recall after the fact (`SPEC.md` §13.1): accepted,
    /// corrected, or discarded. This is the signal trigger policy derives from.
    Outcome {
        /// Ordinal of the recall invocation being judged.
        #[arg(long)]
        ordinal: u64,
        /// accepted | corrected | discarded | unknown
        #[arg(long)]
        outcome: String,
    },
    /// The effectiveness ledger: recall records and their outcome distribution.
    Ledger {
        #[arg(long, default_value_t = 0)]
        since: Millis,
    },
    /// Advance the current-state projection over the claim log (`SPEC.md`
    /// §8.2). Applies the interval above the watermark and advances the
    /// watermark in the same write.
    Rebuild,
    /// Rebuild to the current sequence, then difference the projection
    /// against a full recomputation (`SPEC.md` §8.3). Divergence halts and
    /// quarantines the projection; it is never repaired here.
    Ground,
    /// Discard the current-state projection and recompute it from the claim
    /// log. The only exit from quarantine, and an explicit operator decision.
    ResetProjection,
    /// Explicitly discard and rebuild the persisted source-routing index.
    /// This is the recovery path for corrupt state and the only way to rebind
    /// one database to a different project root.
    ResetRouting {
        #[arg(long, default_value = ".")]
        root: std::path::PathBuf,
    },
    /// The moment of attunement (`PLAN.md` Step P): detect the stack, check
    /// estate health and adapter verification, and assemble a task-specific
    /// bounded source and memory context.
    Preflight {
        /// Project root for stack detection.
        #[arg(long, default_value = ".")]
        root: std::path::PathBuf,
        /// Harness adapter in use, so its verification drift can make noise.
        #[arg(long)]
        harness: Option<String>,
        /// Prompt or task used for deterministic source and claim routing.
        /// When absent, the active work item supplies the task.
        #[arg(long)]
        task: Option<String>,
        #[arg(long, default_value_t = 1500)]
        budget: usize,
    },
    /// Harness lifecycle dispatch: reads the harness JSON on stdin, answers
    /// on stdout. Wired by `rrflow init`; recorded with trigger `event`.
    Hook {
        /// session-start | user-prompt-submit | pre-tool-use |
        /// post-tool-use | stop | pre-compact | session-end
        event: String,
        #[arg(long)]
        harness: Option<String>,
        /// Project root override; defaults to the `cwd` field of the hook
        /// input, then the working directory.
        #[arg(long)]
        root: Option<std::path::PathBuf>,
        #[arg(long, default_value_t = 1500)]
        budget: usize,
    },
    /// Write the harness wiring for this project: context-file block, plus
    /// hook configuration where the harness supports it. Refuses retired
    /// harnesses.
    Init {
        #[arg(long)]
        harness: String,
        #[arg(long, default_value = ".")]
        root: std::path::PathBuf,
    },
    /// Execute one explicit, durably traced RRFlowQL query. Raw query and
    /// parameter values are returned but never persisted as trace attributes.
    Query {
        /// Project root used to prove this database belongs to one instance.
        #[arg(long, default_value = ".")]
        root: std::path::PathBuf,
        /// One concrete runtime scope. Cross-scope queries are not implicit.
        #[arg(long, default_value = "instance:default")]
        scope: String,
        /// RRFlowQL source text.
        #[arg(long)]
        ql: String,
        /// Scalar binder value as NAME=JSON. Repeatable.
        #[arg(long = "parameter")]
        parameters: Vec<String>,
        #[arg(long, default_value_t = 100_000)]
        max_scanned_changes: usize,
        #[arg(long, default_value_t = 10_000)]
        max_rows: usize,
        #[arg(long, default_value_t = 8 * 1024 * 1024)]
        max_output_bytes: usize,
        #[arg(long, default_value_t = 256)]
        max_batch_rows: usize,
    },
    /// Execute one exact argv vector through RRFlow's durable lifecycle gate.
    /// No shell parses, expands, redirects, or composes this command.
    Exec {
        /// Project root whose attunement, work plan, and instance bind this run.
        #[arg(long, default_value = ".")]
        root: std::path::PathBuf,
        /// Canonical RRFlow lifecycle session. Required when the project has
        /// a checked-in work plan; may also be supplied by RRFLOW_SESSION_ID.
        #[arg(long, env = "RRFLOW_SESSION_ID")]
        session_id: Option<String>,
        /// Working directory, relative to the project root unless absolute.
        #[arg(long)]
        cwd: Option<std::path::PathBuf>,
        /// Maximum child runtime before RRFlow kills it.
        #[arg(long, default_value_t = 15 * 60 * 1_000)]
        timeout_ms: u64,
        /// Maximum bytes retained from each output stream. Both streams are
        /// fully drained and hashed even when their displayed prefix truncates.
        #[arg(long, default_value_t = 1024 * 1024)]
        max_output_bytes: usize,
        /// Executable followed by its exact arguments. The `--` separator is
        /// mandatory so RRFlow flags cannot be confused with child flags.
        #[arg(last = true, required = true, num_args = 1.., allow_hyphen_values = true)]
        exact_argv: Vec<String>,
    },
    /// Record or inspect the typed operational reasoning contract.
    Reasoning {
        #[command(subcommand)]
        action: ReasoningAction,
    },
    /// The harness registry and its drift alarm.
    Harness {
        #[command(subcommand)]
        action: HarnessAction,
    },
    /// Offline storage migration and recovery operations.
    Storage {
        #[command(subcommand)]
        action: StorageAction,
    },
}

#[derive(Args, Debug, Clone)]
pub struct RuntimeAuthorityArgs {
    /// Runtime authority. Embedded owns the bound store; daemon owns no
    /// storage and authenticates to rrd-server through rrd-client.
    #[arg(long, value_enum, default_value_t = RuntimeMode::Embedded)]
    pub mode: RuntimeMode,
    /// Project root for embedded authority discovery.
    #[arg(long)]
    pub root: Option<std::path::PathBuf>,
    /// Loopback HTTP URL for daemon authority.
    #[arg(long)]
    pub url: Option<String>,
    /// Canonical instance identifier for daemon authority.
    #[arg(long)]
    pub instance: Option<String>,
    /// Canonical security principal for daemon authority.
    #[arg(long)]
    pub principal: Option<String>,
    /// Absolute owner-only API-key file for daemon authentication.
    #[arg(long)]
    pub api_key_file: Option<std::path::PathBuf>,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeMode {
    Embedded,
    Daemon,
}

impl RuntimeMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Embedded => "embedded",
            Self::Daemon => "daemon",
        }
    }
}

#[derive(Subcommand, Debug, Clone)]
pub enum RuntimeAction {
    /// Emit the exact generated runtime-tool catalogue.
    List,
    /// Invoke one generated runtime tool with a JSON object argument.
    Call {
        #[arg(long)]
        tool: String,
        #[arg(long, default_value = "{}")]
        arguments: String,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum DevAction {
    /// Emit every development-topology invariant and fail when a required
    /// invariant is blocked.
    Doctor {
        /// Workspace root to inspect.
        #[arg(long, default_value = ".")]
        root: std::path::PathBuf,
    },
    /// Build and start one RRD authority followed by its authenticated
    /// Connectome client, then wait for both readiness probes.
    Up {
        #[arg(long, default_value = ".")]
        root: std::path::PathBuf,
        #[arg(long)]
        instance: Option<String>,
        #[arg(long, default_value = "127.0.0.1:9477")]
        rrd_bind: std::net::SocketAddr,
        #[arg(long, default_value = "127.0.0.1:4387")]
        connectome_bind: std::net::SocketAddr,
        /// Reuse companion binaries beside rrflow (or in RRFLOW_DEV_BIN_DIR).
        #[arg(long)]
        no_build: bool,
    },
    /// Probe the services recorded by the recoverable supervisor manifest.
    Status {
        #[arg(long, default_value = ".")]
        root: std::path::PathBuf,
    },
    /// Read a bounded tail of the retained service logs.
    Logs {
        #[arg(long, default_value = ".")]
        root: std::path::PathBuf,
        #[arg(long, default_value = "all")]
        service: String,
        #[arg(long, default_value_t = 100)]
        lines: usize,
    },
    /// Request graceful shutdown from both services and wait for completion.
    Stop {
        #[arg(long, default_value = ".")]
        root: std::path::PathBuf,
        #[arg(long, default_value_t = 10_000)]
        timeout_ms: u64,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum ReasoningAction {
    /// Append one typed transition. `payload` is a tagged ReasoningPayload JSON
    /// object, for example `{"kind":"goal",...}`.
    Record {
        #[arg(long)]
        run: String,
        #[arg(long, default_value = "operator:cli")]
        actor: String,
        #[arg(long)]
        payload: String,
    },
    /// Show one run, or the active run when `--run` is omitted.
    Show {
        #[arg(long)]
        run: Option<String>,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum HarnessAction {
    /// Record a verification for an adapter: a claim valid for 21 days. The
    /// evidence names what was checked, because an audit without evidence is
    /// an assertion.
    Audit {
        #[arg(long)]
        name: String,
        #[arg(long)]
        evidence: String,
    },
    /// Every registry row with its verification state at now.
    Status,
}

#[derive(Subcommand, Debug, Clone)]
pub enum StorageAction {
    /// Start or resume the explicit Fjall-to-RRD LSM migration.
    Migrate,
    /// Inspect the durable migration marker without opening the database.
    Status,
    /// Restore the retained Fjall source if native has not diverged.
    Rollback,
    /// Export a content-authenticated, backend-independent logical archive.
    ArchiveExport {
        #[arg(long)]
        archive: std::path::PathBuf,
    },
    /// Validate a logical archive without mutating a database.
    ArchiveInspect {
        #[arg(long)]
        archive: std::path::PathBuf,
    },
    /// Restore a logical archive into the absent `--db` root.
    ArchiveRestore {
        #[arg(long)]
        archive: std::path::PathBuf,
    },
    /// Create an authenticated catalogue entry and retained logical archive.
    BackupCreate {
        #[arg(long)]
        catalogue: std::path::PathBuf,
        #[arg(long)]
        label: String,
    },
    /// List a catalogue after authenticating it and all retained archives.
    BackupList {
        #[arg(long)]
        catalogue: std::path::PathBuf,
    },
    /// Restore one catalogued backup into the absent `--db` root.
    BackupRestore {
        #[arg(long)]
        catalogue: std::path::PathBuf,
        #[arg(long)]
        backup_id: String,
    },
    /// Start or resume the exact-successor native application-format upgrade.
    FormatUpgrade,
    /// Restore the retained predecessor if the visible successor has not diverged.
    FormatRollback,
    /// Inspect the authenticated native application-format migration ledger.
    FormatStatus,
}

impl Command {
    /// Stable name used in the invocation record.
    pub fn name(&self) -> &'static str {
        match self {
            Command::Dev {
                action: DevAction::Doctor { .. },
            } => "dev-doctor",
            Command::Dev {
                action: DevAction::Up { .. },
            } => "dev-up",
            Command::Dev {
                action: DevAction::Status { .. },
            } => "dev-status",
            Command::Dev {
                action: DevAction::Logs { .. },
            } => "dev-logs",
            Command::Dev {
                action: DevAction::Stop { .. },
            } => "dev-stop",
            Command::WorkPlan { action } => action.name(),
            Command::Runtime {
                action: RuntimeAction::List,
                ..
            } => "runtime-list",
            Command::Runtime {
                action: RuntimeAction::Call { .. },
                ..
            } => "runtime-call",
            Command::Assert { .. } => "assert",
            Command::AsOf { .. } => "as-of",
            Command::History { .. } => "history",
            Command::Status => "status",
            Command::Gc { .. } => "gc",
            Command::Invocations { .. } => "invocations",
            Command::Recall { .. } => "recall",
            Command::Outcome { .. } => "outcome",
            Command::Ledger { .. } => "ledger",
            Command::Rebuild => "rebuild",
            Command::Ground => "ground",
            Command::ResetProjection => "reset-projection",
            Command::ResetRouting { .. } => "reset-routing",
            Command::Preflight { .. } => "preflight",
            Command::Hook { .. } => "hook",
            Command::Init { .. } => "init",
            Command::Query { .. } => "query",
            Command::Exec { .. } => "exec",
            Command::Reasoning {
                action: ReasoningAction::Record { .. },
            } => "reasoning-record",
            Command::Reasoning {
                action: ReasoningAction::Show { .. },
            } => "reasoning-show",
            Command::Harness {
                action: HarnessAction::Audit { .. },
            } => "harness-audit",
            Command::Harness {
                action: HarnessAction::Status,
            } => "harness-status",
            Command::Storage {
                action: StorageAction::Migrate,
            } => "storage-migrate",
            Command::Storage {
                action: StorageAction::Status,
            } => "storage-status",
            Command::Storage {
                action: StorageAction::Rollback,
            } => "storage-rollback",
            Command::Storage {
                action: StorageAction::ArchiveExport { .. },
            } => "storage-archive-export",
            Command::Storage {
                action: StorageAction::ArchiveInspect { .. },
            } => "storage-archive-inspect",
            Command::Storage {
                action: StorageAction::ArchiveRestore { .. },
            } => "storage-archive-restore",
            Command::Storage {
                action: StorageAction::BackupCreate { .. },
            } => "storage-backup-create",
            Command::Storage {
                action: StorageAction::BackupList { .. },
            } => "storage-backup-list",
            Command::Storage {
                action: StorageAction::BackupRestore { .. },
            } => "storage-backup-restore",
            Command::Storage {
                action: StorageAction::FormatUpgrade,
            } => "storage-format-upgrade",
            Command::Storage {
                action: StorageAction::FormatRollback,
            } => "storage-format-rollback",
            Command::Storage {
                action: StorageAction::FormatStatus,
            } => "storage-format-status",
        }
    }

    /// What caused the invocation. Hook dispatches are the promoted
    /// automation: they record `Event`, exactly as the `Trigger` enum
    /// anticipated — a change of value, not of schema.
    pub fn trigger(&self) -> Trigger {
        match self {
            Command::Hook { .. } => Trigger::Event,
            _ => Trigger::Manual,
        }
    }

    /// Arguments recorded alongside the invocation, so a recorded run can be
    /// reproduced.
    pub fn arguments(&self) -> Vec<String> {
        match self {
            Command::Assert {
                subject,
                predicate,
                object,
                valid_from,
                actor,
                on_behalf_of,
            } => {
                let mut a = vec![
                    format!("subject={subject}"),
                    format!("predicate={predicate}"),
                    format!("object={object}"),
                    format!("actor={actor}"),
                ];
                if let Some(v) = valid_from {
                    a.push(format!("valid_from={v}"));
                }
                if let Some(o) = on_behalf_of {
                    a.push(format!("on_behalf_of={o}"));
                }
                a
            }
            Command::AsOf {
                subject,
                predicate,
                at,
            } => {
                let mut a = vec![
                    format!("subject={subject}"),
                    format!("predicate={predicate}"),
                ];
                if let Some(t) = at {
                    a.push(format!("at={t}"));
                }
                a
            }
            Command::History { subject, predicate } => {
                vec![
                    format!("subject={subject}"),
                    format!("predicate={predicate}"),
                ]
            }
            Command::Status => Vec::new(),
            Command::Gc { since } => vec![format!("since={since}")],
            Command::Invocations { since } => vec![format!("since={since}")],
            Command::Recall {
                subjects,
                predicates,
                at,
                budget,
                provider,
            } => {
                let mut a: Vec<String> = subjects.iter().map(|s| format!("subject={s}")).collect();
                a.extend(predicates.iter().map(|p| format!("predicate={p}")));
                if let Some(t) = at {
                    a.push(format!("at={t}"));
                }
                a.push(format!("budget={budget}"));
                a.push(format!("provider={provider}"));
                a
            }
            Command::Outcome { ordinal, outcome } => {
                vec![format!("ordinal={ordinal}"), format!("outcome={outcome}")]
            }
            Command::Ledger { since } => vec![format!("since={since}")],
            Command::Rebuild | Command::Ground | Command::ResetProjection => Vec::new(),
            Command::ResetRouting { root } => vec![format!("root={}", root.display())],
            Command::Preflight {
                root,
                harness,
                task,
                budget,
            } => {
                let mut a = vec![
                    format!("root={}", root.display()),
                    format!("budget={budget}"),
                ];
                if let Some(h) = harness {
                    a.push(format!("harness={h}"));
                }
                if let Some(task) = task {
                    a.push(format!("task={task}"));
                }
                a
            }
            Command::Hook {
                event,
                harness,
                root,
                budget,
            } => {
                let mut a = vec![format!("event={event}"), format!("budget={budget}")];
                if let Some(h) = harness {
                    a.push(format!("harness={h}"));
                }
                if let Some(r) = root {
                    a.push(format!("root={}", r.display()));
                }
                a
            }
            Command::Init { harness, root } => {
                vec![
                    format!("harness={harness}"),
                    format!("root={}", root.display()),
                ]
            }
            Command::Query {
                root,
                scope,
                ql,
                parameters,
                max_scanned_changes,
                max_rows,
                max_output_bytes,
                max_batch_rows,
            } => vec![
                format!("root={}", root.display()),
                format!("scope={scope}"),
                format!("query_digest={}", digest::sha256_hex(ql.as_bytes())),
                format!(
                    "parameter_input_digest={}",
                    digest::sha256_hex(parameters.join("\0").as_bytes())
                ),
                format!("parameter_count={}", parameters.len()),
                format!("max_scanned_changes={max_scanned_changes}"),
                format!("max_rows={max_rows}"),
                format!("max_output_bytes={max_output_bytes}"),
                format!("max_batch_rows={max_batch_rows}"),
            ],
            Command::Exec {
                root,
                session_id,
                cwd,
                timeout_ms,
                max_output_bytes,
                exact_argv,
            } => vec![
                format!("root={}", root.display()),
                format!(
                    "session_id_sha256={}",
                    session_id.as_deref().map_or_else(
                        || "none".into(),
                        |value| digest::sha256_hex(value.as_bytes())
                    )
                ),
                format!(
                    "cwd={}",
                    cwd.as_deref()
                        .unwrap_or_else(|| std::path::Path::new("."))
                        .display()
                ),
                format!("timeout_ms={timeout_ms}"),
                format!("max_output_bytes={max_output_bytes}"),
                format!("argc={}", exact_argv.len()),
                format!(
                    "exact_argv_sha256={}",
                    digest::sha256_hex(&serde_json::to_vec(exact_argv).unwrap_or_default())
                ),
            ],
            Command::Reasoning {
                action:
                    ReasoningAction::Record {
                        run,
                        actor,
                        payload,
                    },
            } => {
                vec![
                    format!("run={run}"),
                    format!("actor={actor}"),
                    format!("payload={payload}"),
                ]
            }
            Command::Reasoning {
                action: ReasoningAction::Show { run },
            } => run.iter().map(|run| format!("run={run}")).collect(),
            Command::Harness {
                action: HarnessAction::Audit { name, evidence },
            } => {
                vec![format!("name={name}"), format!("evidence={evidence}")]
            }
            Command::Harness {
                action: HarnessAction::Status,
            } => Vec::new(),
            Command::Storage {
                action:
                    StorageAction::ArchiveExport { archive }
                    | StorageAction::ArchiveInspect { archive }
                    | StorageAction::ArchiveRestore { archive },
            } => {
                vec![format!("archive={}", archive.display())]
            }
            Command::Storage {
                action: StorageAction::BackupCreate { catalogue, label },
            } => vec![
                format!("catalogue={}", catalogue.display()),
                format!("label={label}"),
            ],
            Command::Storage {
                action: StorageAction::BackupList { catalogue },
            } => {
                vec![format!("catalogue={}", catalogue.display())]
            }
            Command::Storage {
                action:
                    StorageAction::BackupRestore {
                        catalogue,
                        backup_id,
                    },
            } => vec![
                format!("catalogue={}", catalogue.display()),
                format!("backup_id={backup_id}"),
            ],
            Command::Storage { .. } => Vec::new(),
            Command::Dev {
                action: DevAction::Doctor { root },
            } => vec![format!("root={}", root.display())],
            Command::Dev {
                action:
                    DevAction::Up {
                        root,
                        instance,
                        rrd_bind,
                        connectome_bind,
                        no_build,
                    },
            } => {
                let mut arguments = vec![
                    format!("root={}", root.display()),
                    format!("rrd_bind={rrd_bind}"),
                    format!("connectome_bind={connectome_bind}"),
                    format!("no_build={no_build}"),
                ];
                if let Some(instance) = instance {
                    arguments.push(format!("instance={instance}"));
                }
                arguments
            }
            Command::Dev {
                action: DevAction::Status { root },
            } => vec![format!("root={}", root.display())],
            Command::Dev {
                action:
                    DevAction::Logs {
                        root,
                        service,
                        lines,
                    },
            } => vec![
                format!("root={}", root.display()),
                format!("service={service}"),
                format!("lines={lines}"),
            ],
            Command::Dev {
                action: DevAction::Stop { root, timeout_ms },
            } => vec![
                format!("root={}", root.display()),
                format!("timeout_ms={timeout_ms}"),
            ],
            Command::WorkPlan { action } => action.arguments(),
            Command::Runtime { authority, action } => {
                let mut arguments = vec![format!("mode={}", authority.mode.as_str())];
                if let Some(root) = &authority.root {
                    arguments.push(format!("root={}", root.display()));
                }
                if let Some(url) = &authority.url {
                    arguments.push(format!("url={url}"));
                }
                if let Some(instance) = &authority.instance {
                    arguments.push(format!("instance={instance}"));
                }
                if let Some(principal) = &authority.principal {
                    arguments.push(format!("principal={principal}"));
                }
                if let Some(api_key_file) = &authority.api_key_file {
                    arguments.push(format!("api_key_file={}", api_key_file.display()));
                }
                match action {
                    RuntimeAction::List => arguments.push("action=list".into()),
                    RuntimeAction::Call {
                        tool,
                        arguments: input,
                    } => {
                        arguments.push("action=call".into());
                        arguments.push(format!("tool={tool}"));
                        arguments.push(format!(
                            "arguments_sha256={}",
                            digest::sha256_hex(input.as_bytes())
                        ));
                    }
                }
                arguments
            }
        }
    }
}

/// Executes commands that must run before the normal persistent Engine is
/// opened. Migration deliberately takes exclusive ownership of the database
/// directory and therefore cannot travel through the invocation wrapper.
pub fn execute_offline(
    db: &std::path::Path,
    command: &Command,
    now: Millis,
    json: bool,
) -> Option<Result<Execution, Box<dyn std::error::Error>>> {
    if let Command::Dev { action } = command {
        return Some((|| match action {
            DevAction::Doctor { root } => {
                let report = crate::dev::doctor(root)?;
                let success = report.ready;
                let text = if json {
                    serde_json::to_string_pretty(&report)?
                } else {
                    report.render()
                };
                Ok(Execution {
                    text,
                    effectiveness: None,
                    detail: Some(format!(
                        "{} passed, {} blocked, {} warnings",
                        report.passed, report.blocked, report.warnings
                    )),
                    success,
                })
            }
            DevAction::Up {
                root,
                instance,
                rrd_bind,
                connectome_bind,
                no_build,
            } => {
                let report = crate::dev::supervisor::up(
                    crate::dev::supervisor::UpOptions {
                        root: root.clone(),
                        instance: instance.clone(),
                        rrd_bind: *rrd_bind,
                        connectome_bind: *connectome_bind,
                        no_build: *no_build,
                    },
                    now,
                )?;
                let text = if json {
                    serde_json::to_string_pretty(&report)?
                } else {
                    report.render()
                };
                Ok(text.into())
            }
            DevAction::Status { root } => {
                let report = crate::dev::supervisor::status(root)?;
                let success = report.rrd_ready && report.connectome_ready;
                let text = if json {
                    serde_json::to_string_pretty(&report)?
                } else {
                    report.render()
                };
                Ok(Execution {
                    text,
                    effectiveness: None,
                    detail: Some(format!("topology status: {}", report.status)),
                    success,
                })
            }
            DevAction::Logs {
                root,
                service,
                lines,
            } => Ok(crate::dev::supervisor::logs(root, service, *lines)?.into()),
            DevAction::Stop { root, timeout_ms } => {
                let report = crate::dev::supervisor::stop(
                    root,
                    std::time::Duration::from_millis(*timeout_ms),
                )?;
                let text = if json {
                    serde_json::to_string_pretty(&report)?
                } else {
                    report.render()
                };
                Ok(text.into())
            }
        })());
    }
    let action = match command {
        Command::Storage { action } => action,
        _ => return None,
    };
    Some((|| match action {
        StorageAction::Migrate | StorageAction::Status | StorageAction::Rollback => {
            let report = match action {
                StorageAction::Migrate => Some(RrdEngine::migrate_storage(db, now)?),
                StorageAction::Status => RrdEngine::storage_migration_status(db)?,
                StorageAction::Rollback => Some(RrdEngine::rollback_storage_migration(db)?),
                _ => unreachable!("matched migration action"),
            };
            let text = if json {
                serde_json::to_string_pretty(&report)?
            } else if let Some(report) = report {
                format!(
                    "storage migration {:?}: {} entries / {} bytes / sha256 {}\nFjall backup: {}\nArchive: {}",
                    report.phase,
                    report.inventory.entries,
                    report.inventory.payload_bytes,
                    report.inventory.archive_sha256,
                    report.fjall_backup.display(),
                    report.archive.display(),
                )
            } else {
                "no storage migration marker".into()
            };
            Ok(text.into())
        }
        StorageAction::ArchiveExport { archive } => {
            let engine = RrdEngine::open_project_store(db)?;
            let inventory = engine.export_logical_archive(archive)?;
            let text = if json {
                serde_json::to_string_pretty(&inventory)?
            } else {
                format!(
                    "logical archive: {} actions / {} claims / {} runtime mutations / sha256 {}\nArchive: {}",
                    inventory.action_count,
                    inventory.claim_sequence,
                    inventory.runtime_mutations,
                    inventory.archive_sha256,
                    archive.display()
                )
            };
            Ok(text.into())
        }
        StorageAction::ArchiveInspect { archive } => {
            let inventory = RrdEngine::inspect_logical_archive(archive)?;
            let text = if json {
                serde_json::to_string_pretty(&inventory)?
            } else {
                format!(
                    "logical archive verified: {} actions / claim sequence {} / runtime cursor {} / sha256 {}",
                    inventory.action_count,
                    inventory.claim_sequence,
                    inventory.runtime_cursor,
                    inventory.archive_sha256
                )
            };
            Ok(text.into())
        }
        StorageAction::ArchiveRestore { archive } => {
            let report = RrdEngine::restore_logical_archive(archive, db, now)?;
            let text = if json {
                serde_json::to_string_pretty(&report)?
            } else {
                format!(
                    "logical archive restored and reopened: {}\nTarget: {}",
                    report.inventory.archive_sha256,
                    report.target.display()
                )
            };
            Ok(text.into())
        }
        StorageAction::BackupCreate { catalogue, label } => {
            let engine = RrdEngine::open_project_store(db)?;
            let entry = engine.create_logical_backup(catalogue, label, now)?;
            let text = if json {
                serde_json::to_string_pretty(&entry)?
            } else {
                format!(
                    "backup {} retained as {}\nCoverage: claims and typed runtime included; object payloads referenced only; application-complete=false",
                    entry.backup_id, entry.archive_file
                )
            };
            Ok(text.into())
        }
        StorageAction::BackupList { catalogue } => {
            let verified = RrdEngine::verify_backup_catalogue(catalogue)?;
            let text = if json {
                serde_json::to_string_pretty(&verified)?
            } else if verified.backups.is_empty() {
                "backup catalogue is empty".into()
            } else {
                verified
                    .backups
                    .iter()
                    .map(|entry| {
                        format!(
                            "{} {} at {} (claims {}, runtime {}, application-complete={})",
                            entry.backup_id,
                            entry.label,
                            entry.created_at,
                            entry.archive.claim_sequence,
                            entry.archive.runtime_cursor,
                            entry.application_complete
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            Ok(text.into())
        }
        StorageAction::BackupRestore {
            catalogue,
            backup_id,
        } => {
            let report = RrdEngine::restore_catalogued_backup(catalogue, backup_id, db, now)?;
            let text = if json {
                serde_json::to_string_pretty(&report)?
            } else {
                format!(
                    "catalogued backup restored and reopened: {}\nTarget: {}",
                    backup_id,
                    report.target.display()
                )
            };
            Ok(text.into())
        }
        StorageAction::FormatUpgrade => {
            let ledger = RrdEngine::migrate_native_format(db, now)?;
            let text = if json {
                serde_json::to_string_pretty(&ledger)?
            } else {
                format!(
                    "native format migration {:?}: {:?} -> {} / {} entries / sha256 {}",
                    ledger.phase,
                    ledger.source_application_format,
                    ledger.target_application_format,
                    ledger.inventory.entries,
                    ledger.inventory.archive_sha256
                )
            };
            Ok(text.into())
        }
        StorageAction::FormatRollback => {
            let ledger = RrdEngine::rollback_native_format(db)?;
            let text = if json {
                serde_json::to_string_pretty(&ledger)?
            } else {
                format!(
                    "native format rollback {:?}: {:?} <- {} / {} entries / sha256 {}",
                    ledger.phase,
                    ledger.source_application_format,
                    ledger.target_application_format,
                    ledger.inventory.entries,
                    ledger.inventory.archive_sha256
                )
            };
            Ok(text.into())
        }
        StorageAction::FormatStatus => {
            let ledger = RrdEngine::native_format_migration_status(db)?;
            let text = if json {
                serde_json::to_string_pretty(&ledger)?
            } else if let Some(ledger) = ledger {
                format!(
                    "native format migration {:?}: {:?} -> {}",
                    ledger.phase,
                    ledger.source_application_format,
                    ledger.target_application_format
                )
            } else {
                "no native format migration ledger".into()
            };
            Ok(text.into())
        }
    })())
}

/// Executes one command.
///
/// `now` is supplied rather than read here, so that tests are deterministic and
/// the clock enters at exactly one place (`main`).
pub fn execute(
    store: &RrdEngine,
    command: &Command,
    reader: &Reader,
    now: Millis,
    json: bool,
) -> Result<Execution, Box<dyn std::error::Error>> {
    match command {
        Command::Runtime { .. } => {
            return Err("runtime commands require the selected runtime authority".into());
        }
        Command::WorkPlan { action } => {
            return crate::workplan::execute(store, action, reader, now, json);
        }
        Command::Exec {
            root,
            session_id,
            cwd,
            timeout_ms,
            max_output_bytes,
            exact_argv,
        } => {
            verify_instance_store(store, root)?;
            return crate::command_proxy::execute(
                store,
                crate::command_proxy::ExactCommandRequest {
                    root,
                    session_id: session_id.as_deref(),
                    requested_cwd: cwd.as_deref(),
                    exact_argv,
                    timeout_ms: *timeout_ms,
                    max_output_bytes: *max_output_bytes,
                },
                reader,
                now,
                json,
            );
        }
        Command::Recall {
            subjects,
            predicates,
            at,
            budget,
            provider,
        } => {
            let query = RecallQuery {
                subjects: subjects
                    .iter()
                    .map(|s| Subject::new(s.clone()))
                    .collect::<CoreResult<Vec<_>>>()?,
                predicates: if predicates.is_empty() {
                    None
                } else {
                    Some(
                        predicates
                            .iter()
                            .map(|p| Predicate::new(p.clone()))
                            .collect::<CoreResult<Vec<_>>>()?,
                    )
                },
                as_of: at.unwrap_or(now),
            };
            let set = store.recall(&query, *budget)?;
            // Every recalled claim is a read, recorded per SPEC.md §7.
            for claim in &set.claims {
                store.observe(reader, &claim.subject, &claim.predicate, now)?;
            }
            let effectiveness = Effectiveness {
                query: query
                    .subjects
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(","),
                claims_returned: set.claims.len(),
                tokens_emitted: set.token_estimate as u64,
                // A manual recall has no baseline arm; the reduction is
                // unverified until the A/B harness supplies one (§13.1).
                baseline_tokens: None,
                baseline_mode: None,
                provider: provider.clone(),
                outcome: RecallOutcome::Unknown,
            };
            let text = if json {
                serde_json::to_string_pretty(&set)?
            } else {
                let mut lines: Vec<String> = set
                    .claims
                    .iter()
                    .map(|c| {
                        format!(
                            "{} {} = {}  [valid_from={} tx={} by {}]",
                            c.subject.as_str(),
                            c.predicate.as_str(),
                            c.object,
                            c.valid_from,
                            c.tx_time,
                            c.producer.actor,
                        )
                    })
                    .collect();
                lines.push(format!(
                    "-- {} claim(s), ~{} token(s), digest {}{}",
                    set.claims.len(),
                    set.token_estimate,
                    set.digest,
                    if set.truncated {
                        ", TRUNCATED by budget"
                    } else {
                        ""
                    },
                ));
                lines.join("\n")
            };
            return Ok(Execution {
                text,
                effectiveness: Some(effectiveness),
                detail: None,
                success: true,
            });
        }

        Command::Outcome { ordinal, outcome } => {
            let judged = match outcome.as_str() {
                "accepted" => RecallOutcome::Accepted,
                "corrected" => RecallOutcome::Corrected,
                "discarded" => RecallOutcome::Discarded,
                "unknown" => RecallOutcome::Unknown,
                other => {
                    return Err(format!(
                    "unknown outcome {other:?}: expected accepted | corrected | discarded | unknown"
                )
                    .into())
                }
            };
            let record = store.set_recall_outcome(*ordinal, judged)?;
            return Ok(if json {
                serde_json::to_string_pretty(&record)?.into()
            } else {
                record.render().into()
            });
        }

        Command::Preflight {
            root,
            harness,
            task,
            budget,
        } => {
            verify_instance_store(store, root)?;
            let flight = store.runtime_task_preflight(
                root,
                harness.as_deref(),
                reader,
                now,
                *budget,
                task.as_deref(),
            )?;
            let detail = (!flight.warnings.is_empty())
                .then(|| format!("{} warning(s)", flight.warnings.len()));
            return Ok(Execution {
                text: flight.context,
                effectiveness: Some(flight.effectiveness),
                detail,
                success: true,
            });
        }

        Command::Hook {
            event,
            harness,
            root,
            budget,
        } => {
            let event = rrd_engine::HookEvent::parse(event)
                .ok_or_else(|| format!("unknown hook event {event:?}"))?;
            let mut raw = String::new();
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut raw)?;
            let input: serde_json::Value = if raw.trim().is_empty() {
                serde_json::Value::Object(Default::default())
            } else {
                serde_json::from_str(&raw)?
            };
            // Root precedence: explicit flag, then the harness's `cwd`, then
            // the working directory the hook process inherited.
            let root = root.clone().unwrap_or_else(|| {
                input
                    .get("cwd")
                    .and_then(serde_json::Value::as_str)
                    .map(Into::into)
                    .unwrap_or_else(|| ".".into())
            });
            verify_instance_store(store, &root)?;
            let response = store.handle_runtime_hook(rrd_engine::RuntimeHookRequest {
                root: &root,
                harness: harness.as_deref(),
                reader,
                now,
                budget: *budget,
                event,
                input: &input,
            })?;
            return Ok(Execution {
                text: response.stdout,
                effectiveness: response.effectiveness,
                detail: response.detail,
                success: true,
            });
        }

        Command::Query {
            root,
            scope,
            ql,
            parameters,
            max_scanned_changes,
            max_rows,
            max_output_bytes,
            max_batch_rows,
        } => {
            verify_instance_store(store, root)?;
            let scope = ScopeId::new(scope.clone())?;
            let parameter_json = query_parameter_object(parameters)?;
            let parameters = rrd_engine::query_parameters_from_json(&parameter_json)?;
            let budget = rrd_engine::ExecutionBudget {
                max_scanned_changes: *max_scanned_changes,
                max_rows: *max_rows,
                max_output_bytes: *max_output_bytes,
                max_batch_rows: *max_batch_rows,
                ..rrd_engine::ExecutionBudget::default()
            };
            let result = store.execute_operator_query(
                scope,
                ql,
                &parameters,
                &budget,
                reader.as_str(),
                now,
            )?;
            let text = if json {
                serde_json::to_string_pretty(&result)?
            } else {
                let mut lines = vec![format!(
                    "plan {} read={} rows={} scanned={} validation={} validation_reads={} proof_nodes={} bytes={} truncated={}",
                    result.plan.digest,
                    result.execution.known_at_cursor,
                    result.execution.returned_rows,
                    result.execution.scanned_changes,
                    result.execution.stamp_validation,
                    result.execution.stamp_validation_max_changes,
                    result.execution.stamp_validation_proof_nodes,
                    result.execution.output_bytes,
                    result.execution.truncated,
                )];
                for row in result
                    .execution
                    .batches
                    .iter()
                    .flat_map(|batch| &batch.rows)
                {
                    lines.push(serde_json::to_string(row)?);
                }
                lines.join("\n")
            };
            return Ok(Execution {
                text,
                effectiveness: None,
                detail: Some(format!("plan={}", result.plan.digest)),
                success: true,
            });
        }

        Command::Ledger { since } => {
            let records: Vec<_> = store
                .invocations_since(*since)?
                .into_iter()
                .filter(|i| i.effectiveness.is_some())
                .collect();
            let mut distribution: std::collections::BTreeMap<String, usize> =
                std::collections::BTreeMap::new();
            for record in &records {
                let outcome = record.effectiveness.as_ref().expect("filtered").outcome;
                *distribution.entry(outcome.to_string()).or_insert(0) += 1;
            }
            return Ok(if json {
                serde_json::to_string_pretty(&serde_json::json!({
                    "records": records,
                    "outcome_distribution": distribution,
                }))?
                .into()
            } else if records.is_empty() {
                "no recall records".to_string().into()
            } else {
                let mut lines: Vec<String> = records.iter().map(|i| i.render()).collect();
                lines.push(format!(
                    "-- outcomes: {}",
                    distribution
                        .iter()
                        .map(|(k, v)| format!("{k}={v}"))
                        .collect::<Vec<_>>()
                        .join(" ")
                ));
                lines.join("\n").into()
            });
        }

        _ => {}
    }

    // Text-only commands, converted to an `Execution` in one place below.
    let text = (|| -> Result<String, Box<dyn std::error::Error>> {
        match command {
            Command::Recall { .. }
            | Command::Outcome { .. }
            | Command::Ledger { .. }
            | Command::Preflight { .. }
            | Command::Hook { .. }
            | Command::Query { .. }
            | Command::Exec { .. }
            | Command::Dev { .. }
            | Command::WorkPlan { .. }
            | Command::Runtime { .. }
            | Command::Storage { .. } => {
                unreachable!("handled above with an early return")
            }

            Command::Init { harness, root } => {
                let registry = rrd_engine::Registry::builtin();
                let adapter = registry.get(harness).ok_or_else(|| {
                    format!(
                        "no harness named {harness:?}; registry knows: {}",
                        registry
                            .all()
                            .iter()
                            .map(|h| h.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                })?;
                let report = store.initialize_runtime(root, adapter, now)?;
                let mut lines: Vec<String> = report
                    .written
                    .iter()
                    .map(|p| format!("wrote {}", p.display()))
                    .collect();
                lines.extend(report.notes.iter().map(|n| format!("note: {n}")));
                Ok(lines.join("\n"))
            }

            Command::Reasoning {
                action:
                    ReasoningAction::Record {
                        run,
                        actor,
                        payload,
                    },
            } => {
                let payload: ReasoningPayload = serde_json::from_str(payload)?;
                let event = store.record_reasoning_event(run, now, actor, payload)?;
                Ok(if json {
                    serde_json::to_string_pretty(&event)?
                } else {
                    format!(
                        "reasoning run {}: recorded {} #{} [{}]",
                        event.run_id,
                        event.payload.name(),
                        event.ordinal,
                        event.digest
                    )
                })
            }

            Command::Reasoning {
                action: ReasoningAction::Show { run },
            } => {
                let run = match run {
                    Some(id) => store.reasoning_run_by_id(id)?,
                    None => store.active_reasoning()?,
                };
                let Some(run) = run else {
                    return Ok("no matching reasoning run".into());
                };
                Ok(if json {
                    serde_json::to_string_pretty(&serde_json::json!({
                        "run_id": run.id(),
                        "state": run.state(),
                        "events": run.events(),
                    }))?
                } else {
                    let mut lines = vec![format!(
                        "reasoning run {}: {:?}; {} event(s)",
                        run.id(),
                        run.state(),
                        run.events().len()
                    )];
                    lines.extend(run.events().iter().map(|event| {
                        format!(
                            "  #{:<3} {:<12} at={} by={} {}",
                            event.ordinal,
                            event.payload.name(),
                            event.at,
                            event.actor,
                            event.digest
                        )
                    }));
                    lines.join("\n")
                })
            }

            Command::Harness {
                action: HarnessAction::Audit { name, evidence },
            } => {
                let registry = rrd_engine::Registry::builtin();
                let claim = store.record_harness_verification(
                    &registry,
                    name,
                    now,
                    evidence,
                    reader.as_str(),
                )?;
                Ok(format!(
                    "recorded: {} verified until {} ({})",
                    name,
                    claim.valid_to.expect("verification claims carry an expiry"),
                    evidence
                ))
            }

            Command::Harness {
                action: HarnessAction::Status,
            } => {
                let registry = rrd_engine::Registry::builtin();
                let mut lines = Vec::new();
                for adapter in registry.all() {
                    let state = if let Some(when) = &adapter.retired {
                        format!("RETIRED ({when})")
                    } else {
                        match store.harness_verification(&registry, adapter, now)? {
                            rrd_engine::Verification::Current { until } => format!(
                                "verified until {}",
                                until
                                    .map(|u| u.to_string())
                                    .unwrap_or_else(|| "open".into())
                            ),
                            rrd_engine::Verification::Expired { days } => {
                                format!("EXPIRED {days} day(s) ago — re-audit")
                            }
                            rrd_engine::Verification::Never => "never audited".to_string(),
                        }
                    };
                    lines.push(format!(
                        "{:<12} hooks={:<5} mcp={:<5} billing={:<24} {}",
                        adapter.name,
                        adapter.hooks,
                        adapter.mcp_client,
                        adapter.billing.join("+"),
                        state,
                    ));
                }
                Ok(lines.join("\n"))
            }
            Command::Rebuild => {
                let outcome = store.rebuild_current()?;
                Ok(if json {
                    serde_json::to_string_pretty(&serde_json::json!({
                        "from": outcome.from,
                        "to": outcome.to,
                        "applied": outcome.applied,
                    }))?
                } else {
                    format!(
                        "applied {} claim(s), watermark {} -> {}",
                        outcome.applied, outcome.from, outcome.to
                    )
                })
            }

            Command::Ground => {
                // §8.3 reaches `as_of = now` by rebuilding first: grounding itself
                // verifies incremental-equals-batch at the projection's watermark,
                // and the rebuild carries that watermark to the current sequence.
                store.rebuild_current()?;
                let report = store.ground_current(now)?;
                Ok(match (&report, json) {
                    (GroundingReport::Grounded(stamp), true) => {
                        serde_json::to_string_pretty(&serde_json::json!({
                            "grounded": { "at": stamp.at, "sequence": stamp.sequence, "digest": stamp.digest },
                        }))?
                    }
                    (GroundingReport::Grounded(stamp), false) => format!(
                        "grounded at={} sequence={} digest={:016x}",
                        stamp.at, stamp.sequence, stamp.digest
                    ),
                    (GroundingReport::Divergence { differences }, true) => {
                        serde_json::to_string_pretty(&serde_json::json!({
                            "divergence": differences,
                            "quarantined": true,
                        }))?
                    }
                    (GroundingReport::Divergence { differences }, false) => {
                        let mut lines = vec![format!(
                            "DIVERGENCE: {} difference(s); projection quarantined",
                            differences.len()
                        )];
                        lines.extend(differences.iter().map(|d| format!("  {d}")));
                        lines.push("recover with `rrflow reset-projection`".into());
                        lines.join("\n")
                    }
                })
            }

            Command::ResetProjection => {
                let outcome = store.reset_current()?;
                Ok(if json {
                    serde_json::to_string_pretty(&serde_json::json!({
                        "recomputed": outcome.applied,
                        "watermark": outcome.to,
                    }))?
                } else {
                    format!(
                        "projection recomputed from the log: {} claim(s), watermark {}",
                        outcome.applied, outcome.to
                    )
                })
            }
            Command::ResetRouting { root } => {
                verify_instance_store(store, root)?;
                let ready = store.reset_project_routing(root)?;
                Ok(if json {
                    serde_json::to_string_pretty(&serde_json::json!({
                        "generation": ready.generation,
                        "files": ready.files,
                        "symbols": ready.symbols,
                        "topology_sha256": ready.topology_sha256,
                        "profile_sha256": ready.profile_sha256,
                        "refresh": {
                            "added": ready.refresh.added,
                            "changed": ready.refresh.changed,
                            "removed": ready.refresh.removed,
                            "skipped_unread": ready.refresh.skipped_unread,
                            "read_but_identical": ready.refresh.read_but_identical,
                            "duration_ms": ready.refresh.duration_ms,
                        },
                    }))?
                } else {
                    format!("routing projection rebuilt: {}", ready.render())
                })
            }
            Command::Assert {
                subject,
                predicate,
                object,
                valid_from,
                actor,
                on_behalf_of,
            } => {
                let subject = Subject::new(subject.clone())?;
                let predicate = Predicate::new(predicate.clone())?;
                let claim = Claim::new(
                    subject,
                    predicate,
                    object.clone(),
                    valid_from.unwrap_or(now),
                    now,
                    Producer {
                        actor: actor.clone(),
                        on_behalf_of: on_behalf_of.clone(),
                        session: None,
                    },
                );
                let outcome = store.assert_claim(&claim)?;
                Ok(if json {
                    serde_json::to_string_pretty(&serde_json::json!({
                        "sequence": outcome.last_sequence,
                        "claim": claim,
                    }))?
                } else {
                    format!("recorded at sequence {}", outcome.last_sequence)
                })
            }

            Command::AsOf {
                subject,
                predicate,
                at,
            } => {
                let subject = Subject::new(subject.clone())?;
                let predicate = Predicate::new(predicate.clone())?;
                let at = at.unwrap_or(now);
                let resolved = store.claim_as_of(&subject, &predicate, at)?;
                // A read is recorded, per SPEC.md §7.
                store.observe(reader, &subject, &predicate, now)?;
                Ok(match (&resolved, json) {
                    (_, true) => serde_json::to_string_pretty(&resolved)?,
                    (Some(claim), false) => format!(
                        "{} valid_from={} valid_to={} tx_time={} producer={}",
                        claim.object,
                        claim.valid_from,
                        claim
                            .valid_to
                            .map(|v| v.to_string())
                            .unwrap_or_else(|| "open".into()),
                        claim.tx_time,
                        claim.producer.actor,
                    ),
                    (None, false) => format!("no claim in force at {at}"),
                })
            }

            Command::History { subject, predicate } => {
                let subject = Subject::new(subject.clone())?;
                let predicate = Predicate::new(predicate.clone())?;
                let versions = store.claim_history(&subject, &predicate)?;
                store.observe(reader, &subject, &predicate, now)?;
                Ok(if json {
                    serde_json::to_string_pretty(&versions)?
                } else if versions.is_empty() {
                    "no versions recorded".to_string()
                } else {
                    versions
                        .iter()
                        .map(|c| {
                            format!(
                                "{:<20} valid=[{}, {}) tx={}",
                                c.object,
                                c.valid_from,
                                c.valid_to
                                    .map(|v| v.to_string())
                                    .unwrap_or_else(|| "open".into()),
                                c.tx_time
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                })
            }

            Command::Status => {
                let sequence = store.sequence()?;
                let invocations = store.invocation_count()?;
                let access = store.access_count()?;
                Ok(if json {
                    serde_json::to_string_pretty(&serde_json::json!({
                        "storage_backend": store.backend_name(),
                        "claim_sequence": sequence,
                        "invocations": invocations,
                        "access_records_approximate": access,
                    }))?
                } else {
                    format!(
                        "storage backend     {}\n\
                     claim sequence      {sequence}\n\
                     invocations         {invocations}\n\
                     access records      {access} (approximate)",
                        store.backend_name()
                    )
                })
            }

            Command::Gc { since } => {
                let report = store.removal_report(*since, now)?;
                Ok(if json {
                    serde_json::to_string_pretty(&serde_json::json!({
                        "since": report.since,
                        "evaluated_at": report.evaluated_at,
                        "candidates": report.candidates()
                            .map(|p| serde_json::json!({
                                "subject": p.subject.as_str(),
                                "predicate": p.predicate.as_str(),
                                "claim_count": p.claim_count,
                                "reason": p.reason(),
                            }))
                            .collect::<Vec<_>>(),
                        "retained": report.retained()
                            .map(|p| serde_json::json!({
                                "subject": p.subject.as_str(),
                                "predicate": p.predicate.as_str(),
                                "last_access": p.last_access,
                                "reason": p.reason(),
                            }))
                            .collect::<Vec<_>>(),
                    }))?
                } else {
                    report.render()
                })
            }

            Command::Invocations { since } => {
                let records = store.invocations_since(*since)?;
                Ok(if json {
                    serde_json::to_string_pretty(&records)?
                } else if records.is_empty() {
                    "no invocations recorded".to_string()
                } else {
                    records
                        .iter()
                        .map(|i| i.render())
                        .collect::<Vec<_>>()
                        .join("\n")
                })
            }
        }
    })()?;
    Ok(text.into())
}

fn verify_instance_store(
    store: &RrdEngine,
    root: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    store.verify_project_store(root)
}

fn query_parameter_object(
    parameters: &[String],
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let mut object = serde_json::Map::new();
    for parameter in parameters {
        let (name, encoded) = parameter
            .split_once('=')
            .ok_or("query parameter must use NAME=JSON")?;
        if name.trim().is_empty() {
            return Err("query parameter name must not be empty".into());
        }
        if object.contains_key(name) {
            return Err(format!("query parameter {name:?} was supplied more than once").into());
        }
        object.insert(name.to_owned(), serde_json::from_str(encoded)?);
    }
    Ok(serde_json::Value::Object(object))
}

/// Maps an execution result onto the recorded outcome.
pub fn outcome_of(
    result: &Result<Execution, Box<dyn std::error::Error>>,
) -> (Outcome, Option<String>) {
    match result {
        Ok(execution) if execution.success => (Outcome::Ok, None),
        Ok(execution) => (
            Outcome::Error,
            execution
                .detail
                .clone()
                .or_else(|| Some("command reported an unsuccessful outcome".into())),
        ),
        Err(error) => (Outcome::Error, Some(error.to_string())),
    }
}
