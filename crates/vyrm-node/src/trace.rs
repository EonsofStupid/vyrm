//! Conflict-safe persistence bridge for durable runtime traces.
//!
//! Process-local `tracing` spans are diagnostics. This module commits the
//! portable `vyrm-core` trace contract into the authoritative scoped runtime
//! log. Schema installation and the event share one cursor CAS, and bounded
//! retries rebase only on an observed concurrent winner.

use std::time::Instant;
use vyrm_core::{
    digest, Millis, RuntimeCommit, RuntimeCommitOutcome, RuntimeMutation, RuntimeProperties,
    RuntimeSchemaRegistry, RuntimeTraceEvent, RuntimeValue, ScopeId, SpanId, TraceDataClass,
    TraceDomain, TraceId, TraceLink, TraceOutcome,
};
use vyrm_store::{Engine, Error};

const TRACE_COMMIT_RETRIES: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceIdentity {
    pub trace_id: TraceId,
    pub span_id: SpanId,
}

impl TraceIdentity {
    /// Derives repeatable W3C-width identities from already bounded invocation
    /// coordinates. Length framing prevents concatenation ambiguity.
    pub fn derive(parts: &[&[u8]]) -> Result<Self, Box<dyn std::error::Error>> {
        let trace = identity_digest(b"vyrm-runtime-trace-id-v1\0", parts);
        let span = identity_digest(b"vyrm-runtime-span-id-v1\0", parts);
        Ok(Self {
            trace_id: TraceId::new(&trace[..32])?,
            span_id: SpanId::new(&span[..16])?,
        })
    }

    /// Derives a deterministic child span while retaining the parent's trace
    /// identity. The parent coordinates are included so equal child labels in
    /// different traces or branches cannot collide.
    pub fn child(&self, parts: &[&[u8]]) -> Result<Self, Box<dyn std::error::Error>> {
        let mut coordinates = Vec::with_capacity(parts.len() + 2);
        coordinates.push(self.trace_id.as_str().as_bytes());
        coordinates.push(self.span_id.as_str().as_bytes());
        coordinates.extend_from_slice(parts);
        let span = identity_digest(b"vyrm-runtime-child-span-id-v1\0", &coordinates);
        Ok(Self {
            trace_id: self.trace_id.clone(),
            span_id: SpanId::new(&span[..16])?,
        })
    }
}

/// One authoritative start that can only be completed by consuming it.
///
/// The helper centralizes start-cursor linkage, monotonic duration measurement,
/// and finish persistence so every instrumented subsystem has identical crash
/// semantics. Dropping it intentionally leaves an incomplete span visible.
pub struct DurableTraceSpan {
    identity: TraceIdentity,
    parent_span_id: Option<SpanId>,
    domain: TraceDomain,
    name: String,
    started_at: Millis,
    started: Instant,
    data_class: TraceDataClass,
    links: Vec<TraceLink>,
    attributes: RuntimeProperties,
    scope: ScopeId,
    actor: String,
    start_cursor: u64,
}

impl DurableTraceSpan {
    #[allow(clippy::too_many_arguments)]
    pub fn start<E: Engine>(
        store: &E,
        scope: ScopeId,
        actor: impl Into<String>,
        identity: TraceIdentity,
        parent_span_id: Option<SpanId>,
        domain: TraceDomain,
        name: impl Into<String>,
        at: Millis,
        data_class: TraceDataClass,
        links: Vec<TraceLink>,
        attributes: RuntimeProperties,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let actor = actor.into();
        let name = name.into();
        let started = Instant::now();
        let event = RuntimeTraceEvent::start(
            identity.trace_id.clone(),
            identity.span_id.clone(),
            parent_span_id.clone(),
            domain,
            name.clone(),
            at,
            data_class,
            links.clone(),
            attributes.clone(),
        )?;
        let outcome = record_runtime_trace(store, &scope, &actor, event)?;
        Ok(Self {
            identity,
            parent_span_id,
            domain,
            name,
            started_at: at,
            started,
            data_class,
            links,
            attributes,
            scope,
            actor,
            start_cursor: outcome.last_cursor,
        })
    }

    pub fn identity(&self) -> &TraceIdentity {
        &self.identity
    }

    /// Wall-clock coordinate derived from the caller-supplied start and a
    /// monotonic elapsed duration. The kernel still never reads a clock.
    pub fn observed_at(&self) -> Millis {
        self.started_at
            .saturating_add(self.started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64)
    }

    pub fn finish<E: Engine>(
        mut self,
        store: &E,
        outcome: TraceOutcome,
        extra_links: Vec<TraceLink>,
        extra_attributes: RuntimeProperties,
    ) -> Result<RuntimeCommitOutcome, Box<dyn std::error::Error>> {
        if outcome == TraceOutcome::Running {
            return Err("a durable trace finish cannot retain the running outcome".into());
        }
        self.links.extend(extra_links);
        self.links.push(TraceLink::RuntimeCursor {
            cursor: store.runtime_cursor()?,
        });
        insert_attribute(
            &mut self.attributes,
            "start_cursor",
            RuntimeValue::Unsigned(self.start_cursor),
        )?;
        for (name, value) in extra_attributes {
            insert_attribute(&mut self.attributes, &name, value)?;
        }
        let elapsed = self.started.elapsed();
        let duration_micros = elapsed.as_micros().min(u128::from(u64::MAX)) as u64;
        let finished_at = self
            .started_at
            .saturating_add(elapsed.as_millis().min(u128::from(u64::MAX)) as u64);
        let event = RuntimeTraceEvent::finish(
            self.identity.trace_id,
            self.identity.span_id,
            self.parent_span_id,
            self.domain,
            self.name,
            finished_at,
            duration_micros,
            outcome,
            self.data_class,
            self.links,
            self.attributes,
        )?;
        record_runtime_trace(store, &self.scope, &self.actor, event)
    }
}

fn insert_attribute(
    attributes: &mut RuntimeProperties,
    name: &str,
    value: RuntimeValue,
) -> Result<(), Box<dyn std::error::Error>> {
    if attributes.insert(name.to_owned(), value).is_some() {
        return Err(format!("runtime trace attribute {name:?} was supplied more than once").into());
    }
    Ok(())
}

/// Installs the strict trace event schema without emitting a synthetic event.
/// Returns `None` when the exact declaration is already installed.
pub fn install_runtime_trace_contract<E: Engine>(
    store: &E,
    scope: &ScopeId,
    at: Millis,
    actor: &str,
) -> Result<Option<RuntimeCommitOutcome>, Box<dyn std::error::Error>> {
    commit_trace(store, scope, at, actor, None)
}

/// Persists one immutable trace micro-event. Missing or outdated trace schema
/// is migrated in the same atomic runtime commit as the event.
pub fn record_runtime_trace<E: Engine>(
    store: &E,
    scope: &ScopeId,
    actor: &str,
    event: RuntimeTraceEvent,
) -> Result<RuntimeCommitOutcome, Box<dyn std::error::Error>> {
    event.validate()?;
    commit_trace(store, scope, event.at, actor, Some(event))?
        .ok_or_else(|| "a runtime trace event produced no commit".into())
}

fn commit_trace<E: Engine>(
    store: &E,
    scope: &ScopeId,
    at: Millis,
    actor: &str,
    event: Option<RuntimeTraceEvent>,
) -> Result<Option<RuntimeCommitOutcome>, Box<dyn std::error::Error>> {
    if actor.trim().is_empty() {
        return Err("runtime trace actor must not be empty".into());
    }
    for _ in 0..TRACE_COMMIT_RETRIES {
        let read = store.runtime_read_stamp(scope)?;
        let current = store.runtime_schema(scope)?;
        if current.as_ref().map(|schema| schema.revision) != read.schema_revision {
            continue;
        }

        let mut mutations = Vec::new();
        let mut registry = current
            .clone()
            .unwrap_or_else(|| RuntimeSchemaRegistry::empty(1, "install runtime trace contract"));
        if RuntimeTraceEvent::register_schema(&mut registry)? {
            if let Some(current) = &current {
                registry.revision = current
                    .revision
                    .checked_add(1)
                    .ok_or("runtime schema revision overflow while installing trace contract")?;
                registry.migration = "install canonical runtime trace contract".into();
            }
            mutations.push(RuntimeMutation::Schema { registry });
        }
        if let Some(event) = &event {
            mutations.push(RuntimeMutation::Event {
                event: event.clone().into_runtime_event()?,
            });
        }
        if mutations.is_empty() {
            return Ok(None);
        }

        let commit = RuntimeCommit {
            scope: scope.clone(),
            at,
            actor: actor.to_owned(),
            expected_cursor: read.commit_cursor,
            mutations,
        };
        match store.commit_runtime(&commit) {
            Ok(outcome) => return Ok(Some(outcome)),
            Err(Error::RuntimeConflict { .. }) => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Err(format!(
        "runtime trace could not acquire cursor CAS after {TRACE_COMMIT_RETRIES} observed conflicts"
    )
    .into())
}

fn identity_digest(domain: &[u8], parts: &[&[u8]]) -> String {
    let mut bytes = Vec::with_capacity(
        domain.len()
            + parts
                .iter()
                .map(|part| std::mem::size_of::<u64>() + part.len())
                .sum::<usize>(),
    );
    bytes.extend_from_slice(domain);
    for part in parts {
        bytes.extend_from_slice(&(part.len() as u64).to_be_bytes());
        bytes.extend_from_slice(part);
    }
    digest::sha256_hex(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_derivation_is_domain_separated_and_length_framed() {
        let first = TraceIdentity::derive(&[b"ab", b"c"]).unwrap();
        let repeated = TraceIdentity::derive(&[b"ab", b"c"]).unwrap();
        let ambiguous_without_framing = TraceIdentity::derive(&[b"a", b"bc"]).unwrap();
        assert_eq!(first, repeated);
        assert_ne!(first, ambiguous_without_framing);
        assert_ne!(first.trace_id.as_str()[..16], *first.span_id.as_str());
        let child = first.child(&[b"query", b"plan"]).unwrap();
        let repeated_child = first.child(&[b"query", b"plan"]).unwrap();
        assert_eq!(child, repeated_child);
        assert_eq!(child.trace_id, first.trace_id);
        assert_ne!(child.span_id, first.span_id);
    }
}
