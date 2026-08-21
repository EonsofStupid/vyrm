//! `vyrmMX`: deterministic binding, planning, and reference execution.
//!
//! The implementation begins from an exact stamped authoritative-log scan and
//! admits narrower authoritative paths only when the bound query proves them
//! equivalent. Faster projections may compete later, but must publish
//! freshness evidence and pass differential verification first.

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
