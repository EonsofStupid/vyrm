use super::state::ReplayState;
use rrd_contract::{
    LifecycleEventCommandV1, LifecycleEventEnvelopeV1, LifecycleEventTypeV1,
    LifecycleSessionSnapshotV1, LIFECYCLE_SPEC_VERSION,
};
use rrd_core::digest;

const MAX_SESSION_EVENTS: usize = 100_000;

#[derive(Debug, Clone)]
pub(super) struct LifecycleAggregate {
    pub state: ReplayState,
    pub events: Vec<LifecycleEventEnvelopeV1>,
}

impl LifecycleAggregate {
    pub fn empty(project_id: &str, session_id: &str) -> Self {
        Self {
            state: ReplayState::empty(project_id, session_id),
            events: Vec::new(),
        }
    }

    pub fn append_replayed(
        &mut self,
        event: LifecycleEventEnvelopeV1,
    ) -> Result<(), Box<dyn std::error::Error>> {
        event.verify()?;
        if self.events.len() >= MAX_SESSION_EVENTS {
            return Err("lifecycle session exceeds the bounded event limit".into());
        }
        let previous = self.events.last();
        let expected_sequence = self.events.len() as u64 + 1;
        let expected_state = (!self.events.is_empty())
            .then(|| self.state.digest())
            .transpose()?;
        if event.sequence != expected_sequence
            || event.previous_event_sha256.as_deref()
                != previous.map(|event| event.event_sha256.as_str())
            || event.causation_id.as_deref() != previous.map(|event| event.event_id.as_str())
            || event.previous_state_sha256 != expected_state
            || previous.is_some_and(|prior| {
                event.occurred_at_unix_ms < prior.occurred_at_unix_ms
                    || event.recorded_at_unix_ms < prior.recorded_at_unix_ms
            })
        {
            return Err("lifecycle event sequence, time, or hash chain is inconsistent".into());
        }
        self.state.apply(&event)?;
        self.events.push(event);
        Ok(())
    }

    pub fn seal_next(
        &self,
        command: LifecycleEventCommandV1,
        recorded_at_unix_ms: u64,
    ) -> Result<LifecycleEventEnvelopeV1, Box<dyn std::error::Error>> {
        command.validate()?;
        let sequence = self.events.len() as u64 + 1;
        let command_sha256 = digest::sha256_hex(&serde_json::to_vec(&command)?);
        let previous = self.events.last();
        let mut event = LifecycleEventEnvelopeV1 {
            spec_version: LIFECYCLE_SPEC_VERSION,
            sequence,
            event_id: format!("lifecycle-{sequence}-{}", &command_sha256[..16]),
            event_type: command.event_type,
            occurred_at_unix_ms: command.occurred_at_unix_ms,
            recorded_at_unix_ms,
            instance_id: command.instance_id,
            project_id: command.project_id,
            member_id: command.member_id,
            session_id: command.session_id,
            turn_id: command.turn_id,
            reasoning_run_id: command.reasoning_run_id,
            attempt_id: command.attempt_id,
            tool_call_id: command.tool_call_id,
            correlation_id: command.correlation_id,
            causation_id: previous.map(|event| event.event_id.clone()),
            actor: command.actor,
            adapter_kind: command.adapter_kind,
            adapter_version: command.adapter_version,
            enforcement_level: command.enforcement_level,
            scope: command.scope,
            payload: command.payload,
            payload_sha256: String::new(),
            previous_state_sha256: (!self.events.is_empty())
                .then(|| self.state.digest())
                .transpose()?,
            read_stamp: command.read_stamp,
            trace: command.trace,
            previous_event_sha256: previous.map(|event| event.event_sha256.clone()),
            event_sha256: String::new(),
        };
        event.seal()?;
        Ok(event)
    }

    pub fn snapshot(&self) -> Result<LifecycleSessionSnapshotV1, Box<dyn std::error::Error>> {
        let phase = self
            .state
            .phase
            .ok_or("lifecycle session has no opening event")?;
        let last = self
            .events
            .last()
            .ok_or("lifecycle session has no events")?;
        Ok(LifecycleSessionSnapshotV1 {
            spec_version: LIFECYCLE_SPEC_VERSION,
            instance_id: self
                .state
                .instance_id
                .clone()
                .ok_or("lifecycle session has no instance identity")?,
            project_id: self.state.project_id.clone(),
            member_id: self.state.member_id.clone(),
            session_id: self.state.session_id.clone(),
            phase,
            active_turn_id: self.state.active_turn_id.clone(),
            active_tool_call_id: self
                .state
                .active_tool
                .as_ref()
                .map(|tool| tool.tool_call_id.clone()),
            event_count: self.state.sequence,
            latest_event: last.clone(),
            state_sha256: self.state.digest()?,
        })
    }
}

pub(super) fn is_opening_event(event: &LifecycleEventEnvelopeV1) -> bool {
    event.sequence == 1 && event.event_type == LifecycleEventTypeV1::SessionOpened
}
