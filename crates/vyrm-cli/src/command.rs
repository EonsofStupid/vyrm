//! Command definitions and execution.
//!
//! Execution is separated from `main` so that every command is exercised by
//! integration tests through the same path the operator uses, rather than
//! through a parallel test-only entry point.

use clap::{Parser, Subcommand};
use vyrm_core::{Claim, ClaimReader, Millis, Predicate, Producer, Reader, RecallQuery, Subject};
use vyrm_store::{Effectiveness, GroundingReport, Outcome, RecallOutcome, Store, Trigger};

/// What a command produced: the operator-facing text, and — for a recall — the
/// `SPEC.md` §13.1 effectiveness fields the invocation record must carry.
pub struct Execution {
    pub text: String,
    pub effectiveness: Option<Effectiveness>,
}

impl From<String> for Execution {
    fn from(text: String) -> Self {
        Execution { text, effectiveness: None }
    }
}

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
            Command::Recall { .. } => "recall",
            Command::Outcome { .. } => "outcome",
            Command::Ledger { .. } => "ledger",
            Command::Rebuild => "rebuild",
            Command::Ground => "ground",
            Command::ResetProjection => "reset-projection",
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
            Command::Recall { subjects, predicates, at, budget, provider } => {
                let mut a: Vec<String> =
                    subjects.iter().map(|s| format!("subject={s}")).collect();
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
        }
    }
}

/// Executes one command.
///
/// `now` is supplied rather than read here, so that tests are deterministic and
/// the clock enters at exactly one place (`main`).
pub fn execute(store: &Store, command: &Command, reader: &Reader, now: Millis, json: bool)
    -> Result<Execution, Box<dyn std::error::Error>>
{
    match command {
        Command::Recall { subjects, predicates, at, budget, provider } => {
            let query = RecallQuery {
                subjects: subjects
                    .iter()
                    .map(|s| Subject::new(s.clone()))
                    .collect::<vyrm_core::Result<Vec<_>>>()?,
                predicates: if predicates.is_empty() {
                    None
                } else {
                    Some(
                        predicates
                            .iter()
                            .map(|p| Predicate::new(p.clone()))
                            .collect::<vyrm_core::Result<Vec<_>>>()?,
                    )
                },
                as_of: at.unwrap_or(now),
            };
            let set = vyrm_core::recall(store, &query, *budget)?;
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
                    if set.truncated { ", TRUNCATED by budget" } else { "" },
                ));
                lines.join("\n")
            };
            return Ok(Execution { text, effectiveness: Some(effectiveness) });
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
        Command::Recall { .. } | Command::Outcome { .. } | Command::Ledger { .. } => {
            unreachable!("handled above with an early return")
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
                    lines.push("recover with `vyrm reset-projection`".into());
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
    })()?;
    Ok(text.into())
}

/// Maps an execution result onto the recorded outcome.
pub fn outcome_of(
    result: &Result<Execution, Box<dyn std::error::Error>>,
) -> (Outcome, Option<String>) {
    match result {
        Ok(_) => (Outcome::Ok, None),
        Err(error) => (Outcome::Error, Some(error.to_string())),
    }
}

/// Trigger for a command invoked from the operator surface. Always `Manual` at
/// stage 1 (`SPEC.md` §13).
pub const CLI_TRIGGER: Trigger = Trigger::Manual;
