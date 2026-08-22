//! Bounded, reset-explicit process telemetry for authenticated cluster work.
//!
//! These counters are operational evidence, not canonical runtime truth. A
//! snapshot always carries the process start and observation times so callers
//! cannot silently compare counters across restarts.

use crate::{ClusterError, NodeId, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

pub const VYRM_CLUSTER_TELEMETRY_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VyrmTransportOperation {
    Append,
    Snapshot,
    Vote,
    Artifact,
    RuntimeCommit,
}

impl VyrmTransportOperation {
    pub const ALL: [Self; 5] = [
        Self::Append,
        Self::Snapshot,
        Self::Vote,
        Self::Artifact,
        Self::RuntimeCommit,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VyrmTransportOutcome {
    Allowed,
    Denied,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VyrmTransportAdmissionPolicy {
    pub max_global_in_flight: usize,
    pub max_identity_in_flight: usize,
    pub max_identity_requests_per_window: u64,
    pub window_millis: u64,
    pub max_tracked_identities: usize,
}

impl Default for VyrmTransportAdmissionPolicy {
    fn default() -> Self {
        Self {
            max_global_in_flight: 256,
            max_identity_in_flight: 64,
            max_identity_requests_per_window: 4_096,
            window_millis: 1_000,
            max_tracked_identities: 1_024,
        }
    }
}

impl VyrmTransportAdmissionPolicy {
    pub fn validate(&self) -> Result<()> {
        if self.max_global_in_flight == 0
            || self.max_global_in_flight > 65_536
            || self.max_identity_in_flight == 0
            || self.max_identity_in_flight > self.max_global_in_flight
            || self.max_identity_requests_per_window == 0
            || self.window_millis == 0
            || self.window_millis > 60_000
            || self.max_tracked_identities == 0
            || self.max_tracked_identities > 65_536
        {
            return Err(ClusterError::Invalid(
                "transport admission policy is outside its bounded contract".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VyrmTransportOperationMetrics {
    pub attempted: u64,
    pub allowed: u64,
    pub denied: u64,
    pub failed: u64,
    pub current_in_flight: u64,
    pub peak_in_flight: u64,
    pub request_bytes: u64,
    pub response_bytes: u64,
    pub total_duration_micros: u64,
    pub max_duration_micros: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VyrmTransportIdentityMetrics {
    pub operations: BTreeMap<VyrmTransportOperation, VyrmTransportOperationMetrics>,
    pub current_in_flight: u64,
    pub peak_in_flight: u64,
    pub rate_window_started_at: u64,
    pub requests_in_rate_window: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VyrmTransportTelemetrySnapshot {
    pub contract_version: u16,
    pub started_at: u64,
    pub observed_at: u64,
    pub policy: VyrmTransportAdmissionPolicy,
    pub operations: BTreeMap<VyrmTransportOperation, VyrmTransportOperationMetrics>,
    pub identities: BTreeMap<NodeId, VyrmTransportIdentityMetrics>,
    pub current_in_flight: u64,
    pub peak_in_flight: u64,
    pub accepted_connections: u64,
    pub denied_connections: u64,
    pub connection_request_bytes: u64,
    pub overflowed: bool,
}

impl VyrmTransportOperationMetrics {
    fn validate(&self, overflowed: bool) -> Result<()> {
        if self.current_in_flight > self.peak_in_flight {
            return Err(ClusterError::Invalid(
                "transport telemetry current work exceeds its recorded peak".into(),
            ));
        }
        if !overflowed {
            let classified = self
                .allowed
                .checked_add(self.denied)
                .and_then(|value| value.checked_add(self.failed))
                .and_then(|value| value.checked_add(self.current_in_flight))
                .ok_or_else(|| {
                    ClusterError::Invalid(
                        "transport telemetry classification total overflowed".into(),
                    )
                })?;
            if classified != self.attempted {
                return Err(ClusterError::Invalid(
                    "transport telemetry attempts are not completely classified".into(),
                ));
            }
        }
        Ok(())
    }
}

impl VyrmTransportTelemetrySnapshot {
    pub fn validate(&self) -> Result<()> {
        if self.contract_version != VYRM_CLUSTER_TELEMETRY_VERSION
            || self.started_at > self.observed_at
            || self.identities.len() > self.policy.max_tracked_identities
            || self.current_in_flight > self.peak_in_flight
            || self.peak_in_flight > self.policy.max_global_in_flight as u64
        {
            return Err(ClusterError::Invalid(
                "transport telemetry snapshot is outside its bounded contract".into(),
            ));
        }
        self.policy.validate()?;
        if self.operations.keys().copied().collect::<Vec<_>>()
            != VyrmTransportOperation::ALL.to_vec()
        {
            return Err(ClusterError::Invalid(
                "transport telemetry does not contain the canonical operation set".into(),
            ));
        }
        for metrics in self.operations.values() {
            metrics.validate(self.overflowed)?;
            if metrics.peak_in_flight > self.policy.max_global_in_flight as u64 {
                return Err(ClusterError::Invalid(
                    "transport operation peak exceeds the global admission bound".into(),
                ));
            }
        }
        for identity in self.identities.values() {
            if identity.operations.keys().copied().collect::<Vec<_>>()
                != VyrmTransportOperation::ALL.to_vec()
                || identity.rate_window_started_at < self.started_at
                || identity.rate_window_started_at > self.observed_at
                || identity.requests_in_rate_window > self.policy.max_identity_requests_per_window
                || identity.current_in_flight > identity.peak_in_flight
                || identity.peak_in_flight > self.policy.max_identity_in_flight as u64
            {
                return Err(ClusterError::Invalid(
                    "transport identity telemetry is outside its bounded contract".into(),
                ));
            }
            let mut current = 0u64;
            for metrics in identity.operations.values() {
                metrics.validate(self.overflowed)?;
                if metrics.peak_in_flight > self.policy.max_identity_in_flight as u64 {
                    return Err(ClusterError::Invalid(
                        "transport identity operation peak exceeds its admission bound".into(),
                    ));
                }
                current = current
                    .checked_add(metrics.current_in_flight)
                    .ok_or_else(|| {
                        ClusterError::Invalid(
                            "transport identity in-flight total overflowed".into(),
                        )
                    })?;
            }
            if current != identity.current_in_flight {
                return Err(ClusterError::Invalid(
                    "transport identity in-flight total is inconsistent".into(),
                ));
            }
        }
        if !self.overflowed {
            let operation_current = self.operations.values().try_fold(0u64, |total, metrics| {
                total.checked_add(metrics.current_in_flight).ok_or_else(|| {
                    ClusterError::Invalid("transport operation in-flight total overflowed".into())
                })
            })?;
            let identity_current = self.identities.values().try_fold(0u64, |total, metrics| {
                total.checked_add(metrics.current_in_flight).ok_or_else(|| {
                    ClusterError::Invalid("transport identity in-flight total overflowed".into())
                })
            })?;
            if operation_current != self.current_in_flight
                || identity_current != self.current_in_flight
            {
                return Err(ClusterError::Invalid(
                    "transport global in-flight total is inconsistent".into(),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct VyrmTransportTelemetry {
    inner: Arc<Mutex<TelemetryState>>,
}

#[derive(Debug)]
struct TelemetryState {
    started_at: u64,
    policy: VyrmTransportAdmissionPolicy,
    operations: BTreeMap<VyrmTransportOperation, VyrmTransportOperationMetrics>,
    identities: BTreeMap<NodeId, IdentityState>,
    current_in_flight: u64,
    peak_in_flight: u64,
    accepted_connections: u64,
    denied_connections: u64,
    connection_request_bytes: u64,
    overflowed: bool,
}

#[derive(Debug)]
struct IdentityState {
    operations: BTreeMap<VyrmTransportOperation, VyrmTransportOperationMetrics>,
    current_in_flight: u64,
    peak_in_flight: u64,
    rate_window_started_at: u64,
    requests_in_rate_window: u64,
}

#[derive(Debug)]
pub struct VyrmTransportAdmissionGuard {
    telemetry: VyrmTransportTelemetry,
    identity: NodeId,
    operation: VyrmTransportOperation,
    started: Instant,
    finished: bool,
}

impl VyrmTransportTelemetry {
    pub fn new(policy: VyrmTransportAdmissionPolicy, started_at: u64) -> Result<Self> {
        policy.validate()?;
        Ok(Self {
            inner: Arc::new(Mutex::new(TelemetryState {
                started_at,
                policy,
                operations: operation_map(),
                identities: BTreeMap::new(),
                current_in_flight: 0,
                peak_in_flight: 0,
                accepted_connections: 0,
                denied_connections: 0,
                connection_request_bytes: 0,
                overflowed: false,
            })),
        })
    }

    pub fn policy(&self) -> Result<VyrmTransportAdmissionPolicy> {
        Ok(self.lock()?.policy.clone())
    }

    pub fn accept_connection(&self, request_bytes: u64) -> Result<()> {
        let mut state = self.lock()?;
        let mut overflowed = state.overflowed;
        add_counter(&mut state.accepted_connections, 1, &mut overflowed);
        add_counter(
            &mut state.connection_request_bytes,
            request_bytes,
            &mut overflowed,
        );
        state.overflowed = overflowed;
        Ok(())
    }

    pub fn reject_connection(&self, request_bytes: u64) -> Result<()> {
        let mut state = self.lock()?;
        let mut overflowed = state.overflowed;
        add_counter(&mut state.denied_connections, 1, &mut overflowed);
        add_counter(
            &mut state.connection_request_bytes,
            request_bytes,
            &mut overflowed,
        );
        state.overflowed = overflowed;
        Ok(())
    }

    pub fn admit(
        &self,
        identity: &NodeId,
        operation: VyrmTransportOperation,
        request_bytes: u64,
        now: u64,
    ) -> Result<VyrmTransportAdmissionGuard> {
        let mut state = self.lock()?;
        let mut overflowed = state.overflowed;
        let policy = state.policy.clone();
        if now < state.started_at {
            return Err(ClusterError::Invalid(
                "transport admission predates this process".into(),
            ));
        }
        if state
            .identities
            .get(identity)
            .is_some_and(|metrics| now < metrics.rate_window_started_at)
        {
            return Err(ClusterError::Invalid(
                "transport admission predates the identity rate window".into(),
            ));
        }
        {
            let global = state
                .operations
                .get_mut(&operation)
                .expect("complete operation map");
            add_counter(&mut global.attempted, 1, &mut overflowed);
            add_counter(&mut global.request_bytes, request_bytes, &mut overflowed);
        }
        if !state.identities.contains_key(identity)
            && state.identities.len() >= policy.max_tracked_identities
        {
            add_counter(
                &mut state
                    .operations
                    .get_mut(&operation)
                    .expect("complete operation map")
                    .denied,
                1,
                &mut overflowed,
            );
            state.overflowed = overflowed;
            return Err(ClusterError::Denied(
                "transport identity telemetry capacity is exhausted".into(),
            ));
        }
        let global_limit_reached = state.current_in_flight >= policy.max_global_in_flight as u64;
        let denied = {
            let identity_state =
                state
                    .identities
                    .entry(identity.clone())
                    .or_insert_with(|| IdentityState {
                        operations: operation_map(),
                        current_in_flight: 0,
                        peak_in_flight: 0,
                        rate_window_started_at: now,
                        requests_in_rate_window: 0,
                    });
            if now.saturating_sub(identity_state.rate_window_started_at) >= policy.window_millis {
                identity_state.rate_window_started_at = now;
                identity_state.requests_in_rate_window = 0;
            }
            let identity_metrics = identity_state
                .operations
                .get_mut(&operation)
                .expect("complete operation map");
            add_counter(&mut identity_metrics.attempted, 1, &mut overflowed);
            add_counter(
                &mut identity_metrics.request_bytes,
                request_bytes,
                &mut overflowed,
            );
            let denied = global_limit_reached
                || identity_state.current_in_flight >= policy.max_identity_in_flight as u64
                || identity_state.requests_in_rate_window
                    >= policy.max_identity_requests_per_window;
            if denied {
                add_counter(&mut identity_metrics.denied, 1, &mut overflowed);
            } else {
                add_counter(
                    &mut identity_state.requests_in_rate_window,
                    1,
                    &mut overflowed,
                );
                add_counter(&mut identity_state.current_in_flight, 1, &mut overflowed);
                identity_state.peak_in_flight = identity_state
                    .peak_in_flight
                    .max(identity_state.current_in_flight);
                add_counter(&mut identity_metrics.current_in_flight, 1, &mut overflowed);
                identity_metrics.peak_in_flight = identity_metrics
                    .peak_in_flight
                    .max(identity_metrics.current_in_flight);
            }
            denied
        };
        if denied {
            add_counter(
                &mut state
                    .operations
                    .get_mut(&operation)
                    .expect("complete operation map")
                    .denied,
                1,
                &mut overflowed,
            );
            state.overflowed = overflowed;
            return Err(ClusterError::Denied(
                "authenticated transport identity exceeded its admission policy".into(),
            ));
        }
        let global = state
            .operations
            .get_mut(&operation)
            .expect("complete operation map");
        add_counter(&mut global.current_in_flight, 1, &mut overflowed);
        global.peak_in_flight = global.peak_in_flight.max(global.current_in_flight);
        add_counter(&mut state.current_in_flight, 1, &mut overflowed);
        state.peak_in_flight = state.peak_in_flight.max(state.current_in_flight);
        state.overflowed = overflowed;
        drop(state);
        Ok(VyrmTransportAdmissionGuard {
            telemetry: self.clone(),
            identity: identity.clone(),
            operation,
            started: Instant::now(),
            finished: false,
        })
    }

    pub fn snapshot(&self, observed_at: u64) -> Result<VyrmTransportTelemetrySnapshot> {
        let state = self.lock()?;
        if observed_at < state.started_at {
            return Err(ClusterError::Invalid(
                "transport telemetry observation predates this process".into(),
            ));
        }
        Ok(VyrmTransportTelemetrySnapshot {
            contract_version: VYRM_CLUSTER_TELEMETRY_VERSION,
            started_at: state.started_at,
            observed_at,
            policy: state.policy.clone(),
            operations: state.operations.clone(),
            identities: state
                .identities
                .iter()
                .map(|(identity, metrics)| {
                    (
                        identity.clone(),
                        VyrmTransportIdentityMetrics {
                            operations: metrics.operations.clone(),
                            current_in_flight: metrics.current_in_flight,
                            peak_in_flight: metrics.peak_in_flight,
                            rate_window_started_at: metrics.rate_window_started_at,
                            requests_in_rate_window: metrics.requests_in_rate_window,
                        },
                    )
                })
                .collect(),
            current_in_flight: state.current_in_flight,
            peak_in_flight: state.peak_in_flight,
            accepted_connections: state.accepted_connections,
            denied_connections: state.denied_connections,
            connection_request_bytes: state.connection_request_bytes,
            overflowed: state.overflowed,
        })
    }

    fn finish(
        &self,
        identity: &NodeId,
        operation: VyrmTransportOperation,
        outcome: VyrmTransportOutcome,
        response_bytes: u64,
        duration_micros: u64,
    ) -> Result<()> {
        let mut state = self.lock()?;
        let mut overflowed = state.overflowed;
        let global = state
            .operations
            .get_mut(&operation)
            .expect("complete operation map");
        finish_metrics(
            global,
            outcome,
            response_bytes,
            duration_micros,
            &mut overflowed,
        );
        let identity_state = state.identities.get_mut(identity).ok_or_else(|| {
            ClusterError::Unavailable("transport telemetry lost an admitted identity".into())
        })?;
        identity_state.current_in_flight = identity_state.current_in_flight.saturating_sub(1);
        let identity_metrics = identity_state
            .operations
            .get_mut(&operation)
            .expect("complete operation map");
        finish_metrics(
            identity_metrics,
            outcome,
            response_bytes,
            duration_micros,
            &mut overflowed,
        );
        state.current_in_flight = state.current_in_flight.saturating_sub(1);
        state.overflowed = overflowed;
        Ok(())
    }

    fn lock(&self) -> Result<MutexGuard<'_, TelemetryState>> {
        self.inner.lock().map_err(|error| {
            ClusterError::Unavailable(format!("transport telemetry lock was poisoned: {error}"))
        })
    }
}

impl VyrmTransportAdmissionGuard {
    pub fn finish(mut self, outcome: VyrmTransportOutcome, response_bytes: u64) -> Result<()> {
        self.finished = true;
        self.telemetry.finish(
            &self.identity,
            self.operation,
            outcome,
            response_bytes,
            self.started.elapsed().as_micros().min(u128::from(u64::MAX)) as u64,
        )
    }
}

impl Drop for VyrmTransportAdmissionGuard {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.telemetry.finish(
                &self.identity,
                self.operation,
                VyrmTransportOutcome::Failed,
                0,
                self.started.elapsed().as_micros().min(u128::from(u64::MAX)) as u64,
            );
        }
    }
}

fn operation_map() -> BTreeMap<VyrmTransportOperation, VyrmTransportOperationMetrics> {
    VyrmTransportOperation::ALL
        .into_iter()
        .map(|operation| (operation, VyrmTransportOperationMetrics::default()))
        .collect()
}

fn finish_metrics(
    metrics: &mut VyrmTransportOperationMetrics,
    outcome: VyrmTransportOutcome,
    response_bytes: u64,
    duration_micros: u64,
    overflowed: &mut bool,
) {
    metrics.current_in_flight = metrics.current_in_flight.saturating_sub(1);
    match outcome {
        VyrmTransportOutcome::Allowed => add_counter(&mut metrics.allowed, 1, overflowed),
        VyrmTransportOutcome::Denied => add_counter(&mut metrics.denied, 1, overflowed),
        VyrmTransportOutcome::Failed => add_counter(&mut metrics.failed, 1, overflowed),
    }
    add_counter(&mut metrics.response_bytes, response_bytes, overflowed);
    add_counter(
        &mut metrics.total_duration_micros,
        duration_micros,
        overflowed,
    );
    metrics.max_duration_micros = metrics.max_duration_micros.max(duration_micros);
}

fn add_counter(counter: &mut u64, value: u64, overflowed: &mut bool) {
    match counter.checked_add(value) {
        Some(next) => *counter = next,
        None => {
            *counter = u64::MAX;
            *overflowed = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> VyrmTransportAdmissionPolicy {
        VyrmTransportAdmissionPolicy {
            max_global_in_flight: 4,
            max_identity_in_flight: 1,
            max_identity_requests_per_window: 2,
            window_millis: 10,
            max_tracked_identities: 2,
        }
    }

    #[test]
    fn identity_concurrency_and_window_limits_are_bounded_and_reset_explicit() {
        let telemetry = VyrmTransportTelemetry::new(policy(), 90).unwrap();
        let one = NodeId::new("node:one").unwrap();
        let two = NodeId::new("node:two").unwrap();
        let first = telemetry
            .admit(&one, VyrmTransportOperation::Append, 100, 100)
            .unwrap();
        assert!(telemetry
            .admit(&one, VyrmTransportOperation::Append, 50, 100)
            .unwrap_err()
            .to_string()
            .contains("admission policy"));
        telemetry
            .admit(&two, VyrmTransportOperation::Append, 75, 100)
            .unwrap()
            .finish(VyrmTransportOutcome::Allowed, 25)
            .unwrap();
        first.finish(VyrmTransportOutcome::Allowed, 40).unwrap();
        telemetry
            .admit(&one, VyrmTransportOperation::Append, 60, 100)
            .unwrap()
            .finish(VyrmTransportOutcome::Denied, 30)
            .unwrap();
        assert!(telemetry
            .admit(&one, VyrmTransportOperation::Append, 10, 100)
            .is_err());
        telemetry
            .admit(&one, VyrmTransportOperation::Append, 10, 110)
            .unwrap()
            .finish(VyrmTransportOutcome::Failed, 0)
            .unwrap();

        let snapshot = telemetry.snapshot(111).unwrap();
        assert_eq!(snapshot.started_at, 90);
        assert_eq!(snapshot.observed_at, 111);
        assert_eq!(snapshot.identities.len(), 2);
        let append = &snapshot.operations[&VyrmTransportOperation::Append];
        assert_eq!(append.attempted, 6);
        assert_eq!(append.allowed, 2);
        assert_eq!(append.denied, 3);
        assert_eq!(append.failed, 1);
        assert_eq!(append.current_in_flight, 0);
        assert_eq!(append.peak_in_flight, 2);
        assert_eq!(append.request_bytes, 305);
        assert_eq!(append.response_bytes, 95);
        assert_eq!(snapshot.current_in_flight, 0);
        assert_eq!(snapshot.peak_in_flight, 2);
        assert!(!snapshot.overflowed);
    }

    #[test]
    fn dropped_guard_records_failed_work_and_connection_denials_are_content_free() {
        let telemetry = VyrmTransportTelemetry::new(policy(), 1).unwrap();
        let identity = NodeId::new("node:one").unwrap();
        drop(
            telemetry
                .admit(&identity, VyrmTransportOperation::Snapshot, 123, 2)
                .unwrap(),
        );
        telemetry.reject_connection(77).unwrap();
        telemetry.accept_connection(23).unwrap();
        let snapshot = telemetry.snapshot(3).unwrap();
        assert_eq!(
            snapshot.operations[&VyrmTransportOperation::Snapshot].failed,
            1
        );
        assert_eq!(snapshot.accepted_connections, 1);
        assert_eq!(snapshot.denied_connections, 1);
        assert_eq!(snapshot.connection_request_bytes, 100);
        let encoded = serde_json::to_string(&snapshot).unwrap();
        assert!(!encoded.contains("credential"));
        assert!(!encoded.contains("payload"));
    }

    #[test]
    fn global_admission_is_enforced_across_identities_and_operation_kinds() {
        let mut admission = policy();
        admission.max_global_in_flight = 1;
        let telemetry = VyrmTransportTelemetry::new(admission, 1).unwrap();
        let one = NodeId::new("node:one").unwrap();
        let two = NodeId::new("node:two").unwrap();
        let first = telemetry
            .admit(&one, VyrmTransportOperation::Append, 10, 2)
            .unwrap();
        assert!(telemetry
            .admit(&two, VyrmTransportOperation::Vote, 20, 2)
            .is_err());
        first.finish(VyrmTransportOutcome::Allowed, 5).unwrap();
        telemetry
            .admit(&two, VyrmTransportOperation::Vote, 20, 3)
            .unwrap()
            .finish(VyrmTransportOutcome::Allowed, 5)
            .unwrap();
        let snapshot = telemetry.snapshot(4).unwrap();
        assert_eq!(snapshot.peak_in_flight, 1);
        assert_eq!(snapshot.current_in_flight, 0);
        assert_eq!(snapshot.operations[&VyrmTransportOperation::Vote].denied, 1);
    }

    #[test]
    fn identity_admission_is_enforced_across_operation_kinds() {
        let telemetry = VyrmTransportTelemetry::new(policy(), 1).unwrap();
        let identity = NodeId::new("node:one").unwrap();
        let first = telemetry
            .admit(&identity, VyrmTransportOperation::Append, 10, 2)
            .unwrap();
        assert!(telemetry
            .admit(&identity, VyrmTransportOperation::Vote, 20, 2)
            .is_err());
        first.finish(VyrmTransportOutcome::Allowed, 5).unwrap();
        let snapshot = telemetry.snapshot(3).unwrap();
        assert_eq!(snapshot.identities[&identity].peak_in_flight, 1);
        assert_eq!(snapshot.identities[&identity].current_in_flight, 0);
        assert_eq!(snapshot.operations[&VyrmTransportOperation::Vote].denied, 1);
    }

    #[test]
    fn observations_and_admission_reject_prestart_time_and_identity_growth_is_bounded() {
        let telemetry = VyrmTransportTelemetry::new(policy(), 100).unwrap();
        let one = NodeId::new("node:one").unwrap();
        let two = NodeId::new("node:two").unwrap();
        let three = NodeId::new("node:three").unwrap();
        assert!(telemetry.snapshot(99).is_err());
        assert!(telemetry
            .admit(&one, VyrmTransportOperation::Append, 1, 99)
            .is_err());
        for identity in [&one, &two] {
            telemetry
                .admit(identity, VyrmTransportOperation::Append, 1, 100)
                .unwrap()
                .finish(VyrmTransportOutcome::Allowed, 1)
                .unwrap();
        }
        assert!(telemetry
            .admit(&three, VyrmTransportOperation::Append, 1, 100)
            .unwrap_err()
            .to_string()
            .contains("capacity"));
        let snapshot = telemetry.snapshot(101).unwrap();
        assert_eq!(snapshot.identities.len(), 2);
        assert_eq!(
            snapshot.operations[&VyrmTransportOperation::Append].denied,
            1
        );
    }
}
