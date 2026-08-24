use super::*;

impl AppState {
    pub(super) fn with_envelope<T, O, F>(
        &self,
        headers: &HeaderMap,
        body: &[u8],
        now: u64,
        mutation: bool,
        action: SecurityAction,
        operation: F,
    ) -> HttpResponse
    where
        T: DeserializeOwned,
        O: Serialize,
        F: FnOnce(
            &RequestEnvelope<T>,
            Option<(CanonicalId, String)>,
        ) -> std::result::Result<O, ApiError>,
    {
        let attempt = HTTP_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
        let envelope =
            match parse_envelope::<T>(headers, body, now, mutation, self.service.instance_id()) {
                Ok(envelope) => envelope,
                Err(error) => {
                    let (context, error) = *error;
                    let response = failure(&context, error);
                    if self.security_enforced {
                        self.audit_response(
                            action,
                            &instance_resource(self.service.instance_id()),
                            &context,
                            None,
                            body,
                            &response,
                            now,
                            attempt,
                        )
                        .unwrap_or_else(|error| {
                            tracing::error!(error = %error, "RRD security audit append failed");
                        });
                    }
                    return response;
                }
            };
        let mut audited_principal = None;
        let identity = if self.security_enforced {
            let (principal, credential) = match api_key_identity(headers) {
                Ok(identity) => identity,
                Err(error) => {
                    let response = failure(&envelope.context, error);
                    self.audit_response(
                        action,
                        &envelope.resource,
                        &envelope.context,
                        None,
                        body,
                        &response,
                        now,
                        attempt,
                    )
                    .unwrap_or_else(|error| {
                        tracing::error!(error = %error, "RRD security audit append failed");
                    });
                    return response;
                }
            };
            if let Err(error) = self.service.authenticate_and_authorize(
                &principal,
                credential.as_bytes(),
                action,
                &envelope.resource,
                now,
            ) {
                let response = failure(&envelope.context, api_error(error));
                self.audit_response(
                    action,
                    &envelope.resource,
                    &envelope.context,
                    Some(principal),
                    body,
                    &response,
                    now,
                    attempt,
                )
                .unwrap_or_else(|error| {
                    tracing::error!(error = %error, "RRD security audit append failed");
                });
                return response;
            }
            if let Err(error) = self.audit_authorized(
                action,
                &envelope.resource,
                &envelope.context,
                Some(principal.clone()),
                body,
                now,
                attempt,
            ) {
                return failure(&envelope.context, api_error(error));
            }
            audited_principal = Some(principal.clone());
            Some((principal, credential))
        } else {
            None
        };
        let response = match operation(&envelope, identity) {
            Ok(payload) => success(StatusCode::OK, &envelope.context, payload),
            Err(error) => failure(&envelope.context, error),
        };
        if self.security_enforced {
            self.audit_response(
                action,
                &envelope.resource,
                &envelope.context,
                audited_principal,
                body,
                &response,
                now,
                attempt,
            )
            .unwrap_or_else(|error| {
                tracing::error!(error = %error, "RRD security audit append failed");
            });
        }
        response
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn with_authenticated_envelope<T, O, F>(
        &self,
        headers: &HeaderMap,
        body: &[u8],
        now: u64,
        mutation: bool,
        action: SecurityAction,
        expected_session: Option<&str>,
        operation: F,
    ) -> HttpResponse
    where
        T: DeserializeOwned,
        O: Serialize,
        F: FnOnce(
            &RequestEnvelope<T>,
            &CorrelationId,
            &CorrelationId,
        ) -> std::result::Result<O, ApiError>,
    {
        let fallback = generated_context(now, "authentication");
        let attempt = HTTP_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
        let session = match authenticated_session(headers, expected_session) {
            Ok(value) => value,
            Err(error) => {
                let response = failure(&fallback, error);
                if self.security_enforced {
                    self.audit_response(
                        action,
                        &instance_resource(self.service.instance_id()),
                        &fallback,
                        None,
                        body,
                        &response,
                        now,
                        attempt,
                    )
                    .unwrap_or_else(|error| {
                        tracing::error!(error = %error, "RRD security audit append failed");
                    });
                }
                return response;
            }
        };
        let envelope =
            match parse_envelope::<T>(headers, body, now, mutation, self.service.instance_id()) {
                Ok(envelope) => envelope,
                Err(error) => {
                    let (context, error) = *error;
                    let response = failure(&context, error);
                    if self.security_enforced {
                        self.audit_response(
                            action,
                            &instance_resource(self.service.instance_id()),
                            &context,
                            None,
                            body,
                            &response,
                            now,
                            attempt,
                        )
                        .unwrap_or_else(|error| {
                            tracing::error!(error = %error, "RRD security audit append failed");
                        });
                    }
                    return response;
                }
            };
        let principal = if self.security_enforced {
            match self.service.session_principal(
                &session.0,
                &session.1,
                now,
                envelope.context.request_id.as_str(),
                envelope.context.operation_id.as_str(),
            ) {
                Ok(Some(principal)) => Some(principal),
                Ok(None) => {
                    let response = failure(
                        &envelope.context,
                        ApiError::new(
                            ErrorCode::Unauthenticated,
                            "session is not bound to a security principal",
                            false,
                        ),
                    );
                    self.audit_response(
                        action,
                        &envelope.resource,
                        &envelope.context,
                        None,
                        body,
                        &response,
                        now,
                        attempt,
                    )
                    .unwrap_or_else(|error| {
                        tracing::error!(error = %error, "RRD security audit append failed");
                    });
                    return response;
                }
                Err(error) => {
                    let response = failure(&envelope.context, api_error(error));
                    self.audit_response(
                        action,
                        &envelope.resource,
                        &envelope.context,
                        None,
                        body,
                        &response,
                        now,
                        attempt,
                    )
                    .unwrap_or_else(|error| {
                        tracing::error!(error = %error, "RRD security audit append failed");
                    });
                    return response;
                }
            }
        } else {
            None
        };
        if let Some(principal) = &principal {
            if let Err(error) =
                self.service
                    .authorize_principal(principal, action, &envelope.resource, now)
            {
                let response = failure(&envelope.context, api_error(error));
                self.audit_response(
                    action,
                    &envelope.resource,
                    &envelope.context,
                    Some(principal.clone()),
                    body,
                    &response,
                    now,
                    attempt,
                )
                .unwrap_or_else(|error| {
                    tracing::error!(error = %error, "RRD security audit append failed");
                });
                return response;
            }
            if let Err(error) = self.audit_authorized(
                action,
                &envelope.resource,
                &envelope.context,
                Some(principal.clone()),
                body,
                now,
                attempt,
            ) {
                return failure(&envelope.context, api_error(error));
            }
        }
        let response = match operation(&envelope, &session.0, &session.1) {
            Ok(payload) => success(StatusCode::OK, &envelope.context, payload),
            Err(error) => failure(&envelope.context, error),
        };
        if self.security_enforced {
            self.audit_response(
                action,
                &envelope.resource,
                &envelope.context,
                principal,
                body,
                &response,
                now,
                attempt,
            )
            .unwrap_or_else(|error| {
                tracing::error!(error = %error, "RRD security audit append failed");
            });
        }
        response
    }
}

fn parse_envelope<T: DeserializeOwned>(
    headers: &HeaderMap,
    bytes: &[u8],
    now: u64,
    mutation: bool,
    instance: &CanonicalId,
) -> std::result::Result<RequestEnvelope<T>, Box<(RequestContext, ApiError)>> {
    let fallback = generated_context(now, "invalid-envelope");
    if !header_values(headers, "Content-Type")
        .is_ok_and(|values| values.len() == 1 && values[0].starts_with("application/json"))
    {
        return Err(Box::new((
            fallback,
            ApiError::new(
                ErrorCode::InvalidArgument,
                "Content-Type must be application/json",
                false,
            ),
        )));
    }
    if bytes.len() > RRD_MAX_BODY_BYTES {
        return Err(Box::new((
            fallback,
            ApiError::new(
                ErrorCode::ResourceExhausted,
                "request body exceeds one MiB",
                false,
            ),
        )));
    }
    let envelope: RequestEnvelope<T> = serde_json::from_slice(bytes).map_err(|error| {
        Box::new((
            fallback.clone(),
            ApiError::new(ErrorCode::InvalidArgument, error.to_string(), false),
        ))
    })?;
    envelope.validate(mutation).map_err(|error| {
        Box::new((
            envelope.context.clone(),
            ApiError::new(ErrorCode::InvalidArgument, error.to_string(), false),
        ))
    })?;
    let target = envelope
        .resource
        .segments
        .iter()
        .find(|segment| segment.kind == ResourceKind::Instance);
    if target.is_none_or(|target| target.id != *instance) {
        return Err(Box::new((
            envelope.context.clone(),
            ApiError::new(
                ErrorCode::FailedPrecondition,
                "request resource does not target this instance",
                false,
            ),
        )));
    }
    if envelope
        .context
        .deadline_unix_ms
        .is_some_and(|deadline| deadline <= now)
    {
        return Err(Box::new((
            envelope.context.clone(),
            ApiError::new(
                ErrorCode::DeadlineExceeded,
                "request deadline elapsed",
                false,
            ),
        )));
    }
    Ok(envelope)
}

pub(super) fn required_idempotency(
    context: &RequestContext,
) -> std::result::Result<&CorrelationId, ApiError> {
    context.idempotency_key.as_ref().ok_or_else(|| {
        ApiError::new(
            ErrorCode::InvalidArgument,
            "mutation requires idempotency key",
            false,
        )
    })
}
