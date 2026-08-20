//! Group-commit writer.
//!
//! `SPEC.md` §8.1. Claims arriving individually are buffered and committed in
//! batches, so that a continuously writing executor pays amortized rather than
//! per-claim durability cost.
//!
//! ```text
//! flush when   batch full   OR   flush_delay elapsed   OR   flush() invoked
//! ```
//!
//! ## Durability contract
//!
//! - After [`Writer::flush`] returns `Ok`, every claim submitted before that call
//!   is durable.
//! - Before `flush` returns, a submitted claim MAY be lost on process
//!   termination. This is the documented and tested boundary; see
//!   `tests/durability.rs`.
//!
//! ## Choosing a pattern
//!
//! Measured on ext4, 2026-08-10 (`examples/sparse_latency.rs`,
//! `examples/throughput.rs`). The interval only amortizes when claims arrive
//! while a batch is open; it cannot batch a producer that waits for each claim.
//!
//! | Producer | Pattern | Cost per claim |
//! |----------|---------|----------------|
//! | Continuous, does not await durability | `submit` repeatedly, `flush` at a task boundary | 0.0055 ms |
//! | Requires durability before proceeding | `submit` then `flush` | 0.438 ms |
//! | Requires durability before proceeding | `submit`, then block on [`Writer::durable_through`] | 21.264 ms at a 20 ms interval |
//!
//! The third row is an anti-pattern. Blocking on `durable_through` waits out the
//! full interval for a batch that will never fill, so it pays the interval as
//! latency and gains no amortization. [`Writer::flush`] commits immediately and
//! bypasses the interval; its cost is independent of `flush_delay`.
//!
//! [`Writer::durable_through`] is provided for progress reporting, not as a
//! synchronization primitive.
//!
//! ## Backpressure
//!
//! The queue is bounded. [`Writer::submit`] blocks when the queue is full rather
//! than growing without limit, so a producer faster than the substrate is slowed
//! rather than exhausting memory. Blocking occurrences are counted in
//! [`WriterStats::backpressure_waits`], which is the signal for tuning
//! [`WriterConfig::queue_capacity`] from evidence rather than assumption.
//!
//! Implemented on `std` synchronization only. The kernel and this adapter carry
//! no async runtime.

use crate::error::{Error, Result};
use crate::store::Store;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;
use vyrm_core::Claim;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriterConfig {
    /// Maximum time a submitted claim waits before its batch is committed.
    pub flush_delay: Duration,
    /// Maximum claims committed in one transaction.
    pub max_batch: usize,
    /// Maximum claims buffered before [`Writer::submit`] blocks.
    pub queue_capacity: usize,
}

impl Default for WriterConfig {
    fn default() -> Self {
        Self {
            flush_delay: Duration::from_millis(5),
            max_batch: 512,
            queue_capacity: 4096,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WriterStats {
    pub claims_submitted: u64,
    pub claims_committed: u64,
    pub batches_committed: u64,
    pub largest_batch: usize,
    /// Occurrences of [`Writer::submit`] blocking on a full queue.
    pub backpressure_waits: u64,
}

impl WriterStats {
    /// Mean claims per committed batch. Zero when nothing has been committed.
    pub fn mean_batch_size(&self) -> f64 {
        if self.batches_committed == 0 {
            0.0
        } else {
            self.claims_committed as f64 / self.batches_committed as f64
        }
    }
}

struct State {
    pending: Vec<Claim>,
    /// Claims accepted by `submit`, in order.
    submitted: u64,
    /// Claims durable on disk.
    durable_through: u64,
    stats: WriterStats,
    failure: Option<String>,
    /// Set by `flush` so the worker can distinguish an explicit durability
    /// boundary from a spurious wakeup or a producer arriving before its wait.
    flush_requested: bool,
    shutdown: bool,
}

struct Shared {
    state: Mutex<State>,
    /// Signalled when work is available or shutdown is requested.
    work: Condvar,
    /// Signalled when `durable_through` advances or a failure is recorded.
    progress: Condvar,
    /// Signalled when the queue drops below capacity.
    drained: Condvar,
}

pub struct Writer {
    shared: Arc<Shared>,
    handle: Option<JoinHandle<()>>,
    stopping: Arc<AtomicBool>,
    config: WriterConfig,
}

impl Writer {
    /// Starts the commit thread.
    pub fn spawn(store: Arc<Store>, config: WriterConfig) -> Self {
        assert!(config.max_batch > 0, "max_batch must be non-zero");
        assert!(config.queue_capacity > 0, "queue_capacity must be non-zero");

        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                pending: Vec::new(),
                submitted: 0,
                durable_through: 0,
                stats: WriterStats::default(),
                failure: None,
                flush_requested: false,
                shutdown: false,
            }),
            work: Condvar::new(),
            progress: Condvar::new(),
            drained: Condvar::new(),
        });
        let stopping = Arc::new(AtomicBool::new(false));

        let thread_shared = Arc::clone(&shared);
        let handle = std::thread::Builder::new()
            .name("vyrm-writer".into())
            .spawn(move || commit_loop(store, thread_shared, config))
            .expect("spawn writer thread");

        Self {
            shared,
            handle: Some(handle),
            stopping,
            config,
        }
    }

    /// Buffers a claim for commit.
    ///
    /// Validates before queueing, so a malformed claim fails at its call site
    /// rather than poisoning a later batch. Blocks while the queue is at
    /// capacity.
    pub fn submit(&self, claim: Claim) -> Result<()> {
        claim.validate()?;
        let mut state = self.shared.state.lock().expect("writer state poisoned");
        loop {
            if let Some(failure) = &state.failure {
                return Err(Error::Substrate(failure.clone()));
            }
            if state.shutdown {
                return Err(Error::Substrate("writer is shut down".into()));
            }
            if state.pending.len() < self.config.queue_capacity {
                break;
            }
            state.stats.backpressure_waits += 1;
            state = self
                .shared
                .drained
                .wait(state)
                .expect("writer state poisoned");
        }
        state.pending.push(claim);
        state.submitted += 1;
        state.stats.claims_submitted += 1;
        let should_wake = state.pending.len() >= self.config.max_batch;
        drop(state);
        if should_wake {
            self.shared.work.notify_one();
        }
        Ok(())
    }

    /// Commits everything submitted before this call and returns once durable.
    pub fn flush(&self) -> Result<()> {
        let mut state = self.shared.state.lock().expect("writer state poisoned");
        if let Some(failure) = &state.failure {
            return Err(Error::Substrate(failure.clone()));
        }
        let target = state.submitted;
        if state.durable_through >= target {
            return Ok(());
        }
        state.flush_requested = true;
        drop(state);
        self.shared.work.notify_one();

        let mut state = self.shared.state.lock().expect("writer state poisoned");
        while state.durable_through < target {
            if let Some(failure) = &state.failure {
                return Err(Error::Substrate(failure.clone()));
            }
            state = self
                .shared
                .progress
                .wait(state)
                .expect("writer state poisoned");
        }
        Ok(())
    }

    pub fn stats(&self) -> WriterStats {
        self.shared.state.lock().expect("writer state poisoned").stats
    }

    /// Count of submitted claims now durable, in submission order.
    ///
    /// Intended for progress reporting. Callers MUST NOT block on this value to
    /// await durability: doing so waits out the full `flush_delay` for a batch
    /// that will never fill, measured at 21.264 ms against 0.438 ms for
    /// [`Writer::flush`] under a 20 ms interval. Use `flush` instead.
    pub fn durable_through(&self) -> u64 {
        self.shared
            .state
            .lock()
            .expect("writer state poisoned")
            .durable_through
    }

    /// Count of claims accepted by [`Writer::submit`].
    pub fn submitted(&self) -> u64 {
        self.shared.state.lock().expect("writer state poisoned").submitted
    }

    /// Flushes pending claims and stops the commit thread.
    pub fn shutdown(mut self) -> Result<()> {
        self.stop()
    }

    fn stop(&mut self) -> Result<()> {
        if self.stopping.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        let flush_result = self.flush();
        {
            let mut state = self.shared.state.lock().expect("writer state poisoned");
            state.shutdown = true;
        }
        self.shared.work.notify_all();
        self.shared.drained.notify_all();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        flush_result
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        // Best effort: a dropped writer must not leave the commit thread running
        // or silently discard buffered claims.
        let _ = self.stop();
    }
}

fn commit_loop(store: Arc<Store>, shared: Arc<Shared>, config: WriterConfig) {
    loop {
        let batch = {
            let mut state = shared.state.lock().expect("writer state poisoned");
            loop {
                if state.shutdown && state.pending.is_empty() {
                    return;
                }
                if state.pending.is_empty() {
                    // A flush requested while the previous batch was in the
                    // substrate is satisfied by that batch's progress update.
                    state.flush_requested = false;
                }
                if !state.pending.is_empty()
                    && (state.pending.len() >= config.max_batch
                        || state.flush_requested
                        || state.shutdown)
                {
                    break;
                }
                let (next, timeout) = shared
                    .work
                    .wait_timeout(state, config.flush_delay)
                    .expect("writer state poisoned");
                state = next;
                if timeout.timed_out() && !state.pending.is_empty() {
                    break;
                }
            }
            if state.pending.is_empty() {
                state.flush_requested = false;
                continue;
            }
            let take = config.max_batch.min(state.pending.len());
            let batch: Vec<Claim> = state.pending.drain(..take).collect();
            if state.pending.is_empty() {
                state.flush_requested = false;
            }
            shared.drained.notify_all();
            batch
        };

        let count = batch.len() as u64;
        let outcome = store.append_batch(&batch);

        let mut state = shared.state.lock().expect("writer state poisoned");
        match outcome {
            Ok(_) => {
                state.durable_through += count;
                state.stats.claims_committed += count;
                state.stats.batches_committed += 1;
                state.stats.largest_batch = state.stats.largest_batch.max(batch.len());
            }
            Err(error) => {
                // Record and stop accepting work. A silent retry could reorder
                // claims relative to their assigned sequences.
                state.failure = Some(error.to_string());
                state.shutdown = true;
                shared.progress.notify_all();
                shared.drained.notify_all();
                return;
            }
        }
        shared.progress.notify_all();
    }
}
