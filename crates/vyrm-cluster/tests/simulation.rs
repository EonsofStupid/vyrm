use vyrm_cluster::*;

fn node(value: &str) -> NodeId {
    NodeId::new(value).unwrap()
}

fn placement() -> ShardPlacement {
    ShardPlacement {
        contract_version: 1,
        cluster: ClusterId::new("sim").unwrap(),
        shard: ShardId(1),
        epoch: 1,
        policy: standard_three_zone_policy(),
        replicas: ["a", "b", "c"]
            .into_iter()
            .map(|name| ReplicaPlacement {
                node: node(name),
                zone: ZoneId::new(format!("az-{name}")).unwrap(),
                role: ReplicaRole::Voter,
            })
            .collect(),
    }
}

#[test]
fn quorum_ack_survives_every_single_disk_loss() {
    let mut base = SimCluster::new(41, placement(), node("a")).unwrap();
    let index = base.propose(b"commit-one").unwrap();
    base.deliver_ready().unwrap();
    assert!(base.is_acknowledged(index));

    for failed in ["a", "b", "c"] {
        let mut simulation = base.clone();
        simulation
            .apply(SimFault::DiskLoss { node: node(failed) })
            .unwrap();
        simulation.verify_safety().unwrap();
        assert_eq!(simulation.evidence().unwrap().seed, 41);
    }
}

#[test]
fn partition_denies_ack_and_linearizable_read_without_quorum() {
    let mut simulation = SimCluster::new(7, placement(), node("a")).unwrap();
    simulation
        .apply(SimFault::Partition {
            left: node("a"),
            right: node("b"),
        })
        .unwrap();
    simulation
        .apply(SimFault::Partition {
            left: node("a"),
            right: node("c"),
        })
        .unwrap();
    let index = simulation.propose(b"isolated").unwrap();
    assert_eq!(simulation.deliver_ready().unwrap(), 0);
    assert!(!simulation.is_acknowledged(index));
    assert!(matches!(
        simulation.leader_stamp(),
        Err(ClusterError::Unavailable(_))
    ));
}

#[test]
fn duplicate_delay_reorder_crash_clock_skew_are_deterministic() {
    fn run() -> SimEvidence {
        let mut simulation = SimCluster::new(99, placement(), node("a")).unwrap();
        simulation.propose(b"one").unwrap();
        simulation.propose(b"two").unwrap();
        let ids = simulation.pending_message_ids();
        simulation
            .apply(SimFault::Duplicate { message_id: ids[0] })
            .unwrap();
        simulation
            .apply(SimFault::Delay {
                message_id: ids[3],
                ticks: 5,
            })
            .unwrap();
        simulation
            .apply(SimFault::Reorder {
                message_ids: vec![ids[1], ids[0]],
            })
            .unwrap();
        simulation
            .apply(SimFault::ClockSkew {
                node: node("b"),
                offset_ms: 120_000,
            })
            .unwrap();
        simulation
            .apply(SimFault::Crash { node: node("c") })
            .unwrap();
        simulation.advance(5);
        let _ = simulation.deliver_ready();
        simulation.verify_safety().unwrap();
        simulation.evidence().unwrap()
    }

    assert_eq!(run(), run());
}

#[test]
fn reordering_cannot_skip_a_log_index() {
    let mut simulation = SimCluster::new(5, placement(), node("a")).unwrap();
    simulation.propose(b"one").unwrap();
    simulation.propose(b"two").unwrap();
    let ids = simulation.pending_message_ids();
    let error = simulation
        .apply(SimFault::Deliver { message_id: ids[2] })
        .unwrap_err();
    assert!(matches!(error, ClusterError::Unavailable(_)));
    simulation.verify_safety().unwrap();
}
