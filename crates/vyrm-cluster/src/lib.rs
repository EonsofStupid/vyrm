//! Protocol-first cluster contracts for Vyrm.
//!
//! It freezes the placement, consistency, snapshot-vector, routing, transfer,
//! and transaction-scope contracts and supplies a deterministic failure
//! simulator. The optional `openraft-adapter` feature adds a real consensus
//! engine, atomic canonical runtime application, and Vyrm-native durable store,
//! including authenticated runtime-bearing snapshot transfer across physically
//! separate local-Raft and canonical-state domains. The further opt-in
//! `openraft-transport` feature adds identity-bound TLS 1.3 RPCs, but production
//! Multi-AZ capability remains denied until independent-process hardware fault
//! and credential-lifecycle evidence pass the same invariants.

mod contract;
#[cfg(feature = "openraft-adapter")]
mod openraft_adapter;
mod sim;
#[cfg(feature = "openraft-transport")]
mod transport;
#[cfg(feature = "openraft-transport")]
mod node_runtime;

pub use contract::*;
#[cfg(feature = "openraft-adapter")]
pub use openraft_adapter::*;
pub use sim::*;
#[cfg(feature = "openraft-transport")]
pub use transport::*;
#[cfg(feature = "openraft-transport")]
pub use node_runtime::*;
