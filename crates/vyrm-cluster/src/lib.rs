//! Protocol-first cluster contracts for Vyrm.
//!
//! It freezes the placement, consistency, snapshot-vector, routing, transfer,
//! and transaction-scope contracts and supplies a deterministic failure
//! simulator. The optional `openraft-adapter` feature adds a real consensus
//! engine, atomic canonical runtime application, and Vyrm-native durable store,
//! but deliberately no production RPC. Runtime-bearing snapshots fail closed
//! until native bundles can transfer all canonical state. Production Multi-AZ
//! capability remains denied until authenticated transport and independent-
//! process hardware fault evidence pass the same invariants.

mod contract;
#[cfg(feature = "openraft-adapter")]
mod openraft_adapter;
mod sim;

pub use contract::*;
#[cfg(feature = "openraft-adapter")]
pub use openraft_adapter::*;
pub use sim::*;
