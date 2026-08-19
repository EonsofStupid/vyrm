use std::path::PathBuf;
use vyrm_edge::{OfflineDocument, OfflineEdgeConfig, OfflineEdgeIndex};

fn main() {
    if let Err(error) = run() {
        eprintln!("vyrm-edge: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args().skip(1);
    let command = arguments.next().ok_or(usage())?;
    match command.as_str() {
        "build" => {
            let input = PathBuf::from(arguments.next().ok_or(usage())?);
            let output = PathBuf::from(arguments.next().ok_or(usage())?);
            let dimensions = parse_u32(arguments.next(), 384, "dimensions")?;
            let seed = parse_u64(arguments.next(), 7, "seed")?;
            reject_extra(arguments)?;
            let documents: Vec<OfflineDocument> = serde_json::from_slice(&std::fs::read(input)?)?;
            let index = OfflineEdgeIndex::build(
                OfflineEdgeConfig::standard(dimensions, seed)?,
                1,
                documents,
            )?;
            index.write_atomic(&output)?;
            println!(
                "{}",
                serde_json::to_string(&serde_json::json!({
                    "artifact": output,
                    "artifact_digest": index.artifact().descriptor().stamp.artifact_digest,
                    "source_cursor": index.artifact().descriptor().stamp.source_cursor,
                    "bytes": index.artifact().as_bytes().len(),
                    "network": "denied"
                }))?
            );
        }
        "query" => {
            let artifact = PathBuf::from(arguments.next().ok_or(usage())?);
            let text = arguments.next().ok_or(usage())?;
            let top_k = parse_usize(arguments.next(), 10, "top_k")?;
            let dimensions = parse_u32(arguments.next(), 384, "dimensions")?;
            let seed = parse_u64(arguments.next(), 7, "seed")?;
            reject_extra(arguments)?;
            let mut index = OfflineEdgeIndex::open_mmap(
                OfflineEdgeConfig::standard(dimensions, seed)?,
                artifact,
            )?;
            println!(
                "{}",
                serde_json::to_string(&index.search_text(text, top_k, 1)?)?
            );
        }
        _ => return Err(usage().into()),
    }
    Ok(())
}

fn parse_u32(value: Option<String>, default: u32, label: &str) -> Result<u32, String> {
    value
        .map(|value| value.parse().map_err(|_| format!("invalid {label}")))
        .unwrap_or(Ok(default))
}

fn parse_u64(value: Option<String>, default: u64, label: &str) -> Result<u64, String> {
    value
        .map(|value| value.parse().map_err(|_| format!("invalid {label}")))
        .unwrap_or(Ok(default))
}

fn parse_usize(value: Option<String>, default: usize, label: &str) -> Result<usize, String> {
    value
        .map(|value| value.parse().map_err(|_| format!("invalid {label}")))
        .unwrap_or(Ok(default))
}

fn reject_extra(mut arguments: impl Iterator<Item = String>) -> Result<(), String> {
    if arguments.next().is_some() {
        Err(usage().into())
    } else {
        Ok(())
    }
}

fn usage() -> &'static str {
    "usage: vyrm-edge build <documents.json> <artifact> [dimensions] [seed]\n       vyrm-edge query <artifact> <text> [top_k] [dimensions] [seed]"
}
