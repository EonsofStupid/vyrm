//! Sparse-arrival latency measurement.
//!
//! The throughput example saturates the queue, so every batch is triggered by
//! `max_batch` and `flush_delay` never takes effect. That leaves the interval
//! unvalidated for the arrival pattern an executor actually produces: a claim
//! written between tool calls, seconds apart, where the constraint is
//! latency to durability rather than throughput.
//!
//! This example measures submit-to-durable latency at controlled arrival rates.
//!
//! The database path MUST reside on a real block device. `/tmp` is tmpfs on this
//! host, where `SyncAll` does not reach a disk.
//!
//! ```text
//! cargo run --release -p vyrm-store --example sparse_latency -- <path-on-ext4>
//! ```

use std::sync::Arc;
use std::time::{Duration, Instant};
use vyrm_core::{Claim, Predicate, Producer, Subject};
use vyrm_store::{Store, Writer, WriterConfig};

fn claim(tag: &str, i: usize) -> Claim {
    Claim::new(
        Subject::new(format!("{tag}-{i}")).unwrap(),
        Predicate::new("status").unwrap(),
        "in_progress",
        100 + i as u64,
        100 + i as u64,
        Producer { actor: "bench".into(), on_behalf_of: None, session: None },
    )
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted[rank]
}

fn main() {
    let path = std::env::args().nth(1).expect("usage: sparse_latency <db-path>");
    let path = std::path::PathBuf::from(path);
    let _ = std::fs::remove_dir_all(&path);
    let store = Arc::new(Store::open(&path).expect("open store"));

    println!(
        "{:>10}  {:>10}  {:>8}  {:>10}  {:>10}  {:>10}  {:>8}",
        "delay(ms)", "gap(ms)", "claims", "p50(ms)", "p99(ms)", "max(ms)", "batches"
    );
    println!("{}", "-".repeat(78));

    for &delay_ms in &[1u64, 5, 20] {
        for &gap_ms in &[0u64, 2, 25] {
            let writer = Writer::spawn(
                Arc::clone(&store),
                WriterConfig {
                    flush_delay: Duration::from_millis(delay_ms),
                    max_batch: 512,
                    queue_capacity: 8192,
                },
            );

            // Fewer samples at wide gaps so the run stays bounded in time.
            let samples = if gap_ms >= 25 { 40 } else { 200 };
            let tag = format!("d{delay_ms}g{gap_ms}");
            let mut latencies = Vec::with_capacity(samples);

            for i in 0..samples {
                if gap_ms > 0 {
                    std::thread::sleep(Duration::from_millis(gap_ms));
                }
                let target = writer.submitted() + 1;
                let submitted = Instant::now();
                writer.submit(claim(&tag, i)).expect("submit");
                while writer.durable_through() < target {
                    std::hint::spin_loop();
                }
                latencies.push(submitted.elapsed().as_secs_f64() * 1000.0);
            }

            let batches = writer.stats().batches_committed;
            writer.shutdown().expect("shutdown");

            latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());
            println!(
                "{delay_ms:>10}  {gap_ms:>10}  {:>8}  {:>10.2}  {:>10.2}  {:>10.2}  {batches:>8}",
                samples,
                percentile(&latencies, 0.50),
                percentile(&latencies, 0.99),
                latencies.last().copied().unwrap_or(0.0),
            );
        }
    }

    // The table above waits on `durable_through` and therefore pays the full
    // interval on every claim. A caller needing immediate durability should
    // instead call `flush`, which commits at once and bypasses the timer. The
    // comparison below establishes which pattern a synchronous producer wants.
    println!("\nsynchronous producer: waiting on the timer versus calling flush");
    println!("{}", "-".repeat(78));
    println!(
        "{:>10}  {:>26}  {:>10}  {:>10}  {:>8}",
        "delay(ms)", "pattern", "p50(ms)", "max(ms)", "batches"
    );

    for &delay_ms in &[1u64, 20] {
        for pattern in ["wait on timer", "submit + flush"] {
            let writer = Writer::spawn(
                Arc::clone(&store),
                WriterConfig {
                    flush_delay: Duration::from_millis(delay_ms),
                    max_batch: 512,
                    queue_capacity: 8192,
                },
            );
            let tag = format!("p{delay_ms}{}", if pattern.starts_with("wait") { "w" } else { "f" });
            let mut latencies = Vec::with_capacity(100);
            for i in 0..100 {
                let target = writer.submitted() + 1;
                let started = Instant::now();
                writer.submit(claim(&tag, i)).expect("submit");
                if pattern == "submit + flush" {
                    writer.flush().expect("flush");
                } else {
                    while writer.durable_through() < target {
                        std::hint::spin_loop();
                    }
                }
                latencies.push(started.elapsed().as_secs_f64() * 1000.0);
            }
            let batches = writer.stats().batches_committed;
            writer.shutdown().expect("shutdown");
            latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());
            println!(
                "{delay_ms:>10}  {pattern:>26}  {:>10.3}  {:>10.3}  {batches:>8}",
                percentile(&latencies, 0.50),
                latencies.last().copied().unwrap_or(0.0),
            );
        }
    }

    println!("\nfinal sequence watermark: {}", store.sequence().unwrap());
}
