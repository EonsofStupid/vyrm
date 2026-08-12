//! Batch amortization measurement.
//!
//! `SPEC.md` §12 requires throughput figures to be measurements. This example
//! produces them.
//!
//! The database path MUST be supplied and MUST reside on a real block device.
//! `/tmp` is tmpfs on this host, where `SyncAll` does not reach a disk and the
//! resulting figures would be fabricated.
//!
//! ```text
//! cargo run --release -p vyrm-store --example throughput -- <path-on-ext4>
//! ```

use std::sync::Arc;
use std::time::{Duration, Instant};
use vyrm_core::{Claim, ClaimReader, Predicate, Producer, Reader, Subject};
use vyrm_store::{Store, Writer, WriterConfig};

fn producer() -> Producer {
    Producer { actor: "bench".into(), on_behalf_of: None, session: None }
}

fn claims(count: usize, tag: &str) -> Vec<Claim> {
    (0..count)
        .map(|i| {
            Claim::new(
                Subject::new(format!("{tag}-{i}")).unwrap(),
                Predicate::new("status").unwrap(),
                "in_progress",
                100 + i as u64,
                100 + i as u64,
                producer(),
            )
        })
        .collect()
}

fn main() {
    let path = std::env::args().nth(1).expect("usage: throughput <db-path>");
    let path = std::path::PathBuf::from(path);
    let _ = std::fs::remove_dir_all(&path);
    let store_arc = Arc::new(Store::open(&path).expect("open store"));
    let store = Arc::clone(&store_arc);

    // Warm the journal and page cache so the first batch is not an outlier.
    store.append_batch(&claims(200, "warm")).unwrap();

    println!("{:>12}  {:>12}  {:>14}  {:>16}", "batch size", "claims", "ms/claim", "claims/s");
    println!("{}", "-".repeat(60));

    for &size in &[1usize, 10, 100, 1000] {
        let total = 2000usize.max(size);
        let rounds = total / size;
        let batches: Vec<Vec<Claim>> = (0..rounds)
            .map(|r| claims(size, &format!("b{size}-r{r}")))
            .collect();

        let start = Instant::now();
        for batch in &batches {
            store.append_batch(batch).unwrap();
        }
        let elapsed = start.elapsed();

        let written = rounds * size;
        let per_claim_ms = elapsed.as_secs_f64() * 1000.0 / written as f64;
        println!(
            "{size:>12}  {written:>12}  {per_claim_ms:>14.4}  {:>16.0}",
            written as f64 / elapsed.as_secs_f64()
        );
    }

    // The path an executor actually takes: claims arrive one at a time. Direct
    // per-claim append pays full durability cost; the group-commit writer
    // amortizes it without the caller batching anything.
    println!("\nsingly-arriving claims (the executor write path)");
    println!("{}", "-".repeat(60));

    let direct = claims(600, "direct");
    let start = Instant::now();
    for claim in &direct {
        store.assert(claim).unwrap();
    }
    let direct_ms = start.elapsed().as_secs_f64() * 1000.0 / direct.len() as f64;
    println!(
        "{:<34}{:>10.4} ms/claim{:>12.0}/s",
        "direct assert per claim",
        direct_ms,
        1000.0 / direct_ms
    );

    for &delay_ms in &[1u64, 5, 20] {
        let store = Arc::clone(&store_arc);
        let writer = Writer::spawn(
            store,
            WriterConfig {
                flush_delay: Duration::from_millis(delay_ms),
                max_batch: 512,
                queue_capacity: 8192,
            },
        );
        let batch = claims(4_000, &format!("w{delay_ms}"));
        let start = Instant::now();
        for claim in batch {
            writer.submit(claim).unwrap();
        }
        writer.flush().unwrap();
        let elapsed = start.elapsed();
        let stats = writer.stats();
        let per_claim = elapsed.as_secs_f64() * 1000.0 / 4_000.0;
        println!(
            "{:<34}{:>10.4} ms/claim{:>12.0}/s   batches={} mean={:.0}",
            format!("writer, flush_delay={delay_ms}ms"),
            per_claim,
            1000.0 / per_claim,
            stats.batches_committed,
            stats.mean_batch_size()
        );
        writer.shutdown().unwrap();
    }

    // SPEC.md §7 makes an access record mandatory on every read, so the cost of
    // recording one is added to the cost of every read in the system.
    println!("\nread path: resolution against mandatory access recording");
    println!("{}", "-".repeat(60));
    let subject = Subject::new("direct-0").unwrap();
    let predicate = Predicate::new("status").unwrap();

    let start = Instant::now();
    for _ in 0..2_000 {
        store.as_of(&subject, &predicate, 1_000).unwrap();
    }
    let read_ms = start.elapsed().as_secs_f64() * 1000.0 / 2_000.0;
    println!("{:<34}{:>10.4} ms/op{:>14.0}/s", "as_of resolution", read_ms, 1000.0 / read_ms);

    let observer = Reader::new("agent:clyffy").unwrap();
    let start = Instant::now();
    for i in 0..2_000 {
        store.observe(&observer, &subject, &predicate, 1_000 + i).unwrap();
    }
    let observe_ms = start.elapsed().as_secs_f64() * 1000.0 / 2_000.0;
    println!(
        "{:<34}{:>10.4} ms/op{:>14.0}/s",
        "observe (one transaction each)",
        observe_ms,
        1000.0 / observe_ms
    );
    println!(
        "{:<34}{:>10.4} ms/op{:>14.0}/s   observe is {:.1}x the read",
        "combined read + observe",
        read_ms + observe_ms,
        1000.0 / (read_ms + observe_ms),
        observe_ms / read_ms
    );

    println!("\nfinal sequence watermark: {}", store.sequence().unwrap());
    println!("database path: {}", path.display());
}
