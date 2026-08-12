//! Child process for the durability test in `tests/durability.rs`.
//!
//! Writes claims through a [`Writer`], optionally flushes, then announces
//! readiness and blocks. The parent terminates it with SIGKILL, so no
//! destructor, no `Drop`, and no shutdown path can contribute to what survives.
//!
//! ```text
//! durability-child <db-path> <count> <flush|noflush>
//! ```

use std::io::Write;
use std::sync::Arc;
use std::time::Duration;
use vyrm_core::{Claim, Predicate, Producer, Subject};
use vyrm_store::{Store, Writer, WriterConfig};

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("db path");
    let count: usize = args.next().expect("count").parse().expect("count");
    let mode = args.next().expect("flush|noflush");

    let store = Arc::new(Store::open(std::path::Path::new(&path)).expect("open store"));

    // In `noflush` mode the delay must exceed the lifetime of the process, so
    // that an absent claim proves the contract rather than losing a race with
    // the interval timer.
    let flush_delay = if mode == "flush" {
        Duration::from_millis(5)
    } else {
        Duration::from_secs(3600)
    };

    let writer = Writer::spawn(
        Arc::clone(&store),
        WriterConfig {
            flush_delay,
            max_batch: 1024,
            queue_capacity: 8192,
        },
    );

    for i in 0..count {
        let claim = Claim::new(
            Subject::new(format!("s{i}")).unwrap(),
            Predicate::new("status").unwrap(),
            "in_progress",
            100 + i as u64,
            100 + i as u64,
            Producer { actor: "durability-child".into(), on_behalf_of: None, session: None },
        );
        writer.submit(claim).expect("submit");
    }

    if mode == "flush" {
        writer.flush().expect("flush");
    }

    println!("READY");
    std::io::stdout().flush().expect("flush stdout");

    // Leak the writer so that no unwinding or destructor can flush after the
    // readiness signal. The parent kills this process.
    std::mem::forget(writer);
    loop {
        std::thread::sleep(Duration::from_secs(60));
    }
}
