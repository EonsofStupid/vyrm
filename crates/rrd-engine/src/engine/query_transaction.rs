use super::*;

impl RrdEngine {
    /// Execute a bounded RRFlowQL mutation program through the canonical
    /// session transaction authority. Parsing and binding complete before a
    /// transaction is opened; publication remains one existing runtime commit.
    pub fn execute_query_transaction(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        request: &ExecuteQueryTransaction,
        context: &RequestContext,
        now: u64,
    ) -> Result<QueryTransactionResult> {
        request
            .validate()
            .map_err(|error| ServiceError::Query(error.to_string()))?;
        context
            .validate(true)
            .map_err(|error| ServiceError::Contract(error.to_string()))?;
        let expected_scope = format!("instance:{}", self.instance);
        if request.scope != expected_scope {
            return Err(ServiceError::WrongScope);
        }
        let program = rrd_query::parse_transaction_program(&request.program)
            .map_err(|error| ServiceError::Query(error.to_string()))?;
        let expected = program
            .mutation_bindings
            .iter()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        let supplied = request
            .mutation_bindings
            .keys()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        if supplied != expected {
            let missing = expected.difference(&supplied).cloned().collect::<Vec<_>>();
            let extra = supplied.difference(&expected).cloned().collect::<Vec<_>>();
            return Err(ServiceError::Query(format!(
                "query transaction bindings disagree with the program; missing={missing:?} extra={extra:?}"
            )));
        }
        let mutations = program
            .mutation_bindings
            .iter()
            .map(|binding| {
                request
                    .mutation_bindings
                    .get(binding)
                    .cloned()
                    .expect("exact binding set was checked")
            })
            .collect::<Vec<_>>();
        let canonical_program = program.canonical();
        let program_sha256 = digest::sha256_hex(canonical_program.as_bytes());
        let mutation_sha256 = transaction_operation_sha256(&mutations);
        let intent_sha256 =
            operation_digest(&(canonical_program.as_str(), mutation_sha256.as_str()))?;
        let outer_idempotency = context
            .idempotency_key
            .as_ref()
            .expect("mutating request context was validated");
        let begin_context = query_transaction_context(
            context,
            outer_idempotency,
            "begin",
            outer_idempotency.as_str(),
        )?;
        let lease = self.begin_transaction_with_intent(
            session_id,
            token,
            &BeginTransaction {
                scope: CanonicalId::new("data").expect("static transaction scope is valid"),
                timeout_ms: request.timeout_ms,
            },
            &begin_context,
            now,
            Some(&intent_sha256),
        )?;
        let mutation_count = u64::try_from(mutations.len())
            .map_err(|_| ServiceError::Query("mutation count exceeds u64".into()))?;

        match program.disposition {
            rrd_query::TransactionDisposition::Commit => {
                let commit_key = query_transaction_id(
                    "commit-key",
                    &[outer_idempotency.as_str(), &program_sha256],
                )?;
                let commit_request = CommitTransaction {
                    operation_sha256: mutation_sha256,
                    mutations,
                };
                let receipt = self.commit_transaction(
                    session_id,
                    token,
                    &lease.transaction_id,
                    &commit_key,
                    &commit_request,
                    now,
                    query_transaction_id(
                        "commit-request",
                        &[context.request_id.as_str(), &program_sha256],
                    )?
                    .as_str(),
                    query_transaction_id(
                        "commit-operation",
                        &[context.operation_id.as_str(), &program_sha256],
                    )?
                    .as_str(),
                )?;
                Ok(QueryTransactionResult {
                    canonical_program,
                    transaction_id: lease.transaction_id,
                    read_cursor: lease.read_cursor,
                    state: TransactionState::Committed,
                    mutation_count,
                    commit: Some(receipt),
                })
            }
            rrd_query::TransactionDisposition::Cancel => {
                let abort_context = query_transaction_context(
                    context,
                    outer_idempotency,
                    "cancel",
                    &program_sha256,
                )?;
                let aborted = self.abort_transaction(
                    session_id,
                    token,
                    &lease.transaction_id,
                    &AbortTransaction {},
                    &abort_context,
                    now,
                )?;
                Ok(QueryTransactionResult {
                    canonical_program,
                    transaction_id: aborted.transaction_id,
                    read_cursor: aborted.read_cursor,
                    state: aborted.state,
                    mutation_count,
                    commit: None,
                })
            }
        }
    }
}

fn query_transaction_context(
    parent: &RequestContext,
    outer_idempotency: &CorrelationId,
    phase: &str,
    intent: &str,
) -> Result<RequestContext> {
    Ok(RequestContext {
        request_id: query_transaction_id(phase, &[parent.request_id.as_str(), intent])?,
        operation_id: query_transaction_id(phase, &[parent.operation_id.as_str(), intent])?,
        idempotency_key: Some(query_transaction_id(
            phase,
            &[outer_idempotency.as_str(), intent],
        )?),
        deadline_unix_ms: parent.deadline_unix_ms,
    })
}

fn query_transaction_id(label: &str, values: &[&str]) -> Result<CorrelationId> {
    let mut bytes = b"rrflow-query-transaction-v1\0".to_vec();
    bytes.extend_from_slice(label.as_bytes());
    for value in values {
        bytes.extend_from_slice(&(value.len() as u64).to_be_bytes());
        bytes.extend_from_slice(value.as_bytes());
    }
    CorrelationId::new(format!("{label}:{}", digest::sha256_hex(&bytes)))
        .map_err(|error| ServiceError::Contract(error.to_string()))
}
