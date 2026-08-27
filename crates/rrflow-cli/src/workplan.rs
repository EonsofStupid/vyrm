use crate::command::Execution;
use clap::Subcommand;
use rrd_engine::operator::{digest, EmbeddedOperator, Reader};

const MAX_VERIFICATION_OUTPUT_BYTES: u64 = 16 * 1024 * 1024;
const MAX_VERIFICATION_RUNTIME: std::time::Duration = std::time::Duration::from_secs(15 * 60);

#[derive(Subcommand, Debug, Clone)]
pub enum WorkPlanAction {
    /// Validate the checked-in plan and install its immutable revision in RRD.
    Sync {
        #[arg(long, default_value = ".")]
        root: std::path::PathBuf,
    },
    /// Read the authoritative persisted status.
    Status {
        #[arg(long, default_value = "rrflow-foundation")]
        plan: String,
    },
    /// Activate one dependency-ready item. Only one item may be active.
    Activate {
        #[arg(long, default_value = "rrflow-foundation")]
        plan: String,
        #[arg(long)]
        item: String,
    },
    /// Bind the active item to fresh attunement and an exact verification plan.
    Record {
        #[arg(long, default_value = ".")]
        root: std::path::PathBuf,
        #[arg(long, default_value = "rrflow-foundation")]
        plan: String,
        #[arg(long)]
        item: String,
        /// File containing the reviewed implementation plan.
        #[arg(long)]
        plan_file: std::path::PathBuf,
        /// Exact argv encoded as a JSON string array. Repeat for each check.
        #[arg(long = "verify-argv", required = true)]
        verification_argv: Vec<String>,
    },
    /// Execute the recorded argv checks and verify only on fresh passing evidence.
    Verify {
        #[arg(long, default_value = ".")]
        root: std::path::PathBuf,
        #[arg(long, default_value = "rrflow-foundation")]
        plan: String,
    },
}

impl WorkPlanAction {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Sync { .. } => "workplan-sync",
            Self::Status { .. } => "workplan-status",
            Self::Activate { .. } => "workplan-activate",
            Self::Record { .. } => "workplan-record",
            Self::Verify { .. } => "workplan-verify",
        }
    }

    pub fn arguments(&self) -> Vec<String> {
        match self {
            Self::Sync { root } => vec![format!("root={}", root.display())],
            Self::Status { plan } => vec![format!("plan={plan}")],
            Self::Activate { plan, item } => {
                vec![format!("plan={plan}"), format!("item={item}")]
            }
            Self::Record {
                root,
                plan,
                item,
                plan_file,
                verification_argv,
            } => vec![
                format!("root={}", root.display()),
                format!("plan={plan}"),
                format!("item={item}"),
                format!("plan_file={}", plan_file.display()),
                format!(
                    "verification_argv_sha256={}",
                    digest::sha256_hex(verification_argv.join("\0").as_bytes())
                ),
            ],
            Self::Verify { root, plan } => {
                vec![format!("root={}", root.display()), format!("plan={plan}")]
            }
        }
    }
}

pub fn execute(
    store: &EmbeddedOperator,
    action: &WorkPlanAction,
    reader: &Reader,
    now: u64,
    json: bool,
) -> Result<Execution, Box<dyn std::error::Error>> {
    let snapshot = match action {
        WorkPlanAction::Sync { root } => {
            verify_project_store(store, root)?;
            let definition = rrd_engine::read_work_plan(root)?;
            rrd_engine::install_work_plan(
                store.runtime_store(),
                definition,
                now,
                reader.as_str(),
                "cli-workplan-sync",
            )?
        }
        WorkPlanAction::Status { plan } => rrd_engine::load_work_plan(store.runtime_store(), plan)?
            .ok_or_else(|| format!("work plan {plan} is not installed"))?,
        WorkPlanAction::Activate { plan, item } => rrd_engine::activate_work_item(
            store.runtime_store(),
            plan,
            item,
            now,
            reader.as_str(),
            "cli-workplan-activate",
        )?,
        WorkPlanAction::Record {
            root,
            plan,
            item,
            plan_file,
            verification_argv,
        } => {
            verify_project_store(store, root)?;
            let ready = rrd_engine::ensure_routing_fresh(store.runtime_store(), root)?;
            let receipt =
                rrd_engine::require_fresh_attunement(store.runtime_store(), root, &ready)?;
            let commands = verification_argv
                .iter()
                .map(|encoded| {
                    let argv: Vec<String> = serde_json::from_str(encoded)?;
                    if argv.is_empty() {
                        return Err("verification argv must not be empty".into());
                    }
                    Ok(argv)
                })
                .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
            let payload = std::fs::read(plan_file).map_err(|error| {
                format!("cannot read reviewed plan {}: {error}", plan_file.display())
            })?;
            rrd_engine::record_work_item_plan(
                store.runtime_store(),
                plan,
                rrd_engine::WorkItemPlanRecord {
                    work_item_id: item.clone(),
                    source_tree_sha256: receipt.source_tree_sha256,
                    attunement_receipt_sha256: receipt.receipt_sha256,
                    plan_payload_sha256: digest::sha256_hex(&payload),
                    verification_commands: commands,
                },
                now,
                reader.as_str(),
                "cli-workplan-record",
            )?
        }
        WorkPlanAction::Verify { root, plan } => {
            verify_project_store(store, root)?;
            let record = rrd_engine::load_active_work_item_plan(store.runtime_store(), plan)?
                .ok_or("work plan has no active recorded implementation plan")?;
            let mut checks = Vec::with_capacity(record.verification_commands.len());
            for argv in &record.verification_commands {
                let output = run_verification(root, argv)?;
                let evidence_sha256 =
                    digest::sha256_hex(&serde_json::to_vec(&serde_json::json!({
                        "argv": argv,
                        "exit_code": output.exit_code,
                        "success": output.success,
                        "stdout_sha256": output.stdout_sha256,
                        "stderr_sha256": output.stderr_sha256,
                    }))?);
                checks.push(rrd_engine::WorkItemVerificationCheck {
                    name: argv.join(" "),
                    argv: argv.clone(),
                    passed: output.success,
                    evidence_sha256,
                });
                if !output.success {
                    return Err(format!(
                        "verification failed for {:?}: stderr sha256 {}",
                        argv, output.stderr_sha256
                    )
                    .into());
                }
            }
            let ready = rrd_engine::ensure_routing_fresh(store.runtime_store(), root)?;
            let receipt =
                rrd_engine::require_fresh_attunement(store.runtime_store(), root, &ready)?;
            if receipt.source_tree_sha256 != record.source_tree_sha256 {
                return Err(
                    "verification denied: project tree changed after the plan was recorded".into(),
                );
            }
            let repository_revision = repository_revision(root, &record.source_tree_sha256)?;
            rrd_engine::verify_work_item(
                store.runtime_store(),
                plan,
                rrd_engine::WorkItemVerification {
                    work_item_id: record.work_item_id,
                    source_tree_sha256: record.source_tree_sha256,
                    plan_payload_sha256: record.plan_payload_sha256,
                    repository_revision,
                    platform: format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
                    checks,
                },
                now,
                reader.as_str(),
                "cli-workplan-verify",
            )?
        }
    };
    let text = if json {
        serde_json::to_string_pretty(&snapshot)?
    } else {
        render(&snapshot)
    };
    Ok(Execution {
        text,
        effectiveness: None,
        detail: Some(format!(
            "work plan {} revision {} event {}",
            snapshot.plan_id, snapshot.revision, snapshot.event_sequence
        )),
        success: true,
    })
}

struct VerificationProcessResult {
    success: bool,
    exit_code: Option<i32>,
    stdout_sha256: String,
    stderr_sha256: String,
}

fn run_verification(
    root: &std::path::Path,
    argv: &[String],
) -> Result<VerificationProcessResult, Box<dyn std::error::Error>> {
    if argv.is_empty() {
        return Err("verification argv must not be empty".into());
    }
    let directory = root.join(".rrflow/verification");
    std::fs::create_dir_all(&directory)?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let identity = digest::sha256_hex(&serde_json::to_vec(&(argv, nonce, std::process::id()))?);
    let stdout_path = directory.join(format!("{}-stdout.log", &identity[..24]));
    let stderr_path = directory.join(format!("{}-stderr.log", &identity[..24]));
    let stdout = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&stdout_path)?;
    let stderr = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&stderr_path)?;
    let mut child = std::process::Command::new(&argv[0])
        .args(&argv[1..])
        .current_dir(root)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::from(stdout))
        .stderr(std::process::Stdio::from(stderr))
        .spawn()
        .map_err(|error| format!("cannot execute verification {:?}: {error}", argv))?;
    let started = std::time::Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        let output_bytes =
            std::fs::metadata(&stdout_path)?.len() + std::fs::metadata(&stderr_path)?.len();
        if output_bytes > MAX_VERIFICATION_OUTPUT_BYTES {
            child.kill()?;
            child.wait()?;
            cleanup_verification_files(&stdout_path, &stderr_path);
            return Err(format!(
                "verification {:?} exceeded {} output bytes",
                argv, MAX_VERIFICATION_OUTPUT_BYTES
            )
            .into());
        }
        if started.elapsed() > MAX_VERIFICATION_RUNTIME {
            child.kill()?;
            child.wait()?;
            cleanup_verification_files(&stdout_path, &stderr_path);
            return Err(format!(
                "verification {:?} exceeded {} seconds",
                argv,
                MAX_VERIFICATION_RUNTIME.as_secs()
            )
            .into());
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    };
    let stdout = std::fs::read(&stdout_path)?;
    let stderr = std::fs::read(&stderr_path)?;
    cleanup_verification_files(&stdout_path, &stderr_path);
    Ok(VerificationProcessResult {
        success: status.success(),
        exit_code: status.code(),
        stdout_sha256: digest::sha256_hex(&stdout),
        stderr_sha256: digest::sha256_hex(&stderr),
    })
}

fn cleanup_verification_files(stdout: &std::path::Path, stderr: &std::path::Path) {
    let _ = std::fs::remove_file(stdout);
    let _ = std::fs::remove_file(stderr);
}

fn verify_project_store(
    store: &EmbeddedOperator,
    root: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let binding = rrd_engine::InstanceBinding::discover(root)?;
    binding.require_runtime_ready()?;
    binding.verify_store_path(store.path())?;
    Ok(())
}

fn repository_revision(
    root: &std::path::Path,
    source_tree_sha256: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(root)
        .output()?;
    if !output.status.success() {
        return Ok(format!("tree:{source_tree_sha256}"));
    }
    let commit = std::str::from_utf8(&output.stdout)?.trim();
    if commit.is_empty() {
        return Ok(format!("tree:{source_tree_sha256}"));
    }
    Ok(format!("{commit}:{source_tree_sha256}"))
}

fn render(snapshot: &rrd_engine::WorkPlanSnapshot) -> String {
    let verified = snapshot
        .items
        .iter()
        .filter(|item| item.status == rrd_engine::WorkItemStatus::Verified)
        .count();
    let mut lines = vec![format!(
        "work plan {} revision={} verified={}/{} active={} events={} digest={}",
        snapshot.plan_id,
        snapshot.revision,
        verified,
        snapshot.items.len(),
        snapshot.active_item_id.as_deref().unwrap_or("none"),
        snapshot.event_sequence,
        snapshot.plan_sha256,
    )];
    lines.extend(snapshot.items.iter().map(|item| {
        format!(
            "[{}] {}{}",
            match item.status {
                rrd_engine::WorkItemStatus::Pending => " ",
                rrd_engine::WorkItemStatus::Active => ">",
                rrd_engine::WorkItemStatus::Verifying => "?",
                rrd_engine::WorkItemStatus::Verified => "x",
            },
            item.item_id,
            item.verification_sha256
                .as_ref()
                .map(|digest| format!(" {digest}"))
                .unwrap_or_default(),
        )
    }));
    lines.join("\n")
}
