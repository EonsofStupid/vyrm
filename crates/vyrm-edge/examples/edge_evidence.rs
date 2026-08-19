use std::path::PathBuf;
use std::time::Instant;
use vyrm_core::RuntimeProperties;
use vyrm_edge::{OfflineDocument, OfflineEdgeConfig, OfflineEdgeIndex};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let output = PathBuf::from(args.next().ok_or("artifact path is required")?);
    let documents = args
        .next()
        .unwrap_or_else(|| "10000".into())
        .parse::<usize>()?;
    let dimensions = args.next().unwrap_or_else(|| "128".into()).parse::<u32>()?;
    if args.next().is_some() {
        return Err("usage: edge_evidence <artifact> [documents] [dimensions]".into());
    }
    let values = (0..documents)
        .map(|id| OfflineDocument {
            id: format!("doc-{id:05}"),
            text: format!(
                "document {id} modular runtime hook event partition {}",
                id % 32
            ),
            properties: RuntimeProperties::new(),
        })
        .collect::<Vec<_>>();
    let config = OfflineEdgeConfig::standard(dimensions, 7)?;
    let started = Instant::now();
    let built = OfflineEdgeIndex::build(config.clone(), 1, values)?;
    let build_ms = started.elapsed().as_secs_f64() * 1_000.0;
    built.write_atomic(&output)?;
    let artifact_bytes = built.artifact().as_bytes().len();
    drop(built);

    let opened = Instant::now();
    let mut mapped = OfflineEdgeIndex::open_mmap(config, &output)?;
    let open_ms = opened.elapsed().as_secs_f64() * 1_000.0;
    let mut latencies = Vec::new();
    let mut first = None;
    for query in 0..25 {
        let started = Instant::now();
        let result = mapped.search_text(
            format!(
                "document {} modular runtime hook event partition {}",
                query * 397 % documents,
                (query * 397 % documents) % 32
            ),
            10,
            1,
        )?;
        latencies.push(started.elapsed().as_secs_f64() * 1_000.0);
        first.get_or_insert_with(|| result.hits[0].reference.id.as_str().to_owned());
    }
    latencies.sort_by(f64::total_cmp);
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "schema": "vyrm-edge-evidence-v1",
            "documents": documents,
            "dimensions": dimensions,
            "queries": latencies.len(),
            "artifact_bytes": artifact_bytes,
            "raw_vector_bytes": documents * dimensions as usize * 4,
            "artifact_to_raw_ratio": artifact_bytes as f64 / (documents * dimensions as usize * 4) as f64,
            "build_ms": build_ms,
            "mmap_open_verify_ms": open_ms,
            "query_p50_ms": latencies[latencies.len() / 2],
            "query_p95_ms": latencies[latencies.len() * 95 / 100],
            "first_hit": first,
            "rss_kib": memory_value("VmRSS:"),
            "peak_rss_kib": memory_value("VmHWM:"),
            "network_policy": "deny",
            "backend": "vyrm:feature-hash:cpu:v1"
        }))?
    );
    Ok(())
}

fn memory_value(label: &str) -> Option<u64> {
    std::fs::read_to_string("/proc/self/status")
        .ok()?
        .lines()
        .find(|line| line.starts_with(label))?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}
