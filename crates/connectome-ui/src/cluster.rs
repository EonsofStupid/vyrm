//! Retained, source-bound observations of reset-explicit cluster health.
//!
//! A sample proves what Connectome observed from one validated node status at
//! one runtime cursor. It does not turn process counters into consensus truth.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{Arc, Mutex};
use vyrm_cluster::{VyrmNodeStatus, VyrmTransportOperationMetrics, VyrmTransportTelemetrySnapshot};
use vyrm_core::{
    digest, RuntimeCommit, RuntimeMutation, RuntimeProperties, RuntimePropertySchema,
    RuntimeRecord, RuntimeRecordSchema, RuntimeRef, RuntimeSchemaRegistry, RuntimeType,
    RuntimeValue, RuntimeValueType, ScopeId,
};
use vyrm_node::InstanceBinding;
use vyrm_store::{Engine, Error as StoreError, PersistentEngine};

pub const CLUSTER_TELEMETRY_SAMPLE_VERSION: u16 = 1;
const SAMPLE_TYPE: &str = "cluster_telemetry_sample";
const REPLAY_PAGE: usize = 4_096;
const MAX_HISTORY_LIMIT: usize = 4_096;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordClusterTelemetry {
    pub status: VyrmNodeStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClusterAlertSeverity {
    Info,
    Warning,
    Critical,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClusterAlert {
    pub code: String,
    pub severity: ClusterAlertSeverity,
    pub value: u64,
    pub detail: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClusterTelemetryDelta {
    pub transport_attempted: u64,
    pub transport_allowed: u64,
    pub transport_denied: u64,
    pub transport_failed: u64,
    pub transport_request_bytes: u64,
    pub transport_response_bytes: u64,
    pub transport_duration_micros: u64,
    pub connection_denials: u64,
    pub artifact_completed: u64,
    pub artifact_quota_denials: u64,
    pub artifact_failed: u64,
    pub artifact_gc_reclaimed_bytes: u64,
    pub trace_commit_acknowledgements: u64,
    pub trace_cursor_conflicts: u64,
    pub trace_leader_changes: u64,
    pub trace_leader_unavailable: u64,
    pub trace_denied: u64,
    pub trace_failed: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClusterTelemetrySamplePayload {
    contract_version: u16,
    sequence: u64,
    captured_at: u64,
    node_key: String,
    source_status_digest: String,
    previous_sample_digest: Option<String>,
    process_reset: bool,
    delta: Option<ClusterTelemetryDelta>,
    alerts: Vec<ClusterAlert>,
    status: VyrmNodeStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClusterTelemetrySample {
    pub contract_version: u16,
    pub sequence: u64,
    pub captured_at: u64,
    pub node_key: String,
    pub source_status_digest: String,
    pub previous_sample_digest: Option<String>,
    pub process_reset: bool,
    pub delta: Option<ClusterTelemetryDelta>,
    pub alerts: Vec<ClusterAlert>,
    pub status: VyrmNodeStatus,
    pub digest: String,
}

impl ClusterTelemetrySample {
    fn payload(&self) -> ClusterTelemetrySamplePayload {
        ClusterTelemetrySamplePayload {
            contract_version: self.contract_version,
            sequence: self.sequence,
            captured_at: self.captured_at,
            node_key: self.node_key.clone(),
            source_status_digest: self.source_status_digest.clone(),
            previous_sample_digest: self.previous_sample_digest.clone(),
            process_reset: self.process_reset,
            delta: self.delta.clone(),
            alerts: self.alerts.clone(),
            status: self.status.clone(),
        }
    }

    pub fn validate(&self, expected_project: &ScopeId) -> Result<(), Box<dyn std::error::Error>> {
        self.status.validate()?;
        if self.contract_version != CLUSTER_TELEMETRY_SAMPLE_VERSION
            || &self.status.project_scope != expected_project
            || self.sequence == 0
            || self.node_key != node_key(&self.status)
            || self.source_status_digest != status_digest(&self.status)?
            || self.digest != sample_digest(&self.payload())?
        {
            return Err("cluster telemetry sample identity is invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ClusterTelemetrySampleView {
    pub cursor: u64,
    pub commit_id: String,
    pub audit_digest: Option<String>,
    #[serde(flatten)]
    pub sample: ClusterTelemetrySample,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ClusterNodeView {
    pub node_key: String,
    pub cluster: String,
    pub shard: u64,
    pub raft_node_id: u64,
    pub canonical_node_id: String,
    pub latest_cursor: u64,
    pub latest_sample_digest: String,
    pub observed_at: u64,
    pub state: String,
    pub current_leader: Option<u64>,
    pub last_log_index: Option<u64>,
    pub last_applied_index: Option<u64>,
    pub applied_lag: u64,
    pub process_started_at: u64,
    pub alerts: Vec<ClusterAlert>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ClusterHistoryView {
    pub format: &'static str,
    pub project_scope: String,
    pub runtime_head: u64,
    pub total_samples: usize,
    pub truncated_before_cursor: Option<u64>,
    pub nodes: Vec<ClusterNodeView>,
    pub alerts: Vec<ClusterAlert>,
    /// The last observation per node before the bounded sample window.
    ///
    /// These anchors make topology reconstruction exact at every returned
    /// cursor without pretending the bounded response contains full history.
    pub baseline_samples: Vec<ClusterTelemetrySampleView>,
    pub samples: Vec<ClusterTelemetrySampleView>,
}

pub struct ClusterTelemetryRecorder {
    store: Arc<PersistentEngine>,
    binding: InstanceBinding,
    mutation: Mutex<()>,
}

impl ClusterTelemetryRecorder {
    pub fn new(store: Arc<PersistentEngine>, binding: InstanceBinding) -> Self {
        Self {
            store,
            binding,
            mutation: Mutex::new(()),
        }
    }

    pub fn history(&self, limit: usize) -> Result<ClusterHistoryView, Box<dyn std::error::Error>> {
        cluster_history(self.store.as_ref(), &self.binding, limit)
    }

    pub fn record(
        &self,
        request: RecordClusterTelemetry,
        captured_at: u64,
    ) -> Result<ClusterTelemetrySampleView, Box<dyn std::error::Error>> {
        let _guard = self
            .mutation
            .lock()
            .map_err(|_| "cluster telemetry recorder lock poisoned")?;
        self.binding.require_runtime_ready()?;
        self.binding.verify_store_path(self.store.path())?;
        let project = ScopeId::new(self.binding.manifest.id.clone())?;
        request.status.validate()?;
        if request.status.project_scope != project {
            return Err(
                "cluster status project scope differs from this Connectome instance".into(),
            );
        }

        for attempt in 0..2 {
            let scan = scan_history(self.store.as_ref(), &self.binding, MAX_HISTORY_LIMIT)?;
            let key = node_key(&request.status);
            let previous = scan.latest.get(&key);
            let source_status_digest = status_digest(&request.status)?;
            if let Some(previous) = previous {
                if previous.sample.source_status_digest == source_status_digest {
                    return sample_view(self.store.as_ref(), previous.clone());
                }
                if request.status.telemetry.observed_at
                    <= previous.sample.status.telemetry.observed_at
                {
                    return Err("cluster status observation time did not advance".into());
                }
            }
            let process_reset = previous.is_some_and(|previous| {
                process_coordinates(&previous.sample.status) != process_coordinates(&request.status)
            });
            if let Some(previous) = previous {
                validate_process_progress(&previous.sample.status, &request.status, process_reset)?;
            }
            let delta = previous
                .filter(|_| !process_reset)
                .map(|previous| telemetry_delta(&previous.sample.status, &request.status))
                .transpose()?
                .flatten();
            let alerts = alerts(&request.status, delta.as_ref(), process_reset);
            let payload = ClusterTelemetrySamplePayload {
                contract_version: CLUSTER_TELEMETRY_SAMPLE_VERSION,
                sequence: previous.map_or(Ok(1), |value| {
                    value
                        .sample
                        .sequence
                        .checked_add(1)
                        .ok_or("cluster telemetry sequence overflowed")
                })?,
                captured_at,
                node_key: key,
                source_status_digest,
                previous_sample_digest: previous.map(|value| value.sample.digest.clone()),
                process_reset,
                delta,
                alerts,
                status: request.status.clone(),
            };
            let sample = ClusterTelemetrySample {
                contract_version: payload.contract_version,
                sequence: payload.sequence,
                captured_at: payload.captured_at,
                node_key: payload.node_key.clone(),
                source_status_digest: payload.source_status_digest.clone(),
                previous_sample_digest: payload.previous_sample_digest.clone(),
                process_reset: payload.process_reset,
                delta: payload.delta.clone(),
                alerts: payload.alerts.clone(),
                status: payload.status.clone(),
                digest: sample_digest(&payload)?,
            };
            sample.validate(&project)?;
            let mut mutations = Vec::new();
            if let Some(registry) = sample_schema_update(self.store.as_ref(), &project)? {
                mutations.push(RuntimeMutation::Schema { registry });
            }
            mutations.push(sample_record(&sample)?);
            let commit = RuntimeCommit {
                scope: project.clone(),
                at: captured_at,
                actor: "connectome:cluster-observer".into(),
                expected_cursor: scan.history.runtime_head,
                mutations,
            };
            match self.store.commit_runtime(&commit) {
                Ok(outcome) => {
                    let scanned = ScannedSample {
                        cursor: outcome.last_cursor,
                        commit_id: outcome.commit_id,
                        sample,
                    };
                    return sample_view(self.store.as_ref(), scanned);
                }
                Err(StoreError::RuntimeConflict { .. }) if attempt == 0 => continue,
                Err(error) => return Err(error.into()),
            }
        }
        Err("cluster telemetry commit retry was exhausted".into())
    }
}

pub fn cluster_history(
    store: &PersistentEngine,
    binding: &InstanceBinding,
    limit: usize,
) -> Result<ClusterHistoryView, Box<dyn std::error::Error>> {
    binding.require_runtime_ready()?;
    binding.verify_store_path(store.path())?;
    Ok(scan_history(store, binding, limit)?.history)
}

#[derive(Clone)]
struct ScannedSample {
    cursor: u64,
    commit_id: String,
    sample: ClusterTelemetrySample,
}

struct ClusterScan {
    history: ClusterHistoryView,
    latest: BTreeMap<String, ScannedSample>,
}

fn scan_history(
    store: &PersistentEngine,
    binding: &InstanceBinding,
    limit: usize,
) -> Result<ClusterScan, Box<dyn std::error::Error>> {
    let project = ScopeId::new(binding.manifest.id.clone())?;
    let runtime_head = store.runtime_cursor()?;
    let limit = limit.clamp(1, MAX_HISTORY_LIMIT);
    let mut cursor = 0;
    let mut total_samples = 0usize;
    let mut retained = VecDeque::with_capacity(limit);
    let mut baseline = BTreeMap::<String, ScannedSample>::new();
    let mut latest = BTreeMap::<String, ScannedSample>::new();
    while cursor < runtime_head {
        let page = store.runtime_changes_since(cursor, REPLAY_PAGE, Some(&project))?;
        let through = page.through_cursor;
        let has_more = page.has_more();
        for change in page
            .changes
            .into_iter()
            .filter(|change| change.cursor <= runtime_head)
        {
            let RuntimeMutation::Record { record } = change.mutation else {
                continue;
            };
            if record.reference.kind.as_str() != SAMPLE_TYPE {
                continue;
            }
            let Some(RuntimeValue::String(encoded)) = record.properties.get("sample_json") else {
                return Err(format!(
                    "cluster telemetry record at cursor {} has no sample_json",
                    change.cursor
                )
                .into());
            };
            let sample: ClusterTelemetrySample = serde_json::from_str(encoded)?;
            sample.validate(&project)?;
            if record.reference.id.as_str() != sample.digest {
                return Err(format!(
                    "cluster telemetry record at cursor {} disagrees with its digest",
                    change.cursor
                )
                .into());
            }
            if let Some(previous) = latest.get(&sample.node_key) {
                if sample.sequence != previous.sample.sequence.saturating_add(1)
                    || sample.previous_sample_digest.as_deref()
                        != Some(previous.sample.digest.as_str())
                {
                    return Err(format!(
                        "cluster telemetry chain for {} is discontinuous at cursor {}",
                        sample.node_key, change.cursor
                    )
                    .into());
                }
            } else if sample.sequence != 1 || sample.previous_sample_digest.is_some() {
                return Err(format!(
                    "cluster telemetry chain for {} has no retained origin",
                    sample.node_key
                )
                .into());
            }
            let scanned = ScannedSample {
                cursor: change.cursor,
                commit_id: change.commit_id,
                sample,
            };
            latest.insert(scanned.sample.node_key.clone(), scanned.clone());
            retained.push_back(scanned);
            total_samples = total_samples
                .checked_add(1)
                .ok_or("cluster telemetry sample count overflowed")?;
            if retained.len() > limit {
                if let Some(sample) = retained.pop_front() {
                    baseline.insert(sample.sample.node_key.clone(), sample);
                }
            }
        }
        cursor = through.min(runtime_head);
        if !has_more || cursor >= runtime_head {
            break;
        }
    }
    let mut samples = Vec::with_capacity(retained.len());
    for sample in retained {
        samples.push(sample_view(store, sample)?);
    }
    let mut baseline_samples = Vec::with_capacity(baseline.len());
    for sample in baseline.into_values() {
        baseline_samples.push(sample_view(store, sample)?);
    }
    baseline_samples.sort_by_key(|sample| sample.cursor);
    let mut nodes = latest.values().map(node_view).collect::<Vec<_>>();
    nodes.sort_by(|left, right| {
        left.cluster
            .cmp(&right.cluster)
            .then_with(|| left.shard.cmp(&right.shard))
            .then_with(|| left.raft_node_id.cmp(&right.raft_node_id))
    });
    let alerts = nodes
        .iter()
        .flat_map(|node| node.alerts.clone())
        .collect::<Vec<_>>();
    let truncated_before_cursor = (total_samples > samples.len())
        .then(|| samples.first().map(|sample| sample.cursor))
        .flatten();
    Ok(ClusterScan {
        history: ClusterHistoryView {
            format: "connectome-cluster-history-v1",
            project_scope: project.as_str().to_owned(),
            runtime_head,
            total_samples,
            truncated_before_cursor,
            nodes,
            alerts,
            baseline_samples,
            samples,
        },
        latest,
    })
}

fn sample_view(
    store: &PersistentEngine,
    sample: ScannedSample,
) -> Result<ClusterTelemetrySampleView, Box<dyn std::error::Error>> {
    let audit_digest = store
        .runtime_audit(&sample.commit_id)?
        .map(|audit| audit.digest);
    Ok(ClusterTelemetrySampleView {
        cursor: sample.cursor,
        commit_id: sample.commit_id,
        audit_digest,
        sample: sample.sample,
    })
}

fn node_view(latest: &ScannedSample) -> ClusterNodeView {
    let status = &latest.sample.status;
    ClusterNodeView {
        node_key: latest.sample.node_key.clone(),
        cluster: status.cluster.as_str().to_owned(),
        shard: status.shard.0,
        raft_node_id: status.raft_node_id,
        canonical_node_id: status.canonical_node_id.as_str().to_owned(),
        latest_cursor: latest.cursor,
        latest_sample_digest: latest.sample.digest.clone(),
        observed_at: status.telemetry.observed_at,
        state: status.state.clone(),
        current_leader: status.current_leader,
        last_log_index: status.last_log_index,
        last_applied_index: status.last_applied_index,
        applied_lag: applied_lag(status),
        process_started_at: status.telemetry.transport_ingress.started_at,
        alerts: latest.sample.alerts.clone(),
    }
}

fn sample_record(
    sample: &ClusterTelemetrySample,
) -> Result<RuntimeMutation, Box<dyn std::error::Error>> {
    let mut properties = RuntimeProperties::new();
    properties.insert(
        "sample_json".into(),
        RuntimeValue::String(serde_json::to_string(sample)?),
    );
    properties.insert(
        "sample_digest".into(),
        RuntimeValue::Digest(sample.digest.clone()),
    );
    properties.insert(
        "source_status_digest".into(),
        RuntimeValue::Digest(sample.source_status_digest.clone()),
    );
    properties.insert(
        "node_key".into(),
        RuntimeValue::String(sample.node_key.clone()),
    );
    properties.insert("sequence".into(), RuntimeValue::Unsigned(sample.sequence));
    properties.insert(
        "observed_at".into(),
        RuntimeValue::Unsigned(sample.status.telemetry.observed_at),
    );
    properties.insert(
        "process_reset".into(),
        RuntimeValue::Bool(sample.process_reset),
    );
    Ok(RuntimeMutation::Record {
        record: RuntimeRecord {
            reference: RuntimeRef::new(SAMPLE_TYPE, sample.digest.clone())?,
            valid_from: sample.captured_at,
            valid_to: None,
            properties,
        },
    })
}

fn sample_schema_update(
    store: &PersistentEngine,
    project: &ScopeId,
) -> Result<Option<RuntimeSchemaRegistry>, Box<dyn std::error::Error>> {
    let current = store.runtime_schema(project)?;
    let mut registry = current
        .clone()
        .unwrap_or_else(|| RuntimeSchemaRegistry::empty(1, "bootstrap cluster telemetry schema"));
    let schema = RuntimeRecordSchema {
        properties: BTreeMap::from([
            (
                "sample_json".into(),
                RuntimePropertySchema::required(RuntimeValueType::String),
            ),
            (
                "sample_digest".into(),
                RuntimePropertySchema::required(RuntimeValueType::Digest),
            ),
            (
                "source_status_digest".into(),
                RuntimePropertySchema::required(RuntimeValueType::Digest),
            ),
            (
                "node_key".into(),
                RuntimePropertySchema::required(RuntimeValueType::String),
            ),
            (
                "sequence".into(),
                RuntimePropertySchema::required(RuntimeValueType::Unsigned),
            ),
            (
                "observed_at".into(),
                RuntimePropertySchema::required(RuntimeValueType::Unsigned),
            ),
            (
                "process_reset".into(),
                RuntimePropertySchema::required(RuntimeValueType::Bool),
            ),
        ]),
        unique_properties: BTreeSet::from(["sample_digest".into()]),
        ..RuntimeRecordSchema::default()
    };
    let kind = RuntimeType::new(SAMPLE_TYPE)?;
    if registry.records.get(&kind) == Some(&schema) {
        return Ok(None);
    }
    registry.records.insert(kind, schema);
    if let Some(current) = current {
        registry.revision = current.revision.saturating_add(1);
        registry.migration = "register retained cluster telemetry observations".into();
    }
    Ok(Some(registry))
}

fn node_key(status: &VyrmNodeStatus) -> String {
    format!(
        "{}/{}/{}:{}",
        status.cluster.as_str(),
        status.shard.0,
        status.raft_node_id,
        status.canonical_node_id.as_str()
    )
}

fn status_digest(status: &VyrmNodeStatus) -> Result<String, serde_json::Error> {
    serde_json::to_vec(status).map(|bytes| digest::sha256_hex(&bytes))
}

fn sample_digest(payload: &ClusterTelemetrySamplePayload) -> Result<String, serde_json::Error> {
    serde_json::to_vec(payload).map(|bytes| digest::sha256_hex(&bytes))
}

fn process_coordinates(status: &VyrmNodeStatus) -> (u64, u64, u64) {
    (
        status.telemetry.transport_ingress.started_at,
        status.telemetry.artifacts.started_at,
        status.telemetry.consensus_traces.started_at,
    )
}

fn validate_process_progress(
    previous: &VyrmNodeStatus,
    current: &VyrmNodeStatus,
    process_reset: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let previous_coordinates = process_coordinates(previous);
    let current_coordinates = process_coordinates(current);
    if current_coordinates.0 < previous_coordinates.0
        || current_coordinates.1 < previous_coordinates.1
        || current_coordinates.2 < previous_coordinates.2
    {
        return Err("cluster telemetry process coordinates regressed".into());
    }
    if process_reset {
        return Ok(());
    }
    ensure_transport_monotonic(
        &previous.telemetry.transport_ingress,
        &current.telemetry.transport_ingress,
    )?;
    let prior_artifacts = &previous.telemetry.artifacts;
    let next_artifacts = &current.telemetry.artifacts;
    ensure_monotonic(
        &[
            prior_artifacts.begin_requests,
            prior_artifacts.chunk_requests,
            prior_artifacts.complete_requests,
            prior_artifacts.begin_responses,
            prior_artifacts.accepted_chunks,
            prior_artifacts.completed_responses,
            prior_artifacts.completed_receipt_replays,
            prior_artifacts.denied,
            prior_artifacts.failed,
            prior_artifacts.quota_denials,
            prior_artifacts.gc_runs,
            prior_artifacts.gc_removed_incomplete,
            prior_artifacts.gc_removed_completed,
            prior_artifacts.gc_reclaimed_partial_bytes,
        ],
        &[
            next_artifacts.begin_requests,
            next_artifacts.chunk_requests,
            next_artifacts.complete_requests,
            next_artifacts.begin_responses,
            next_artifacts.accepted_chunks,
            next_artifacts.completed_responses,
            next_artifacts.completed_receipt_replays,
            next_artifacts.denied,
            next_artifacts.failed,
            next_artifacts.quota_denials,
            next_artifacts.gc_runs,
            next_artifacts.gc_removed_incomplete,
            next_artifacts.gc_removed_completed,
            next_artifacts.gc_reclaimed_partial_bytes,
        ],
        "artifact telemetry",
    )?;
    let prior_traces = &previous.telemetry.consensus_traces;
    let next_traces = &current.telemetry.consensus_traces;
    ensure_monotonic(
        &[
            prior_traces.prepared_observations,
            prior_traces.chunk_observations,
            prior_traces.completed_observations,
            prior_traces.failed_observations,
            prior_traces.commit_acknowledgements,
            prior_traces.cursor_conflicts,
            prior_traces.leader_changes,
            prior_traces.leader_unavailable,
            prior_traces.denied,
            prior_traces.failed,
        ],
        &[
            next_traces.prepared_observations,
            next_traces.chunk_observations,
            next_traces.completed_observations,
            next_traces.failed_observations,
            next_traces.commit_acknowledgements,
            next_traces.cursor_conflicts,
            next_traces.leader_changes,
            next_traces.leader_unavailable,
            next_traces.denied,
            next_traces.failed,
        ],
        "consensus trace telemetry",
    )
}

fn ensure_transport_monotonic(
    previous: &VyrmTransportTelemetrySnapshot,
    current: &VyrmTransportTelemetrySnapshot,
) -> Result<(), Box<dyn std::error::Error>> {
    ensure_monotonic(
        &[
            previous.accepted_connections,
            previous.denied_connections,
            previous.connection_request_bytes,
        ],
        &[
            current.accepted_connections,
            current.denied_connections,
            current.connection_request_bytes,
        ],
        "transport connection telemetry",
    )?;
    for operation in vyrm_cluster::VyrmTransportOperation::ALL {
        ensure_operation_monotonic(
            &previous.operations[&operation],
            &current.operations[&operation],
        )?;
    }
    Ok(())
}

fn ensure_operation_monotonic(
    previous: &VyrmTransportOperationMetrics,
    current: &VyrmTransportOperationMetrics,
) -> Result<(), Box<dyn std::error::Error>> {
    ensure_monotonic(
        &[
            previous.attempted,
            previous.allowed,
            previous.denied,
            previous.failed,
            previous.request_bytes,
            previous.response_bytes,
            previous.total_duration_micros,
            previous.max_duration_micros,
            previous.peak_in_flight,
        ],
        &[
            current.attempted,
            current.allowed,
            current.denied,
            current.failed,
            current.request_bytes,
            current.response_bytes,
            current.total_duration_micros,
            current.max_duration_micros,
            current.peak_in_flight,
        ],
        "transport operation telemetry",
    )
}

fn ensure_monotonic(
    previous: &[u64],
    current: &[u64],
    label: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if previous.len() != current.len()
        || previous
            .iter()
            .zip(current)
            .any(|(previous, current)| current < previous)
    {
        return Err(format!("{label} regressed without a process reset").into());
    }
    Ok(())
}

fn telemetry_delta(
    previous: &VyrmNodeStatus,
    current: &VyrmNodeStatus,
) -> Result<Option<ClusterTelemetryDelta>, Box<dyn std::error::Error>> {
    if previous.telemetry.transport_ingress.overflowed
        || current.telemetry.transport_ingress.overflowed
        || previous.telemetry.artifacts.overflowed
        || current.telemetry.artifacts.overflowed
        || previous.telemetry.consensus_traces.overflowed
        || current.telemetry.consensus_traces.overflowed
    {
        return Ok(None);
    }
    let prior_transport = aggregate_transport(&previous.telemetry.transport_ingress);
    let next_transport = aggregate_transport(&current.telemetry.transport_ingress);
    let prior_artifacts = &previous.telemetry.artifacts;
    let next_artifacts = &current.telemetry.artifacts;
    let prior_traces = &previous.telemetry.consensus_traces;
    let next_traces = &current.telemetry.consensus_traces;
    Ok(Some(ClusterTelemetryDelta {
        transport_attempted: next_transport.attempted - prior_transport.attempted,
        transport_allowed: next_transport.allowed - prior_transport.allowed,
        transport_denied: next_transport.denied - prior_transport.denied,
        transport_failed: next_transport.failed - prior_transport.failed,
        transport_request_bytes: next_transport.request_bytes - prior_transport.request_bytes,
        transport_response_bytes: next_transport.response_bytes - prior_transport.response_bytes,
        transport_duration_micros: next_transport.total_duration_micros
            - prior_transport.total_duration_micros,
        connection_denials: current.telemetry.transport_ingress.denied_connections
            - previous.telemetry.transport_ingress.denied_connections,
        artifact_completed: next_artifacts.completed_responses
            - prior_artifacts.completed_responses,
        artifact_quota_denials: next_artifacts.quota_denials - prior_artifacts.quota_denials,
        artifact_failed: next_artifacts.failed - prior_artifacts.failed,
        artifact_gc_reclaimed_bytes: next_artifacts.gc_reclaimed_partial_bytes
            - prior_artifacts.gc_reclaimed_partial_bytes,
        trace_commit_acknowledgements: next_traces.commit_acknowledgements
            - prior_traces.commit_acknowledgements,
        trace_cursor_conflicts: next_traces.cursor_conflicts - prior_traces.cursor_conflicts,
        trace_leader_changes: next_traces.leader_changes - prior_traces.leader_changes,
        trace_leader_unavailable: next_traces.leader_unavailable - prior_traces.leader_unavailable,
        trace_denied: next_traces.denied - prior_traces.denied,
        trace_failed: next_traces.failed - prior_traces.failed,
    }))
}

fn aggregate_transport(snapshot: &VyrmTransportTelemetrySnapshot) -> VyrmTransportOperationMetrics {
    snapshot.operations.values().fold(
        VyrmTransportOperationMetrics::default(),
        |mut total, operation| {
            total.attempted = total.attempted.saturating_add(operation.attempted);
            total.allowed = total.allowed.saturating_add(operation.allowed);
            total.denied = total.denied.saturating_add(operation.denied);
            total.failed = total.failed.saturating_add(operation.failed);
            total.request_bytes = total.request_bytes.saturating_add(operation.request_bytes);
            total.response_bytes = total
                .response_bytes
                .saturating_add(operation.response_bytes);
            total.total_duration_micros = total
                .total_duration_micros
                .saturating_add(operation.total_duration_micros);
            total
        },
    )
}

fn alerts(
    status: &VyrmNodeStatus,
    delta: Option<&ClusterTelemetryDelta>,
    process_reset: bool,
) -> Vec<ClusterAlert> {
    let mut alerts = Vec::new();
    if status.telemetry.transport_ingress.overflowed
        || status.telemetry.artifacts.overflowed
        || status.telemetry.consensus_traces.overflowed
    {
        alerts.push(alert(
            "telemetry_overflow",
            ClusterAlertSeverity::Critical,
            1,
            "at least one process counter saturated; deltas are unavailable",
        ));
    }
    if status.current_leader.is_none() {
        alerts.push(alert(
            "leader_unavailable",
            ClusterAlertSeverity::Warning,
            1,
            "this node did not report a current Raft leader",
        ));
    }
    let lag = applied_lag(status);
    if lag > 0 {
        alerts.push(alert(
            "apply_lag",
            ClusterAlertSeverity::Warning,
            lag,
            "last log index is ahead of the applied state-machine index",
        ));
    }
    if process_reset {
        alerts.push(alert(
            "process_reset",
            ClusterAlertSeverity::Info,
            1,
            "process start coordinates advanced; cumulative counters restarted",
        ));
    }
    if let Some(delta) = delta {
        for (code, severity, value, detail) in [
            (
                "transport_denied",
                ClusterAlertSeverity::Warning,
                delta.transport_denied,
                "authenticated operations were denied since the prior sample",
            ),
            (
                "transport_failed",
                ClusterAlertSeverity::Warning,
                delta.transport_failed,
                "transport operations failed since the prior sample",
            ),
            (
                "connection_denied",
                ClusterAlertSeverity::Info,
                delta.connection_denials,
                "connections were rejected before or during authenticated admission",
            ),
            (
                "artifact_quota_denied",
                ClusterAlertSeverity::Warning,
                delta.artifact_quota_denials,
                "artifact sessions reached a configured receiver quota",
            ),
            (
                "artifact_failed",
                ClusterAlertSeverity::Warning,
                delta.artifact_failed,
                "artifact receiver operations failed since the prior sample",
            ),
            (
                "trace_commit_failed",
                ClusterAlertSeverity::Warning,
                delta.trace_failed,
                "consensus trace commits failed since the prior sample",
            ),
            (
                "trace_commit_denied",
                ClusterAlertSeverity::Warning,
                delta.trace_denied,
                "consensus trace commits were denied since the prior sample",
            ),
            (
                "trace_leader_changed",
                ClusterAlertSeverity::Info,
                delta.trace_leader_changes,
                "trace routing observed a leader change",
            ),
        ] {
            if value > 0 {
                alerts.push(alert(code, severity, value, detail));
            }
        }
    }
    alerts
}

fn alert(code: &str, severity: ClusterAlertSeverity, value: u64, detail: &str) -> ClusterAlert {
    ClusterAlert {
        code: code.into(),
        severity,
        value,
        detail: detail.into(),
    }
}

fn applied_lag(status: &VyrmNodeStatus) -> u64 {
    status
        .last_log_index
        .unwrap_or(0)
        .saturating_sub(status.last_applied_index.unwrap_or(0))
}
