//! The single RRFlow engine composition root.
//!
//! Physical storage, query, vector, estate, and security components are
//! composed here. Daemon and embedded faces consume this API instead of
//! constructing those components independently.

mod engine;

pub use engine::{
    AuditEvent, Result, RrflowEngine, ServiceError, ServiceErrorKind, MAX_AUDIT_PAGE_RECORDS,
};
