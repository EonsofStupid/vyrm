use super::*;

#[derive(Debug)]
struct FusionCandidate {
    subject: DataReference,
    vector_reference: Option<DataReference>,
    text_score: Option<f64>,
    vector_score: Option<f64>,
    text_rank: Option<u64>,
    vector_rank: Option<u64>,
}

impl RrdEngine {
    /// Executes BM25 and vector retrieval against one captured RRD read stamp,
    /// then applies deterministic weighted reciprocal-rank fusion.
    #[allow(clippy::too_many_arguments)]
    pub fn search_hybrid(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        request: &SearchHybrid,
        now: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<HybridSearchResult> {
        request
            .validate()
            .map_err(|error| ServiceError::Contract(error.to_string()))?;
        self.authorize(
            session_id,
            token,
            SecurityAction::QueryExecute,
            now,
            request_id,
            operation_id,
        )?;
        self.authorize(
            session_id,
            token,
            SecurityAction::VectorSearch,
            now,
            request_id,
            operation_id,
        )?;
        let scope = self.query_scope(&request.scope)?;
        let read = self.storage.runtime_read_stamp(&scope)?;

        let query = rrd_query::parse(&format!(
            "FROM record:{} AT VALID {} KNOWN {} WHERE {} MATCH $hybrid_text LIMIT {}",
            request.document_kind,
            request.valid_at,
            read.commit_cursor,
            request.text_field,
            request.candidate_k,
        ))
        .map_err(|error| ServiceError::Query(error.to_string()))?;
        let parameters = rrd_query::Parameters::from([(
            "hybrid_text".into(),
            RuntimeValue::String(request.text_query.clone()),
        )]);
        let query_catalogue = rrd_query::Catalog::capture_at(&self.storage, read.clone())
            .map_err(|error| ServiceError::Query(error.to_string()))?;
        let bound = rrd_query::bind(&query, &parameters, &query_catalogue)
            .map_err(|error| ServiceError::Query(error.to_string()))?;
        let text_plan =
            rrd_query::plan(&bound).map_err(|error| ServiceError::Query(error.to_string()))?;
        let candidate_limit = usize::try_from(request.candidate_k)
            .map_err(|_| ServiceError::Query("hybrid candidate_k exceeds usize".into()))?;
        let text_execution = rrd_query::execute(
            &self.storage,
            &text_plan,
            &rrd_query::ExecutionBudget {
                max_scanned_changes: usize::try_from(request.max_scanned_changes)
                    .map_err(|_| ServiceError::Query("hybrid scan budget exceeds usize".into()))?,
                max_rows: candidate_limit,
                max_output_bytes: 8 * 1024 * 1024,
                max_batch_rows: candidate_limit.clamp(1, 1_024),
                ..rrd_query::ExecutionBudget::default()
            },
        )
        .map_err(|error| ServiceError::Query(error.to_string()))?;
        if text_execution.truncated {
            return Err(ServiceError::Query(
                "hybrid BM25 candidates were truncated by the execution budget".into(),
            ));
        }
        let text_access_path = if text_plan.operators.iter().any(|operator| {
            matches!(
                operator,
                rrd_query::PhysicalOperator::MaterializedIndex {
                    kind: rrd_query::IndexKind::Bm25 { .. },
                    ..
                }
            )
        }) {
            "bm25_index"
        } else {
            "bm25_exact"
        };
        let text_hits = text_execution
            .batches
            .iter()
            .flat_map(|batch| batch.rows.iter())
            .map(|row| {
                let subject = text_subject(&row.identity, &request.document_kind)?;
                let score = match row.values.get("_score") {
                    Some(RuntimeValue::Decimal(value)) => value.parse::<f64>().map_err(|_| {
                        ServiceError::Query("BM25 score is not a finite decimal".into())
                    })?,
                    _ => {
                        return Err(ServiceError::Query(
                            "BM25 result is missing its score".into(),
                        ))
                    }
                };
                if !score.is_finite() {
                    return Err(ServiceError::Query("BM25 score is not finite".into()));
                }
                Ok((subject, score))
            })
            .collect::<Result<Vec<_>>>()?;

        let vector_request = SearchVectors {
            scope: request.scope.clone(),
            valid_at: request.valid_at,
            collection_id: Some(request.collection_id.clone()),
            vector_name: Some(request.vector_name.clone()),
            field: None,
            query: request.vector_query.clone(),
            filter: request.vector_filter.clone(),
            metric: None,
            top_k: request.candidate_k,
            mode: request.vector_mode,
            max_scanned_changes: request.max_scanned_changes,
        };
        let vector = self.search_vectors_at(&vector_request, read.clone())?;
        if vector.read_manifest_sha256 != read.manifest_id
            || vector.known_at_cursor != read.commit_cursor
            || text_execution.read_manifest != read.manifest_id
            || text_execution.known_at_cursor != read.commit_cursor
        {
            return Err(ServiceError::StorageConflict(
                "hybrid branches did not execute at the shared read stamp".into(),
            ));
        }

        let text_candidates = u64::try_from(text_hits.len())
            .map_err(|_| ServiceError::Contract("text candidate count exceeds u64".into()))?;
        let mut candidates = BTreeMap::<DataReference, FusionCandidate>::new();
        for (ordinal, (subject, score)) in text_hits.into_iter().enumerate() {
            let rank = u64::try_from(ordinal + 1)
                .map_err(|_| ServiceError::Contract("text rank exceeds u64".into()))?;
            candidates.insert(
                subject.clone(),
                FusionCandidate {
                    subject,
                    vector_reference: None,
                    text_score: Some(score),
                    vector_score: None,
                    text_rank: Some(rank),
                    vector_rank: None,
                },
            );
        }
        let mut vector_candidates = 0_u64;
        for (ordinal, hit) in vector.hits.iter().enumerate() {
            if hit.subject.kind != request.document_kind {
                continue;
            }
            vector_candidates = vector_candidates.checked_add(1).ok_or_else(|| {
                ServiceError::Contract("vector candidate count overflowed".into())
            })?;
            let rank = u64::try_from(ordinal + 1)
                .map_err(|_| ServiceError::Contract("vector rank exceeds u64".into()))?;
            let candidate =
                candidates
                    .entry(hit.subject.clone())
                    .or_insert_with(|| FusionCandidate {
                        subject: hit.subject.clone(),
                        vector_reference: None,
                        text_score: None,
                        vector_score: None,
                        text_rank: None,
                        vector_rank: None,
                    });
            if candidate.vector_rank.is_none() {
                candidate.vector_reference = Some(hit.reference.clone());
                candidate.vector_score = Some(hit.score);
                candidate.vector_rank = Some(rank);
            }
        }
        let mut hits = candidates
            .into_values()
            .map(|candidate| HybridSearchHit {
                fused_score: reciprocal_rank_score(
                    request.fusion,
                    candidate.text_rank,
                    candidate.vector_rank,
                ),
                subject: candidate.subject,
                vector_reference: candidate.vector_reference,
                text_score: candidate.text_score,
                vector_score: candidate.vector_score,
                text_rank: candidate.text_rank,
                vector_rank: candidate.vector_rank,
            })
            .collect::<Vec<_>>();
        hits.sort_by(|left, right| {
            right
                .fused_score
                .total_cmp(&left.fused_score)
                .then_with(|| left.subject.cmp(&right.subject))
        });
        hits.truncate(
            usize::try_from(request.top_k)
                .map_err(|_| ServiceError::Contract("hybrid top_k exceeds usize".into()))?,
        );
        let fusion_plan_sha256 = digest::sha256_hex(
            &serde_json::to_vec(&(
                &read.manifest_id,
                &text_plan.digest,
                &vector.plan_sha256,
                request.fusion,
                request.top_k,
                request.candidate_k,
            ))
            .map_err(contract_json)?,
        );
        Ok(HybridSearchResult {
            scope: request.scope.clone(),
            read_manifest_sha256: read.manifest_id,
            known_at_cursor: read.commit_cursor,
            text_plan_sha256: text_plan.digest,
            vector_plan_sha256: vector.plan_sha256,
            fusion_plan_sha256,
            text_access_path: CanonicalId::new(text_access_path)
                .map_err(|error| ServiceError::Contract(error.to_string()))?,
            vector_access_path: vector.access_path,
            vector_exact: vector.exact,
            text_candidates,
            vector_candidates,
            fusion: request.fusion,
            hits,
        })
    }
}

fn text_subject(identity: &str, kind: &CanonicalId) -> Result<DataReference> {
    let prefix = format!("record:{kind}:");
    let id = identity.strip_prefix(&prefix).ok_or_else(|| {
        ServiceError::Query("BM25 result identity differs from the hybrid document kind".into())
    })?;
    Ok(DataReference {
        kind: kind.clone(),
        id: CanonicalId::new(id).map_err(|error| ServiceError::Query(error.to_string()))?,
    })
}

fn reciprocal_rank_score(
    fusion: HybridFusion,
    text_rank: Option<u64>,
    vector_rank: Option<u64>,
) -> f64 {
    let HybridFusion::ReciprocalRank {
        rank_constant,
        text_weight_millionths,
        vector_weight_millionths,
    } = fusion;
    let contribution = |rank: Option<u64>, weight: u32| {
        rank.map_or(0.0, |rank| {
            f64::from(weight) / 1_000_000.0 / (f64::from(rank_constant) + rank as f64)
        })
    };
    contribution(text_rank, text_weight_millionths)
        + contribution(vector_rank, vector_weight_millionths)
}
