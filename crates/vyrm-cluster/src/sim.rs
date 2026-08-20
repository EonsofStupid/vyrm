use crate::{
    ClusterError, NodeId, PlacementPolicy, ReplicaRole, Result, ShardPlacement, ShardReadStamp,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use vyrm_core::digest::sha256_hex;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogEntry {
    pub term: u64,
    pub index: u64,
    pub command_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum MessageKind {
    Append(LogEntry),
    Ack { term: u64, index: u64 },
    Commit { term: u64, index: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Message {
    id: u64,
    from: NodeId,
    to: NodeId,
    deliver_at: u64,
    kind: MessageKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReplicaState {
    online: bool,
    disk_present: bool,
    clock_offset_ms: i64,
    log: BTreeMap<u64, LogEntry>,
    commit_index: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SimFault {
    Partition { left: NodeId, right: NodeId },
    Heal { left: NodeId, right: NodeId },
    Delay { message_id: u64, ticks: u64 },
    Duplicate { message_id: u64 },
    Deliver { message_id: u64 },
    Reorder { message_ids: Vec<u64> },
    Crash { node: NodeId },
    Restart { node: NodeId },
    ClockSkew { node: NodeId, offset_ms: i64 },
    DiskLoss { node: NodeId },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimEvidence {
    pub seed: u64,
    pub now: u64,
    pub leader: NodeId,
    pub placement_digest: String,
    pub acknowledged: BTreeMap<u64, String>,
    pub replica_logs: BTreeMap<NodeId, Vec<LogEntry>>,
    pub replica_commit_indexes: BTreeMap<NodeId, u64>,
    pub pending_message_ids: Vec<u64>,
    pub disk_losses: BTreeSet<NodeId>,
    pub trace: Vec<String>,
}

/// Single-term, per-shard quorum simulator. It models durable append,
/// acknowledgement, commit propagation, and fault scheduling. Election and
/// reconfiguration are intentionally out of scope for this first model gate.
#[derive(Debug, Clone)]
pub struct SimCluster {
    seed: u64,
    placement: ShardPlacement,
    leader: NodeId,
    now: u64,
    next_message_id: u64,
    replicas: BTreeMap<NodeId, ReplicaState>,
    blocked: BTreeSet<(NodeId, NodeId)>,
    pending: Vec<Message>,
    acknowledgements: BTreeMap<u64, BTreeSet<NodeId>>,
    acknowledged: BTreeMap<u64, String>,
    disk_losses: BTreeSet<NodeId>,
    trace: Vec<String>,
}

impl SimCluster {
    pub fn new(seed: u64, placement: ShardPlacement, leader: NodeId) -> Result<Self> {
        placement.validate()?;
        let leader_replica = placement
            .replicas
            .iter()
            .find(|replica| replica.node == leader)
            .ok_or_else(|| ClusterError::Invalid("leader is absent from placement".into()))?;
        if leader_replica.role != ReplicaRole::Voter {
            return Err(ClusterError::Invalid("leader must be a voter".into()));
        }
        let replicas = placement
            .replicas
            .iter()
            .map(|replica| {
                (
                    replica.node.clone(),
                    ReplicaState {
                        online: true,
                        disk_present: true,
                        clock_offset_ms: 0,
                        log: BTreeMap::new(),
                        commit_index: 0,
                    },
                )
            })
            .collect();
        Ok(Self {
            seed,
            placement,
            leader,
            now: 0,
            next_message_id: 1,
            replicas,
            blocked: BTreeSet::new(),
            pending: Vec::new(),
            acknowledgements: BTreeMap::new(),
            acknowledged: BTreeMap::new(),
            disk_losses: BTreeSet::new(),
            trace: Vec::new(),
        })
    }

    pub fn propose(&mut self, command: &[u8]) -> Result<u64> {
        let leader = self.replica(&self.leader)?;
        if !leader.online || !leader.disk_present {
            return Err(ClusterError::Unavailable(
                "leader cannot durably accept a proposal".into(),
            ));
        }
        let index = leader.log.keys().next_back().copied().unwrap_or(0) + 1;
        let entry = LogEntry {
            term: 1,
            index,
            command_digest: sha256_hex(command),
        };
        self.replica_mut(&self.leader.clone())?
            .log
            .insert(index, entry.clone());
        self.acknowledgements
            .entry(index)
            .or_default()
            .insert(self.leader.clone());
        let followers: Vec<_> = self
            .placement
            .voters()
            .filter(|replica| replica.node != self.leader)
            .map(|replica| replica.node.clone())
            .collect();
        for follower in followers {
            self.enqueue(
                self.leader.clone(),
                follower,
                MessageKind::Append(entry.clone()),
            );
        }
        self.trace.push(format!("propose:{index}"));
        self.maybe_commit(index)?;
        Ok(index)
    }

    pub fn apply(&mut self, fault: SimFault) -> Result<()> {
        match fault {
            SimFault::Partition { left, right } => {
                self.require_node(&left)?;
                self.require_node(&right)?;
                self.blocked.insert((left.clone(), right.clone()));
                self.blocked.insert((right.clone(), left.clone()));
                self.trace.push(format!("partition:{left}:{right}"));
            }
            SimFault::Heal { left, right } => {
                self.blocked.remove(&(left.clone(), right.clone()));
                self.blocked.remove(&(right.clone(), left.clone()));
                self.trace.push(format!("heal:{left}:{right}"));
            }
            SimFault::Delay { message_id, ticks } => {
                let message = self.message_mut(message_id)?;
                message.deliver_at = message.deliver_at.saturating_add(ticks);
                self.trace.push(format!("delay:{message_id}:{ticks}"));
            }
            SimFault::Duplicate { message_id } => {
                let mut duplicate = self.message(message_id)?.clone();
                duplicate.id = self.take_message_id();
                let duplicate_id = duplicate.id;
                self.pending.push(duplicate);
                self.trace
                    .push(format!("duplicate:{message_id}:{duplicate_id}"));
            }
            SimFault::Deliver { message_id } => self.deliver(message_id)?,
            SimFault::Reorder { message_ids } => {
                for message_id in message_ids {
                    self.deliver(message_id)?;
                }
            }
            SimFault::Crash { node } => {
                self.replica_mut(&node)?.online = false;
                self.trace.push(format!("crash:{node}"));
            }
            SimFault::Restart { node } => {
                let replica = self.replica_mut(&node)?;
                if !replica.disk_present {
                    return Err(ClusterError::Unavailable(format!(
                        "node {node} cannot restart without a replacement disk and transfer"
                    )));
                }
                replica.online = true;
                self.trace.push(format!("restart:{node}"));
            }
            SimFault::ClockSkew { node, offset_ms } => {
                self.replica_mut(&node)?.clock_offset_ms = offset_ms;
                self.trace.push(format!("clock_skew:{node}:{offset_ms}"));
            }
            SimFault::DiskLoss { node } => {
                let replica = self.replica_mut(&node)?;
                replica.online = false;
                replica.disk_present = false;
                replica.log.clear();
                replica.commit_index = 0;
                self.disk_losses.insert(node.clone());
                self.trace.push(format!("disk_loss:{node}"));
            }
        }
        self.verify_safety()
    }

    pub fn advance(&mut self, ticks: u64) {
        self.now = self.now.saturating_add(ticks);
        self.trace.push(format!("advance:{ticks}"));
    }

    pub fn deliver_ready(&mut self) -> Result<usize> {
        let mut delivered = 0;
        loop {
            let next = self
                .pending
                .iter()
                .filter(|message| message.deliver_at <= self.now && self.can_deliver(message))
                .map(|message| message.id)
                .min();
            let Some(message_id) = next else {
                break;
            };
            self.deliver(message_id)?;
            delivered += 1;
        }
        Ok(delivered)
    }

    pub fn pending_message_ids(&self) -> Vec<u64> {
        let mut ids: Vec<_> = self.pending.iter().map(|message| message.id).collect();
        ids.sort_unstable();
        ids
    }

    pub fn is_acknowledged(&self, index: u64) -> bool {
        self.acknowledged.contains_key(&index)
    }

    pub fn verify_safety(&self) -> Result<()> {
        for replica in self.replicas.values() {
            if replica.commit_index > 0 && !replica.log.contains_key(&replica.commit_index) {
                return Err(ClusterError::Denied(
                    "replica commit index advances beyond its durable log".into(),
                ));
            }
        }
        let indexes: BTreeSet<_> = self
            .replicas
            .values()
            .flat_map(|replica| replica.log.keys().copied())
            .collect();
        for index in indexes {
            let digests: BTreeSet<_> = self
                .replicas
                .values()
                .filter_map(|replica| replica.log.get(&index))
                .map(|entry| (&entry.term, &entry.command_digest))
                .collect();
            if digests.len() > 1 {
                return Err(ClusterError::Denied(format!(
                    "replicas diverged at log index {index}"
                )));
            }
        }
        let tolerance = self.placement.policy.tolerated_failures();
        for (index, digest) in &self.acknowledged {
            let durable = self
                .replicas
                .values()
                .filter_map(|replica| replica.log.get(index))
                .filter(|entry| &entry.command_digest == digest)
                .count();
            if self.disk_losses.len() <= tolerance && durable == 0 {
                return Err(ClusterError::Denied(format!(
                    "acknowledged entry {index} was lost within configured fault tolerance"
                )));
            }
        }
        Ok(())
    }

    pub fn evidence(&self) -> Result<SimEvidence> {
        self.verify_safety()?;
        Ok(SimEvidence {
            seed: self.seed,
            now: self.now,
            leader: self.leader.clone(),
            placement_digest: self.placement.digest()?,
            acknowledged: self.acknowledged.clone(),
            replica_logs: self
                .replicas
                .iter()
                .map(|(node, replica)| (node.clone(), replica.log.values().cloned().collect()))
                .collect(),
            replica_commit_indexes: self
                .replicas
                .iter()
                .map(|(node, replica)| (node.clone(), replica.commit_index))
                .collect(),
            pending_message_ids: self.pending_message_ids(),
            disk_losses: self.disk_losses.clone(),
            trace: self.trace.clone(),
        })
    }

    pub fn leader_stamp(&self) -> Result<ShardReadStamp> {
        let leader = self.replica(&self.leader)?;
        if !leader.online || !leader.disk_present || self.reachable_voters() < self.quorum() {
            return Err(ClusterError::Unavailable(
                "linearizable read requires an online leader connected to quorum".into(),
            ));
        }
        let digest = state_digest(&leader.log, leader.commit_index);
        Ok(ShardReadStamp {
            term: 1,
            commit_index: leader.commit_index,
            placement_epoch: self.placement.epoch,
            state_digest: digest,
        })
    }

    fn maybe_commit(&mut self, index: u64) -> Result<()> {
        if self.is_acknowledged(index)
            || self.acknowledgements.get(&index).map_or(0, BTreeSet::len) < self.quorum()
        {
            return Ok(());
        }
        let entry = self
            .replica(&self.leader)?
            .log
            .get(&index)
            .cloned()
            .ok_or_else(|| ClusterError::Denied("leader lost proposed entry".into()))?;
        self.acknowledged
            .insert(index, entry.command_digest.clone());
        self.replica_mut(&self.leader.clone())?.commit_index = index;
        let followers: Vec<_> = self
            .placement
            .voters()
            .filter(|replica| replica.node != self.leader)
            .map(|replica| replica.node.clone())
            .collect();
        for follower in followers {
            self.enqueue(
                self.leader.clone(),
                follower,
                MessageKind::Commit {
                    term: entry.term,
                    index,
                },
            );
        }
        self.trace.push(format!("acknowledged:{index}"));
        Ok(())
    }

    fn deliver(&mut self, message_id: u64) -> Result<()> {
        let position = self
            .pending
            .iter()
            .position(|message| message.id == message_id)
            .ok_or_else(|| ClusterError::NotFound(format!("message {message_id}")))?;
        if !self.can_deliver(&self.pending[position]) {
            return Err(ClusterError::Unavailable(format!(
                "message {message_id} is delayed, partitioned, or targets an offline node"
            )));
        }
        let message = self.pending.remove(position);
        match message.kind {
            MessageKind::Append(entry) => {
                let target = self.replica_mut(&message.to)?;
                if entry.index > 1 && !target.log.contains_key(&(entry.index - 1)) {
                    return Err(ClusterError::Unavailable(format!(
                        "replica {} rejected out-of-order index {}",
                        message.to, entry.index
                    )));
                }
                if let Some(existing) = target.log.get(&entry.index) {
                    if existing != &entry {
                        return Err(ClusterError::Denied(format!(
                            "conflicting duplicate at index {}",
                            entry.index
                        )));
                    }
                } else {
                    target.log.insert(entry.index, entry.clone());
                }
                self.enqueue(
                    message.to,
                    message.from,
                    MessageKind::Ack {
                        term: entry.term,
                        index: entry.index,
                    },
                );
            }
            MessageKind::Ack { term, index } => {
                if term != 1 || message.to != self.leader {
                    return Err(ClusterError::Denied("invalid acknowledgement route".into()));
                }
                self.acknowledgements
                    .entry(index)
                    .or_default()
                    .insert(message.from);
                self.maybe_commit(index)?;
            }
            MessageKind::Commit { term, index } => {
                if term != 1 || message.from != self.leader {
                    return Err(ClusterError::Denied("invalid commit route".into()));
                }
                let target = self.replica_mut(&message.to)?;
                if !target.log.contains_key(&index) {
                    return Err(ClusterError::Unavailable(format!(
                        "replica {} cannot commit absent index {index}",
                        message.to
                    )));
                }
                target.commit_index = target.commit_index.max(index);
            }
        }
        self.trace.push(format!("deliver:{message_id}"));
        self.verify_safety()
    }

    fn enqueue(&mut self, from: NodeId, to: NodeId, kind: MessageKind) {
        let id = self.take_message_id();
        self.pending.push(Message {
            id,
            from,
            to,
            deliver_at: self.now,
            kind,
        });
    }

    fn take_message_id(&mut self) -> u64 {
        let id = self.next_message_id;
        self.next_message_id = self.next_message_id.saturating_add(1);
        id
    }

    fn can_deliver(&self, message: &Message) -> bool {
        message.deliver_at <= self.now
            && self
                .replicas
                .get(&message.from)
                .is_some_and(|replica| replica.online && replica.disk_present)
            && self
                .replicas
                .get(&message.to)
                .is_some_and(|replica| replica.online && replica.disk_present)
            && !self
                .blocked
                .contains(&(message.from.clone(), message.to.clone()))
    }

    fn reachable_voters(&self) -> usize {
        self.placement
            .voters()
            .filter(|replica| {
                let node = &replica.node;
                self.replicas
                    .get(node)
                    .is_some_and(|state| state.online && state.disk_present)
                    && (node == &self.leader
                        || !self.blocked.contains(&(self.leader.clone(), node.clone())))
            })
            .count()
    }

    fn quorum(&self) -> usize {
        self.placement.policy.quorum()
    }

    fn require_node(&self, node: &NodeId) -> Result<()> {
        self.replica(node).map(|_| ())
    }

    fn replica(&self, node: &NodeId) -> Result<&ReplicaState> {
        self.replicas
            .get(node)
            .ok_or_else(|| ClusterError::NotFound(format!("node {node}")))
    }

    fn replica_mut(&mut self, node: &NodeId) -> Result<&mut ReplicaState> {
        self.replicas
            .get_mut(node)
            .ok_or_else(|| ClusterError::NotFound(format!("node {node}")))
    }

    fn message(&self, id: u64) -> Result<&Message> {
        self.pending
            .iter()
            .find(|message| message.id == id)
            .ok_or_else(|| ClusterError::NotFound(format!("message {id}")))
    }

    fn message_mut(&mut self, id: u64) -> Result<&mut Message> {
        self.pending
            .iter_mut()
            .find(|message| message.id == id)
            .ok_or_else(|| ClusterError::NotFound(format!("message {id}")))
    }
}

fn state_digest(log: &BTreeMap<u64, LogEntry>, through: u64) -> String {
    let mut bytes = b"vyrm.cluster.state.v1".to_vec();
    for entry in log.values().filter(|entry| entry.index <= through) {
        bytes.extend_from_slice(&entry.term.to_be_bytes());
        bytes.extend_from_slice(&entry.index.to_be_bytes());
        bytes.extend_from_slice(entry.command_digest.as_bytes());
    }
    sha256_hex(&bytes)
}

pub fn standard_three_zone_policy() -> PlacementPolicy {
    PlacementPolicy {
        voter_count: 3,
        minimum_voter_zones: 3,
        maximum_voters_per_zone: 1,
    }
}
