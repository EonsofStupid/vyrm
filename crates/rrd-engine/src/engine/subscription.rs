use super::*;

const SUBSCRIPTION_STATE_FORMAT: u16 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SubscriptionState {
    format_version: u16,
    subscription_id: CorrelationId,
    owner_session_id: CorrelationId,
    open_idempotency_key: CorrelationId,
    open_operation_sha256: String,
    stream: SubscriptionStream,
    stream_sha256: String,
    acknowledged_cursor: u64,
    batch_size: u64,
    max_in_flight: u16,
    retention_cursor_window: u64,
    lease_ms: u64,
    heartbeat_interval_ms: u64,
    connection_generation: u64,
    lease_expires_at_unix_ms: u64,
    next_delivery_sequence: u64,
    pending: Vec<PendingDelivery>,
    status: SubscriptionStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    closure: Option<SubscriptionClosure>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PendingDelivery {
    connection_generation: u64,
    delivery_sequence: u64,
    from_cursor: u64,
    through_cursor: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SubscriptionClosure {
    idempotency_key: CorrelationId,
    operation_sha256: String,
    closed_at_unix_ms: u64,
}

impl RrdEngine {
    #[allow(clippy::too_many_arguments)]
    pub fn open_subscription(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        idempotency_key: &CorrelationId,
        request: &OpenSubscription,
        now: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<OpenSubscriptionResult> {
        request
            .validate()
            .map_err(|error| ServiceError::Subscription(error.to_string()))?;
        let (_, session) = self.authorize(
            session_id,
            token,
            SecurityAction::SubscriptionOpen,
            now,
            request_id,
            operation_id,
        )?;
        self.authorize_session_policy(&session, stream_action(&request.stream), now)?;
        ensure_subscription_scope(self, &request.stream)?;

        let head = self.storage.runtime_cursor()?;
        validate_resume_cursor(request.after_cursor, head, request.retention_cursor_window)?;
        let operation_sha256 = operation_digest(request)?;
        let key = subscription_key(&self.instance, &request.subscription_id);
        if let Some(bytes) = self.storage.control_record(&key)? {
            let state = decode_subscription(&bytes)?;
            ensure_owner(&state, session_id)?;
            if state.open_idempotency_key != *idempotency_key
                || state.open_operation_sha256 != operation_sha256
            {
                return Err(ServiceError::IdempotencyConflict);
            }
            return Ok(OpenSubscriptionResult {
                subscription: subscription_snapshot(&state, head),
                idempotent_replay: true,
            });
        }

        let stream_sha256 = operation_digest(&request.stream)?;
        let state = SubscriptionState {
            format_version: SUBSCRIPTION_STATE_FORMAT,
            subscription_id: request.subscription_id.clone(),
            owner_session_id: session_id.clone(),
            open_idempotency_key: idempotency_key.clone(),
            open_operation_sha256: operation_sha256,
            stream: request.stream.clone(),
            stream_sha256,
            acknowledged_cursor: request.after_cursor,
            batch_size: request.batch_size,
            max_in_flight: request.max_in_flight,
            retention_cursor_window: request.retention_cursor_window,
            lease_ms: request.lease_ms,
            heartbeat_interval_ms: request.heartbeat_interval_ms,
            connection_generation: 0,
            lease_expires_at_unix_ms: now.saturating_add(request.lease_ms),
            next_delivery_sequence: 1,
            pending: Vec::new(),
            status: SubscriptionStatus::Open,
            closure: None,
        };
        replace_subscription(
            self,
            key,
            None,
            &state,
            now,
            session_id,
            "subscription.opened",
            request_id,
            operation_id,
        )?;
        Ok(OpenSubscriptionResult {
            subscription: subscription_snapshot(&state, head),
            idempotent_replay: false,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn connect_subscription(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        subscription_id: &CorrelationId,
        now: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<SubscriptionSnapshot> {
        let (_, session) = self.authorize(
            session_id,
            token,
            SecurityAction::SubscriptionConnect,
            now,
            request_id,
            operation_id,
        )?;
        let key = subscription_key(&self.instance, subscription_id);
        let bytes = self
            .storage
            .control_record(&key)?
            .ok_or(ServiceError::SubscriptionNotFound)?;
        let mut state = decode_subscription(&bytes)?;
        ensure_owner(&state, session_id)?;
        ensure_subscription_open(&state, now)?;
        self.authorize_session_policy(&session, stream_action(&state.stream), now)?;
        let head = self.storage.runtime_cursor()?;
        validate_resume_cursor(
            state.acknowledged_cursor,
            head,
            state.retention_cursor_window,
        )?;
        state.connection_generation = state
            .connection_generation
            .checked_add(1)
            .ok_or_else(|| ServiceError::Subscription("connection generation overflow".into()))?;
        state.lease_expires_at_unix_ms = now.saturating_add(state.lease_ms);
        // An interrupted connection never advances the durable ACK. Clear its
        // outstanding window so the replacement connection replays exactly.
        state.pending.clear();
        replace_subscription(
            self,
            key,
            Some(bytes),
            &state,
            now,
            session_id,
            "subscription.connected",
            request_id,
            operation_id,
        )?;
        Ok(subscription_snapshot(&state, head))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn next_subscription_frame(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        subscription_id: &CorrelationId,
        connection_generation: u64,
        now: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<SubscriptionServerFrame> {
        let (_, session) = self.authorize(
            session_id,
            token,
            SecurityAction::SubscriptionConnect,
            now,
            request_id,
            operation_id,
        )?;
        let key = subscription_key(&self.instance, subscription_id);
        let bytes = self
            .storage
            .control_record(&key)?
            .ok_or(ServiceError::SubscriptionNotFound)?;
        let mut state = decode_subscription(&bytes)?;
        ensure_owner(&state, session_id)?;
        ensure_subscription_open(&state, now)?;
        ensure_generation(&state, connection_generation)?;
        self.authorize_session_policy(&session, stream_action(&state.stream), now)?;
        let head = self.storage.runtime_cursor()?;
        validate_resume_cursor(
            state.acknowledged_cursor,
            head,
            state.retention_cursor_window,
        )?;
        if state.pending.len() >= usize::from(state.max_in_flight) {
            return Err(ServiceError::SubscriptionBackpressure);
        }
        let after_cursor = state
            .pending
            .last()
            .map(|delivery| delivery.through_cursor)
            .unwrap_or(state.acknowledged_cursor);
        if after_cursor >= head {
            return Ok(SubscriptionServerFrame::Heartbeat {
                connection_generation,
                acknowledged_cursor: state.acknowledged_cursor,
                head_cursor: head,
                lease_expires_at_unix_ms: state.lease_expires_at_unix_ms,
            });
        }

        let delivery_sequence = state.next_delivery_sequence;
        state.next_delivery_sequence = state
            .next_delivery_sequence
            .checked_add(1)
            .ok_or_else(|| ServiceError::Subscription("delivery sequence overflow".into()))?;
        let (through_cursor, frame) = match &state.stream {
            SubscriptionStream::Changefeed { scope } => {
                let page = self.read_changefeed_page(&ReadChangefeed {
                    scope: scope.clone(),
                    after_cursor,
                    limit: state.batch_size,
                })?;
                let through = page.through_cursor;
                (
                    through,
                    SubscriptionServerFrame::Changefeed {
                        connection_generation,
                        delivery_sequence,
                        page,
                    },
                )
            }
            SubscriptionStream::LiveQuery {
                scope,
                query,
                parameters,
                budget,
                max_delta_rows,
            } => {
                let delta = self.poll_live_query_page(&PollLiveQuery {
                    scope: scope.clone(),
                    query: query.clone(),
                    parameters: parameters.clone(),
                    after_cursor,
                    budget: budget.clone(),
                    max_delta_rows: *max_delta_rows,
                    wait_timeout_ms: 0,
                })?;
                let through = delta.through_cursor;
                (
                    through,
                    SubscriptionServerFrame::LiveQuery {
                        connection_generation,
                        delivery_sequence,
                        delta,
                    },
                )
            }
        };
        if through_cursor <= after_cursor {
            return Ok(SubscriptionServerFrame::Heartbeat {
                connection_generation,
                acknowledged_cursor: state.acknowledged_cursor,
                head_cursor: head,
                lease_expires_at_unix_ms: state.lease_expires_at_unix_ms,
            });
        }
        state.pending.push(PendingDelivery {
            connection_generation,
            delivery_sequence,
            from_cursor: after_cursor,
            through_cursor,
        });
        replace_subscription(
            self,
            key,
            Some(bytes),
            &state,
            now,
            session_id,
            "subscription.delivery-recorded",
            request_id,
            operation_id,
        )?;
        Ok(frame)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn apply_subscription_frame(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        subscription_id: &CorrelationId,
        frame: &SubscriptionClientFrame,
        now: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<SubscriptionSnapshot> {
        let (_, session) = self.authorize(
            session_id,
            token,
            match frame {
                SubscriptionClientFrame::Close { .. } => SecurityAction::SubscriptionClose,
                SubscriptionClientFrame::Ack { .. } | SubscriptionClientFrame::Heartbeat { .. } => {
                    SecurityAction::SubscriptionAck
                }
            },
            now,
            request_id,
            operation_id,
        )?;
        let key = subscription_key(&self.instance, subscription_id);
        let bytes = self
            .storage
            .control_record(&key)?
            .ok_or(ServiceError::SubscriptionNotFound)?;
        let mut state = decode_subscription(&bytes)?;
        ensure_owner(&state, session_id)?;
        ensure_subscription_open(&state, now)?;
        self.authorize_session_policy(&session, stream_action(&state.stream), now)?;
        let generation = match frame {
            SubscriptionClientFrame::Ack {
                connection_generation,
                ..
            }
            | SubscriptionClientFrame::Heartbeat {
                connection_generation,
            }
            | SubscriptionClientFrame::Close {
                connection_generation,
            } => *connection_generation,
        };
        ensure_generation(&state, generation)?;
        let action = match frame {
            SubscriptionClientFrame::Ack {
                delivery_sequence,
                through_cursor,
                ..
            } => {
                let position = state.pending.iter().position(|delivery| {
                    delivery.connection_generation == generation
                        && delivery.delivery_sequence == *delivery_sequence
                        && delivery.through_cursor == *through_cursor
                });
                let position = position.ok_or_else(|| {
                    ServiceError::Subscription(
                        "ACK does not identify an outstanding delivery in this connection".into(),
                    )
                })?;
                state.acknowledged_cursor = *through_cursor;
                state.pending.drain(..=position);
                state.lease_expires_at_unix_ms = now.saturating_add(state.lease_ms);
                "subscription.acknowledged"
            }
            SubscriptionClientFrame::Heartbeat { .. } => {
                state.lease_expires_at_unix_ms = now.saturating_add(state.lease_ms);
                "subscription.heartbeat-acknowledged"
            }
            SubscriptionClientFrame::Close { .. } => {
                state.status = SubscriptionStatus::Closed;
                state.pending.clear();
                "subscription.closed-by-stream"
            }
        };
        let head = self.storage.runtime_cursor()?;
        replace_subscription(
            self,
            key,
            Some(bytes),
            &state,
            now,
            session_id,
            action,
            request_id,
            operation_id,
        )?;
        Ok(subscription_snapshot(&state, head))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn close_subscription(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        idempotency_key: &CorrelationId,
        request: &CloseSubscription,
        now: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<CloseSubscriptionResult> {
        let _ = self.authorize(
            session_id,
            token,
            SecurityAction::SubscriptionClose,
            now,
            request_id,
            operation_id,
        )?;
        let key = subscription_key(&self.instance, &request.subscription_id);
        let bytes = self
            .storage
            .control_record(&key)?
            .ok_or(ServiceError::SubscriptionNotFound)?;
        let mut state = decode_subscription(&bytes)?;
        ensure_owner(&state, session_id)?;
        let digest = operation_digest(request)?;
        if let Some(closure) = &state.closure {
            if closure.idempotency_key != *idempotency_key || closure.operation_sha256 != digest {
                return Err(ServiceError::IdempotencyConflict);
            }
            return Ok(CloseSubscriptionResult {
                subscription: subscription_snapshot(&state, self.storage.runtime_cursor()?),
                idempotent_replay: true,
            });
        }
        state.status = SubscriptionStatus::Closed;
        state.pending.clear();
        state.closure = Some(SubscriptionClosure {
            idempotency_key: idempotency_key.clone(),
            operation_sha256: digest,
            closed_at_unix_ms: now,
        });
        let head = self.storage.runtime_cursor()?;
        replace_subscription(
            self,
            key,
            Some(bytes),
            &state,
            now,
            session_id,
            "subscription.closed",
            request_id,
            operation_id,
        )?;
        Ok(CloseSubscriptionResult {
            subscription: subscription_snapshot(&state, head),
            idempotent_replay: false,
        })
    }
}

fn stream_action(stream: &SubscriptionStream) -> SecurityAction {
    match stream {
        SubscriptionStream::Changefeed { .. } => SecurityAction::ChangefeedFollow,
        SubscriptionStream::LiveQuery { .. } => SecurityAction::QueryLivePoll,
    }
}

fn ensure_subscription_scope(engine: &RrdEngine, stream: &SubscriptionStream) -> Result<()> {
    let scope = match stream {
        SubscriptionStream::Changefeed { scope } | SubscriptionStream::LiveQuery { scope, .. } => {
            scope
        }
    };
    if scope != &format!("instance:{}", engine.instance) {
        return Err(ServiceError::WrongScope);
    }
    Ok(())
}

fn validate_resume_cursor(after: u64, head: u64, retention_window: u64) -> Result<()> {
    if after > head {
        return Err(ServiceError::Subscription(format!(
            "subscription resume cursor {after} exceeds runtime head {head}"
        )));
    }
    let floor = head.saturating_sub(retention_window);
    if after < floor {
        return Err(ServiceError::SubscriptionExpired {
            requested: after,
            retention_floor: floor,
            head,
        });
    }
    Ok(())
}

fn subscription_key(instance: &CanonicalId, subscription_id: &CorrelationId) -> String {
    format!(
        "server/state/{}/subscription/{}",
        instance.as_str(),
        subscription_id.as_str()
    )
}

fn decode_subscription(bytes: &[u8]) -> Result<SubscriptionState> {
    let state: SubscriptionState = serde_json::from_slice(bytes).map_err(contract_json)?;
    if state.format_version != SUBSCRIPTION_STATE_FORMAT {
        return Err(ServiceError::Subscription(format!(
            "unsupported subscription-state format {}",
            state.format_version
        )));
    }
    if state.pending.len() > usize::from(state.max_in_flight)
        || state.pending.windows(2).any(|pair| {
            pair[0].delivery_sequence >= pair[1].delivery_sequence
                || pair[0].through_cursor != pair[1].from_cursor
        })
    {
        return Err(ServiceError::Subscription(
            "durable subscription delivery window is invalid".into(),
        ));
    }
    Ok(state)
}

fn ensure_owner(state: &SubscriptionState, session_id: &CorrelationId) -> Result<()> {
    if state.owner_session_id != *session_id {
        return Err(ServiceError::PermissionDenied);
    }
    Ok(())
}

fn ensure_subscription_open(state: &SubscriptionState, now: u64) -> Result<()> {
    if state.status != SubscriptionStatus::Open {
        return Err(ServiceError::SubscriptionClosed);
    }
    if now >= state.lease_expires_at_unix_ms {
        return Err(ServiceError::SubscriptionLeaseExpired);
    }
    Ok(())
}

fn ensure_generation(state: &SubscriptionState, generation: u64) -> Result<()> {
    if generation == 0 || generation != state.connection_generation {
        return Err(ServiceError::SubscriptionConnectionReplaced);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn replace_subscription(
    engine: &RrdEngine,
    key: String,
    expected: Option<Vec<u8>>,
    state: &SubscriptionState,
    now: u64,
    session_id: &CorrelationId,
    action: &str,
    request_id: &str,
    operation_id: &str,
) -> Result<()> {
    commit_engine_control(
        &engine.storage,
        key,
        expected,
        state,
        EngineControlContext {
            now,
            session_id,
            action,
            request_id,
            operation_id,
        },
    )
}

fn subscription_snapshot(state: &SubscriptionState, head: u64) -> SubscriptionSnapshot {
    SubscriptionSnapshot {
        subscription_id: state.subscription_id.clone(),
        stream_sha256: state.stream_sha256.clone(),
        acknowledged_cursor: state.acknowledged_cursor,
        retention_floor_cursor: head.saturating_sub(state.retention_cursor_window),
        head_cursor: head,
        batch_size: state.batch_size,
        max_in_flight: state.max_in_flight,
        connection_generation: state.connection_generation,
        lease_expires_at_unix_ms: state.lease_expires_at_unix_ms,
        heartbeat_interval_ms: state.heartbeat_interval_ms,
        status: state.status,
    }
}
