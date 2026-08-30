use super::super::policy::tool_request_digest;
use super::super::{
    consume_attuned_tool_authorization, consume_lifecycle_tool_authorization, handle, HookContext,
    HookEvent,
};
use super::RuntimeToolLifecyclePolicy;
use crate::ServiceError;
use rrd_core::Reader;
use rrd_store::Engine;
use schemars::{schema_for, JsonSchema};
use serde_json::{json, Value};
use std::path::Path;

#[derive(Debug, Clone, serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RuntimeToolLifecycleCoordinates {
    /// Active canonical lifecycle session already advanced through
    /// plan.recorded for the checked-in work item.
    session_id: String,
    /// Stable identity for this one runtime-tool call. Retries must retain it.
    tool_call_id: String,
}

pub(super) struct RuntimeToolLifecycle {
    input: Value,
    exact: bool,
}

impl RuntimeToolLifecycle {
    pub(super) fn prepare(
        name: &str,
        args: &Value,
        lifecycle: RuntimeToolLifecyclePolicy,
    ) -> crate::Result<(Value, Self)> {
        let exact = lifecycle == RuntimeToolLifecyclePolicy::PlannedMutation;
        let mut execution_args = args.clone();
        let coordinates = if exact {
            execution_args
                .as_object_mut()
                .ok_or_else(|| {
                    ServiceError::Contract("runtime tool arguments must be an object".into())
                })?
                .remove("rrflow_lifecycle")
                .map(|value| {
                    serde_json::from_value::<RuntimeToolLifecycleCoordinates>(value)
                        .map_err(|error| ServiceError::Contract(error.to_string()))
                })
                .transpose()?
        } else {
            None
        };
        let mut input = json!({"tool_name": name, "tool_input": args});
        if let Some(coordinates) = coordinates {
            let object = input
                .as_object_mut()
                .expect("runtime lifecycle input is an object");
            object.insert("session_id".into(), Value::String(coordinates.session_id));
            object.insert(
                "tool_use_id".into(),
                Value::String(coordinates.tool_call_id),
            );
        }
        Ok((execution_args, Self { input, exact }))
    }

    pub(super) fn authorize<E: Engine>(
        &self,
        store: &E,
        root: &Path,
        at: u64,
    ) -> crate::Result<()> {
        if !self.exact {
            return Ok(());
        }
        let reader = Reader::new("agent:mcp-dispatch")
            .map_err(|error| ServiceError::Runtime(error.to_string()))?;
        let authorization = handle(
            &HookContext {
                store,
                root,
                harness: Some("rrflow-mcp-dispatch"),
                reader: &reader,
                now: at,
                budget: 1_500,
            },
            HookEvent::PreToolUse,
            &self.input,
        )
        .map_err(|error| ServiceError::Runtime(error.to_string()))?;
        if !authorization.stdout.is_empty() {
            return Err(ServiceError::Runtime(authorization.stdout));
        }
        match (
            authorization.lifecycle_context.as_ref(),
            authorization.lifecycle_authorization.as_ref(),
        ) {
            (Some(supervisor), Some(tool)) => {
                consume_lifecycle_tool_authorization(store, supervisor, tool, at)
                    .map_err(|error| ServiceError::Runtime(error.to_string()))?;
            }
            (None, None) => {
                consume_attuned_tool_authorization(
                    store,
                    root,
                    &tool_request_digest(&self.input)
                        .map_err(|error| ServiceError::Runtime(error.to_string()))?,
                    at,
                    "agent:mcp-dispatch",
                )
                .map_err(|error| ServiceError::Runtime(error.to_string()))?;
            }
            _ => {
                return Err(ServiceError::Runtime(
                    "runtime lifecycle gate returned a partial authorization".into(),
                ))
            }
        }
        Ok(())
    }

    pub(super) fn complete<E: Engine>(
        self,
        store: &E,
        root: &Path,
        response: Value,
        at: u64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if !self.exact {
            return Ok(());
        }
        let mut post_input = self.input;
        post_input
            .as_object_mut()
            .expect("runtime lifecycle input is an object")
            .insert("tool_response".into(), response);
        let reader = Reader::new("agent:mcp-dispatch")?;
        handle(
            &HookContext {
                store,
                root,
                harness: Some("rrflow-mcp-dispatch"),
                reader: &reader,
                now: at,
                budget: 1_500,
            },
            HookEvent::PostToolUse,
            &post_input,
        )?;
        Ok(())
    }
}

pub(super) fn add_coordinates_schema(schema: &mut Value) {
    let coordinates = serde_json::to_value(schema_for!(RuntimeToolLifecycleCoordinates))
        .expect("lifecycle coordinate schema must serialize");
    schema
        .as_object_mut()
        .and_then(|object| object.get_mut("properties"))
        .and_then(Value::as_object_mut)
        .expect("generated object schema has properties")
        .insert("rrflow_lifecycle".into(), coordinates);
}
