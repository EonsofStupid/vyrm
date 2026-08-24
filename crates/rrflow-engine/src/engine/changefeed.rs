use super::*;

impl RrflowEngine {
    pub fn read_changefeed(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        request: &ReadChangefeed,
        now: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<ChangefeedPage> {
        self.authorize(
            session_id,
            token,
            SecurityAction::ChangefeedRead,
            now,
            request_id,
            operation_id,
        )?;
        self.read_changefeed_page(request)
    }

    fn read_changefeed_page(&self, request: &ReadChangefeed) -> Result<ChangefeedPage> {
        request
            .validate()
            .map_err(|error| ServiceError::Changefeed(error.to_string()))?;
        let expected_scope = format!("instance:{}", self.instance);
        if request.scope != expected_scope {
            return Err(ServiceError::WrongScope);
        }
        let scope = ScopeId::new(request.scope.clone()).map_err(core_changefeed)?;
        let limit = usize::try_from(request.limit)
            .map_err(|_| ServiceError::Changefeed("changefeed limit exceeds usize".into()))?;
        let page = self
            .storage
            .runtime_changes_since(request.after_cursor, limit, Some(&scope))?;
        Ok(ChangefeedPage {
            requested_after_cursor: page.requested_after,
            through_cursor: page.through_cursor,
            head_cursor: page.head_cursor,
            has_more: page.has_more(),
            validation: ChangefeedValidation {
                method: page.validation.method,
                change_reads: page.validation.change_reads,
                proof_nodes: page.validation.proof_nodes,
            },
            changes: page
                .changes
                .iter()
                .map(public_runtime_change)
                .collect::<Result<_>>()?,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn follow_changefeed(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        request: &FollowChangefeed,
        now: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<ChangefeedFollowResult> {
        request
            .validate()
            .map_err(|error| ServiceError::Changefeed(error.to_string()))?;
        self.authorize(
            session_id,
            token,
            SecurityAction::ChangefeedFollow,
            now,
            request_id,
            operation_id,
        )?;
        let started = Instant::now();
        let timeout = Duration::from_millis(request.wait_timeout_ms);
        loop {
            let page = self.read_changefeed_page(&request.read)?;
            let elapsed = started.elapsed();
            if !page.changes.is_empty() || elapsed >= timeout {
                return Ok(ChangefeedFollowResult {
                    timed_out: page.changes.is_empty(),
                    waited_ms: u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX),
                    page,
                });
            }
            std::thread::sleep(
                timeout
                    .saturating_sub(elapsed)
                    .min(Duration::from_millis(25)),
            );
        }
    }
}
