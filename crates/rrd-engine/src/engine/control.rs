use super::*;

pub(in crate::engine) struct EngineControlContext<'a> {
    pub now: u64,
    pub session_id: &'a CorrelationId,
    pub action: &'a str,
    pub request_id: &'a str,
    pub operation_id: &'a str,
}

pub(in crate::engine) fn commit_engine_control(
    engine: &impl Engine,
    key: String,
    expected: Option<Vec<u8>>,
    state: &impl Serialize,
    context: EngineControlContext<'_>,
) -> Result<()> {
    engine.commit_control_transition(&ControlTransition {
        key,
        expected,
        replacement: Some(serde_json::to_vec(state).map_err(contract_json)?),
        at: context.now,
        actor: format!("session:{}", context.session_id.as_str()),
        action: context.action.into(),
        request_id: context.request_id.into(),
        operation_id: context.operation_id.into(),
    })?;
    Ok(())
}

pub(in crate::engine) fn operation_digest(value: &impl Serialize) -> Result<String> {
    Ok(digest::sha256_hex(
        &serde_json::to_vec(value).map_err(contract_json)?,
    ))
}

pub(in crate::engine) fn contract_json(error: serde_json::Error) -> ServiceError {
    ServiceError::Contract(error.to_string())
}
