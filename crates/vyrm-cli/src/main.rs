//! vyrm operator surface. `SPEC.md` §13 stage 1.
//!
//! Every invocation is recorded with its trigger, arguments, outcome, and
//! duration, so that a trigger's promotion to automatic can later be justified
//! from evidence rather than assumption.
//!
//! Recording wraps execution in one place, so a command cannot be added that
//! forgets to record itself.
//!
//! This is an outer adapter and therefore may read a clock, which the kernel
//! must not. The clock is read once, here, and passed inward.

mod command;

use clap::Parser;
use command::Cli;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use vyrm_core::Reader;
use vyrm_store::{InvocationInput, Store};

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is before the Unix epoch")
        .as_millis() as u64
}

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();

    let store = match Store::open(&cli.db) {
        Ok(store) => store,
        Err(error) => {
            eprintln!("cannot open database at {}: {error}", cli.db.display());
            return std::process::ExitCode::from(2);
        }
    };

    let reader = match Reader::new(cli.reader.clone()) {
        Ok(reader) => reader,
        Err(error) => {
            eprintln!("invalid reader identity: {error}");
            return std::process::ExitCode::from(2);
        }
    };

    let now = now_millis();
    let started = Instant::now();
    let result = command::execute(&store, &cli.command, &reader, now, cli.json);
    let duration_ms = started.elapsed().as_millis() as u64;

    let (outcome, error_detail) = command::outcome_of(&result);
    // A successful execution may still carry a detail line (hook dispatches
    // journal what they did through it); a failure's detail is the error.
    let detail =
        error_detail.or_else(|| result.as_ref().ok().and_then(|e| e.detail.clone()));

    // The invocation is recorded whether the command succeeded or failed. A log
    // containing only successes would misrepresent which triggers are useful.
    // A recall's §13.1 effectiveness fields travel in the same record.
    if let Err(error) = store.record_invocation(InvocationInput {
        at: now,
        trigger: cli.command.trigger(),
        command: cli.command.name(),
        arguments: &cli.command.arguments(),
        outcome,
        duration_ms,
        detail,
        effectiveness: result.as_ref().ok().and_then(|e| e.effectiveness.clone()),
    }) {
        // Recording is the point of this surface, so a failure to record is
        // reported rather than swallowed, even when the command itself worked.
        eprintln!("warning: invocation was not recorded: {error}");
    }

    match result {
        Ok(execution) => {
            // An empty answer prints nothing at all: hook stdout is injected
            // into model context, and a stray newline is not an answer.
            if !execution.text.is_empty() {
                println!("{}", execution.text);
            }
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("error: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
