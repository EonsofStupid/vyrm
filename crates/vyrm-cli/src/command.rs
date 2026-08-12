//! Command definitions and execution.
//!
//! Execution is separated from `main` so that every command is exercised by
//! integration tests through the same path the operator uses, rather than
//! through a parallel test-only entry point.

use clap::{Parser, Subcommand};
use vyrm_core::{Claim, ClaimReader, Millis, Predicate, Producer, Reader, Subject};
use vyrm_store::{Outcome, Store, Trigger};

#[derive(Parser, Debug)]
#[command(
    name = "vyrm",
    about = "vyrm operator surface",
    long_about = "Every invocation is recorded (SPEC.md §13). No trigger may be \
                  automated before its recorded invocations justify it."
)]
pub struct Cli {
    /// Database directory.
    #[arg(long, short = 'd', env = "VYRM_DB")]
    pub db: std::path::PathBuf,

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
}

impl Command {
    /// Stable name used in the invocation record.
    pub fn name(&self) -> &'static str {
        match self {
            Command::Assert { .. } => "assert",
            Command::AsOf { .. } => "as-of",
            Command::History { .. } => "history",
            Command::Status => "status",
            Command::Gc { .. } => "gc",
            Command::Invocations { .. } => "invocations",
        }
    }

    /// Arguments recorded alongside the invocation, so a recorded run can be
    /// reproduced.
    pub fn arguments(&self) -> Vec<String> {
        match self {
            Command::Assert { subject, predicate, object, valid_from, actor, on_behalf_of } => {
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
            Command::AsOf { subject, predicate, at } => {
                let mut a = vec![format!("subject={subject}"), format!("predicate={predicate}")];
                if let Some(t) = at {
                    a.push(format!("at={t}"));
                }
                a
            }
            Command::History { subject, predicate } => {
                vec![format!("subject={subject}"), format!("predicate={predicate}")]
            }
            Command::Status => Vec::new(),
            Command::Gc { since } => vec![format!("since={since}")],
            Command::Invocations { since } => vec![format!("since={since}")],
        }
    }
}

/// Executes one command.
///
/// `now` is supplied rather than read here, so that tests are deterministic and
/// the clock enters at exactly one place (`main`).
pub fn execute(store: &Store, command: &Command, reader: &Reader, now: Millis, json: bool)
    -> Result<String, Box<dyn std::error::Error>>
{
    match command {
        Command::Assert { subject, predicate, object, valid_from, actor, on_behalf_of } => {
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
            let outcome = store.assert(&claim)?;
            Ok(if json {
                serde_json::to_string_pretty(&serde_json::json!({
                    "sequence": outcome.last_sequence,
                    "claim": claim,
                }))?
            } else {
                format!("recorded at sequence {}", outcome.last_sequence)
            })
        }

        Command::AsOf { subject, predicate, at } => {
            let subject = Subject::new(subject.clone())?;
            let predicate = Predicate::new(predicate.clone())?;
            let at = at.unwrap_or(now);
            let resolved = store.as_of(&subject, &predicate, at)?;
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
            let versions = store.history(&subject, &predicate)?;
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
            let access = store.access_count();
            Ok(if json {
                serde_json::to_string_pretty(&serde_json::json!({
                    "claim_sequence": sequence,
                    "invocations": invocations,
                    "access_records_approximate": access,
                }))?
            } else {
                format!(
                    "claim sequence      {sequence}\n\
                     invocations         {invocations}\n\
                     access records      {access} (approximate)"
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
}

/// Maps an execution result onto the recorded outcome.
pub fn outcome_of(result: &Result<String, Box<dyn std::error::Error>>) -> (Outcome, Option<String>) {
    match result {
        Ok(_) => (Outcome::Ok, None),
        Err(error) => (Outcome::Error, Some(error.to_string())),
    }
}

/// Trigger for a command invoked from the operator surface. Always `Manual` at
/// stage 1 (`SPEC.md` §13).
pub const CLI_TRIGGER: Trigger = Trigger::Manual;
