//! Deny-by-default tool policy derived from the reasoning-run contract.

use rrd_core::{ReasoningRun, ReasoningState};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{runtime_tool_catalogue, RuntimeToolAttunement};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractDifferential {
    pub run_id: Option<String>,
    pub state: Option<ReasoningState>,
    pub actual: String,
    pub expected: Vec<String>,
    pub differences: Vec<String>,
}

impl ContractDifferential {
    pub fn render(&self) -> String {
        format!(
            "contract differential: actual={}; expected={}; {}",
            self.actual,
            self.expected.join(" | "),
            self.differences.join("; ")
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolPolicy {
    ReadOnly,
    ControlPlane,
    Allow { differential: ContractDifferential },
    Deny { differential: ContractDifferential },
}

/// Evaluates a harness tool call without side effects.
pub fn evaluate_tool(run: Option<&ReasoningRun>, input: &Value) -> ToolPolicy {
    let tool = input
        .get("tool_name")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    if !is_project_mutation_tool(tool) {
        return ToolPolicy::ReadOnly;
    }
    if tool == "Bash"
        && input
            .pointer("/tool_input/command")
            .and_then(Value::as_str)
            .is_some_and(is_control_command)
    {
        return ToolPolicy::ControlPlane;
    }

    let actual = if tool == "Bash" {
        format!(
            "Bash({})",
            input
                .pointer("/tool_input/command")
                .and_then(Value::as_str)
                .unwrap_or("unknown command")
                .lines()
                .next()
                .unwrap_or_default()
        )
    } else {
        tool.to_owned()
    };
    let mut differential = ContractDifferential {
        run_id: run.map(|run| run.id().to_owned()),
        state: run.map(ReasoningRun::state),
        actual,
        expected: Vec::new(),
        differences: Vec::new(),
    };

    match run.map(ReasoningRun::state) {
        None => {
            differential
                .expected
                .push("an active reasoning run with goal and plan".into());
            differential
                .differences
                .push("no active run; record goal → plan → attempt before project mutation".into());
            ToolPolicy::Deny { differential }
        }
        Some(ReasoningState::NeedsObservation) => {
            differential.expected.push(
                "tool execution for the recorded attempt, followed by an observation with evidence"
                    .into(),
            );
            ToolPolicy::Allow { differential }
        }
        Some(ReasoningState::NeedsVerification) if tool == "Bash" => {
            differential
                .expected
                .push("a verification command followed by typed verification checks".into());
            ToolPolicy::Allow { differential }
        }
        Some(state) => {
            differential.expected.push(
                match state {
                    ReasoningState::NeedsPlan => "record the hypothesis and plan",
                    ReasoningState::NeedsAttempt => "record an attempt before invoking a tool",
                    ReasoningState::NeedsObservation => "record an observation after the attempt",
                    ReasoningState::NeedsDecision => "record a continue/verify/stop decision",
                    ReasoningState::NeedsVerification => "run verification through Bash",
                    ReasoningState::NeedsPostVerificationDecision => {
                        "decide whether failed verification requires another attempt or stopping"
                    }
                    ReasoningState::NeedsOutcome => "record the outcome",
                    ReasoningState::Complete => "start a new reasoning run",
                    ReasoningState::Empty => "record a goal",
                }
                .into(),
            );
            differential.differences.push(format!(
                "project mutation is not valid while the run is {state:?}"
            ));
            ToolPolicy::Deny { differential }
        }
    }
}

pub(super) fn is_project_mutation_tool(tool: &str) -> bool {
    matches!(
        tool,
        "Edit" | "Write" | "NotebookEdit" | "Bash" | "RRFlowExec"
    ) || runtime_tool_catalogue().iter().any(|definition| {
        definition.name == tool && definition.attunement == RuntimeToolAttunement::ExactTool
    })
}

pub(super) fn tool_request_digest(input: &Value) -> Result<String, serde_json::Error> {
    serde_json::to_vec(&serde_json::json!({
        "tool_name": input.get("tool_name").unwrap_or(&Value::Null),
        "tool_input": input.get("tool_input").unwrap_or(&Value::Null),
    }))
    .map(|bytes| rrd_core::digest::sha256_hex(&bytes))
}

fn is_control_command(command: &str) -> bool {
    // The bypass is for one direct control-plane process, never a shell
    // program that happens to begin with one. Otherwise
    // `rrflow reasoning ... && <mutation>` would smuggle an arbitrary mutation
    // through the recovery path.
    if command.chars().any(|character| {
        matches!(
            character,
            '\n' | '\r' | ';' | '|' | '&' | '>' | '<' | '`' | '$' | '(' | ')'
        )
    }) {
        return false;
    }
    let words: Vec<&str> = command.split_whitespace().take(3).collect();
    let executable = words.first().and_then(|word| word.rsplit('/').next());
    executable == Some("rrflow")
        && matches!(
            words.get(1).copied(),
            Some("reasoning" | "work-plan" | "reset-projection" | "reset-routing" | "ground")
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rrd_core::ReasoningPayload;

    fn planned() -> ReasoningRun {
        let mut run = ReasoningRun::empty("run").unwrap();
        run.append(
            1,
            "agent",
            ReasoningPayload::Goal {
                statement: "change".into(),
                acceptance: vec!["green".into()],
            },
        )
        .unwrap();
        run.append(
            2,
            "agent",
            ReasoningPayload::Plan {
                hypothesis: "x".into(),
                steps: vec!["edit".into()],
            },
        )
        .unwrap();
        run
    }

    #[test]
    fn mutation_is_denied_without_a_declared_attempt() {
        let input = serde_json::json!({"tool_name":"Edit"});
        assert!(matches!(
            evaluate_tool(None, &input),
            ToolPolicy::Deny { .. }
        ));
        assert!(matches!(
            evaluate_tool(Some(&planned()), &input),
            ToolPolicy::Deny { .. }
        ));
    }

    #[test]
    fn a_declared_attempt_allows_execution_and_control_commands_cannot_deadlock() {
        let mut run = planned();
        run.append(
            3,
            "agent",
            ReasoningPayload::Attempt {
                summary: "edit".into(),
                actions: vec![],
            },
        )
        .unwrap();
        assert!(matches!(
            evaluate_tool(Some(&run), &serde_json::json!({"tool_name":"Edit"})),
            ToolPolicy::Allow { .. }
        ));
        assert!(matches!(
            evaluate_tool(
                Some(&run),
                &serde_json::json!({
                    "tool_name":"RRFlowExec",
                    "tool_input":{"exact_argv":["printf", "%s", "a; b"]}
                })
            ),
            ToolPolicy::Allow { .. }
        ));
        assert_eq!(
            evaluate_tool(
                None,
                &serde_json::json!({"tool_name":"Bash","tool_input":{"command":"rrflow reasoning record --run r"}})
            ),
            ToolPolicy::ControlPlane
        );
        assert!(matches!(
            evaluate_tool(
                None,
                &serde_json::json!({
                    "tool_name":"Bash",
                    "tool_input":{"command":"rrflow reasoning show && rm -f important"}
                })
            ),
            ToolPolicy::Deny { .. }
        ));
    }
}
