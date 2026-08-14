//! Measures §8.2 rebuild and §8.3 grounding cost against corpus size.
//!
//! Grounding is O(claims) and SHOULD run on a longer interval than rebuild
//! (§8.3); this harness puts numbers on that SHOULD. Run it on a real block
//! device — `/tmp` is tmpfs on the reference host and `SPEC.md` standing
//! rule 2 forbids timing durability there.
//!
//! ```text
//! cargo run --release -p vyrm-store --example ground_cost -- <dir> [claims]
//! ```

use std::time::Instant;
use vyrm_core::{Claim, Predicate, Producer, Subject};
use vyrm_store::{GroundingReport, Store};

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = args.next().expect("usage: ground_cost <dir> [claims]");
    let count: u64 = args.next().map(|c| c.parse().expect("claim count")).unwrap_or(10_000);

    let store = Store::open(std::path::Path::new(&dir)).expect("open store");

    // 200 subjects x 5 predicates, versions distributed across them: enough
    // pair cardinality that the projection is a real map, not a scalar.
    let producer = Producer { actor: "harness".into(), on_behalf_of: None, session: None };
    let mut batch = Vec::new();
    let started = Instant::now();
    for i in 0..count {
        batch.push(Claim::new(
            Subject::new(format!("s{}", i % 200)).unwrap(),
            Predicate::new(format!("p{}", i % 5)).unwrap(),
            format!("v{i}"),
            i,
            i,
            producer.clone(),
        ));
        if batch.len() == 500 {
            store.append_batch(&batch).expect("append");
            batch.clear();
        }
    }
    if !batch.is_empty() {
        store.append_batch(&batch).expect("append");
    }
    println!("append   {:>8} claims in {:?}", count, started.elapsed());

    let started = Instant::now();
    let outcome = store.rebuild_current().expect("rebuild");
    println!(
        "rebuild  {:>8} applied in {:?} (watermark {} -> {})",
        outcome.applied, started.elapsed(), outcome.from, outcome.to
    );

    let started = Instant::now();
    match store.ground_current(count).expect("ground") {
        GroundingReport::Grounded(stamp) => println!(
            "ground   {:>8} claims in {:?} (digest {:016x})",
            count, started.elapsed(), stamp.digest
        ),
        GroundingReport::Divergence { differences } => {
            println!("DIVERGENCE: {differences:?}");
        }
    }

    // The incremental case grounding exists to protect: a small interval on
    // top of a large log.
    store
        .append_batch(&[Claim::new(
            Subject::new("s0").unwrap(),
            Predicate::new("p0").unwrap(),
            "tail",
            count + 1,
            count + 1,
            producer.clone(),
        )])
        .expect("append tail");
    let started = Instant::now();
    let outcome = store.rebuild_current().expect("incremental rebuild");
    println!(
        "rebuild  {:>8} applied in {:?} (incremental)",
        outcome.applied, started.elapsed()
    );
}
