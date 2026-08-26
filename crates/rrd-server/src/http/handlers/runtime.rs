use super::super::*;
use crate::http::envelope::parse_envelope;

impl AppState {
    pub(in crate::http) fn list_runtime_tools(
        &self,
        headers: &HeaderMap,
        body: &[u8],
        now: u64,
    ) -> HttpResponse {
        self.with_authenticated_envelope::<ListRuntimeTools, _, _>(
            headers,
            body,
            now,
            RrdOperation::RuntimeToolCatalogueRead,
            None,
            |_envelope, _session, _token| {
                let catalogue = runtime_tool_contract_catalogue();
                catalogue.validate().map_err(|error| {
                    ApiError::new(ErrorCode::Internal, error.to_string(), false)
                })?;
                Ok(catalogue)
            },
        )
    }

    pub(in crate::http) fn invoke_runtime_tool(
        &self,
        headers: &HeaderMap,
        body: &[u8],
        now: u64,
    ) -> HttpResponse {
        let attempt = HTTP_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
        let envelope = match parse_envelope::<RuntimeToolInvocation>(
            headers,
            body,
            now,
            false,
            self.service.instance_id(),
        ) {
            Ok(envelope) => envelope,
            Err(error) => {
                let (context, error) = *error;
                return self.rejected_response(
                    context,
                    instance_resource(self.service.instance_id()),
                    body,
                    now,
                    attempt,
                    RrdOperation::UnknownRequest,
                    None,
                    error,
                );
            }
        };

        let tool_name = envelope.payload.tool.as_str();
        let catalogue = runtime_tool_contract_catalogue();
        let Some(descriptor) = catalogue
            .tools
            .iter()
            .find(|descriptor| descriptor.name.as_str() == tool_name)
        else {
            return self.rejected_response(
                envelope.context,
                envelope.resource,
                body,
                now,
                attempt,
                RrdOperation::UnknownRequest,
                None,
                ApiError::new(ErrorCode::NotFound, "runtime tool not found", false),
            );
        };
        let operation = match runtime_tool_operation(tool_name) {
            Ok(operation) => operation,
            Err(error) => return failure(&envelope.context, api_error(error)),
        };
        if let Err(error) = envelope.validate(descriptor.mutation) {
            return self.rejected_response(
                envelope.context,
                envelope.resource,
                body,
                now,
                attempt,
                operation,
                None,
                ApiError::new(ErrorCode::InvalidArgument, error.to_string(), false),
            );
        }
        let session = match authenticated_session(headers, None) {
            Ok(session) => session,
            Err(error) => {
                return self.rejected_response(
                    envelope.context,
                    envelope.resource,
                    body,
                    now,
                    attempt,
                    operation,
                    None,
                    error,
                );
            }
        };
        let Some(project_root) = self.project_root.as_deref() else {
            return self.rejected_response(
                envelope.context,
                envelope.resource,
                body,
                now,
                attempt,
                operation,
                None,
                ApiError::new(
                    ErrorCode::FailedPrecondition,
                    "runtime tools require a persisted project authority binding",
                    false,
                ),
            );
        };

        let request_sha256 = match serde_json::to_vec(&envelope.payload) {
            Ok(payload) => sha256_hex(&payload),
            Err(error) => {
                return self.rejected_response(
                    envelope.context,
                    envelope.resource,
                    body,
                    now,
                    attempt,
                    operation,
                    None,
                    ApiError::new(ErrorCode::InvalidArgument, error.to_string(), false),
                );
            }
        };
        let invocation = Invocation {
            context: envelope.context.clone(),
            resource: envelope.resource.clone(),
            observed_at_unix_ms: now,
            attempt,
            request_sha256,
        };
        match self.service.invoke_runtime_tool(
            project_root,
            &envelope.payload,
            invocation,
            InvocationCredential::Session {
                session_id: &session.0,
                token: &session.1,
            },
        ) {
            Ok(result) => success(StatusCode::OK, &envelope.context, result),
            Err(error) => failure(&envelope.context, api_error(error)),
        }
    }
}
