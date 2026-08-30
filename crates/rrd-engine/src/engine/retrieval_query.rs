use super::vector::{
    core_vector, internal_vector_filter, internal_vector_query, public_data_ref,
    validate_collection_query, vector_collection_error,
};
use super::*;
use std::collections::BTreeSet;

#[derive(Debug, Clone)]
struct RankedCandidate {
    subject: DataReference,
    vector_references: BTreeSet<DataReference>,
    score: f64,
    contributions: Vec<RetrievalContribution>,
    properties: RuntimeProperties,
}

#[derive(Debug)]
struct QueryCandidates {
    values: Vec<RankedCandidate>,
    exact: bool,
}

struct RetrievalExecutionContext<'a> {
    engine: &'a RrdEngine,
    request: &'a ExecuteRetrievalQuery,
    scope: ScopeId,
    read: ReadStamp,
    vectors: Vec<rrd_vector::VectorCandidate>,
    configurations: BTreeMap<ProjectionId, rrd_vector::NamedVectorConfig>,
    stages: Vec<RetrievalStageEvidence>,
}

impl RrdEngine {
    /// Executes one bounded recursive retrieval program against one immutable
    /// RRD read stamp. Every leaf remains collection-bound; every fusion,
    /// reranking, and result-shaping transition publishes stage evidence.
    #[allow(clippy::too_many_arguments)]
    pub fn execute_retrieval_query(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        request: &ExecuteRetrievalQuery,
        now: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<RetrievalQueryResult> {
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
        let catalogue = rrd_vector::VectorCollectionRepository::new(&self.storage, scope.clone())
            .load()
            .map_err(vector_collection_error)?;
        let collection_id =
            ProjectionId::new(request.collection_id.as_str()).map_err(core_vector)?;
        let collection = catalogue.collections.get(&collection_id).ok_or_else(|| {
            ServiceError::Vector(format!(
                "unknown vector collection {}",
                request.collection_id
            ))
        })?;
        for property in required_payload_properties(&request.query, &request.result_shape) {
            let property_id = ProjectionId::new(&property).map_err(core_vector)?;
            if !collection.payload_indexes.contains_key(&property_id) {
                return Err(ServiceError::Vector(format!(
                    "retrieval property {property} is not an active typed payload index"
                )));
            }
        }
        let scan_limit = usize::try_from(request.max_scanned_changes)
            .map_err(|_| ServiceError::Vector("retrieval scan budget exceeds usize".into()))?;
        let page = self.storage.runtime_read_changes(&read, 0, scan_limit)?;
        if page.through_cursor < page.head_cursor {
            return Err(ServiceError::Vector(format!(
                "retrieval query requires more than {} retained changes",
                request.max_scanned_changes
            )));
        }
        let vectors =
            rrd_vector::candidates_from_changes(&page.changes, &scope)
                .into_iter()
                .filter(|candidate| {
                    candidate.vector.collection.as_ref().is_some_and(|address| {
                        address.collection_id == request.collection_id.as_str()
                    })
                })
                .collect();
        let mut context = RetrievalExecutionContext {
            engine: self,
            request,
            scope,
            read: read.clone(),
            vectors,
            configurations: collection.definition.vectors.clone(),
            stages: Vec::new(),
        };
        let candidates = context.execute(&request.query, request.candidate_limit)?;
        let output = context.shape(candidates.values)?;
        let query_plan_sha256 = digest::sha256_hex(
            &serde_json::to_vec(&(
                &read.manifest_id,
                read.commit_cursor,
                request,
                &context.stages,
            ))
            .map_err(contract_json)?,
        );
        Ok(RetrievalQueryResult {
            scope: request.scope.clone(),
            collection_id: request.collection_id.clone(),
            read_manifest_sha256: read.manifest_id,
            known_at_cursor: read.commit_cursor,
            query_plan_sha256,
            stages: context.stages,
            output,
        })
    }
}

impl RetrievalExecutionContext<'_> {
    fn execute(&mut self, query: &RetrievalQuery, limit: u64) -> Result<QueryCandidates> {
        match query {
            RetrievalQuery::Nearest {
                using,
                query,
                filter,
                mode,
            } => self.nearest(using, query, filter.as_ref(), *mode, limit),
            RetrievalQuery::Keyword {
                document_kind,
                text_field,
                query,
            } => self.keyword(document_kind, text_field, query, limit),
            RetrievalQuery::Recommend {
                using,
                positive,
                negative,
                strategy,
                filter,
            } => self.recommend(using, positive, negative, *strategy, filter.as_ref(), limit),
            RetrievalQuery::Discover {
                using,
                target,
                context,
                filter,
            } => self.discover(using, Some(target), context, filter.as_ref(), limit),
            RetrievalQuery::Context {
                using,
                context,
                filter,
            } => self.discover(using, None, context, filter.as_ref(), limit),
            RetrievalQuery::Fusion { prefetch, fusion } => self.fusion(prefetch, fusion, limit),
            RetrievalQuery::Rerank { prefetch, stages } => self.rerank(prefetch, stages, limit),
        }
    }

    fn nearest(
        &mut self,
        using: &CanonicalId,
        query: &VectorSearchQuery,
        filter: Option<&VectorPayloadFilter>,
        mode: VectorSearchMode,
        limit: u64,
    ) -> Result<QueryCandidates> {
        let vector_request = SearchVectors {
            scope: self.request.scope.clone(),
            valid_at: self.request.valid_at,
            collection_id: Some(self.request.collection_id.clone()),
            vector_name: Some(using.clone()),
            field: None,
            query: query.clone(),
            filter: filter.cloned(),
            metric: None,
            top_k: limit,
            mode,
            max_scanned_changes: self.request.max_scanned_changes,
        };
        let search = self
            .engine
            .search_vectors_at(&vector_request, self.read.clone())?;
        let visible = self.visible_named(using, filter)?;
        let by_version = visible
            .into_iter()
            .map(|candidate| {
                Ok((
                    (
                        public_data_ref(&candidate.vector.reference)?,
                        candidate.source_cursor,
                    ),
                    candidate,
                ))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        let mut values = search
            .hits
            .into_iter()
            .map(|hit| {
                let candidate = by_version
                    .get(&(hit.reference.clone(), hit.source_cursor))
                    .ok_or_else(|| {
                        ServiceError::Vector(
                            "vector search hit is absent from its authoritative visible set".into(),
                        )
                    })?;
                Ok(RankedCandidate {
                    subject: hit.subject,
                    vector_references: BTreeSet::from([hit.reference]),
                    score: hit.score,
                    contributions: Vec::new(),
                    properties: candidate.vector.properties.clone(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let input = self.vectors.len();
        self.register_stage("nearest", input, &mut values, search.exact, &vector_request)?;
        Ok(QueryCandidates {
            values,
            exact: search.exact,
        })
    }

    fn keyword(
        &mut self,
        document_kind: &CanonicalId,
        text_field: &CanonicalId,
        text: &str,
        limit: u64,
    ) -> Result<QueryCandidates> {
        let query = rrd_query::parse(&format!(
            "FROM record:{document_kind} AT VALID {} KNOWN {} WHERE {text_field} MATCH $retrieval_text LIMIT {limit}",
            self.request.valid_at, self.read.commit_cursor,
        ))
        .map_err(|error| ServiceError::Query(error.to_string()))?;
        let parameters = rrd_query::Parameters::from([(
            "retrieval_text".into(),
            RuntimeValue::String(text.to_owned()),
        )]);
        let catalogue = rrd_query::Catalog::capture_at(&self.engine.storage, self.read.clone())
            .map_err(|error| ServiceError::Query(error.to_string()))?;
        let bound = rrd_query::bind(&query, &parameters, &catalogue)
            .map_err(|error| ServiceError::Query(error.to_string()))?;
        let plan =
            rrd_query::plan(&bound).map_err(|error| ServiceError::Query(error.to_string()))?;
        let limit = usize::try_from(limit)
            .map_err(|_| ServiceError::Query("retrieval keyword limit exceeds usize".into()))?;
        let execution = rrd_query::execute(
            &self.engine.storage,
            &plan,
            &rrd_query::ExecutionBudget {
                max_scanned_changes: usize::try_from(self.request.max_scanned_changes).map_err(
                    |_| ServiceError::Query("retrieval keyword budget exceeds usize".into()),
                )?,
                max_rows: limit,
                max_output_bytes: 8 * 1024 * 1024,
                max_batch_rows: limit.clamp(1, 1_024),
                ..rrd_query::ExecutionBudget::default()
            },
        )
        .map_err(|error| ServiceError::Query(error.to_string()))?;
        if execution.truncated
            || execution.read_manifest != self.read.manifest_id
            || execution.known_at_cursor != self.read.commit_cursor
        {
            return Err(ServiceError::StorageConflict(
                "retrieval keyword branch was truncated or changed read stamp".into(),
            ));
        }
        let payloads = self.subject_payloads()?;
        let mut values = execution
            .batches
            .iter()
            .flat_map(|batch| batch.rows.iter())
            .map(|row| {
                let subject = text_subject(&row.identity, document_kind)?;
                let Some(properties) = payloads.get(&subject).cloned() else {
                    return Ok(None);
                };
                let score = match row.values.get("_score") {
                    Some(RuntimeValue::Decimal(value)) => value.parse::<f64>().map_err(|_| {
                        ServiceError::Query("keyword score is not a finite decimal".into())
                    })?,
                    _ => {
                        return Err(ServiceError::Query(
                            "keyword result is missing its score".into(),
                        ))
                    }
                };
                if !score.is_finite() {
                    return Err(ServiceError::Query("keyword score is not finite".into()));
                }
                Ok(Some(RankedCandidate {
                    subject,
                    vector_references: BTreeSet::new(),
                    score,
                    contributions: Vec::new(),
                    properties,
                }))
            })
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        sort_candidates(&mut values);
        values.truncate(limit);
        self.register_stage("keyword", payloads.len(), &mut values, true, &plan.digest)?;
        Ok(QueryCandidates {
            values,
            exact: true,
        })
    }

    fn recommend(
        &mut self,
        using: &CanonicalId,
        positive: &[RetrievalVectorExample],
        negative: &[RetrievalVectorExample],
        strategy: RetrievalRecommendStrategy,
        filter: Option<&VectorPayloadFilter>,
        limit: u64,
    ) -> Result<QueryCandidates> {
        let config = self.configuration(using)?.clone();
        let positive = positive
            .iter()
            .map(|example| self.resolve_example(using, example))
            .collect::<Result<Vec<_>>>()?;
        let negative = negative
            .iter()
            .map(|example| self.resolve_example(using, example))
            .collect::<Result<Vec<_>>>()?;
        let averaged = (strategy == RetrievalRecommendStrategy::AverageVector)
            .then(|| average_query(&positive, &negative))
            .transpose()?;
        let visible = self.visible_named(using, filter)?;
        let input = visible.len();
        let mut values = visible
            .into_iter()
            .map(|candidate| {
                let score = match &averaged {
                    Some(query) => {
                        rrd_vector::score_query(query, &candidate.vector.value, config.metric)
                    }
                    None => best_score_recommendation(
                        &candidate.vector.value,
                        &positive,
                        &negative,
                        config.metric,
                    ),
                }
                .map_err(core_vector)?;
                Ok(RankedCandidate {
                    subject: public_data_ref(&candidate.vector.subject)?,
                    vector_references: BTreeSet::from([public_data_ref(
                        &candidate.vector.reference,
                    )?]),
                    score,
                    contributions: Vec::new(),
                    properties: candidate.vector.properties.clone(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        sort_candidates(&mut values);
        values.truncate(usize_limit(limit)?);
        self.register_stage(
            "recommend",
            input,
            &mut values,
            true,
            &(using, positive.len(), negative.len(), strategy),
        )?;
        Ok(QueryCandidates {
            values,
            exact: true,
        })
    }

    fn discover(
        &mut self,
        using: &CanonicalId,
        target: Option<&RetrievalVectorExample>,
        context: &[RetrievalContextPair],
        filter: Option<&VectorPayloadFilter>,
        limit: u64,
    ) -> Result<QueryCandidates> {
        let config = self.configuration(using)?.clone();
        let target = target
            .map(|example| self.resolve_example(using, example))
            .transpose()?;
        let context = context
            .iter()
            .map(|pair| {
                Ok((
                    self.resolve_example(using, &pair.positive)?,
                    self.resolve_example(using, &pair.negative)?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let visible = self.visible_named(using, filter)?;
        let input = visible.len();
        let mut values = visible
            .into_iter()
            .map(|candidate| {
                let score = discovery_score(
                    &candidate.vector.value,
                    target.as_ref(),
                    &context,
                    config.metric,
                )
                .map_err(core_vector)?;
                Ok(RankedCandidate {
                    subject: public_data_ref(&candidate.vector.subject)?,
                    vector_references: BTreeSet::from([public_data_ref(
                        &candidate.vector.reference,
                    )?]),
                    score,
                    contributions: Vec::new(),
                    properties: candidate.vector.properties.clone(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        sort_candidates(&mut values);
        values.truncate(usize_limit(limit)?);
        let kind = if target.is_some() {
            "discover"
        } else {
            "context"
        };
        self.register_stage(kind, input, &mut values, true, &(using, context.len()))?;
        Ok(QueryCandidates {
            values,
            exact: true,
        })
    }

    fn fusion(
        &mut self,
        prefetch: &[RetrievalPrefetch],
        fusion: &RetrievalFusion,
        limit: u64,
    ) -> Result<QueryCandidates> {
        let RetrievalFusion::ReciprocalRank {
            rank_constant,
            weights_millionths,
        } = fusion;
        let mut branches = Vec::with_capacity(prefetch.len());
        for branch in prefetch {
            branches.push(self.execute(&branch.query, branch.limit)?);
        }
        let exact = branches.iter().all(|branch| branch.exact);
        let input = branches.iter().map(|branch| branch.values.len()).sum();
        let mut merged = BTreeMap::<DataReference, RankedCandidate>::new();
        for (branch, weight) in branches.into_iter().zip(weights_millionths) {
            for (offset, candidate) in branch.values.into_iter().enumerate() {
                let rank = u64::try_from(offset + 1)
                    .map_err(|_| ServiceError::Vector("retrieval branch rank overflowed".into()))?;
                let contribution =
                    f64::from(*weight) / 1_000_000.0 / (f64::from(*rank_constant) + rank as f64);
                let entry =
                    merged
                        .entry(candidate.subject.clone())
                        .or_insert_with(|| RankedCandidate {
                            subject: candidate.subject.clone(),
                            vector_references: BTreeSet::new(),
                            score: 0.0,
                            contributions: Vec::new(),
                            properties: RuntimeProperties::new(),
                        });
                entry.score += contribution;
                entry.vector_references.extend(candidate.vector_references);
                entry.contributions.extend(candidate.contributions);
                merge_properties(&mut entry.properties, candidate.properties)?;
            }
        }
        let mut values = merged.into_values().collect::<Vec<_>>();
        sort_candidates(&mut values);
        values.truncate(usize_limit(limit)?);
        self.register_stage("fusion-rrf", input, &mut values, exact, fusion)?;
        Ok(QueryCandidates { values, exact })
    }

    fn rerank(
        &mut self,
        prefetch: &RetrievalPrefetch,
        stages: &[RetrievalRerankStage],
        limit: u64,
    ) -> Result<QueryCandidates> {
        let mut result = self.execute(&prefetch.query, prefetch.limit)?;
        for stage in stages {
            let input = result.values.len();
            let (kind, exact) = match stage {
                RetrievalRerankStage::ScoreBoost {
                    filter,
                    add_millionths,
                    multiply_millionths,
                } => {
                    let filter = internal_vector_filter(filter)?;
                    for candidate in &mut result.values {
                        if filter.matches(&candidate.properties) {
                            candidate.score = candidate.score * f64::from(*multiply_millionths)
                                / 1_000_000.0
                                + f64::from(*add_millionths) / 1_000_000.0;
                            if !candidate.score.is_finite() {
                                return Err(ServiceError::Vector(
                                    "retrieval score boost produced a non-finite score".into(),
                                ));
                            }
                        }
                    }
                    sort_candidates(&mut result.values);
                    ("score-boost", true)
                }
                RetrievalRerankStage::Exact { using, query } => {
                    self.exact_rerank(&mut result.values, using, query, None)?;
                    ("exact-rerank", true)
                }
                RetrievalRerankStage::Model {
                    using,
                    model,
                    query,
                } => {
                    self.exact_rerank(&mut result.values, using, query, Some(model))?;
                    ("model-rerank", true)
                }
                RetrievalRerankStage::Mmr {
                    using,
                    diversity_millionths,
                } => {
                    self.mmr(&mut result.values, using, *diversity_millionths)?;
                    ("mmr", true)
                }
            };
            self.register_stage(kind, input, &mut result.values, exact, stage)?;
        }
        let input = result.values.len();
        result.values.truncate(usize_limit(limit)?);
        self.register_stage("limit", input, &mut result.values, true, &limit)?;
        Ok(result)
    }

    fn exact_rerank(
        &self,
        candidates: &mut Vec<RankedCandidate>,
        using: &CanonicalId,
        query: &VectorSearchQuery,
        model: Option<&VectorEmbeddingModel>,
    ) -> Result<()> {
        let config = self.configuration(using)?;
        validate_collection_query(config, query)?;
        if let Some(model) = model {
            let expected = config.embedding_model.as_ref().ok_or_else(|| {
                ServiceError::Vector("model rerank requires a model-bound named vector".into())
            })?;
            if expected.name != model.name || expected.digest != model.digest {
                return Err(ServiceError::Vector(
                    "model rerank identity differs from the named-vector binding".into(),
                ));
            }
        }
        let by_subject = self.visible_by_subject(using, None)?;
        let query = internal_vector_query(query);
        let mut reranked = Vec::with_capacity(candidates.len());
        for mut candidate in candidates.drain(..) {
            let Some(vector) = by_subject.get(&candidate.subject) else {
                continue;
            };
            let score = rrd_vector::score_query(&query, &vector.vector.value, config.metric)
                .map_err(core_vector)?;
            let reference = public_data_ref(&vector.vector.reference)?;
            candidate.score = score;
            candidate.vector_references.insert(reference);
            reranked.push(candidate);
        }
        *candidates = reranked;
        sort_candidates(candidates);
        Ok(())
    }

    fn mmr(
        &self,
        candidates: &mut Vec<RankedCandidate>,
        using: &CanonicalId,
        diversity_millionths: u32,
    ) -> Result<()> {
        let config = self.configuration(using)?;
        let by_subject = self.visible_by_subject(using, None)?;
        candidates.retain(|candidate| by_subject.contains_key(&candidate.subject));
        sort_candidates(candidates);
        let mut remaining = std::mem::take(candidates)
            .into_iter()
            .enumerate()
            .collect::<Vec<_>>();
        let diversity = f64::from(diversity_millionths) / 1_000_000.0;
        let relevance_weight = 1.0 - diversity;
        let mut selected = Vec::<RankedCandidate>::new();
        while !remaining.is_empty() {
            let mut best = None::<(usize, f64)>;
            for (position, (original_rank, candidate)) in remaining.iter().enumerate() {
                let relevance = 1.0 / (*original_rank as f64 + 1.0);
                let candidate_vector = &by_subject[&candidate.subject].vector.value;
                let max_similarity = selected
                    .iter()
                    .filter_map(|chosen| by_subject.get(&chosen.subject))
                    .map(|chosen| {
                        let query = query_from_value(&chosen.vector.value);
                        rrd_vector::score_query(&query, candidate_vector, config.metric)
                            .map(bounded_similarity)
                    })
                    .collect::<rrd_core::Result<Vec<_>>>()
                    .map_err(core_vector)?
                    .into_iter()
                    .max_by(f64::total_cmp)
                    .unwrap_or(0.0);
                let score = relevance_weight * relevance - diversity * max_similarity;
                if best.is_none_or(|(_, current)| score > current) {
                    best = Some((position, score));
                }
            }
            let (position, score) = best.expect("remaining MMR candidates are non-empty");
            let (_, mut candidate) = remaining.remove(position);
            candidate.score = score;
            selected.push(candidate);
        }
        *candidates = selected;
        Ok(())
    }

    fn shape(&mut self, mut candidates: Vec<RankedCandidate>) -> Result<RetrievalOutput> {
        sort_candidates(&mut candidates);
        match &self.request.result_shape {
            RetrievalResultShape::Points => {
                let input = candidates.len();
                candidates.truncate(usize_limit(self.request.limit)?);
                self.register_stage("points", input, &mut candidates, true, &self.request.limit)?;
                Ok(RetrievalOutput::Points {
                    hits: candidates.into_iter().map(public_hit).collect(),
                })
            }
            RetrievalResultShape::Groups {
                property,
                max_groups,
                hits_per_group,
            } => {
                let input = candidates.len();
                let mut grouped = BTreeMap::<Vec<u8>, (QueryValue, Vec<RankedCandidate>)>::new();
                for candidate in candidates {
                    let Some(value) = candidate.properties.get(property.as_str()) else {
                        continue;
                    };
                    let public = query_value(value)?;
                    let key = serde_json::to_vec(&public).map_err(contract_json)?;
                    grouped
                        .entry(key)
                        .or_insert_with(|| (public, Vec::new()))
                        .1
                        .push(candidate);
                }
                let mut grouped = grouped.into_values().collect::<Vec<_>>();
                grouped.sort_by(|left, right| {
                    right
                        .1
                        .first()
                        .map(|candidate| candidate.score)
                        .unwrap_or(f64::NEG_INFINITY)
                        .total_cmp(
                            &left
                                .1
                                .first()
                                .map(|candidate| candidate.score)
                                .unwrap_or(f64::NEG_INFINITY),
                        )
                });
                grouped.truncate(usize_limit(*max_groups)?);
                let mut groups = Vec::with_capacity(grouped.len());
                for (value, mut hits) in grouped {
                    hits.truncate(usize_limit(*hits_per_group)?);
                    groups.push(RetrievalGroup {
                        value,
                        hits: hits.into_iter().map(public_hit).collect(),
                    });
                }
                self.register_shape_stage(
                    "groups",
                    input,
                    groups.len(),
                    &self.request.result_shape,
                )?;
                Ok(RetrievalOutput::Groups { groups })
            }
            RetrievalResultShape::Facets { property, limit } => {
                let input = candidates.len();
                let mut counts = BTreeMap::<Vec<u8>, (QueryValue, u64)>::new();
                for candidate in candidates {
                    let Some(value) = candidate.properties.get(property.as_str()) else {
                        continue;
                    };
                    let public = query_value(value)?;
                    let key = serde_json::to_vec(&public).map_err(contract_json)?;
                    let entry = counts.entry(key).or_insert((public, 0));
                    entry.1 = entry.1.checked_add(1).ok_or_else(|| {
                        ServiceError::Vector("retrieval facet count overflowed".into())
                    })?;
                }
                let mut counts = counts.into_iter().collect::<Vec<_>>();
                counts.sort_by(|left, right| {
                    right
                        .1
                         .1
                        .cmp(&left.1 .1)
                        .then_with(|| left.0.cmp(&right.0))
                });
                counts.truncate(usize_limit(*limit)?);
                let facets = counts
                    .into_iter()
                    .map(|(_, (value, count))| RetrievalFacet { value, count })
                    .collect::<Vec<_>>();
                self.register_shape_stage(
                    "facets",
                    input,
                    facets.len(),
                    &self.request.result_shape,
                )?;
                Ok(RetrievalOutput::Facets { facets })
            }
            RetrievalResultShape::Matrix { using, sample } => {
                let input = candidates.len();
                let config = self.configuration(using)?.clone();
                let by_subject = self.visible_by_subject(using, None)?;
                let sampled = candidates
                    .into_iter()
                    .filter(|candidate| by_subject.contains_key(&candidate.subject))
                    .take(usize_limit(*sample)?)
                    .collect::<Vec<_>>();
                let subjects = sampled
                    .iter()
                    .map(|candidate| candidate.subject.clone())
                    .collect::<Vec<_>>();
                let mut cells = Vec::new();
                for left in &sampled {
                    let left_vector = &by_subject[&left.subject].vector.value;
                    let query = query_from_value(left_vector);
                    for right in &sampled {
                        if left.subject == right.subject {
                            continue;
                        }
                        let right_vector = &by_subject[&right.subject].vector.value;
                        cells.push(RetrievalMatrixCell {
                            left: left.subject.clone(),
                            right: right.subject.clone(),
                            score: rrd_vector::score_query(&query, right_vector, config.metric)
                                .map_err(core_vector)?,
                        });
                    }
                }
                self.register_shape_stage(
                    "matrix",
                    input,
                    cells.len(),
                    &self.request.result_shape,
                )?;
                Ok(RetrievalOutput::Matrix { subjects, cells })
            }
        }
    }

    fn configuration(&self, using: &CanonicalId) -> Result<&rrd_vector::NamedVectorConfig> {
        self.configurations
            .get(&ProjectionId::new(using.as_str()).map_err(core_vector)?)
            .ok_or_else(|| {
                ServiceError::Vector(format!(
                    "unknown named vector {using} in collection {}",
                    self.request.collection_id
                ))
            })
    }

    fn visible_named(
        &self,
        using: &CanonicalId,
        filter: Option<&VectorPayloadFilter>,
    ) -> Result<Vec<rrd_vector::VectorCandidate>> {
        let config = self.configuration(using)?;
        let visibility = rrd_vector::VectorVisibilityRequest {
            scope: self.scope.clone(),
            read: self.read.clone(),
            valid_at: self.request.valid_at,
            field: config.field.clone(),
            embedding_model: config.embedding_model.clone(),
            filter: filter.map(internal_vector_filter).transpose()?,
        };
        rrd_vector::materialize_visible(&visibility, self.vectors.clone()).map_err(core_vector)
    }

    fn visible_by_subject(
        &self,
        using: &CanonicalId,
        filter: Option<&VectorPayloadFilter>,
    ) -> Result<BTreeMap<DataReference, rrd_vector::VectorCandidate>> {
        let mut values = BTreeMap::new();
        for candidate in self.visible_named(using, filter)? {
            let subject = public_data_ref(&candidate.vector.subject)?;
            if values.insert(subject, candidate).is_some() {
                return Err(ServiceError::Vector(format!(
                    "collection subject has multiple active {using} vectors"
                )));
            }
        }
        Ok(values)
    }

    fn subject_payloads(&self) -> Result<BTreeMap<DataReference, RuntimeProperties>> {
        let mut payloads = BTreeMap::<DataReference, RuntimeProperties>::new();
        for name in self.configurations.keys() {
            let name = CanonicalId::new(name.as_str())
                .map_err(|error| ServiceError::Vector(error.to_string()))?;
            for candidate in self.visible_named(&name, None)? {
                let subject = public_data_ref(&candidate.vector.subject)?;
                merge_properties(
                    payloads.entry(subject).or_default(),
                    candidate.vector.properties,
                )?;
            }
        }
        Ok(payloads)
    }

    fn resolve_example(
        &self,
        using: &CanonicalId,
        example: &RetrievalVectorExample,
    ) -> Result<rrd_vector::VectorQuery> {
        let config = self.configuration(using)?;
        match example {
            RetrievalVectorExample::Vector { value } => {
                validate_collection_query(config, value)?;
                Ok(internal_vector_query(value))
            }
            RetrievalVectorExample::Reference { reference } => self
                .visible_named(using, None)?
                .into_iter()
                .find(|candidate| {
                    public_data_ref(&candidate.vector.reference)
                        .is_ok_and(|candidate| &candidate == reference)
                })
                .map(|candidate| query_from_value(&candidate.vector.value))
                .ok_or_else(|| {
                    ServiceError::Vector(format!(
                        "retrieval example {}:{} is not visible in named vector {using}",
                        reference.kind, reference.id
                    ))
                }),
        }
    }

    fn register_stage<T: Serialize + ?Sized>(
        &mut self,
        kind: &str,
        input: usize,
        candidates: &mut [RankedCandidate],
        exact: bool,
        contract: &T,
    ) -> Result<u32> {
        let ordinal = self.push_stage(kind, input, candidates.len(), exact, contract)?;
        for (offset, candidate) in candidates.iter_mut().enumerate() {
            candidate.contributions.push(RetrievalContribution {
                stage_ordinal: ordinal,
                rank: u64::try_from(offset + 1)
                    .map_err(|_| ServiceError::Vector("retrieval rank overflowed".into()))?,
                score: candidate.score,
            });
        }
        Ok(ordinal)
    }

    fn register_shape_stage<T: Serialize + ?Sized>(
        &mut self,
        kind: &str,
        input: usize,
        output: usize,
        contract: &T,
    ) -> Result<()> {
        self.push_stage(kind, input, output, true, contract)?;
        Ok(())
    }

    fn push_stage<T: Serialize + ?Sized>(
        &mut self,
        kind: &str,
        input: usize,
        output: usize,
        exact: bool,
        contract: &T,
    ) -> Result<u32> {
        let ordinal = u32::try_from(self.stages.len() + 1)
            .map_err(|_| ServiceError::Vector("retrieval stage ordinal overflowed".into()))?;
        let plan_sha256 = digest::sha256_hex(
            &serde_json::to_vec(&(
                &self.read.manifest_id,
                self.read.commit_cursor,
                ordinal,
                kind,
                contract,
            ))
            .map_err(contract_json)?,
        );
        self.stages.push(RetrievalStageEvidence {
            ordinal,
            kind: CanonicalId::new(kind)
                .map_err(|error| ServiceError::Contract(error.to_string()))?,
            input_candidates: u64::try_from(input)
                .map_err(|_| ServiceError::Vector("retrieval input count overflowed".into()))?,
            output_candidates: u64::try_from(output)
                .map_err(|_| ServiceError::Vector("retrieval output count overflowed".into()))?,
            exact,
            plan_sha256,
        });
        Ok(ordinal)
    }
}

fn text_subject(identity: &str, kind: &CanonicalId) -> Result<DataReference> {
    let prefix = format!("record:{kind}:");
    let id = identity.strip_prefix(&prefix).ok_or_else(|| {
        ServiceError::Query("keyword result identity differs from its document kind".into())
    })?;
    Ok(DataReference {
        kind: kind.clone(),
        id: CanonicalId::new(id).map_err(|error| ServiceError::Query(error.to_string()))?,
    })
}

fn required_payload_properties(
    query: &RetrievalQuery,
    shape: &RetrievalResultShape,
) -> BTreeSet<String> {
    let mut properties = BTreeSet::new();
    fn visit(query: &RetrievalQuery, properties: &mut BTreeSet<String>) {
        match query {
            RetrievalQuery::Nearest { filter, .. }
            | RetrievalQuery::Recommend { filter, .. }
            | RetrievalQuery::Discover { filter, .. }
            | RetrievalQuery::Context { filter, .. } => {
                if let Some(filter) = filter {
                    properties.extend(public_filter_properties(filter));
                }
            }
            RetrievalQuery::Fusion { prefetch, .. } => {
                for branch in prefetch {
                    visit(&branch.query, properties);
                }
            }
            RetrievalQuery::Rerank { prefetch, stages } => {
                visit(&prefetch.query, properties);
                for stage in stages {
                    if let RetrievalRerankStage::ScoreBoost { filter, .. } = stage {
                        properties.extend(public_filter_properties(filter));
                    }
                }
            }
            RetrievalQuery::Keyword { .. } => {}
        }
    }
    visit(query, &mut properties);
    match shape {
        RetrievalResultShape::Groups { property, .. }
        | RetrievalResultShape::Facets { property, .. } => {
            properties.insert(property.as_str().to_owned());
        }
        RetrievalResultShape::Points | RetrievalResultShape::Matrix { .. } => {}
    }
    properties
}

fn public_filter_properties(filter: &VectorPayloadFilter) -> Vec<String> {
    let mut properties = Vec::new();
    match filter {
        VectorPayloadFilter::Condition { condition } => {
            properties.push(condition.property.as_str().to_owned());
        }
        VectorPayloadFilter::All { filters } | VectorPayloadFilter::Any { filters } => {
            for filter in filters {
                properties.extend(public_filter_properties(filter));
            }
        }
        VectorPayloadFilter::Not { filter } => {
            properties.extend(public_filter_properties(filter));
        }
    }
    properties
}

fn average_query(
    positive: &[rrd_vector::VectorQuery],
    negative: &[rrd_vector::VectorQuery],
) -> Result<rrd_vector::VectorQuery> {
    let values = positive
        .iter()
        .map(rrd_vector::VectorQuery::as_value)
        .chain(negative.iter().map(rrd_vector::VectorQuery::as_value))
        .collect::<Vec<_>>();
    let first = values
        .first()
        .ok_or_else(|| ServiceError::Vector("recommendation has no positive vector".into()))?;
    let signed_scale = |position: usize| {
        if position < positive.len() {
            1.0 / positive.len() as f32
        } else if negative.is_empty() {
            0.0
        } else {
            -1.0 / negative.len() as f32
        }
    };
    let value = match first {
        VectorValue::Dense { values: first } => {
            let mut mean = vec![0.0_f32; first.len()];
            for (position, value) in values.iter().enumerate() {
                let VectorValue::Dense { values } = value else {
                    return Err(ServiceError::Vector(
                        "recommendation examples use different vector kinds".into(),
                    ));
                };
                if values.len() != mean.len() {
                    return Err(ServiceError::Vector(
                        "recommendation examples use different dimensions".into(),
                    ));
                }
                let scale = signed_scale(position);
                for (output, value) in mean.iter_mut().zip(values) {
                    *output += value * scale;
                }
            }
            VectorValue::Dense { values: mean }
        }
        VectorValue::Sparse {
            dimensions,
            indices: _,
            values: _,
        } => {
            let mut mean = BTreeMap::<u32, f32>::new();
            for (position, value) in values.iter().enumerate() {
                let VectorValue::Sparse {
                    dimensions: current,
                    indices,
                    values,
                } = value
                else {
                    return Err(ServiceError::Vector(
                        "recommendation examples use different vector kinds".into(),
                    ));
                };
                if current != dimensions {
                    return Err(ServiceError::Vector(
                        "recommendation examples use different dimensions".into(),
                    ));
                }
                let scale = signed_scale(position);
                for (index, value) in indices.iter().zip(values) {
                    *mean.entry(*index).or_default() += value * scale;
                }
            }
            mean.retain(|_, value| *value != 0.0);
            VectorValue::Sparse {
                dimensions: *dimensions,
                indices: mean.keys().copied().collect(),
                values: mean.values().copied().collect(),
            }
        }
        VectorValue::MultiDense {
            dimensions,
            vectors: first,
        } => {
            let mut mean = vec![vec![0.0_f32; *dimensions as usize]; first.len()];
            for (position, value) in values.iter().enumerate() {
                let VectorValue::MultiDense {
                    dimensions: current,
                    vectors,
                } = value
                else {
                    return Err(ServiceError::Vector(
                        "recommendation examples use different vector kinds".into(),
                    ));
                };
                if current != dimensions || vectors.len() != mean.len() {
                    return Err(ServiceError::Vector(
                        "multi-vector recommendation examples use different shapes".into(),
                    ));
                }
                let scale = signed_scale(position);
                for (output_row, row) in mean.iter_mut().zip(vectors) {
                    for (output, value) in output_row.iter_mut().zip(row) {
                        *output += value * scale;
                    }
                }
            }
            VectorValue::MultiDense {
                dimensions: *dimensions,
                vectors: mean,
            }
        }
    };
    value.validate().map_err(core_vector)?;
    Ok(query_from_value(&value))
}

fn best_score_recommendation(
    candidate: &VectorValue,
    positive: &[rrd_vector::VectorQuery],
    negative: &[rrd_vector::VectorQuery],
    metric: rrd_vector::ScoreMetric,
) -> rrd_core::Result<f64> {
    let positive = positive
        .iter()
        .map(|query| rrd_vector::score_query(query, candidate, metric))
        .collect::<rrd_core::Result<Vec<_>>>()?
        .into_iter()
        .max_by(f64::total_cmp)
        .expect("recommendation has positive examples");
    let negative = negative
        .iter()
        .map(|query| rrd_vector::score_query(query, candidate, metric))
        .collect::<rrd_core::Result<Vec<_>>>()?
        .into_iter()
        .max_by(f64::total_cmp);
    Ok(negative.map_or(positive, |negative| {
        if positive > negative {
            positive
        } else {
            -negative
        }
    }))
}

fn discovery_score(
    candidate: &VectorValue,
    target: Option<&rrd_vector::VectorQuery>,
    context: &[(rrd_vector::VectorQuery, rrd_vector::VectorQuery)],
    metric: rrd_vector::ScoreMetric,
) -> rrd_core::Result<f64> {
    let mut wins = 0_u64;
    let mut bounded_margin = 0.0;
    for (positive, negative) in context {
        let margin = rrd_vector::score_query(positive, candidate, metric)?
            - rrd_vector::score_query(negative, candidate, metric)?;
        if margin > 0.0 {
            wins += 1;
        }
        bounded_margin += margin.atan() / std::f64::consts::PI;
    }
    let context_score = wins as f64 + bounded_margin / context.len() as f64;
    let target_score = target
        .map(|target| rrd_vector::score_query(target, candidate, metric))
        .transpose()?
        .map_or(0.0, |score| score.atan() / std::f64::consts::PI * 1e-6);
    Ok(context_score + target_score)
}

fn query_from_value(value: &VectorValue) -> rrd_vector::VectorQuery {
    match value {
        VectorValue::Dense { values } => rrd_vector::VectorQuery::Dense {
            values: values.clone(),
        },
        VectorValue::Sparse {
            dimensions,
            indices,
            values,
        } => rrd_vector::VectorQuery::Sparse {
            dimensions: *dimensions,
            indices: indices.clone(),
            values: values.clone(),
        },
        VectorValue::MultiDense {
            dimensions,
            vectors,
        } => rrd_vector::VectorQuery::MultiDense {
            dimensions: *dimensions,
            vectors: vectors.clone(),
            comparator: rrd_vector::MultiVectorComparator::MaxSim,
        },
    }
}

fn bounded_similarity(score: f64) -> f64 {
    0.5 + score.atan() / std::f64::consts::PI
}

fn sort_candidates(candidates: &mut [RankedCandidate]) {
    candidates.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.subject.cmp(&right.subject))
    });
}

fn merge_properties(target: &mut RuntimeProperties, source: RuntimeProperties) -> Result<()> {
    for (name, value) in source {
        if target
            .insert(name.clone(), value.clone())
            .is_some_and(|current| current != value)
        {
            return Err(ServiceError::Vector(format!(
                "named vectors disagree on point payload property {name}"
            )));
        }
    }
    Ok(())
}

fn public_hit(candidate: RankedCandidate) -> RetrievalHit {
    RetrievalHit {
        subject: candidate.subject,
        vector_references: candidate.vector_references.into_iter().collect(),
        score: candidate.score,
        contributions: candidate.contributions,
    }
}

fn usize_limit(limit: u64) -> Result<usize> {
    usize::try_from(limit).map_err(|_| ServiceError::Vector("retrieval limit exceeds usize".into()))
}
