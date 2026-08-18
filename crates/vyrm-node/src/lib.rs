//! # vyrm-node
//!
//! The runtime experience (`PLAN.md` Step P). Everything below vyrm-cli is a
//! library an agent must *choose* to call; this crate is the layer that makes
//! choosing unnecessary. A harness lifecycle event arrives (session start, a
//! prompt, a tool call, a turn end), and the memory layer answers before the
//! model reasons — recall injected, application runs journaled, a quarantined
//! projection enforced as a gate rather than hoped about in prose.
//!
//! Three axes, never conflated (`PLAN.md` Step P): **harness** (the agent
//! runtime this crate adapts to, [`registry`]), **provider** (the model
//! backend), and **billing mode** (subscription quota vs per-usage tokens).
//! An adapter's verification is a bi-temporal claim with an expiry, so the
//! registry audits itself through `resolve_as_of` rather than a scheduler.
//!
//! The 2026 field converged on the lesson this crate encodes: memory is
//! placed into context by the runtime, deterministically — not left to the
//! model's discipline. Writes ride Buffered durability and never block the
//! turn; recall is the only synchronous path.

pub mod hook;
pub mod init;
pub mod instance;
pub mod policy;
pub mod preflight;
pub mod reasoning;
pub mod registry;
pub mod routing;
pub mod stack;

pub use hook::{handle, HookContext, HookEvent, HookResponse};
pub use init::{init, InitReport, STORE_DIR};
pub use instance::{
    InstanceBinding, InstanceManifest, InstanceMode, INSTANCE_FILE, INSTANCE_FORMAT,
};
pub use policy::{evaluate_tool, ContractDifferential, ToolPolicy};
pub use preflight::{preflight, Preflight};
pub use reasoning::{active_reasoning_run, reasoning_run, reasoning_runs, record_reasoning};
pub use registry::{Harness, Registry, Verification, VERIFICATION_TTL_MS};
pub use routing::{ensure_routing_fresh, load_routing, reset_routing, RoutingReady};
pub use stack::{detect, StackProfile};
