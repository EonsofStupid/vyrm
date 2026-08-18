//! Controlled paired evaluation runner. Provider output is evidence, not prose.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Trial {
    provider: String,
    repository: String,
    scenario: String,
    arm: String,
    success: bool,
    expected: String,
    attempts: u64,
    #[serde(default)]
    retries: u64,
    tool_calls: u64,
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    latency_ms: u64,
    exit_code: Option<i32>,
    raw_output: String,
    stderr: String,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("vyrm-eval: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("run") => run_trial(&args[2..]),
        Some("summarize") => summarize(Path::new(args.get(2).ok_or("result directory required")?)),
        Some("verify") => verify(Path::new(args.get(2).ok_or("summary file required")?)),
        _ => Err("usage: vyrm-eval run <provider> <repo> <scenario> <arm> <expected> <prompt-file> <output-file> | summarize <result-dir> | verify <summary-file>".into()),
    }
}

fn run_trial(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if args.len() != 7 { return Err("run expects seven arguments".into()); }
    let provider = &args[0];
    let repo = PathBuf::from(&args[1]);
    let scenario = &args[2];
    let arm = &args[3];
    let expected = &args[4];
    let prompt = std::fs::read_to_string(&args[5])?;
    let output_file = PathBuf::from(&args[6]);
    let started = Instant::now();
    let output = match provider.as_str() {
        "codex" => Command::new("codex")
            .args(["exec", "--ephemeral", "--sandbox", "read-only", "--json", "--skip-git-repo-check", "--cd"])
            .arg(&repo)
            .arg(&prompt)
            .output()?,
        "claude" => Command::new("claude")
            .args(["-p", "--permission-mode", "plan", "--allowedTools", "Read,Grep,Glob", "--output-format", "json", "--no-session-persistence"])
            .arg(&prompt)
            .current_dir(&repo)
            .output()?,
        "gemini" => Command::new("gemini")
            .args(["--prompt", &prompt, "--approval-mode", "plan", "--output-format", "json", "--skip-trust"])
            .current_dir(&repo)
            .output()?,
        other => return Err(format!("unsupported provider {other:?}").into()),
    };
    let latency_ms = started.elapsed().as_millis() as u64;
    let raw_output = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let (input_tokens, output_tokens, tool_calls) = metrics(provider, &raw_output);
    let trial = Trial {
        provider: provider.clone(),
        repository: repo.display().to_string(),
        scenario: scenario.clone(),
        arm: arm.clone(),
        success: output.status.success() && raw_output.contains(expected),
        expected: expected.clone(),
        attempts: 1,
        retries: 0,
        tool_calls,
        input_tokens,
        output_tokens,
        latency_ms,
        exit_code: output.status.code(),
        raw_output,
        stderr,
    };
    if let Some(parent) = output_file.parent() { std::fs::create_dir_all(parent)?; }
    std::fs::write(output_file, serde_json::to_vec_pretty(&trial)?)?;
    println!("{} {} {} {} success={} latency={}ms", provider, scenario, arm, repo.display(), trial.success, trial.latency_ms);
    Ok(())
}

fn metrics(provider: &str, output: &str) -> (Option<u64>, Option<u64>, u64) {
    match provider {
        "claude" | "gemini" => {
            let value: Value = serde_json::from_str(output).unwrap_or(Value::Null);
            let usage = value.get("usage").or_else(|| value.get("stats").and_then(|v| v.get("models")).and_then(Value::as_object).and_then(|models| models.values().next()));
            let input = usage.map(|usage| {
                ["input_tokens", "inputTokens", "cache_creation_input_tokens", "cache_read_input_tokens"]
                    .iter()
                    .filter_map(|field| usage.get(field).and_then(Value::as_u64))
                    .sum()
            });
            let output_tokens = usage.and_then(|u| u.get("output_tokens").or_else(|| u.get("outputTokens"))).and_then(Value::as_u64);
            let calls = value.get("num_turns").and_then(Value::as_u64).unwrap_or(1).saturating_sub(1);
            (input, output_tokens, calls)
        }
        "codex" => {
            let values: Vec<Value> = output.lines().filter_map(|line| serde_json::from_str(line).ok()).collect();
            let usage = values.iter().find_map(|value| value.get("usage"));
            let input = usage.and_then(|u| u.get("input_tokens")).and_then(Value::as_u64);
            let output_tokens = usage.and_then(|u| u.get("output_tokens")).and_then(Value::as_u64);
            let calls = values.iter().filter(|value| value.get("type").and_then(Value::as_str).is_some_and(|kind| kind.contains("tool") || kind == "item.completed" && value.pointer("/item/type").and_then(Value::as_str) == Some("command_execution"))).count() as u64;
            (input, output_tokens, calls)
        }
        _ => (None, None, 0),
    }
}

fn summarize(dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut trials = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().and_then(|value| value.to_str()) == Some("json") {
            trials.push(serde_json::from_slice::<Trial>(&std::fs::read(path)?)?);
        }
    }
    trials.sort_by(|a, b| (&a.provider, &a.scenario, &a.arm).cmp(&(&b.provider, &b.scenario, &b.arm)));
    let success = trials.iter().filter(|trial| trial.success).count();
    let regressions = trials.iter().filter(|trial| trial.arm == "runtime" && !trial.success && trials.iter().any(|base| base.provider == trial.provider && base.scenario == trial.scenario && base.arm == "baseline" && base.success)).count();
    let latency: u64 = trials.iter().map(|trial| trial.latency_ms).sum();
    let known_tokens: u64 = trials.iter().filter_map(|trial| trial.input_tokens.zip(trial.output_tokens).map(|(a,b)| a+b)).sum();
    println!("trials={} success={}/{} regressions={} total_latency_ms={} reported_tokens={}", trials.len(), success, trials.len(), regressions, latency, known_tokens);
    for trial in trials {
        println!("{} {} {} success={} retries={} tools={} tokens={:?}+{:?} latency={}ms", trial.provider, trial.scenario, trial.arm, trial.success, trial.retries, trial.tool_calls, trial.input_tokens, trial.output_tokens, trial.latency_ms);
    }
    Ok(())
}

fn verify(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let value: Value = serde_json::from_slice(&std::fs::read(path)?)?;
    let trials = value.get("trials").and_then(Value::as_u64).ok_or("trials missing")?;
    let successes = value.get("successes").and_then(Value::as_u64).ok_or("successes missing")?;
    let regressions = value.get("paired_regressions").and_then(Value::as_u64).ok_or("paired_regressions missing")?;
    let providers = value.get("providers").and_then(Value::as_array).ok_or("providers missing")?;
    let repositories = value.get("repositories").and_then(Value::as_array).ok_or("repositories missing")?;
    let detail = value.get("trials_detail").and_then(Value::as_array).ok_or("trials_detail missing")?;
    if trials < 8 || detail.len() as u64 != trials { return Err("evaluation must retain at least eight detailed trials".into()); }
    if providers.len() < 2 || repositories.len() < 2 { return Err("evaluation must cover multiple providers and repositories".into()); }
    if successes > trials { return Err("success count exceeds trial count".into()); }
    if regressions > trials { return Err("regression count exceeds trial count".into()); }
    for trial in detail {
        for field in ["provider", "scenario", "arm", "success", "input_tokens", "output_tokens", "tool_calls", "latency_ms"] {
            if trial.get(field).is_none() { return Err(format!("trial missing {field}").into()); }
        }
    }
    println!("verified evaluation evidence: trials={trials} successes={successes} regressions={regressions}");
    Ok(())
}
