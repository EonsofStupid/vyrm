//! # vyrm-core
//!
//! The vyrm kernel: bi-temporal claims, key encoding, and supersession.
//!
//! ## Boundary
//!
//! This crate depends on `serde` alone. It must not depend on the substrate, a
//! transport, or tier policy, and must not expose substrate types in its public
//! API. Adapters implement [`temporal::ClaimSource`].
//!
//! `SPEC.md` §5 states the modularity criterion: a module satisfies the
//! specification if and only if it compiles and passes its tests with every
//! outward module removed. [`reference::MemoryClaims`] is the in-crate
//! implementation that allows this crate to satisfy that criterion, and serves
//! as the grounding reference defined in `SPEC.md` §8.3.
//!
//! ## Timelines
//!
//! - **valid time** (`valid_from`, `valid_to`) — the interval during which a
//!   claim holds in the modelled domain
//! - **transaction time** (`tx_time`) — the instant the kernel recorded it
//!
//! Superseded claims are retired, not deleted. Retirement makes staleness
//! representable, which is the mechanism by which this specification prevents
//! drift.
//!
//! ## Clocks
//!
//! The kernel does not read a clock. Every operation requiring the current
//! instant takes it as a parameter, so that results are reproducible and tests
//! are deterministic.

pub mod claim;
pub mod error;
pub mod ident;
pub mod key;
pub mod reference;
pub mod temporal;

pub use claim::{Claim, Millis, Producer, PromotionState, Tier};
pub use error::{Error, Result};
pub use ident::{Predicate, Reader, Subject};
pub use temporal::{changed_since, resolve_as_of, ClaimReader, ClaimSource};
