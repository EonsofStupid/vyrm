use super::*;

impl RrflowEngine {
    pub fn search_vectors(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        request: &SearchVectors,
        now: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<VectorSearchResult> {
        request
            .validate()
            .map_err(|error| ServiceError::Vector(error.to_string()))?;
        self.authorize(
            session_id,
            token,
            SecurityAction::VectorSearch,
            now,
            request_id,
            operation_id,
        )?;
        let expected_scope = format!("instance:{}", self.instance);
        if request.scope != expected_scope {
            return Err(ServiceError::WrongScope);
        }
        let scope = ScopeId::new(request.scope.clone()).map_err(core_vector)?;
        // Capture the shared data/catalogue stamp before loading collection
        // metadata. The subsequent runtime read validates the same stamp and
        // rejects a concurrent catalogue transition instead of combining old
        // collection metadata with a new read manifest.
        let read = self.storage.runtime_read_stamp(&scope)?;
        let (field, metric, embedding_model) = if let (Some(collection_id), Some(vector_name)) =
            (&request.collection_id, &request.vector_name)
        {
            let catalogue =
                vyrm_vector::VectorCollectionRepository::new(&self.storage, scope.clone())
                    .load()
                    .map_err(vector_collection_error)?;
            let collection = catalogue
                .collections
                .get(&ProjectionId::new(collection_id.as_str()).map_err(core_vector)?)
                .ok_or_else(|| {
                    ServiceError::Vector(format!("unknown vector collection {collection_id}"))
                })?;
            let vector = collection
                .definition
                .vectors
                .get(&ProjectionId::new(vector_name.as_str()).map_err(core_vector)?)
                .ok_or_else(|| {
                    ServiceError::Vector(format!(
                        "unknown named vector {vector_name} in collection {collection_id}"
                    ))
                })?;
            validate_collection_query(vector, &request.query)?;
            (
                vector.field.clone(),
                vector.metric,
                vector.embedding_model.clone(),
            )
        } else {
            let field = request
                .field
                .as_ref()
                .ok_or_else(|| ServiceError::Vector("vector field is absent".into()))?;
            let metric = request
                .metric
                .ok_or_else(|| ServiceError::Vector("vector metric is absent".into()))?;
            (
                field.as_str().to_owned(),
                internal_vector_metric(metric),
                None,
            )
        };
        let limit = usize::try_from(request.max_scanned_changes)
            .map_err(|_| ServiceError::Vector("vector scan budget exceeds usize".into()))?;
        let page = self.storage.runtime_read_changes(&read, 0, limit)?;
        if page.through_cursor < page.head_cursor {
            return Err(ServiceError::Vector(format!(
                "vector exact scan requires more than {} retained changes",
                request.max_scanned_changes
            )));
        }
        let candidates = vyrm_vector::candidates_from_changes(&page.changes, &scope);
        let runtime = vyrm_vector::VectorRuntime::new(candidates).map_err(core_vector)?;
        let query = match &request.query {
            VectorSearchQuery::Dense { values } => vyrm_vector::VectorQuery::Dense {
                values: values.clone(),
            },
            VectorSearchQuery::Sparse {
                dimensions,
                indices,
                values,
            } => vyrm_vector::VectorQuery::Sparse {
                dimensions: *dimensions,
                indices: indices.clone(),
                values: values.clone(),
            },
            VectorSearchQuery::MultiDense {
                dimensions,
                vectors,
                comparator: _,
            } => vyrm_vector::VectorQuery::MultiDense {
                dimensions: *dimensions,
                vectors: vectors.clone(),
                comparator: vyrm_vector::MultiVectorComparator::MaxSim,
            },
        };
        let search = vyrm_vector::SearchRequest {
            scope,
            read: read.clone(),
            valid_at: request.valid_at,
            field,
            query,
            metric,
            embedding_model,
            top_k: usize::try_from(request.top_k)
                .map_err(|_| ServiceError::Vector("vector top_k exceeds usize".into()))?,
            mode: vyrm_vector::SearchMode::Exact,
            filter: request
                .filter
                .as_ref()
                .map(internal_vector_filter)
                .transpose()?,
        };
        let prepared = runtime.prepare_search(&search, 1).map_err(core_vector)?;
        let execution = runtime
            .execute_search(&search, &prepared)
            .map_err(core_vector)?;
        let access_path = match execution.plan.selected.kind {
            vyrm_vector::AccessPathKind::ExactScan => "exact_scan",
            vyrm_vector::AccessPathKind::ExactSegment => "exact_segment",
            vyrm_vector::AccessPathKind::Hnsw => "hnsw",
            vyrm_vector::AccessPathKind::TurboQuant => "turboquant",
        };
        Ok(VectorSearchResult {
            scope: request.scope.clone(),
            collection_id: request.collection_id.clone(),
            vector_name: request.vector_name.clone(),
            read_manifest_sha256: read.manifest_id,
            known_at_cursor: read.commit_cursor,
            scanned_changes: page.validation.change_reads,
            plan_sha256: prepared.plan_digest().into(),
            access_path: CanonicalId::new(access_path)
                .map_err(|error| ServiceError::Vector(error.to_string()))?,
            exact: !execution.plan.selected.kind.is_approximate(),
            hits: execution
                .hits
                .into_iter()
                .map(|hit| {
                    Ok(VectorSearchHit {
                        reference: public_data_ref(&hit.reference)?,
                        subject: public_data_ref(&hit.subject)?,
                        source_cursor: hit.source_cursor,
                        score: hit.score,
                    })
                })
                .collect::<Result<_>>()?,
        })
    }
}

pub(in crate::engine) fn internal_vector_filter(
    filter: &VectorPayloadFilter,
) -> Result<vyrm_vector::FilterExpression> {
    Ok(match filter {
        VectorPayloadFilter::Condition { condition } => vyrm_vector::FilterExpression::Condition {
            condition: vyrm_vector::FilterCondition {
                property: condition.property.as_str().into(),
                operator: match &condition.operator {
                    VectorPayloadOperator::Equals { value } => {
                        vyrm_vector::FilterOperator::Equals {
                            value: runtime_value(value)?,
                        }
                    }
                    VectorPayloadOperator::NotEquals { value } => {
                        vyrm_vector::FilterOperator::NotEquals {
                            value: runtime_value(value)?,
                        }
                    }
                    VectorPayloadOperator::In { values } => vyrm_vector::FilterOperator::In {
                        values: values.iter().map(runtime_value).collect::<Result<_>>()?,
                    },
                    VectorPayloadOperator::Range { gt, gte, lt, lte } => {
                        vyrm_vector::FilterOperator::Range {
                            gt: gt.as_ref().map(runtime_value).transpose()?,
                            gte: gte.as_ref().map(runtime_value).transpose()?,
                            lt: lt.as_ref().map(runtime_value).transpose()?,
                            lte: lte.as_ref().map(runtime_value).transpose()?,
                        }
                    }
                    VectorPayloadOperator::Exists { value } => {
                        vyrm_vector::FilterOperator::Exists { value: *value }
                    }
                },
            },
        },
        VectorPayloadFilter::All { filters } => vyrm_vector::FilterExpression::All {
            filters: filters
                .iter()
                .map(internal_vector_filter)
                .collect::<Result<_>>()?,
        },
        VectorPayloadFilter::Any { filters } => vyrm_vector::FilterExpression::Any {
            filters: filters
                .iter()
                .map(internal_vector_filter)
                .collect::<Result<_>>()?,
        },
        VectorPayloadFilter::Not { filter } => vyrm_vector::FilterExpression::Not {
            filter: Box::new(internal_vector_filter(filter)?),
        },
    })
}
