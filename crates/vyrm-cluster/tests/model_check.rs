use vyrm_cluster::*;

fn node(value: &str) -> NodeId {
    NodeId::new(value).unwrap()
}

fn placement() -> ShardPlacement {
    ShardPlacement {
        contract_version: CLUSTER_CONTRACT_VERSION,
        cluster: ClusterId::new("model-check").unwrap(),
        shard: ShardId(9),
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

/// Enumerates each possible first follower acknowledgement and each permitted
/// single-disk loss. Every acknowledged value must retain a durable copy.
#[test]
fn enumerates_quorum_paths_cross_single_disk_losses() {
    let mut schedules = 0;
    for first_append in [0_usize, 1] {
        let mut committed = SimCluster::new(1, placement(), node("a")).unwrap();
        let index = committed.propose(b"model-checked-command").unwrap();
        let appends = committed.pending_message_ids();
        committed
            .apply(SimFault::Deliver {
                message_id: appends[first_append],
            })
            .unwrap();
        let acknowledgement = committed.pending_message_ids().into_iter().max().unwrap();
        committed
            .apply(SimFault::Deliver {
                message_id: acknowledgement,
            })
            .unwrap();
        assert!(committed.is_acknowledged(index));

        for failed in ["a", "b", "c"] {
            let mut after_loss = committed.clone();
            after_loss
                .apply(SimFault::DiskLoss { node: node(failed) })
                .unwrap();
            after_loss.verify_safety().unwrap();
            schedules += 1;
        }
    }
    assert_eq!(schedules, 6);
}

/// Enumerates every two-way isolation of the leader. No minority schedule may
/// create an acknowledged entry.
#[test]
fn enumerates_leader_minority_partitions() {
    for isolated_from in [["b", "c"], ["c", "b"]] {
        let mut simulation = SimCluster::new(2, placement(), node("a")).unwrap();
        for peer in isolated_from {
            simulation
                .apply(SimFault::Partition {
                    left: node("a"),
                    right: node(peer),
                })
                .unwrap();
        }
        let index = simulation.propose(b"minority-write").unwrap();
        simulation.deliver_ready().unwrap();
        assert!(!simulation.is_acknowledged(index));
        simulation.verify_safety().unwrap();
    }
}
