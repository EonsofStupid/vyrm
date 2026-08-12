//! Group-commit writer behaviour. `SPEC.md` §8.1.

use std::sync::Arc;
use std::time::{Duration, Instant};
use vyrm_core::{Claim, ClaimReader, Predicate, Producer, Subject};
use vyrm_store::{Store, Writer, WriterConfig};

fn claim(i: usize) -> Claim {
    Claim::new(
        Subject::new(format!("s{i}")).unwrap(),
        Predicate::new("status").unwrap(),
        format!("v{i}"),
        100 + i as u64,
        100 + i as u64,
        Producer { actor: "test".into(), on_behalf_of: None, session: None },
    )
}

fn store() -> (tempfile::TempDir, Arc<Store>) {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open(dir.path()).unwrap());
    (dir, store)
}

#[test]
fn flush_makes_every_prior_submission_readable() {
    let (_dir, store) = store();
    let writer = Writer::spawn(Arc::clone(&store), WriterConfig::default());
    for i in 0..250 {
        writer.submit(claim(i)).unwrap();
    }
    writer.flush().unwrap();

    assert_eq!(store.sequence().unwrap(), 250);
    for i in [0usize, 1, 124, 249] {
        let subject = Subject::new(format!("s{i}")).unwrap();
        let predicate = Predicate::new("status").unwrap();
        assert_eq!(
            store.as_of(&subject, &predicate, 1_000).unwrap().map(|c| c.object),
            Some(format!("v{i}"))
        );
    }
}

#[test]
fn flush_on_an_empty_queue_returns_without_committing() {
    let (_dir, store) = store();
    let writer = Writer::spawn(Arc::clone(&store), WriterConfig::default());
    writer.flush().unwrap();
    writer.flush().unwrap();
    assert_eq!(store.sequence().unwrap(), 0);
    assert_eq!(writer.stats().batches_committed, 0);
}

#[test]
fn elapsed_delay_commits_without_an_explicit_flush() {
    let (_dir, store) = store();
    let writer = Writer::spawn(
        Arc::clone(&store),
        WriterConfig { flush_delay: Duration::from_millis(20), ..Default::default() },
    );
    writer.submit(claim(0)).unwrap();

    let deadline = Instant::now() + Duration::from_secs(5);
    while store.sequence().unwrap() == 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        store.sequence().unwrap(),
        1,
        "interval-triggered commit did not occur"
    );
}

#[test]
fn a_lone_claim_reaches_durability_within_a_bound_set_by_the_delay() {
    // The property that makes flush_delay meaningful for sparse arrival: a
    // single claim with no follow-up traffic must not wait indefinitely for a
    // batch that never fills. The assertion carries wide headroom over the
    // 20 ms interval so that it verifies the timer without depending on
    // scheduler precision.
    let (_dir, store) = store();
    let writer = Writer::spawn(
        Arc::clone(&store),
        WriterConfig {
            flush_delay: Duration::from_millis(20),
            max_batch: 512,
            queue_capacity: 4096,
        },
    );

    let submitted = Instant::now();
    writer.submit(claim(0)).unwrap();
    while writer.durable_through() < 1 {
        assert!(
            submitted.elapsed() < Duration::from_millis(500),
            "a lone claim did not reach durability within 500 ms under a 20 ms interval"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(store.sequence().unwrap(), 1);
    assert_eq!(writer.stats().batches_committed, 1);
}

#[test]
fn durable_through_never_exceeds_submitted() {
    let (_dir, store) = store();
    let writer = Writer::spawn(
        Arc::clone(&store),
        WriterConfig { flush_delay: Duration::from_millis(1), ..Default::default() },
    );
    for i in 0..500 {
        writer.submit(claim(i)).unwrap();
        assert!(
            writer.durable_through() <= writer.submitted(),
            "reported durability ran ahead of submission"
        );
    }
    writer.flush().unwrap();
    assert_eq!(writer.durable_through(), 500);
    assert_eq!(writer.submitted(), 500);
}

#[test]
fn a_full_batch_commits_without_waiting_for_the_delay() {
    let (_dir, store) = store();
    // A delay long enough that any commit within the deadline must have been
    // triggered by batch size rather than by elapsed time.
    let writer = Writer::spawn(
        Arc::clone(&store),
        WriterConfig {
            flush_delay: Duration::from_secs(3600),
            max_batch: 16,
            queue_capacity: 4096,
        },
    );
    for i in 0..16 {
        writer.submit(claim(i)).unwrap();
    }

    let deadline = Instant::now() + Duration::from_secs(5);
    while store.sequence().unwrap() < 16 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(store.sequence().unwrap(), 16, "size-triggered commit did not occur");
}

#[test]
fn a_full_queue_applies_backpressure_rather_than_growing() {
    let (_dir, store) = store();
    let writer = Writer::spawn(
        Arc::clone(&store),
        WriterConfig {
            flush_delay: Duration::from_millis(1),
            max_batch: 8,
            queue_capacity: 16,
        },
    );
    // Far more claims than the queue can hold. This must complete by blocking
    // the producer, not by unbounded buffering.
    for i in 0..2_000 {
        writer.submit(claim(i)).unwrap();
    }
    writer.flush().unwrap();

    let stats = writer.stats();
    assert_eq!(store.sequence().unwrap(), 2_000);
    assert_eq!(stats.claims_committed, 2_000);
    assert!(
        stats.backpressure_waits > 0,
        "queue capacity was never reached; the test did not exercise backpressure"
    );
    assert!(
        stats.largest_batch <= 8,
        "batch exceeded max_batch: {}",
        stats.largest_batch
    );
}

#[test]
fn shutdown_commits_outstanding_claims() {
    let (_dir, store) = store();
    let writer = Writer::spawn(
        Arc::clone(&store),
        WriterConfig { flush_delay: Duration::from_secs(3600), ..Default::default() },
    );
    for i in 0..40 {
        writer.submit(claim(i)).unwrap();
    }
    writer.shutdown().unwrap();
    assert_eq!(store.sequence().unwrap(), 40);
}

#[test]
fn dropping_the_writer_commits_outstanding_claims() {
    let (_dir, store) = store();
    {
        let writer = Writer::spawn(
            Arc::clone(&store),
            WriterConfig { flush_delay: Duration::from_secs(3600), ..Default::default() },
        );
        for i in 0..40 {
            writer.submit(claim(i)).unwrap();
        }
    }
    assert_eq!(store.sequence().unwrap(), 40);
}

#[test]
fn concurrent_producers_all_reach_durability() {
    let (_dir, store) = store();
    let writer = Arc::new(Writer::spawn(Arc::clone(&store), WriterConfig::default()));
    let threads: Vec<_> = (0..8)
        .map(|t| {
            let writer = Arc::clone(&writer);
            std::thread::spawn(move || {
                for i in 0..250 {
                    writer.submit(claim(t * 250 + i)).unwrap();
                }
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    writer.flush().unwrap();
    assert_eq!(store.sequence().unwrap(), 2_000);
    assert_eq!(writer.stats().claims_committed, 2_000);
}

#[test]
fn a_malformed_claim_is_rejected_at_submit_and_does_not_enter_the_queue() {
    let (_dir, store) = store();
    let writer = Writer::spawn(Arc::clone(&store), WriterConfig::default());

    let mut bad = claim(0);
    bad.valid_to = Some(50); // inverted interval against valid_from = 100
    assert!(writer.submit(bad).is_err());

    writer.submit(claim(1)).unwrap();
    writer.flush().unwrap();

    // Only the well-formed claim was committed; the batch was never poisoned.
    assert_eq!(store.sequence().unwrap(), 1);
    assert_eq!(writer.stats().claims_submitted, 1);
}

#[test]
fn batching_reduces_commits_far_below_claim_count() {
    let (_dir, store) = store();
    let writer = Writer::spawn(
        Arc::clone(&store),
        WriterConfig {
            flush_delay: Duration::from_millis(50),
            max_batch: 512,
            queue_capacity: 8192,
        },
    );
    for i in 0..4_000 {
        writer.submit(claim(i)).unwrap();
    }
    writer.flush().unwrap();

    let stats = writer.stats();
    assert_eq!(stats.claims_committed, 4_000);
    // The property that makes amortization real: far fewer transactions, and
    // therefore fsyncs, than claims.
    assert!(
        stats.batches_committed < 400,
        "expected substantial batching, got {} batches for 4000 claims (mean {:.1})",
        stats.batches_committed,
        stats.mean_batch_size()
    );
    assert!(stats.mean_batch_size() > 10.0);
}
