use super::*;

macro_rules! http_dispatch_catalogue {
    ($( $variant:ident => ($operation_id:literal, $audit_operation:expr) ),+ $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
        enum HttpDispatch {
            $( $variant, )+
        }

        impl HttpDispatch {
            #[cfg(test)]
            const ALL: &'static [Self] = &[$( Self::$variant, )+];

            fn resolve(method: &Method, path: &str) -> Option<Self> {
                let method = match *method {
                    Method::GET => ContractHttpMethod::Get,
                    Method::POST => ContractHttpMethod::Post,
                    Method::DELETE => ContractHttpMethod::Delete,
                    _ => return None,
                };
                let catalogue = rrd_contract::endpoint_catalogue();
                let endpoint = catalogue.resolve_http(method, path)?;
                Self::from_operation(endpoint.operation.as_str())
            }

            fn from_operation(operation_id: &str) -> Option<Self> {
                match operation_id {
                    $( $operation_id => Some(Self::$variant), )+
                    _ => None,
                }
            }

            #[cfg(test)]
            const fn operation_id(self) -> &'static str {
                match self {
                    $( Self::$variant => $operation_id, )+
                }
            }

            fn audit_operation(self) -> RrdOperation {
                match self {
                    $( Self::$variant => $audit_operation, )+
                }
            }
        }
    };
}

http_dispatch_catalogue! {
    AuditExport => ("audit-export", RrdOperation::AuditExport),
    AuditRead => ("audit-read", RrdOperation::AuditRead),
    BackupCreate => ("backup-create", RrdOperation::BackupCreate),
    BackupList => ("backup-list", RrdOperation::BackupList),
    CapabilitiesRead => ("capabilities-read", RrdOperation::ServiceInspect),
    ChangefeedFollow => ("changefeed-follow", RrdOperation::ChangefeedFollow),
    ChangefeedRead => ("changefeed-read", RrdOperation::ChangefeedRead),
    DiagnosticsRead => ("diagnostics-read", RrdOperation::DiagnosticsRead),
    EndpointCatalogue => ("endpoint-catalogue", RrdOperation::ServiceInspect),
    EstateRead => ("estate-read", RrdOperation::EstateRead),
    HealthLive => ("health-live", RrdOperation::ServiceInspect),
    HealthReady => ("health-ready", RrdOperation::ServiceInspect),
    OpenapiRead => ("openapi-read", RrdOperation::ServiceInspect),
    QueryExecute => ("query-execute", RrdOperation::QueryExecute),
    QueryIndexEnsure => ("query-index-ensure", RrdOperation::QueryIndexEnsure),
    QueryIndexList => ("query-index-list", RrdOperation::QueryIndexList),
    QueryLivePoll => ("query-live-poll", RrdOperation::QueryLivePoll),
    RestoreCreate => ("restore-create", RrdOperation::RestoreCreate),
    RuntimeToolCatalogueRead => (
        "runtime-tool-catalogue-read",
        RrdOperation::RuntimeToolCatalogueRead
    ),
    // Tool invocation resolves its granular policy action from the versioned
    // descriptor after decoding. Transport failures before that point cannot
    // safely claim a broader operation.
    RuntimeToolInvoke => ("runtime-tool-invoke", RrdOperation::UnknownRequest),
    SessionClose => ("session-close", RrdOperation::SessionClose),
    SessionCreate => ("session-create", RrdOperation::SessionCreate),
    SessionRenew => ("session-renew", RrdOperation::SessionRenew),
    SubscriptionClose => ("subscription-close", RrdOperation::SubscriptionClose),
    SubscriptionOpen => ("subscription-open", RrdOperation::SubscriptionOpen),
    TransactionAbort => ("transaction-abort", RrdOperation::TransactionAbort),
    TransactionBegin => ("transaction-begin", RrdOperation::TransactionBegin),
    TransactionCommit => ("transaction-commit", RrdOperation::TransactionCommit),
    TransactionPreview => ("transaction-preview", RrdOperation::TransactionPreview),
    VectorCollectionEnsure => (
        "vector-collection-ensure",
        RrdOperation::VectorCollectionEnsure
    ),
    VectorCollectionList => ("vector-collection-list", RrdOperation::VectorCollectionList),
    VectorPointRetrieve => ("vector-point-retrieve", RrdOperation::VectorPointRetrieve),
    VectorPointScroll => ("vector-point-scroll", RrdOperation::VectorPointScroll),
    VectorSearch => ("vector-search", RrdOperation::VectorSearch),
}

pub(super) async fn dispatch(State(state): State<Arc<AppState>>, request: Request) -> HttpResponse {
    let now = unix_time_ms();
    let (parts, body) = request.into_parts();
    let path = parts.uri.path().to_owned();
    let operation = route_operation(&parts.method, &path);
    let body = match to_bytes(body, RRD_MAX_BODY_BYTES).await {
        Ok(body) => body,
        Err(_) => {
            let context = generated_context(now, "body-limit");
            return state.rejected_response(
                context,
                instance_resource(state.service.instance_id()),
                b"rrd-request-body-exceeded-transport-limit",
                now,
                HTTP_ID_COUNTER.fetch_add(1, Ordering::Relaxed),
                operation,
                None,
                ApiError::new(
                    ErrorCode::ResourceExhausted,
                    "request body exceeds one MiB",
                    false,
                ),
            );
        }
    };
    let handler_state = Arc::clone(&state);
    match tokio::task::spawn_blocking(move || {
        handler_state.handle(parts.method, path, parts.headers, body, now)
    })
    .await
    {
        Ok(response) => response,
        Err(error) => {
            let context = generated_context(now, "handler-join");
            state.rejected_response(
                context,
                instance_resource(state.service.instance_id()),
                b"rrd-blocking-handler-join-failed",
                now,
                HTTP_ID_COUNTER.fetch_add(1, Ordering::Relaxed),
                operation,
                None,
                ApiError::new(ErrorCode::Internal, error.to_string(), false),
            )
        }
    }
}

fn route_operation(method: &Method, path: &str) -> RrdOperation {
    HttpDispatch::resolve(method, path)
        .map(HttpDispatch::audit_operation)
        .unwrap_or(RrdOperation::UnknownRequest)
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
        let response = match HttpDispatch::resolve(&method, &path) {
            Some(HttpDispatch::HealthLive) => {
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
            Some(HttpDispatch::HealthReady) => {
                let context = generated_context(now, "health-ready");
                public_audit = Some((RrdOperation::ServiceInspect, context.clone()));
                match self.readiness(now) {
                    Ok(ready) => success(StatusCode::OK, &context, ready),
                    Err(error) => failure(&context, api_error(error)),
                }
            }
            Some(HttpDispatch::CapabilitiesRead) => {
                let context = generated_context(now, "capabilities");
                public_audit = Some((RrdOperation::ServiceInspect, context.clone()));
                success(StatusCode::OK, &context, self.capabilities.clone())
            }
            Some(HttpDispatch::EndpointCatalogue) => {
                let context = generated_context(now, "endpoint-catalogue");
                public_audit = Some((RrdOperation::ServiceInspect, context.clone()));
                success(StatusCode::OK, &context, rrd_contract::endpoint_catalogue())
            }
            Some(HttpDispatch::OpenapiRead) => {
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
            Some(HttpDispatch::SessionCreate) => self.create_session(&headers, &body, now),
            Some(HttpDispatch::SessionRenew) => self.renew_session(&headers, &body, &path, now),
            Some(HttpDispatch::SessionClose) => self.close_session(&headers, &body, &path, now),
            Some(HttpDispatch::TransactionBegin) => self.begin_transaction(&headers, &body, now),
            Some(HttpDispatch::QueryExecute) => self.execute_query(&headers, &body, now),
            Some(HttpDispatch::QueryIndexEnsure) => self.ensure_query_index(&headers, &body, now),
            Some(HttpDispatch::QueryIndexList) => self.list_query_indexes(&headers, &body, now),
            Some(HttpDispatch::QueryLivePoll) => self.poll_live_query(&headers, &body, now),
            Some(HttpDispatch::RuntimeToolCatalogueRead) => {
                self.list_runtime_tools(&headers, &body, now)
            }
            Some(HttpDispatch::RuntimeToolInvoke) => self.invoke_runtime_tool(&headers, &body, now),
            Some(HttpDispatch::BackupCreate) => self.create_instance_backup(&headers, &body, now),
            Some(HttpDispatch::BackupList) => self.list_instance_backups(&headers, &body, now),
            Some(HttpDispatch::RestoreCreate) => self.restore_instance_backup(&headers, &body, now),
            Some(HttpDispatch::AuditRead) => self.read_audit(&headers, &body, now),
            Some(HttpDispatch::AuditExport) => self.export_audit(&headers, &body, now),
            Some(HttpDispatch::ChangefeedRead) => self.read_changefeed(&headers, &body, now),
            Some(HttpDispatch::ChangefeedFollow) => self.follow_changefeed(&headers, &body, now),
            Some(HttpDispatch::SubscriptionOpen) => self.open_subscription(&headers, &body, now),
            Some(HttpDispatch::SubscriptionClose) => self.close_subscription(&headers, &body, now),
            Some(HttpDispatch::DiagnosticsRead) => {
                self.read_diagnostic_snapshot(&headers, &body, now)
            }
            Some(HttpDispatch::VectorCollectionEnsure) => {
                self.ensure_vector_collection(&headers, &body, now)
            }
            Some(HttpDispatch::VectorCollectionList) => {
                self.list_vector_collections(&headers, &body, now)
            }
            Some(HttpDispatch::VectorPointScroll) => {
                self.scroll_vector_points(&headers, &body, now)
            }
            Some(HttpDispatch::VectorPointRetrieve) => {
                self.retrieve_vector_points(&headers, &body, now)
            }
            Some(HttpDispatch::VectorSearch) => self.search_vectors(&headers, &body, now),
            Some(HttpDispatch::EstateRead) => self.read_estate(&headers, &body, &path, now),
            Some(HttpDispatch::TransactionPreview) => {
                self.preview_transaction(&headers, &body, &path, now)
            }
            Some(HttpDispatch::TransactionCommit) => {
                self.commit_transaction(&headers, &body, &path, now)
            }
            Some(HttpDispatch::TransactionAbort) => {
                self.abort_transaction(&headers, &body, &path, now)
            }
            None => {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn every_catalogued_http_operation_resolves_to_one_executable_dispatch() {
        let catalogue = rrd_contract::endpoint_catalogue();
        let mut resolved = BTreeSet::new();
        for endpoint in &catalogue.endpoints {
            let method = match endpoint.method {
                ContractHttpMethod::Get => Method::GET,
                ContractHttpMethod::Post => Method::POST,
                ContractHttpMethod::Delete => Method::DELETE,
            };
            let path = concrete_path(&endpoint.path);
            let dispatch = HttpDispatch::resolve(&method, &path).unwrap_or_else(|| {
                panic!(
                    "catalogued operation {} has no server dispatch",
                    endpoint.operation
                )
            });
            assert_eq!(dispatch.operation_id(), endpoint.operation.as_str());
            assert!(resolved.insert(dispatch.operation_id()));
            assert_eq!(route_operation(&method, &path), dispatch.audit_operation());
        }

        let declared = catalogue
            .endpoints
            .iter()
            .map(|endpoint| endpoint.operation.as_str())
            .collect::<BTreeSet<_>>();
        let executable = HttpDispatch::ALL
            .iter()
            .map(|dispatch| dispatch.operation_id())
            .collect::<BTreeSet<_>>();
        assert_eq!(executable, declared);
    }

    #[test]
    fn route_resolution_rejects_unknown_methods_paths_and_empty_parameters() {
        assert!(HttpDispatch::resolve(&Method::PUT, "/v1/query").is_none());
        assert!(HttpDispatch::resolve(&Method::GET, "/v1/not-real").is_none());
        assert!(HttpDispatch::resolve(&Method::POST, "/v1/estates//read").is_none());
        assert!(HttpDispatch::resolve(&Method::POST, "/v1/estates/a/b/read").is_none());
    }

    fn concrete_path(template: &str) -> String {
        template
            .split('/')
            .map(|segment| {
                if segment.starts_with('{') && segment.ends_with('}') {
                    "fixture"
                } else {
                    segment
                }
            })
            .collect::<Vec<_>>()
            .join("/")
    }
}
