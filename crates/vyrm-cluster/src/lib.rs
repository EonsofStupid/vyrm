//! Protocol-first cluster contracts for Vyrm.
//!
//! This crate is deliberately not a networked cluster implementation. It
//! freezes the placement, consistency, snapshot-vector, routing, transfer, and
//! transaction-scope contracts and supplies a deterministic failure simulator.
//! Production Multi-AZ capability remains denied until a consensus adapter and
//! hardware fault evidence pass the same invariants.

mod contract;
mod sim;

pub use contract::*;
pub use sim::*;
