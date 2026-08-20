//! `vyrmMX`: deterministic binding, planning, and reference execution.
//!
//! The implementation deliberately begins with one exact physical path: a
//! stamped authoritative-log scan. Faster projections may compete later, but
//! must publish freshness evidence and pass differential verification first.

mod catalog;
mod error;
mod execute;
mod plan;

pub use catalog::{Catalog, SchemaVersion};
pub use error::{Error, Result};
pub use execute::{execute, ExecutionBudget, QueryBatch, QueryExecution, QueryRow};
pub use plan::{
    bind, plan, BoundFilter, BoundQuery, CandidatePath, ExecutionContract, LogicalOperator,
    LogicalPlan, Parameters, PhysicalOperator, PhysicalPlan, PlanExplanation,
};
