use super::*;

impl RrdEngine {
    #[allow(clippy::too_many_arguments)]
    pub fn execute_query(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        request: &ExecuteQuery,
        now: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<QueryResult> {
        request
            .validate()
            .map_err(|error| ServiceError::Query(error.to_string()))?;
        self.authorize(
            session_id,
            token,
            SecurityAction::QueryExecute,
            now,
            request_id,
            operation_id,
        )?;
        let expected_scope = format!("instance:{}", self.instance);
        if request.scope != expected_scope {
            return Err(ServiceError::WrongScope);
        }
        let scope = ScopeId::new(request.scope.clone())
            .map_err(|error| ServiceError::Query(error.to_string()))?;
        let query = rrd_query::parse(&request.query)
            .map_err(|error| ServiceError::Query(error.to_string()))?;
        let parameters = request
            .parameters
            .iter()
            .map(|(name, value)| Ok((name.clone(), runtime_value(value)?)))
            .collect::<Result<rrd_query::Parameters>>()?;
        let catalog = rrd_query::Catalog::capture(&self.storage, &scope)
            .map_err(|error| ServiceError::Query(error.to_string()))?;
        let bound = rrd_query::bind(&query, &parameters, &catalog)
            .map_err(|error| ServiceError::Query(error.to_string()))?;
        let plan =
            rrd_query::plan(&bound).map_err(|error| ServiceError::Query(error.to_string()))?;
        let budget = query_execution_budget(&request.budget)?;
        let execution = rrd_query::execute(&self.storage, &plan, &budget)
            .map_err(|error| ServiceError::Query(error.to_string()))?;
        let rows = execution
            .batches
            .iter()
            .flat_map(|batch| batch.rows.iter())
            .map(|row| {
                Ok(QueryRowSnapshot {
                    identity: row.identity.clone(),
                    values: row
                        .values
                        .iter()
                        .map(|(name, value)| Ok((name.clone(), query_value(value)?)))
                        .collect::<Result<_>>()?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(QueryResult {
            canonical_query: query.canonical(),
            scope: request.scope.clone(),
            read_manifest_sha256: execution.read_manifest.clone(),
            known_at_cursor: execution.known_at_cursor,
            schema_revision: bound.schema_revision,
            plan: QueryPlanSnapshot {
                plan_sha256: plan.digest.clone(),
                exact: plan.explanation.contract.exact,
                deterministic_order: plan.explanation.contract.deterministic_order.clone(),
                authorization_boundary: plan.explanation.contract.authorization_boundary.clone(),
                candidates: plan
                    .explanation
                    .candidates
                    .iter()
                    .map(|candidate| QueryPlanCandidate {
                        name: candidate.name.clone(),
                        selected: candidate.selected,
                        exact: candidate.exact,
                        reason: candidate.reason.clone(),
                    })
                    .collect(),
            },
            execution: QueryExecutionSnapshot {
                scanned_changes: u64::try_from(execution.scanned_changes)
                    .map_err(|_| ServiceError::Query("scanned changes exceed u64".into()))?,
                stamp_validation: execution.stamp_validation,
                stamp_validation_max_changes: u64::try_from(execution.stamp_validation_max_changes)
                    .map_err(|_| ServiceError::Query("stamp evidence exceeds u64".into()))?,
                stamp_validation_proof_nodes: execution.stamp_validation_proof_nodes,
                returned_rows: u64::try_from(execution.returned_rows)
                    .map_err(|_| ServiceError::Query("returned rows exceed u64".into()))?,
                output_bytes: u64::try_from(execution.output_bytes)
                    .map_err(|_| ServiceError::Query("output bytes exceed u64".into()))?,
                truncated: execution.truncated,
                analysis: execution
                    .analysis
                    .as_ref()
                    .map(public_query_analysis)
                    .transpose()?,
            },
            rows,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn poll_live_query(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        request: &PollLiveQuery,
        now: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<LiveQueryDeltaResult> {
        request
            .validate()
            .map_err(|error| ServiceError::Query(error.to_string()))?;
        self.authorize(
            session_id,
            token,
            SecurityAction::QueryLivePoll,
            now,
            request_id,
            operation_id,
        )?;
        let expected_scope = format!("instance:{}", self.instance);
        if request.scope != expected_scope {
            return Err(ServiceError::WrongScope);
        }
        let scope = ScopeId::new(request.scope.clone())
            .map_err(|error| ServiceError::Query(error.to_string()))?;
        let query = rrd_query::parse(&request.query)
            .map_err(|error| ServiceError::Query(error.to_string()))?;
        let parameters = request
            .parameters
            .iter()
            .map(|(name, value)| Ok((name.clone(), runtime_value(value)?)))
            .collect::<Result<rrd_query::Parameters>>()?;
        let live_budget = rrd_query::LiveQueryBudget {
            execution: query_execution_budget(&request.budget)?,
            max_delta_rows: usize::try_from(request.max_delta_rows)
                .map_err(|_| ServiceError::Query("live query delta budget exceeds usize".into()))?,
        };
        let started = Instant::now();
        let timeout = Duration::from_millis(request.wait_timeout_ms);
        let (delta, timed_out, waited_ms) = loop {
            let delta = rrd_query::poll_live_query(
                &self.storage,
                &scope,
                &query,
                &parameters,
                request.after_cursor,
                &live_budget,
            )
            .map_err(|error| ServiceError::Query(error.to_string()))?;
            let elapsed = started.elapsed();
            if delta.through_cursor > request.after_cursor
                || request.wait_timeout_ms == 0
                || elapsed >= timeout
            {
                let timed_out = delta.through_cursor == request.after_cursor
                    && delta.added.is_empty()
                    && delta.updated.is_empty()
                    && delta.removed.is_empty()
                    && request.wait_timeout_ms > 0
                    && elapsed >= timeout;
                break (
                    delta,
                    timed_out,
                    u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX),
                );
            }
            std::thread::sleep(
                timeout
                    .saturating_sub(elapsed)
                    .min(Duration::from_millis(25)),
            );
        };
        Ok(LiveQueryDeltaResult {
            timed_out,
            waited_ms,
            query_sha256: delta.query_digest,
            from_cursor: delta.from_cursor,
            through_cursor: delta.through_cursor,
            head_cursor: delta.head_cursor,
            added: delta
                .added
                .iter()
                .map(public_query_row)
                .collect::<Result<_>>()?,
            updated: delta
                .updated
                .iter()
                .map(|change| {
                    Ok(LiveQueryRowChange {
                        before: public_query_row(&change.before)?,
                        after: public_query_row(&change.after)?,
                    })
                })
                .collect::<Result<_>>()?,
            removed: delta
                .removed
                .iter()
                .map(public_query_row)
                .collect::<Result<_>>()?,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn ensure_query_index(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        idempotency_key: &CorrelationId,
        request: &EnsureQueryIndex,
        now: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<EnsureQueryIndexResult> {
        request
            .validate()
            .map_err(|error| ServiceError::Query(error.to_string()))?;
        let (session_bytes, mut session) = self.load_authenticated(session_id, token)?;
        self.authorize_session_policy(&session, SecurityAction::QueryIndexEnsure, now)?;
        let scope = self.query_scope(&request.scope)?;
        let operation_digest = digest::sha256_hex(
            &serde_json::to_vec(request).map_err(|error| ServiceError::Query(error.to_string()))?,
        );
        let repository = rrd_query::IndexCatalogueRepository::new(&self.storage, scope.clone());
        if let Some(receipt) = repository
            .operation_receipt(idempotency_key.as_str(), &operation_digest)
            .map_err(query_index_error)?
        {
            let catalogue = repository
                .load()
                .map_err(|error| ServiceError::Query(error.to_string()))?;
            return Ok(EnsureQueryIndexResult {
                index: public_query_index(&receipt.entry)?,
                catalogue_revision: catalogue.revision,
                idempotent_replay: true,
            });
        }
        // An exact durable operation replay above remains recoverable after
        // the adapter session expires. Creating or rebuilding an index is a
        // new mutation and therefore still requires an active session.
        self.require_active_or_expire(
            session_id,
            session_bytes,
            &mut session,
            now,
            request_id,
            operation_id,
        )?;
        let (definition, valid_at) = query_index_definition(request)?;
        let preliminary = repository
            .load()
            .map_err(|error| ServiceError::Query(error.to_string()))?;
        let requires_unique_install = definition.unique
            && preliminary
                .entries
                .get(&definition.id)
                .is_none_or(|entry| !entry.unique_validated);
        // Ordinary rebuilds never enter the authoritative commit path. The
        // first installation of a unique constraint is the exception: it must
        // prove existing rows and publish its enforcement bit without a write
        // racing between those two events.
        let _transaction_guard = requires_unique_install
            .then(|| self.transaction_gate.lock())
            .transpose()
            .map_err(|_| ServiceError::Storage("engine transaction gate is poisoned".into()))?;
        let current = if requires_unique_install {
            repository
                .load()
                .map_err(|error| ServiceError::Query(error.to_string()))?
        } else {
            preliminary
        };
        let query_catalogue = rrd_query::Catalog::capture(&self.storage, &scope)
            .map_err(|error| ServiceError::Query(error.to_string()))?;
        let source_cursor = query_catalogue.source_cursor(&definition.source);
        let mutation = rrd_query::IndexMutationContext {
            at: now,
            actor: format!("session:{}", session_id.as_str()),
            request_id: request_id.into(),
            operation_id: operation_id.into(),
        };
        if let Some(existing) = current.entries.get(&definition.id) {
            if existing.definition != definition {
                return Err(ServiceError::Query(format!(
                    "index {} already exists with a different definition",
                    definition.id
                )));
            }
            match existing.stamp.state {
                ProjectionState::Building => {}
                ProjectionState::Ready
                    if existing.built_valid_at == Some(valid_at)
                        && existing.stamp.source_cursor == source_cursor
                        && existing.maintenance.is_some() => {}
                ProjectionState::Ready => {
                    repository
                        .begin_rebuild(&mutation, &definition.id)
                        .map_err(|error| ServiceError::Query(error.to_string()))?;
                }
                ProjectionState::Quarantined | ProjectionState::Retiring => {
                    return Err(ServiceError::Query(format!(
                        "index {} must be explicitly recovered before ensure",
                        definition.id
                    )));
                }
            }
        } else {
            repository
                .create(&mutation, &query_catalogue, definition.clone())
                .map_err(|error| ServiceError::Query(error.to_string()))?;
        }
        let catalogue = repository
            .load()
            .map_err(|error| ServiceError::Query(error.to_string()))?;
        let entry = catalogue
            .entries
            .get(&definition.id)
            .ok_or_else(|| ServiceError::Query("created index disappeared".into()))?;
        let ready = if entry.stamp.state == ProjectionState::Ready
            && entry.built_valid_at == Some(valid_at)
            && entry.stamp.source_cursor == source_cursor
        {
            catalogue
        } else {
            repository
                .build(
                    &mutation,
                    &definition.id,
                    valid_at,
                    &query_execution_budget(&request.budget)?,
                )
                .map_err(|error| ServiceError::Query(error.to_string()))?
        };
        let entry = ready
            .entries
            .get(&definition.id)
            .cloned()
            .ok_or_else(|| ServiceError::Query("ready index disappeared".into()))?;
        let recorded = repository
            .record_operation(
                &mutation,
                idempotency_key.as_str().into(),
                operation_digest,
                entry.clone(),
            )
            .map_err(query_index_error)?;
        Ok(EnsureQueryIndexResult {
            index: public_query_index(&entry)?,
            catalogue_revision: recorded.revision,
            idempotent_replay: false,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn list_query_indexes(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        request: &ListQueryIndexes,
        now: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<QueryIndexCatalogueSnapshot> {
        request
            .validate()
            .map_err(|error| ServiceError::Query(error.to_string()))?;
        self.authorize(
            session_id,
            token,
            SecurityAction::QueryIndexList,
            now,
            request_id,
            operation_id,
        )?;
        let scope = self.query_scope(&request.scope)?;
        let catalogue = rrd_query::IndexCatalogueRepository::new(&self.storage, scope)
            .load()
            .map_err(|error| ServiceError::Query(error.to_string()))?;
        Ok(QueryIndexCatalogueSnapshot {
            scope: request.scope.clone(),
            revision: catalogue.revision,
            indexes: catalogue
                .entries
                .values()
                .map(public_query_index)
                .collect::<Result<_>>()?,
        })
    }
}

fn public_query_row(row: &rrd_query::QueryRow) -> Result<QueryRowSnapshot> {
    Ok(QueryRowSnapshot {
        identity: row.identity.clone(),
        values: row
            .values
            .iter()
            .map(|(name, value)| Ok((name.clone(), query_value(value)?)))
            .collect::<Result<_>>()?,
    })
}

fn query_execution_budget(
    budget: &rrd_contract::QueryBudget,
) -> Result<rrd_query::ExecutionBudget> {
    Ok(rrd_query::ExecutionBudget {
        max_scanned_changes: usize::try_from(budget.max_scanned_changes)
            .map_err(|_| ServiceError::Query("query scan budget exceeds usize".into()))?,
        max_rows: usize::try_from(budget.max_rows)
            .map_err(|_| ServiceError::Query("query row budget exceeds usize".into()))?,
        max_output_bytes: usize::try_from(budget.max_output_bytes)
            .map_err(|_| ServiceError::Query("query output budget exceeds usize".into()))?,
        max_batch_rows: usize::try_from(budget.max_batch_rows)
            .map_err(|_| ServiceError::Query("query batch budget exceeds usize".into()))?,
        max_memory_bytes: usize::try_from(budget.max_memory_bytes)
            .map_err(|_| ServiceError::Query("query memory budget exceeds usize".into()))?,
        max_spill_bytes: usize::try_from(budget.max_spill_bytes)
            .map_err(|_| ServiceError::Query("query spill budget exceeds usize".into()))?,
        max_elapsed_ms: budget.max_elapsed_ms,
    })
}

fn public_query_analysis(
    analysis: &rrd_query::FusionAnalysis,
) -> Result<QueryExecutionAnalysisSnapshot> {
    let number = |value: usize, label: &str| {
        u64::try_from(value).map_err(|_| ServiceError::Query(format!("query {label} exceeds u64")))
    };
    Ok(QueryExecutionAnalysisSnapshot {
        engine: analysis.engine.clone(),
        provider_scans: number(analysis.provider_scans, "provider scans")?,
        input_rows: number(analysis.input_rows, "analysis input rows")?,
        input_batches: number(analysis.input_batches, "analysis input batches")?,
        input_memory_bytes: number(analysis.input_memory_bytes, "analysis input memory")?,
        output_batches: number(analysis.output_batches, "analysis output batches")?,
        projection_pushdown: analysis.projection_pushdown.clone(),
        filter_pushdown: analysis.filter_pushdown.clone(),
        limit_pushdown: analysis.limit_pushdown.clone(),
        physical_operators: number(analysis.physical_operators, "physical operators")?,
        peak_memory_bytes: number(analysis.peak_memory_bytes, "peak memory bytes")?,
        spill_count: number(analysis.spill_count, "spill count")?,
        spilled_bytes: number(analysis.spilled_bytes, "spilled bytes")?,
        spilled_rows: number(analysis.spilled_rows, "spilled rows")?,
        elapsed_micros: analysis.elapsed_micros,
    })
}

fn query_index_definition(request: &EnsureQueryIndex) -> Result<(rrd_query::IndexDefinition, u64)> {
    let query = rrd_query::parse(&request.definition_query)
        .map_err(|error| ServiceError::Query(error.to_string()))?;
    if query.join.is_some()
        || query.limit.is_some()
        || query.explain_contract
        || query.explain_analyze
        || !matches!(query.temporal.known_at, CursorExpr::Head)
    {
        return Err(ServiceError::Query(
            "index definition query requires KNOWN HEAD and cannot contain joins, limits, or EXPLAIN"
                .into(),
        ));
    }
    let TimeExpr::Literal(valid_at) = query.temporal.valid_at else {
        return Err(ServiceError::Query(
            "index definition valid-time must be a literal".into(),
        ));
    };
    let fields = match query.projection {
        Projection::Fields(fields) => fields,
        Projection::All if request.kind == QueryIndexKind::Count => Vec::new(),
        Projection::All => return Err(ServiceError::Query(
            "only a count index may use PROJECT *; other index definitions require named fields"
                .into(),
        )),
    };
    let full_text = request.full_text.clone().unwrap_or_default();
    Ok((
        rrd_query::IndexDefinition {
            id: ProjectionId::new(request.index_id.as_str())
                .map_err(|error| ServiceError::Query(error.to_string()))?,
            source: query.source,
            fields,
            unique: request.unique,
            kind: match request.kind {
                QueryIndexKind::Scalar => rrd_query::IndexKind::Scalar,
                QueryIndexKind::Count => rrd_query::IndexKind::Count,
                QueryIndexKind::Geo => rrd_query::IndexKind::Geo,
                QueryIndexKind::MaterializedView => rrd_query::IndexKind::MaterializedView,
                QueryIndexKind::AggregateCount => rrd_query::IndexKind::AggregateCount,
                QueryIndexKind::Bm25 => rrd_query::IndexKind::Bm25 {
                    config: rrd_query::Bm25Config {
                        analyzer: match full_text.analyzer {
                            QueryTextAnalyzer::UnicodeLowercase => {
                                rrd_query::Bm25Analyzer::UnicodeLowercase
                            }
                            QueryTextAnalyzer::UnicodeCaseSensitive => {
                                rrd_query::Bm25Analyzer::UnicodeCaseSensitive
                            }
                        },
                        tokenizer: match full_text.tokenizer {
                            QueryTextTokenizer::UnicodeAlphanumeric => {
                                rrd_query::Bm25Tokenizer::UnicodeAlphanumeric
                            }
                            QueryTextTokenizer::Whitespace => rrd_query::Bm25Tokenizer::Whitespace,
                        },
                        ascii_folding: full_text.ascii_folding,
                        stop_words: full_text.stop_words,
                        min_token_chars: full_text.min_token_chars,
                        max_token_chars: full_text.max_token_chars,
                        stemmer: match full_text.stemmer {
                            QueryTextStemmer::None => rrd_query::Bm25Stemmer::None,
                            QueryTextStemmer::English => rrd_query::Bm25Stemmer::English,
                        },
                        k1_micros: full_text.k1_micros,
                        b_micros: full_text.b_micros,
                    },
                },
            },
            filters: query.filters,
        },
        valid_at,
    ))
}

pub(in crate::engine) fn public_query_index(
    entry: &rrd_query::IndexEntry,
) -> Result<QueryIndexSnapshot> {
    let valid_at = entry.built_valid_at.unwrap_or(0);
    let mut definition = rrd_query::Query::new(
        entry.definition.source.clone(),
        rrd_query::TemporalSelector {
            valid_at: TimeExpr::Literal(valid_at),
            known_at: CursorExpr::Head,
        },
    );
    definition.projection = Projection::Fields(entry.definition.fields.clone());
    definition.filters = entry.definition.filters.clone();
    if entry.definition.fields.is_empty() {
        definition.projection = Projection::All;
    }
    let full_text = match &entry.definition.kind {
        rrd_query::IndexKind::Bm25 { config } => Some(QueryFullTextConfiguration {
            analyzer: match config.analyzer {
                rrd_query::Bm25Analyzer::UnicodeLowercase => QueryTextAnalyzer::UnicodeLowercase,
                rrd_query::Bm25Analyzer::UnicodeCaseSensitive => {
                    QueryTextAnalyzer::UnicodeCaseSensitive
                }
            },
            tokenizer: match config.tokenizer {
                rrd_query::Bm25Tokenizer::UnicodeAlphanumeric => {
                    QueryTextTokenizer::UnicodeAlphanumeric
                }
                rrd_query::Bm25Tokenizer::Whitespace => QueryTextTokenizer::Whitespace,
            },
            ascii_folding: config.ascii_folding,
            stop_words: config.stop_words.clone(),
            min_token_chars: config.min_token_chars,
            max_token_chars: config.max_token_chars,
            stemmer: match config.stemmer {
                rrd_query::Bm25Stemmer::None => QueryTextStemmer::None,
                rrd_query::Bm25Stemmer::English => QueryTextStemmer::English,
            },
            k1_micros: config.k1_micros,
            b_micros: config.b_micros,
        }),
        _ => None,
    };
    Ok(QueryIndexSnapshot {
        index_id: CanonicalId::new(entry.definition.id.to_string())
            .map_err(|error| ServiceError::Query(error.to_string()))?,
        definition_query: definition.canonical(),
        unique: entry.definition.unique,
        kind: match &entry.definition.kind {
            rrd_query::IndexKind::Scalar => QueryIndexKind::Scalar,
            rrd_query::IndexKind::Count => QueryIndexKind::Count,
            rrd_query::IndexKind::Geo => QueryIndexKind::Geo,
            rrd_query::IndexKind::MaterializedView => QueryIndexKind::MaterializedView,
            rrd_query::IndexKind::AggregateCount => QueryIndexKind::AggregateCount,
            rrd_query::IndexKind::Bm25 { .. } => QueryIndexKind::Bm25,
        },
        full_text,
        generation: entry.stamp.generation,
        source_cursor: entry.stamp.source_cursor,
        built_valid_at: entry.built_valid_at,
        artifact_rows: entry.artifact_rows,
        analytics_total_count: entry.analytics_total_count,
        analytics_group_count: entry.analytics_group_count,
        maintenance: entry
            .maintenance
            .as_ref()
            .map(|maintenance| QueryIndexMaintenanceSnapshot {
                mode: maintenance.mode.clone(),
                prior_source_cursor: maintenance.prior_source_cursor,
                source_cursor: maintenance.source_cursor,
                inserted_rows: maintenance.inserted_rows,
                updated_rows: maintenance.updated_rows,
                removed_rows: maintenance.removed_rows,
            }),
        configuration_sha256: entry.stamp.config_digest.clone(),
        artifact_sha256: entry.stamp.artifact_digest.clone(),
        state: match entry.stamp.state {
            ProjectionState::Building => QueryIndexState::Building,
            ProjectionState::Ready => QueryIndexState::Ready,
            ProjectionState::Quarantined => QueryIndexState::Quarantined,
            ProjectionState::Retiring => QueryIndexState::Retiring,
        },
    })
}

fn query_index_error(error: rrd_query::Error) -> ServiceError {
    if matches!(&error, rrd_query::Error::Catalog(message) if message.contains("idempotency key")) {
        ServiceError::IdempotencyConflict
    } else {
        ServiceError::Query(error.to_string())
    }
}
