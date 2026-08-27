use crate::command::Execution;
use clap::Subcommand;
use rrd_engine::{digest, Reader, RrdEngine, WorkPlanOperation};

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
        /// Qualify implementation already present on the recorded tree.
        /// Mutation is denied and the tree must remain unchanged.
        #[arg(long)]
        qualification_only: bool,
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
    pub fn operation(&self) -> WorkPlanOperation {
        match self {
            Self::Sync { .. } => WorkPlanOperation::Sync,
            Self::Status { .. } => WorkPlanOperation::Status,
            Self::Activate { .. } => WorkPlanOperation::Activate,
            Self::Record { .. } => WorkPlanOperation::Record,
            Self::Verify { .. } => WorkPlanOperation::Verify,
        }
    }

    pub fn name(&self) -> &'static str {
        self.operation().capability_id()
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
                qualification_only,
                verification_argv,
            } => vec![
                format!("root={}", root.display()),
                format!("plan={plan}"),
                format!("item={item}"),
                format!("plan_file={}", plan_file.display()),
                format!("qualification_only={qualification_only}"),
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
    store: &RrdEngine,
    action: &WorkPlanAction,
    reader: &Reader,
    now: u64,
    json: bool,
) -> Result<Execution, Box<dyn std::error::Error>> {
    let snapshot = match action {
        WorkPlanAction::Sync { root } => {
            verify_project_store(store, root)?;
            let definition = rrd_engine::read_work_plan(root)?;
            store.install_operator_work_plan(
                definition,
                now,
                reader.as_str(),
                "cli-workplan-sync",
            )?
        }
        WorkPlanAction::Status { plan } => store
            .operator_work_plan(plan)?
            .ok_or_else(|| format!("work plan {plan} is not installed"))?,
        WorkPlanAction::Activate { plan, item } => store.activate_operator_work_item(
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
            qualification_only,
            verification_argv,
        } => {
            verify_project_store(store, root)?;
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
            store.record_operator_work_item_plan(
                root,
                plan,
                item,
                if *qualification_only {
                    rrd_engine::WorkItemExecutionMode::Qualification
                } else {
                    rrd_engine::WorkItemExecutionMode::Change
                },
                &payload,
                commands,
                now,
                reader.as_str(),
                "cli-workplan-record",
            )?
        }
        WorkPlanAction::Verify { root, plan } => {
            verify_project_store(store, root)?;
            store.verify_operator_work_item(
                root,
                plan,
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

fn verify_project_store(
    store: &RrdEngine,
    root: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    store.verify_project_store(root)
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

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_parser_has_exact_engine_catalogue_operations() {
        let command = crate::command::Cli::command();
        let work_plan = command
            .get_subcommands()
            .find(|command| command.get_name() == "work-plan")
            .unwrap();
        let actual = work_plan
            .get_subcommands()
            .map(|command| command.get_name())
            .collect::<std::collections::BTreeSet<_>>();
        let expected = WorkPlanOperation::ALL
            .into_iter()
            .map(|operation| operation.cli_command().rsplit_once(' ').unwrap().1)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(actual, expected);
    }
}
