use super::*;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::Path;

pub(super) async fn subscription_websocket_upgrade(
    State(state): State<Arc<AppState>>,
    Path(subscription): Path<String>,
    headers: HeaderMap,
    websocket: WebSocketUpgrade,
) -> HttpResponse {
    let now = unix_time_ms();
    let mut context = generated_context(now, "subscription-connect");
    context.idempotency_key = Some(context.operation_id.clone());
    let subscription_id = match CorrelationId::new(subscription) {
        Ok(subscription) => subscription,
        Err(error) => {
            return failure(
                &context,
                ApiError::new(ErrorCode::InvalidArgument, error.to_string(), false),
            );
        }
    };
    let (session_id, token) = match authenticated_session(&headers, None) {
        Ok(session) => session,
        Err(error) => return failure(&context, error),
    };
    let authorized = match state.service.begin_invocation(
        Invocation {
            context: context.clone(),
            resource: instance_resource(state.service.instance_id()),
            observed_at_unix_ms: now,
            attempt: HTTP_ID_COUNTER.fetch_add(1, Ordering::Relaxed),
            request_sha256: sha256_hex(subscription_id.as_str().as_bytes()),
        },
        RrdOperation::SubscriptionConnect,
        InvocationCredential::Session {
            session_id: &session_id,
            token: &token,
        },
    ) {
        Ok(authorized) => authorized,
        Err(error) => return failure(&context, api_error(error)),
    };
    let snapshot = state.service.connect_subscription(
        &session_id,
        &token,
        &subscription_id,
        now,
        context.request_id.as_str(),
        context.operation_id.as_str(),
    );
    let completion = match &snapshot {
        Ok(snapshot) => InvocationCompletion {
            decision: AuditDecision::Allowed,
            // The audit contract records completed logical outcomes (2xx+),
            // while the HTTP handshake itself returns 101 Switching Protocols.
            status_code: StatusCode::OK.as_u16(),
            response_sha256: sha256_hex(
                &serde_json::to_vec(snapshot).expect("subscription snapshot serializes"),
            ),
        },
        Err(error) => InvocationCompletion {
            decision: if matches!(
                error.kind(),
                ServiceErrorKind::Unauthenticated | ServiceErrorKind::PermissionDenied
            ) {
                AuditDecision::Denied
            } else {
                AuditDecision::Failed
            },
            status_code: service_status(error.kind()).as_u16(),
            response_sha256: sha256_hex(error.to_string().as_bytes()),
        },
    };
    if let Err(error) = state.service.complete_invocation(&authorized, completion) {
        return failure(&context, api_error(error));
    }
    let snapshot = match snapshot {
        Ok(snapshot) => snapshot,
        Err(error) => return failure(&context, api_error(error)),
    };
    websocket
        .max_message_size(64 * 1024)
        .max_frame_size(64 * 1024)
        .on_upgrade(move |socket| {
            serve_subscription_socket(socket, state, session_id, token, subscription_id, snapshot)
        })
}

async fn serve_subscription_socket(
    mut socket: WebSocket,
    state: Arc<AppState>,
    session_id: CorrelationId,
    token: CorrelationId,
    subscription_id: CorrelationId,
    snapshot: rrd_contract::SubscriptionSnapshot,
) {
    let generation = snapshot.connection_generation;
    let heartbeat_interval_ms = snapshot.heartbeat_interval_ms;
    let mut acknowledged_cursor = snapshot.acknowledged_cursor;
    if send_frame(
        &mut socket,
        &SubscriptionServerFrame::Opened {
            subscription: snapshot,
        },
    )
    .await
    .is_err()
    {
        return;
    }
    let mut interval =
        tokio::time::interval(std::time::Duration::from_millis(heartbeat_interval_ms));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    interval.tick().await;
    loop {
        tokio::select! {
            inbound = socket.recv() => {
                match inbound {
                    Some(Ok(Message::Text(text))) => {
                        let frame = match serde_json::from_str::<SubscriptionClientFrame>(&text) {
                            Ok(frame) => frame,
                            Err(error) => {
                                let _ = send_frame(&mut socket, &SubscriptionServerFrame::Error {
                                    error: ErrorBody {
                                        code: ErrorCode::InvalidArgument,
                                        message: format!("invalid subscription client frame: {error}"),
                                        retryable: false,
                                        details: BTreeMap::new(),
                                    },
                                    acknowledged_cursor,
                                }).await;
                                break;
                            }
                        };
                        let close_requested = matches!(frame, SubscriptionClientFrame::Close { .. });
                        let now = unix_time_ms();
                        let mut context = generated_context(now, "subscription-client-frame");
                        context.idempotency_key = Some(context.operation_id.clone());
                        let state_for_call = Arc::clone(&state);
                        let session_for_call = session_id.clone();
                        let token_for_call = token.clone();
                        let subscription_for_call = subscription_id.clone();
                        let result = tokio::task::spawn_blocking(move || {
                            invoke_client_frame(
                                &state_for_call,
                                &session_for_call,
                                &token_for_call,
                                &subscription_for_call,
                                &frame,
                                now,
                                context,
                            )
                        }).await;
                        match result {
                            Ok(Ok(snapshot)) if close_requested => {
                                acknowledged_cursor = snapshot.acknowledged_cursor;
                                let _ = send_frame(&mut socket, &SubscriptionServerFrame::Closed {
                                    acknowledged_cursor,
                                    reason: "client_closed".into(),
                                }).await;
                                break;
                            }
                            Ok(Ok(snapshot)) => {
                                acknowledged_cursor = snapshot.acknowledged_cursor;
                                if send_frame(&mut socket, &SubscriptionServerFrame::Acknowledged {
                                    subscription: snapshot,
                                }).await.is_err() {
                                    break;
                                }
                            }
                            Ok(Err(error)) => {
                                let _ = send_frame(&mut socket, &SubscriptionServerFrame::Error {
                                    error: websocket_error(error),
                                    acknowledged_cursor,
                                }).await;
                                break;
                            }
                            Err(error) => {
                                let _ = send_frame(&mut socket, &SubscriptionServerFrame::Error {
                                    error: ErrorBody {
                                        code: ErrorCode::Internal,
                                        message: error.to_string(),
                                        retryable: false,
                                        details: BTreeMap::new(),
                                    },
                                    acknowledged_cursor,
                                }).await;
                                break;
                            }
                        }
                    }
                    Some(Ok(Message::Ping(bytes))) => {
                        if socket.send(Message::Pong(bytes)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Pong(_))) => {}
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    Some(Ok(Message::Binary(_))) => {
                        let _ = socket.send(Message::Close(None)).await;
                        break;
                    }
                }
            }
            _ = interval.tick() => {
                let now = unix_time_ms();
                let context = generated_context(now, "subscription-delivery");
                let state_for_call = Arc::clone(&state);
                let session_for_call = session_id.clone();
                let token_for_call = token.clone();
                let subscription_for_call = subscription_id.clone();
                let result = tokio::task::spawn_blocking(move || {
                    state_for_call.service.next_subscription_frame(
                        &session_for_call,
                        &token_for_call,
                        &subscription_for_call,
                        generation,
                        now,
                        context.request_id.as_str(),
                        context.operation_id.as_str(),
                    )
                }).await;
                match result {
                    Ok(Ok(frame)) => {
                        if send_frame(&mut socket, &frame).await.is_err() {
                            break;
                        }
                    }
                    Ok(Err(ServiceError::SubscriptionBackpressure)) => {}
                    Ok(Err(error)) => {
                        let _ = send_frame(&mut socket, &SubscriptionServerFrame::Error {
                            error: websocket_error(error),
                            acknowledged_cursor,
                        }).await;
                        break;
                    }
                    Err(error) => {
                        let _ = send_frame(&mut socket, &SubscriptionServerFrame::Error {
                            error: ErrorBody {
                                code: ErrorCode::Internal,
                                message: error.to_string(),
                                retryable: false,
                                details: BTreeMap::new(),
                            },
                            acknowledged_cursor,
                        }).await;
                        break;
                    }
                }
            }
        }
    }
    let _ = socket.send(Message::Close(None)).await;
}

async fn send_frame(
    socket: &mut WebSocket,
    frame: &SubscriptionServerFrame,
) -> std::result::Result<(), ()> {
    let text = serde_json::to_string(frame).map_err(|_| ())?;
    socket
        .send(Message::Text(text.into()))
        .await
        .map_err(|_| ())
}

fn service_status(kind: ServiceErrorKind) -> StatusCode {
    match kind {
        ServiceErrorKind::InvalidArgument => StatusCode::BAD_REQUEST,
        ServiceErrorKind::NotFound => StatusCode::NOT_FOUND,
        ServiceErrorKind::Unauthenticated => StatusCode::UNAUTHORIZED,
        ServiceErrorKind::PermissionDenied => StatusCode::FORBIDDEN,
        ServiceErrorKind::Conflict => StatusCode::CONFLICT,
        ServiceErrorKind::FailedPrecondition => StatusCode::PRECONDITION_FAILED,
        ServiceErrorKind::ResourceExhausted => StatusCode::TOO_MANY_REQUESTS,
        ServiceErrorKind::DeadlineExceeded => StatusCode::GATEWAY_TIMEOUT,
        ServiceErrorKind::Internal => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

fn invoke_client_frame(
    state: &AppState,
    session_id: &CorrelationId,
    token: &CorrelationId,
    subscription_id: &CorrelationId,
    frame: &SubscriptionClientFrame,
    now: u64,
    context: RequestContext,
) -> std::result::Result<rrd_contract::SubscriptionSnapshot, ServiceError> {
    let operation = match frame {
        SubscriptionClientFrame::Close { .. } => RrdOperation::SubscriptionClose,
        SubscriptionClientFrame::Ack { .. } | SubscriptionClientFrame::Heartbeat { .. } => {
            RrdOperation::SubscriptionAck
        }
    };
    let authorized = state.service.begin_invocation(
        Invocation {
            context: context.clone(),
            resource: instance_resource(state.service.instance_id()),
            observed_at_unix_ms: now,
            attempt: HTTP_ID_COUNTER.fetch_add(1, Ordering::Relaxed),
            request_sha256: sha256_hex(
                &serde_json::to_vec(frame).expect("subscription client frame serializes"),
            ),
        },
        operation,
        InvocationCredential::Session { session_id, token },
    )?;
    let result = state.service.apply_subscription_frame(
        session_id,
        token,
        subscription_id,
        frame,
        now,
        context.request_id.as_str(),
        context.operation_id.as_str(),
    );
    let completion = match &result {
        Ok(snapshot) => InvocationCompletion {
            decision: AuditDecision::Allowed,
            status_code: StatusCode::OK.as_u16(),
            response_sha256: sha256_hex(
                &serde_json::to_vec(snapshot).expect("subscription snapshot serializes"),
            ),
        },
        Err(error) => InvocationCompletion {
            decision: if matches!(
                error.kind(),
                ServiceErrorKind::Unauthenticated | ServiceErrorKind::PermissionDenied
            ) {
                AuditDecision::Denied
            } else {
                AuditDecision::Failed
            },
            status_code: service_status(error.kind()).as_u16(),
            response_sha256: sha256_hex(error.to_string().as_bytes()),
        },
    };
    state.service.complete_invocation(&authorized, completion)?;
    result
}
