use super::*;

pub(super) async fn dispatch(State(state): State<Arc<AppState>>, request: Request) -> HttpResponse {
    let now = unix_time_ms();
    let (parts, body) = request.into_parts();
    let body = match to_bytes(body, RRD_MAX_BODY_BYTES).await {
        Ok(body) => body,
        Err(_) => {
            let context = generated_context(now, "body-limit");
            return failure(
                &context,
                ApiError::new(
                    ErrorCode::ResourceExhausted,
                    "request body exceeds one MiB",
                    false,
                ),
            );
        }
    };
    let path = parts.uri.path().to_owned();
    match tokio::task::spawn_blocking(move || {
        state.handle(parts.method, path, parts.headers, body, now)
    })
    .await
    {
        Ok(response) => response,
        Err(error) => {
            let context = generated_context(now, "handler-join");
            failure(
                &context,
                ApiError::new(ErrorCode::Internal, error.to_string(), false),
            )
        }
    }
}

impl AppState {
    fn handle(
        &self,
        method: Method,
        path: String,
        headers: HeaderMap,
        body: Bytes,
        now: u64,
    ) -> HttpResponse {
        let span = tracing::info_span!(
            "rrd.http.request",
            method = %method,
            path = %path,
            status = tracing::field::Empty,
        );
        let _entered = span.enter();
        let mut public_audit = None;
        let response = match (method, path.as_str()) {
            (Method::GET, "/v1/health/live") => {
                let context = generated_context(now, "health-live");
                public_audit = Some((RrdOperation::ServiceInspect, context.clone()));
                success(
                    StatusCode::OK,
                    &context,
                    Liveness {
                        observed_at_unix_ms: now,
                    },
                )
            }
            (Method::GET, "/v1/health/ready") => {
                let context = generated_context(now, "health-ready");
                public_audit = Some((RrdOperation::ServiceInspect, context.clone()));
                match self.readiness(now) {
                    Ok(ready) => success(StatusCode::OK, &context, ready),
                    Err(error) => failure(&context, api_error(error)),
                }
            }
            (Method::GET, "/v1/capabilities") => {
                let context = generated_context(now, "capabilities");
                public_audit = Some((RrdOperation::ServiceInspect, context.clone()));
                success(StatusCode::OK, &context, self.capabilities.clone())
            }
            (Method::GET, "/v1/schema/endpoints") => {
                let context = generated_context(now, "endpoint-catalogue");
                public_audit = Some((RrdOperation::ServiceInspect, context.clone()));
                success(StatusCode::OK, &context, rrd_contract::endpoint_catalogue())
            }
            (Method::GET, "/v1/schema/openapi") => {
                let context = generated_context(now, "openapi-read");
                public_audit = Some((RrdOperation::ServiceInspect, context.clone()));
                match rrd_contract::openapi_document() {
                    Ok(document) => success(StatusCode::OK, &context, document),
                    Err(error) => failure(
                        &context,
                        ApiError::new(ErrorCode::Internal, error.to_string(), false),
                    ),
                }
            }
            (Method::POST, "/v1/sessions") => self.create_session(&headers, &body, now),
            (Method::POST, path) if session_action(path, "renew").is_some() => {
                self.renew_session(&headers, &body, path, now)
            }
            (Method::DELETE, path) if session_id(path).is_some() => {
                self.close_session(&headers, &body, path, now)
            }
            (Method::POST, "/v1/transactions") => self.begin_transaction(&headers, &body, now),
            (Method::POST, "/v1/query") => self.execute_query(&headers, &body, now),
            (Method::POST, "/v1/query/indexes/ensure") => {
                self.ensure_query_index(&headers, &body, now)
            }
            (Method::POST, "/v1/query/indexes/list") => {
                self.list_query_indexes(&headers, &body, now)
            }
            (Method::POST, "/v1/query/live/poll") => self.poll_live_query(&headers, &body, now),
            (Method::POST, "/v1/runtime/tools/list") => {
                self.list_runtime_tools(&headers, &body, now)
            }
            (Method::POST, "/v1/runtime/tools/invoke") => {
                self.invoke_runtime_tool(&headers, &body, now)
            }
            (Method::POST, "/v1/backups") => self.create_instance_backup(&headers, &body, now),
            (Method::POST, "/v1/backups/list") => self.list_instance_backups(&headers, &body, now),
            (Method::POST, "/v1/restores") => self.restore_instance_backup(&headers, &body, now),
            (Method::POST, "/v1/audit/read") => self.read_audit(&headers, &body, now),
            (Method::POST, "/v1/changes/read") => self.read_changefeed(&headers, &body, now),
            (Method::POST, "/v1/changes/follow") => self.follow_changefeed(&headers, &body, now),
            (Method::POST, "/v1/diagnostics/read") => {
                self.read_diagnostic_snapshot(&headers, &body, now)
            }
            (Method::POST, "/v1/vector/collections/ensure") => {
                self.ensure_vector_collection(&headers, &body, now)
            }
            (Method::POST, "/v1/vector/collections/list") => {
                self.list_vector_collections(&headers, &body, now)
            }
            (Method::POST, "/v1/vector/points/scroll") => {
                self.scroll_vector_points(&headers, &body, now)
            }
            (Method::POST, "/v1/vector/points/retrieve") => {
                self.retrieve_vector_points(&headers, &body, now)
            }
            (Method::POST, "/v1/vector/search") => self.search_vectors(&headers, &body, now),
            (Method::POST, path) if estate_action(path, "read").is_some() => {
                self.read_estate(&headers, &body, path, now)
            }
            (Method::POST, path) if transaction_action(path, "preview").is_some() => {
                self.preview_transaction(&headers, &body, path, now)
            }
            (Method::POST, path) if transaction_action(path, "commit").is_some() => {
                self.commit_transaction(&headers, &body, path, now)
            }
            (Method::DELETE, path) if transaction_id(path).is_some() => {
                self.abort_transaction(&headers, &body, path, now)
            }
            _ => {
                let context = generated_context(now, "not-found");
                public_audit = Some((RrdOperation::UnknownRequest, context.clone()));
                failure(
                    &context,
                    ApiError::new(ErrorCode::NotFound, "endpoint not found", false),
                )
            }
        };
        let response = if let Some((operation, context)) = public_audit {
            let invocation = Invocation {
                context: context.clone(),
                resource: instance_resource(self.service.instance_id()),
                observed_at_unix_ms: now,
                attempt: HTTP_ID_COUNTER.fetch_add(1, Ordering::Relaxed),
                request_sha256: sha256_hex(&body),
            };
            match self.service.record_public_invocation(
                invocation,
                operation,
                invocation_completion(&response),
            ) {
                Ok(()) => response,
                Err(error) => failure(&context, api_error(error)),
            }
        } else {
            response
        };
        span.record("status", response.status().as_u16());
        response
    }

    fn readiness(&self, now: u64) -> std::result::Result<Readiness, ServiceError> {
        self.service.readiness(now)
    }
}
