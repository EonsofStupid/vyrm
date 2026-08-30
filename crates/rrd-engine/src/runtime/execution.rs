//! Durable exact-process evidence owned by the engine authority.
use rrd_store::Engine;

pub use rrd_contract::{
    ExactExecutionObservationV1, ExactExecutionRepositoryV1, ExactExecutionRequestV1,
    ExactExecutionStreamV1, EXACT_EXECUTION_CONTRACT, MAX_EXACT_EXECUTION_ARGV,
    MAX_EXACT_EXECUTION_ARG_BYTES, MAX_EXACT_EXECUTION_CHANGED_PATHS,
    MAX_EXACT_EXECUTION_OUTPUT_BYTES, MAX_EXACT_EXECUTION_TIMEOUT_MS,
};

const PROJECTION_PREFIX: &str = "exact-execution-observation-v1-";

pub fn record_exact_execution_observation<E: Engine>(
    store: &E,
    observation: &ExactExecutionObservationV1,
) -> Result<(), Box<dyn std::error::Error>> {
    observation.verify()?;
    let name = projection_name(&observation.request.request_sha256)?;
    let bytes = serde_json::to_vec(observation)?;
    if let Some(existing) = store.get_projection(&name)? {
        if existing == bytes {
            return Ok(());
        }
        return Err(
            "exact execution observation identity was already bound to different evidence".into(),
        );
    }
    store.put_projection(&name, &bytes)?;
    Ok(())
}

pub fn load_exact_execution_observation<E: Engine>(
    store: &E,
    request_sha256: &str,
) -> Result<Option<ExactExecutionObservationV1>, Box<dyn std::error::Error>> {
    let name = projection_name(request_sha256)?;
    store
        .get_projection(&name)?
        .map(|bytes| {
            let observation: ExactExecutionObservationV1 = serde_json::from_slice(&bytes)?;
            observation.verify()?;
            if observation.request.request_sha256 != request_sha256 {
                return Err("stored exact execution observation belongs to another request".into());
            }
            Ok(observation)
        })
        .transpose()
}

fn projection_name(request_sha256: &str) -> Result<String, Box<dyn std::error::Error>> {
    if request_sha256.len() != 64
        || !request_sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err("exact execution lookup requires lowercase SHA-256 hex".into());
    }
    Ok(format!("{PROJECTION_PREFIX}{request_sha256}"))
}
