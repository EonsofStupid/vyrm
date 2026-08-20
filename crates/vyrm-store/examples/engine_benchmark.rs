//! Reproducible Fjall/native promotion benchmark.
//!
//! Each backend runs in an isolated child process against a fresh directory.
//! The parent emits one versioned JSON evidence document; it does not turn a
//! single machine run into a universal performance claim.

use serde::{Deserialize, Serialize};
use std::fs;
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use vyrm_core::{Claim, Predicate, Producer, Subject};
use vyrm_store::{Engine, NativeEngine, Store};

const FORMAT_VERSION: u16 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Config {
    trials: usize,
    operations: usize,
    batch_size: usize,
    reads: usize,
    read_width: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            trials: 3,
            operations: 4_096,
            batch_size: 64,
            reads: 512,
            read_width: 32,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Latency {
    samples: usize,
    minimum_ns: u64,
    p50_ns: u64,
    p95_ns: u64,
    p99_ns: u64,
    maximum_ns: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BackendResult {
    backend: String,
    correctness_verified: bool,
    write_operations_per_second: f64,
    write_batch_latency: Latency,
    read_operations_per_second: f64,
    read_latency: Latency,
    recovery_ns: u64,
    uncompacted_recovery_ns: Option<u64>,
    maintenance_ns: Option<u64>,
    write_peak_rss_kib: Option<u64>,
    peak_rss_kib: Option<u64>,
    maintenance_peak_rss_kib: Option<u64>,
    disk_bytes: u64,
    semantic_sequence: u64,
    native_maintenance: Option<MaintenanceEvidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MaintenanceEvidence {
    wal_payload_bytes: u64,
    wal_payload_max_bytes: u64,
    memtable_versions: u64,
    memtable_max_versions: u64,
    memtable_bytes: u64,
    automatic_flushes: u64,
    write_stalls: u64,
    failed_flushes: u64,
    oversized_batches: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProbeResult {
    correctness_verified: bool,
    semantic_sequence: u64,
    recovery_ns: u64,
    read_operations_per_second: f64,
    read_latency: Latency,
    peak_rss_kib: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Ratios {
    native_to_fjall_write_throughput: f64,
    native_to_fjall_read_throughput: f64,
    native_to_fjall_write_p95: f64,
    native_to_fjall_read_p95: f64,
    native_to_fjall_recovery: f64,
    native_to_fjall_peak_rss: Option<f64>,
    native_to_fjall_disk: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PromotionVerdict {
    policy: String,
    passes: bool,
    failures: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Evidence {
    format_version: u16,
    measured_at_unix_ms: u128,
    architecture: String,
    operating_system: String,
    logical_cpus: usize,
    aggregation: String,
    config: Config,
    fjall_trials: Vec<BackendResult>,
    native_trials: Vec<BackendResult>,
    fjall: BackendResult,
    native: BackendResult,
    ratios: Ratios,
    promotion: PromotionVerdict,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("engine benchmark failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    if arguments
        .first()
        .is_some_and(|argument| argument == "--child")
    {
        return run_child(&arguments);
    }
    if arguments
        .first()
        .is_some_and(|argument| argument == "--probe")
    {
        return run_probe(&arguments);
    }
    let (config, output, require_promotion) = parse_parent(&arguments)?;
    let directory = tempfile::tempdir().map_err(|error| error.to_string())?;
    let mut fjall_trials = Vec::with_capacity(config.trials);
    let mut native_trials = Vec::with_capacity(config.trials);
    for trial in 0..config.trials {
        let trial_root = directory.path().join(format!("trial-{trial}"));
        fs::create_dir_all(&trial_root).map_err(|error| error.to_string())?;
        let mut measure = |backend: &str| -> Result<(), String> {
            let result = launch_child(backend, &trial_root.join(backend), &config)?;
            if backend == "fjall" {
                fjall_trials.push(result);
            } else {
                native_trials.push(result);
            }
            Ok(())
        };
        if trial % 2 == 0 {
            measure("fjall")?;
            measure("native")?;
        } else {
            measure("native")?;
            measure("fjall")?;
        }
    }
    let fjall = aggregate("fjall", &fjall_trials);
    let native = aggregate("native", &native_trials);
    let ratios = ratios(&fjall, &native);
    let promotion = promotion(&fjall, &native, &ratios);
    let evidence = Evidence {
        format_version: FORMAT_VERSION,
        measured_at_unix_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_millis(),
        architecture: std::env::consts::ARCH.into(),
        operating_system: std::env::consts::OS.into(),
        logical_cpus: std::thread::available_parallelism()
            .map(usize::from)
            .unwrap_or(1),
        aggregation: "median of per-trial metrics; latency percentiles are medians of each isolated trial's percentile"
            .into(),
        config,
        fjall_trials,
        native_trials,
        fjall,
        native,
        ratios,
        promotion,
    };
    let promotion_passes = evidence.promotion.passes;
    let promotion_failures = evidence.promotion.failures.clone();
    let bytes = serde_json::to_vec_pretty(&evidence).map_err(|error| error.to_string())?;
    if let Some(output) = output {
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        fs::write(&output, &bytes).map_err(|error| error.to_string())?;
        eprintln!("wrote benchmark evidence to {}", output.display());
    }
    println!(
        "{}",
        String::from_utf8(bytes).map_err(|error| error.to_string())?
    );
    if require_promotion && !promotion_passes {
        return Err(format!(
            "strict promotion gate failed: {}",
            promotion_failures.join("; ")
        ));
    }
    Ok(())
}

fn parse_parent(arguments: &[String]) -> Result<(Config, Option<PathBuf>, bool), String> {
    let mut config = Config::default();
    let mut output = None;
    let mut require_promotion = false;
    let mut index = 0;
    while index < arguments.len() {
        if arguments[index] == "--require-promotion" {
            require_promotion = true;
            index += 1;
            continue;
        }
        let value = arguments
            .get(index + 1)
            .ok_or_else(|| format!("missing value after {}", arguments[index]))?;
        match arguments[index].as_str() {
            "--trials" => config.trials = parse_positive(value, "trials")?,
            "--operations" => config.operations = parse_positive(value, "operations")?,
            "--batch-size" => config.batch_size = parse_positive(value, "batch size")?,
            "--reads" => config.reads = parse_positive(value, "reads")?,
            "--read-width" => config.read_width = parse_positive(value, "read width")?,
            "--output" => output = Some(PathBuf::from(value)),
            unknown => return Err(format!("unknown argument {unknown}")),
        }
        index += 2;
    }
    if config.batch_size > config.operations || config.read_width > config.operations {
        return Err("batch size and read width cannot exceed operations".into());
    }
    Ok((config, output, require_promotion))
}

fn parse_positive(value: &str, label: &str) -> Result<usize, String> {
    let parsed = value
        .parse::<usize>()
        .map_err(|error| format!("invalid {label}: {error}"))?;
    if parsed == 0 {
        return Err(format!("{label} must be greater than zero"));
    }
    Ok(parsed)
}

fn launch_child(backend: &str, path: &Path, config: &Config) -> Result<BackendResult, String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let output = Command::new(executable)
        .arg("--child")
        .arg(backend)
        .arg("--path")
        .arg(path)
        .arg("--operations")
        .arg(config.operations.to_string())
        .arg("--batch-size")
        .arg(config.batch_size.to_string())
        .arg("--reads")
        .arg(config.reads.to_string())
        .arg("--read-width")
        .arg(config.read_width.to_string())
        .arg("--trials")
        .arg("1")
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "{backend} child failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    serde_json::from_slice(&output.stdout).map_err(|error| error.to_string())
}

fn run_child(arguments: &[String]) -> Result<(), String> {
    let backend = arguments
        .get(1)
        .ok_or_else(|| "missing child backend".to_owned())?;
    if arguments.get(2).map(String::as_str) != Some("--path") {
        return Err("child requires --path".into());
    }
    let path = PathBuf::from(
        arguments
            .get(3)
            .ok_or_else(|| "child requires a path".to_owned())?,
    );
    let (config, output, require_promotion) = parse_parent(&arguments[4..])?;
    if output.is_some() {
        return Err("child cannot write an output file".into());
    }
    if require_promotion {
        return Err("child cannot require a promotion verdict".into());
    }
    let result = match backend.as_str() {
        "fjall" => run_fjall(&path, &config)?,
        "native" => run_native(&path, &config)?,
        _ => return Err(format!("unknown child backend {backend}")),
    };
    println!(
        "{}",
        serde_json::to_string(&result).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn launch_probe(backend: &str, path: &Path, config: &Config) -> Result<ProbeResult, String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let output = Command::new(executable)
        .arg("--probe")
        .arg(backend)
        .arg("--path")
        .arg(path)
        .arg("--operations")
        .arg(config.operations.to_string())
        .arg("--batch-size")
        .arg(config.batch_size.to_string())
        .arg("--reads")
        .arg(config.reads.to_string())
        .arg("--read-width")
        .arg(config.read_width.to_string())
        .arg("--trials")
        .arg("1")
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "{backend} probe failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    serde_json::from_slice(&output.stdout).map_err(|error| error.to_string())
}

fn run_probe(arguments: &[String]) -> Result<(), String> {
    let backend = arguments
        .get(1)
        .ok_or_else(|| "missing probe backend".to_owned())?;
    if arguments.get(2).map(String::as_str) != Some("--path") {
        return Err("probe requires --path".into());
    }
    let path = PathBuf::from(
        arguments
            .get(3)
            .ok_or_else(|| "probe requires a path".to_owned())?,
    );
    let (config, output, require_promotion) = parse_parent(&arguments[4..])?;
    if output.is_some() {
        return Err("probe cannot write an output file".into());
    }
    if require_promotion {
        return Err("probe cannot require a promotion verdict".into());
    }
    let started = Instant::now();
    let engine: Box<dyn Engine> = match backend.as_str() {
        "fjall" => Box::new(Store::open(&path).map_err(|error| error.to_string())?),
        "native" => Box::new(NativeEngine::open(&path).map_err(|error| error.to_string())?),
        _ => return Err(format!("unknown probe backend {backend}")),
    };
    let sequence = engine.sequence().map_err(|error| error.to_string())?;
    let recovery = started.elapsed();
    let correctness_verified = verify(engine.as_ref(), &config, sequence)?;
    let (read_samples, read_elapsed) = read_workload(engine.as_ref(), &config)?;
    let result = ProbeResult {
        correctness_verified,
        semantic_sequence: sequence,
        recovery_ns: nanos(recovery),
        read_operations_per_second: rate(config.reads, read_elapsed),
        read_latency: summarize(read_samples),
        peak_rss_kib: peak_rss_kib(),
    };
    println!(
        "{}",
        serde_json::to_string(&result).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn run_fjall(path: &Path, config: &Config) -> Result<BackendResult, String> {
    let store = Store::open(path).map_err(|error| error.to_string())?;
    let (write_samples, write_elapsed) = write_workload(&store, config)?;
    let write_peak_rss_kib = peak_rss_kib();
    drop(store);
    let probe = launch_probe("fjall", path, config)?;
    Ok(BackendResult {
        backend: "fjall".into(),
        correctness_verified: probe.correctness_verified,
        write_operations_per_second: rate(config.operations, write_elapsed),
        write_batch_latency: summarize(write_samples),
        read_operations_per_second: probe.read_operations_per_second,
        read_latency: probe.read_latency,
        recovery_ns: probe.recovery_ns,
        uncompacted_recovery_ns: None,
        maintenance_ns: None,
        write_peak_rss_kib,
        peak_rss_kib: probe.peak_rss_kib,
        maintenance_peak_rss_kib: None,
        disk_bytes: directory_bytes(path)?,
        semantic_sequence: probe.semantic_sequence,
        native_maintenance: None,
    })
}

fn run_native(path: &Path, config: &Config) -> Result<BackendResult, String> {
    let store = NativeEngine::open(path).map_err(|error| error.to_string())?;
    let (write_samples, write_elapsed) = write_workload(&store, config)?;
    let physical = store
        .physical_store_evidence()
        .map_err(|error| error.to_string())?;
    let native_maintenance = Some(MaintenanceEvidence {
        wal_payload_bytes: required(physical.wal_payload_bytes, "WAL payload bytes")?,
        wal_payload_max_bytes: required(physical.wal_payload_max_bytes, "WAL payload byte limit")?,
        memtable_versions: required(physical.memtable_versions, "memtable versions")?,
        memtable_max_versions: required(physical.memtable_max_versions, "memtable version limit")?,
        memtable_bytes: required(physical.memtable_bytes, "memtable bytes")?,
        automatic_flushes: required(physical.automatic_flushes, "automatic flushes")?,
        write_stalls: required(
            physical.maintenance_write_stalls,
            "maintenance write stalls",
        )?,
        failed_flushes: required(
            physical.failed_maintenance_flushes,
            "failed maintenance flushes",
        )?,
        oversized_batches: required(physical.oversized_batches, "oversized batches")?,
    });
    let write_peak_rss_kib = peak_rss_kib();
    drop(store);
    let recovery_started = Instant::now();
    let reopened = NativeEngine::open(path).map_err(|error| error.to_string())?;
    let sequence = reopened.sequence().map_err(|error| error.to_string())?;
    let recovery = recovery_started.elapsed();
    let verified = verify(&reopened, config, sequence)?;
    let maintenance_started = Instant::now();
    reopened.compact(1, 1).map_err(|error| error.to_string())?;
    reopened
        .garbage_collect(1, 1)
        .map_err(|error| error.to_string())?;
    let maintenance = maintenance_started.elapsed();
    let maintenance_peak_rss_kib = peak_rss_kib();
    drop(reopened);
    let probe = launch_probe("native", path, config)?;
    Ok(BackendResult {
        backend: "native".into(),
        correctness_verified: verified && probe.correctness_verified,
        write_operations_per_second: rate(config.operations, write_elapsed),
        write_batch_latency: summarize(write_samples),
        read_operations_per_second: probe.read_operations_per_second,
        read_latency: probe.read_latency,
        recovery_ns: probe.recovery_ns,
        uncompacted_recovery_ns: Some(nanos(recovery)),
        maintenance_ns: Some(nanos(maintenance)),
        write_peak_rss_kib,
        peak_rss_kib: probe.peak_rss_kib,
        maintenance_peak_rss_kib,
        disk_bytes: directory_bytes(path)?,
        semantic_sequence: probe.semantic_sequence,
        native_maintenance,
    })
}

fn required(value: Option<u64>, name: &str) -> Result<u64, String> {
    value.ok_or_else(|| format!("native physical evidence omitted {name}"))
}

fn write_workload(
    engine: &dyn Engine,
    config: &Config,
) -> Result<(Vec<Duration>, Duration), String> {
    let corpus = (0..config.operations)
        .map(|index| {
            Claim::new(
                Subject::new(format!("benchmark:{index:012}")).expect("valid subject"),
                Predicate::new("value").expect("valid predicate"),
                format!("payload-{index:012}"),
                index as u64 + 1,
                index as u64 + 1,
                Producer {
                    actor: "benchmark".into(),
                    on_behalf_of: None,
                    session: Some("engine-promotion-v1".into()),
                },
            )
        })
        .collect::<Vec<_>>();
    let write_started = Instant::now();
    let mut write_samples = Vec::new();
    for claims in corpus.chunks(config.batch_size) {
        let started = Instant::now();
        engine
            .append_batch(claims)
            .map_err(|error| error.to_string())?;
        write_samples.push(started.elapsed());
    }
    let write_elapsed = write_started.elapsed();

    Ok((write_samples, write_elapsed))
}

fn read_workload(
    engine: &dyn Engine,
    config: &Config,
) -> Result<(Vec<Duration>, Duration), String> {
    let read_started = Instant::now();
    let mut read_samples = Vec::with_capacity(config.reads);
    let span = config.operations - config.read_width + 1;
    for iteration in 0..config.reads {
        let start = (iteration.wrapping_mul(7_919) % span) as u64;
        let started = Instant::now();
        let claims = engine
            .claims_in_range(start, start + config.read_width as u64)
            .map_err(|error| error.to_string())?;
        if claims.len() != config.read_width {
            return Err(format!(
                "bounded replay returned {} claims, expected {}",
                claims.len(),
                config.read_width
            ));
        }
        black_box(claims);
        read_samples.push(started.elapsed());
    }
    Ok((read_samples, read_started.elapsed()))
}

fn verify(engine: &dyn Engine, config: &Config, sequence: u64) -> Result<bool, String> {
    if sequence != config.operations as u64 {
        return Ok(false);
    }
    let claims = engine
        .claims_in_range(0, sequence)
        .map_err(|error| error.to_string())?;
    Ok(claims.len() == config.operations
        && claims
            .first()
            .is_some_and(|claim| claim.object == "payload-000000000000")
        && claims
            .last()
            .is_some_and(|claim| claim.object == format!("payload-{:012}", config.operations - 1)))
}

fn summarize(mut samples: Vec<Duration>) -> Latency {
    samples.sort_unstable();
    Latency {
        samples: samples.len(),
        minimum_ns: sample(&samples, 0.0),
        p50_ns: sample(&samples, 0.50),
        p95_ns: sample(&samples, 0.95),
        p99_ns: sample(&samples, 0.99),
        maximum_ns: sample(&samples, 1.0),
    }
}

fn aggregate(backend: &str, trials: &[BackendResult]) -> BackendResult {
    BackendResult {
        backend: backend.into(),
        correctness_verified: trials.iter().all(|trial| trial.correctness_verified),
        write_operations_per_second: median_f64(
            trials
                .iter()
                .map(|trial| trial.write_operations_per_second)
                .collect(),
        ),
        write_batch_latency: aggregate_latency(
            trials
                .iter()
                .map(|trial| &trial.write_batch_latency)
                .collect(),
        ),
        read_operations_per_second: median_f64(
            trials
                .iter()
                .map(|trial| trial.read_operations_per_second)
                .collect(),
        ),
        read_latency: aggregate_latency(trials.iter().map(|trial| &trial.read_latency).collect()),
        recovery_ns: median_u64(trials.iter().map(|trial| trial.recovery_ns).collect()),
        uncompacted_recovery_ns: median_option(
            trials
                .iter()
                .filter_map(|trial| trial.uncompacted_recovery_ns)
                .collect(),
        ),
        maintenance_ns: median_option(
            trials
                .iter()
                .filter_map(|trial| trial.maintenance_ns)
                .collect(),
        ),
        write_peak_rss_kib: median_option(
            trials
                .iter()
                .filter_map(|trial| trial.write_peak_rss_kib)
                .collect(),
        ),
        peak_rss_kib: median_option(
            trials
                .iter()
                .filter_map(|trial| trial.peak_rss_kib)
                .collect(),
        ),
        maintenance_peak_rss_kib: median_option(
            trials
                .iter()
                .filter_map(|trial| trial.maintenance_peak_rss_kib)
                .collect(),
        ),
        disk_bytes: median_u64(trials.iter().map(|trial| trial.disk_bytes).collect()),
        semantic_sequence: median_u64(trials.iter().map(|trial| trial.semantic_sequence).collect()),
        native_maintenance: aggregate_maintenance(
            trials
                .iter()
                .filter_map(|trial| trial.native_maintenance.as_ref())
                .collect(),
        ),
    }
}

fn aggregate_maintenance(values: Vec<&MaintenanceEvidence>) -> Option<MaintenanceEvidence> {
    (!values.is_empty()).then(|| MaintenanceEvidence {
        wal_payload_bytes: median_u64(values.iter().map(|value| value.wal_payload_bytes).collect()),
        wal_payload_max_bytes: median_u64(
            values
                .iter()
                .map(|value| value.wal_payload_max_bytes)
                .collect(),
        ),
        memtable_versions: median_u64(values.iter().map(|value| value.memtable_versions).collect()),
        memtable_max_versions: median_u64(
            values
                .iter()
                .map(|value| value.memtable_max_versions)
                .collect(),
        ),
        memtable_bytes: median_u64(values.iter().map(|value| value.memtable_bytes).collect()),
        automatic_flushes: median_u64(values.iter().map(|value| value.automatic_flushes).collect()),
        write_stalls: median_u64(values.iter().map(|value| value.write_stalls).collect()),
        failed_flushes: median_u64(values.iter().map(|value| value.failed_flushes).collect()),
        oversized_batches: median_u64(values.iter().map(|value| value.oversized_batches).collect()),
    })
}

fn aggregate_latency(trials: Vec<&Latency>) -> Latency {
    Latency {
        samples: trials.iter().map(|latency| latency.samples).sum(),
        minimum_ns: median_u64(trials.iter().map(|latency| latency.minimum_ns).collect()),
        p50_ns: median_u64(trials.iter().map(|latency| latency.p50_ns).collect()),
        p95_ns: median_u64(trials.iter().map(|latency| latency.p95_ns).collect()),
        p99_ns: median_u64(trials.iter().map(|latency| latency.p99_ns).collect()),
        maximum_ns: median_u64(trials.iter().map(|latency| latency.maximum_ns).collect()),
    }
}

fn median_u64(mut values: Vec<u64>) -> u64 {
    values.sort_unstable();
    values[values.len() / 2]
}

fn median_f64(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

fn median_option(values: Vec<u64>) -> Option<u64> {
    (!values.is_empty()).then(|| median_u64(values))
}

fn sample(samples: &[Duration], percentile: f64) -> u64 {
    let index = ((samples.len() - 1) as f64 * percentile).ceil() as usize;
    nanos(samples[index])
}

fn nanos(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

fn rate(operations: usize, elapsed: Duration) -> f64 {
    operations as f64 / elapsed.as_secs_f64()
}

fn ratios(fjall: &BackendResult, native: &BackendResult) -> Ratios {
    Ratios {
        native_to_fjall_write_throughput: native.write_operations_per_second
            / fjall.write_operations_per_second,
        native_to_fjall_read_throughput: native.read_operations_per_second
            / fjall.read_operations_per_second,
        native_to_fjall_write_p95: native.write_batch_latency.p95_ns as f64
            / fjall.write_batch_latency.p95_ns.max(1) as f64,
        native_to_fjall_read_p95: native.read_latency.p95_ns as f64
            / fjall.read_latency.p95_ns.max(1) as f64,
        native_to_fjall_recovery: native.recovery_ns as f64 / fjall.recovery_ns.max(1) as f64,
        native_to_fjall_peak_rss: native
            .peak_rss_kib
            .zip(fjall.peak_rss_kib)
            .map(|(native, fjall)| native as f64 / fjall.max(1) as f64),
        native_to_fjall_disk: native.disk_bytes as f64 / fjall.disk_bytes.max(1) as f64,
    }
}

fn promotion(fjall: &BackendResult, native: &BackendResult, ratios: &Ratios) -> PromotionVerdict {
    let mut failures = Vec::new();
    if !fjall.correctness_verified || !native.correctness_verified {
        failures.push("correctness verification failed".into());
    }
    if ratios.native_to_fjall_write_throughput < 1.0 {
        failures.push("native write throughput is below Fjall".into());
    }
    if ratios.native_to_fjall_read_throughput < 1.0 {
        failures.push("native bounded-replay throughput is below Fjall".into());
    }
    if ratios.native_to_fjall_write_p95 > 1.0 {
        failures.push("native write p95 exceeds Fjall".into());
    }
    if ratios.native_to_fjall_read_p95 > 1.0 {
        failures.push("native read p95 exceeds Fjall".into());
    }
    if ratios.native_to_fjall_recovery > 1.0 {
        failures.push("native cold recovery exceeds Fjall".into());
    }
    if ratios
        .native_to_fjall_peak_rss
        .is_some_and(|ratio| ratio > 1.0)
    {
        failures.push("native peak RSS exceeds Fjall".into());
    }
    if ratios.native_to_fjall_disk > 1.0 {
        failures.push("native disk footprint exceeds Fjall".into());
    }
    PromotionVerdict {
        policy: "correctness required; native must be equal-or-better in every measured dimension"
            .into(),
        passes: failures.is_empty(),
        failures,
    }
}

fn peak_rss_kib() -> Option<u64> {
    let status = fs::read_to_string("/proc/self/status").ok()?;
    status.lines().find_map(|line| {
        line.strip_prefix("VmHWM:")?
            .split_whitespace()
            .next()?
            .parse()
            .ok()
    })
}

fn directory_bytes(path: &Path) -> Result<u64, String> {
    let mut bytes = 0u64;
    for entry in fs::read_dir(path).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let metadata = entry.metadata().map_err(|error| error.to_string())?;
        if metadata.is_dir() {
            bytes = bytes
                .checked_add(directory_bytes(&entry.path())?)
                .ok_or_else(|| "directory byte count overflowed".to_owned())?;
        } else if metadata.is_file() {
            bytes = bytes
                .checked_add(metadata.len())
                .ok_or_else(|| "directory byte count overflowed".to_owned())?;
        }
    }
    Ok(bytes)
}
