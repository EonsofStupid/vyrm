use super::*;

impl RrdEngine {
    /// Builds a deterministic forward-only compensation plan from the
    /// transaction's original read cursor. Planning is side-effect free; the
    /// caller commits `plan.mutations` through the normal transaction path.
    pub fn plan_forward_rollback(
        &self,
        request: &ForwardRollbackRequest,
        read_cursor: u64,
        idempotency_key: &CorrelationId,
    ) -> Result<ForwardRollbackPlan> {
        request
            .validate()
            .map_err(|error| ServiceError::Contract(error.to_string()))?;
        if request.target_known_at_cursor >= read_cursor {
            return Err(ServiceError::Contract(
                "rollback target cursor must be older than the transaction read cursor".into(),
            ));
        }
        let scope = ScopeId::new(format!("instance:{}", self.instance)).map_err(core_contract)?;
        let live_read = self.storage.runtime_read_stamp(&scope)?;
        if read_cursor > live_read.commit_cursor {
            return Err(ServiceError::StorageConflict(format!(
                "rollback read cursor {read_cursor} exceeds live head {}",
                live_read.commit_cursor
            )));
        }
        let limit = usize::try_from(read_cursor).map_err(|_| {
            ServiceError::Contract("rollback read cursor exceeds this platform".into())
        })?;
        let changes = if limit == 0 {
            Vec::new()
        } else {
            let page = self.storage.runtime_read_changes(&live_read, 0, limit)?;
            if page.through_cursor != read_cursor {
                return Err(ServiceError::Storage(format!(
                    "rollback replay stopped at {}, expected {read_cursor}",
                    page.through_cursor
                )));
            }
            page.changes
        };
        let structural_changes = changes
            .into_iter()
            .filter(|change| {
                matches!(
                    &change.mutation,
                    RuntimeMutation::Record { .. }
                        | RuntimeMutation::Relation { .. }
                        | RuntimeMutation::Retire { .. }
                )
            })
            .collect::<Vec<_>>();
        let current = RuntimeGraphSnapshot::from_changes(
            &structural_changes,
            scope.clone(),
            request.effective_at,
            read_cursor,
        );
        let target = RuntimeGraphSnapshot::from_changes(
            &structural_changes,
            scope,
            request.target_valid_at,
            request.target_known_at_cursor,
        );
        let target_state_sha256 = digest::sha256_hex(
            &serde_json::to_vec(&target)
                .map_err(|error| ServiceError::Contract(error.to_string()))?,
        );
        let diff = current.diff(&target);
        let counts = ForwardRollbackCounts {
            restored_records: u64::try_from(diff.added_records.len() + diff.changed_records.len())
                .map_err(|_| ServiceError::Contract("rollback record count overflowed".into()))?,
            retired_records: u64::try_from(diff.removed_records.len())
                .map_err(|_| ServiceError::Contract("rollback record count overflowed".into()))?,
            restored_relations: u64::try_from(
                diff.added_relations.len() + diff.changed_relations.len(),
            )
            .map_err(|_| ServiceError::Contract("rollback relation count overflowed".into()))?,
            retired_relations: u64::try_from(diff.removed_relations.len())
                .map_err(|_| ServiceError::Contract("rollback relation count overflowed".into()))?,
        };
        let mut mutations = Vec::new();
        for record in diff
            .added_records
            .iter()
            .chain(diff.changed_records.iter().map(|change| &change.after))
        {
            mutations.push(restore_record(record, request.effective_at)?);
        }
        for record in &diff.removed_records {
            mutations.push(retire_record(record, request.effective_at)?);
        }
        for relation in diff
            .added_relations
            .iter()
            .chain(diff.changed_relations.iter().map(|change| &change.after))
        {
            mutations.push(restore_relation(relation, request.effective_at)?);
        }
        for relation in &diff.removed_relations {
            mutations.push(retire_relation(relation, request.effective_at)?);
        }
        mutations.push(rollback_evidence(
            request,
            read_cursor,
            &target_state_sha256,
            counts,
            idempotency_key,
        )?);
        let plan = ForwardRollbackPlan {
            request: request.clone(),
            read_cursor,
            target_state_sha256,
            counts,
            operation_sha256: transaction_operation_sha256(&mutations),
            mutations,
        };
        plan.validate()
            .map_err(|error| ServiceError::Contract(error.to_string()))?;
        Ok(plan)
    }
}

fn restore_record(record: &RuntimeRecord, effective_at: u64) -> Result<TransactionMutation> {
    Ok(TransactionMutation::PutRecord {
        reference: public_change_ref(&record.reference)?,
        valid_from: effective_at,
        valid_to: None,
        properties: public_properties(&record.properties)?,
    })
}

fn retire_record(record: &RuntimeRecord, effective_at: u64) -> Result<TransactionMutation> {
    if effective_at <= record.valid_from {
        return Err(ServiceError::Contract(format!(
            "rollback effective time {effective_at} does not follow record {}:{} valid_from {}",
            record.reference.kind, record.reference.id, record.valid_from
        )));
    }
    Ok(TransactionMutation::PutRecord {
        reference: public_change_ref(&record.reference)?,
        valid_from: record.valid_from,
        valid_to: Some(effective_at),
        properties: public_properties(&record.properties)?,
    })
}

fn restore_relation(relation: &RuntimeRelation, effective_at: u64) -> Result<TransactionMutation> {
    Ok(TransactionMutation::PutRelation {
        reference: public_change_ref(&relation.reference)?,
        from: public_change_ref(&relation.from)?,
        to: public_change_ref(&relation.to)?,
        valid_from: effective_at,
        valid_to: None,
        properties: public_properties(&relation.properties)?,
    })
}

fn retire_relation(relation: &RuntimeRelation, effective_at: u64) -> Result<TransactionMutation> {
    if effective_at <= relation.valid_from {
        return Err(ServiceError::Contract(format!(
            "rollback effective time {effective_at} does not follow relation {}:{} valid_from {}",
            relation.reference.kind, relation.reference.id, relation.valid_from
        )));
    }
    Ok(TransactionMutation::PutRelation {
        reference: public_change_ref(&relation.reference)?,
        from: public_change_ref(&relation.from)?,
        to: public_change_ref(&relation.to)?,
        valid_from: relation.valid_from,
        valid_to: Some(effective_at),
        properties: public_properties(&relation.properties)?,
    })
}

fn rollback_evidence(
    request: &ForwardRollbackRequest,
    read_cursor: u64,
    target_state_sha256: &str,
    counts: ForwardRollbackCounts,
    idempotency_key: &CorrelationId,
) -> Result<TransactionMutation> {
    let identity = digest::sha256_hex(
        &serde_json::to_vec(&(
            "rrflow-forward-rollback-v1",
            request,
            read_cursor,
            target_state_sha256,
            counts,
            idempotency_key.as_str(),
        ))
        .map_err(|error| ServiceError::Contract(error.to_string()))?,
    );
    let object = serde_json::to_string(&serde_json::json!({
        "contract_version": 1,
        "read_cursor": read_cursor,
        "target_known_at_cursor": request.target_known_at_cursor,
        "target_valid_at": request.target_valid_at,
        "effective_at": request.effective_at,
        "target_state_sha256": target_state_sha256,
        "reason_sha256": digest::sha256_hex(request.reason.as_bytes()),
        "counts": counts,
    }))
    .map_err(|error| ServiceError::Contract(error.to_string()))?;
    Ok(TransactionMutation::AssertClaim {
        subject: CanonicalId::new(format!("rollback-{}", &identity[..32]))
            .map_err(|error| ServiceError::Contract(error.to_string()))?,
        predicate: CanonicalId::new("historical-rollback")
            .map_err(|error| ServiceError::Contract(error.to_string()))?,
        object,
        valid_from: request.effective_at,
        tx_time: request.effective_at,
        producer: CanonicalId::new("rrflow-rollback")
            .map_err(|error| ServiceError::Contract(error.to_string()))?,
        confidence: Some(1.0),
    })
}
