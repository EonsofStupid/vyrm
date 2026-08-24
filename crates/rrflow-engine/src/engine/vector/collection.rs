use super::*;

impl RrflowEngine {
    pub fn ensure_vector_collection(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        request: &EnsureVectorCollection,
        context: &RequestContext,
        now: u64,
    ) -> Result<EnsureVectorCollectionResult> {
        context
            .validate(true)
            .map_err(|error| ServiceError::Contract(error.to_string()))?;
        let idempotency_key = context.idempotency_key.as_ref().ok_or_else(|| {
            ServiceError::Contract("vector collection ensure requires idempotency".into())
        })?;
        request
            .validate()
            .map_err(|error| ServiceError::Vector(error.to_string()))?;
        self.authorize(
            session_id,
            token,
            SecurityAction::VectorCollectionEnsure,
            now,
            context.request_id.as_str(),
            context.operation_id.as_str(),
        )?;
        let scope = self.query_scope(&request.scope)?;
        let operation_digest = operation_digest(request)?;
        let repository = vyrm_vector::VectorCollectionRepository::new(&self.storage, scope);
        if let Some(receipt) = repository
            .operation_receipt(idempotency_key.as_str(), &operation_digest)
            .map_err(vector_collection_error)?
        {
            let catalogue = repository.load().map_err(vector_collection_error)?;
            return Ok(EnsureVectorCollectionResult {
                collection: public_vector_collection(&receipt.entry)?,
                catalogue_revision: catalogue.revision,
                idempotent_replay: true,
            });
        }
        let definition = internal_vector_collection(request)?;
        let context = vyrm_vector::CollectionMutationContext {
            at: now,
            actor: format!("session:{}", session_id.as_str()),
            request_id: context.request_id.as_str().into(),
            operation_id: context.operation_id.as_str().into(),
        };
        let (catalogue, idempotent_replay) = repository
            .ensure(
                &context,
                idempotency_key.as_str().into(),
                operation_digest,
                definition,
            )
            .map_err(vector_collection_error)?;
        let entry = catalogue
            .collections
            .get(&ProjectionId::new(request.collection_id.as_str()).map_err(core_vector)?)
            .ok_or_else(|| ServiceError::Vector("ensured vector collection disappeared".into()))?;
        Ok(EnsureVectorCollectionResult {
            collection: public_vector_collection(entry)?,
            catalogue_revision: catalogue.revision,
            idempotent_replay,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn list_vector_collections(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        request: &ListVectorCollections,
        now: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<VectorCollectionCatalogueSnapshot> {
        request
            .validate()
            .map_err(|error| ServiceError::Vector(error.to_string()))?;
        self.authorize(
            session_id,
            token,
            SecurityAction::VectorCollectionList,
            now,
            request_id,
            operation_id,
        )?;
        let scope = self.query_scope(&request.scope)?;
        let catalogue = vyrm_vector::VectorCollectionRepository::new(&self.storage, scope)
            .load()
            .map_err(vector_collection_error)?;
        Ok(VectorCollectionCatalogueSnapshot {
            scope: request.scope.clone(),
            revision: catalogue.revision,
            collections: catalogue
                .collections
                .values()
                .map(public_vector_collection)
                .collect::<Result<_>>()?,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::engine) fn validate_collection_vector_mutations(
        &self,
        request: &CommitTransaction,
    ) -> Result<()> {
        let addressed = request.mutations.iter().any(|mutation| {
            matches!(
                mutation,
                TransactionMutation::PutVector {
                    collection_id: Some(_),
                    ..
                }
            )
        });
        if !addressed {
            return Ok(());
        }
        let scope = ScopeId::new(format!("instance:{}", self.instance)).map_err(core_vector)?;
        let catalogue = vyrm_vector::VectorCollectionRepository::new(&self.storage, scope)
            .load()
            .map_err(vector_collection_error)?;
        for mutation in &request.mutations {
            let TransactionMutation::PutVector {
                collection_id: Some(collection_id),
                vector_name: Some(vector_name),
                field,
                value,
                provenance,
                ..
            } = mutation
            else {
                continue;
            };
            let collection = catalogue
                .collections
                .get(&ProjectionId::new(collection_id.as_str()).map_err(core_vector)?)
                .ok_or_else(|| {
                    ServiceError::Vector(format!("unknown vector collection {collection_id}"))
                })?;
            let config = collection
                .definition
                .vectors
                .get(&ProjectionId::new(vector_name.as_str()).map_err(core_vector)?)
                .ok_or_else(|| {
                    ServiceError::Vector(format!(
                        "unknown named vector {vector_name} in collection {collection_id}"
                    ))
                })?;
            if config.field != field.as_str() {
                return Err(ServiceError::Vector(
                    "vector mutation field differs from its named-vector contract".into(),
                ));
            }
            let query = match value {
                DataVectorValue::Dense { values } => VectorSearchQuery::Dense {
                    values: values.clone(),
                },
                DataVectorValue::Sparse {
                    dimensions,
                    indices,
                    values,
                } => VectorSearchQuery::Sparse {
                    dimensions: *dimensions,
                    indices: indices.clone(),
                    values: values.clone(),
                },
                DataVectorValue::MultiDense {
                    dimensions,
                    vectors,
                } => VectorSearchQuery::MultiDense {
                    dimensions: *dimensions,
                    vectors: vectors.clone(),
                    comparator: rrd_contract::MultiVectorComparator::MaxSim,
                },
            };
            validate_collection_query(config, &query)?;
            if let Some(model) = &config.embedding_model {
                let provenance = provenance.as_ref().ok_or_else(|| {
                    ServiceError::Vector(
                        "model-bound named vector requires embedding provenance".into(),
                    )
                })?;
                if provenance.model != model.name || provenance.model_sha256 != model.digest {
                    return Err(ServiceError::Vector(
                        "vector mutation provenance differs from its named-vector model".into(),
                    ));
                }
            }
        }
        Ok(())
    }
}

pub(in crate::engine) fn vector_collection_error(
    error: vyrm_vector::CollectionError,
) -> ServiceError {
    if matches!(error, vyrm_vector::CollectionError::IdempotencyConflict) {
        ServiceError::IdempotencyConflict
    } else {
        ServiceError::Vector(error.to_string())
    }
}

pub(in crate::engine) fn internal_vector_metric(
    metric: VectorSearchMetric,
) -> vyrm_vector::ScoreMetric {
    match metric {
        VectorSearchMetric::Cosine => vyrm_vector::ScoreMetric::Cosine,
        VectorSearchMetric::Dot => vyrm_vector::ScoreMetric::Dot,
        VectorSearchMetric::Euclidean => vyrm_vector::ScoreMetric::Euclidean,
        VectorSearchMetric::Manhattan => vyrm_vector::ScoreMetric::Manhattan,
    }
}

fn public_vector_metric(metric: vyrm_vector::ScoreMetric) -> VectorSearchMetric {
    match metric {
        vyrm_vector::ScoreMetric::Cosine => VectorSearchMetric::Cosine,
        vyrm_vector::ScoreMetric::Dot => VectorSearchMetric::Dot,
        vyrm_vector::ScoreMetric::Euclidean => VectorSearchMetric::Euclidean,
        vyrm_vector::ScoreMetric::Manhattan => VectorSearchMetric::Manhattan,
    }
}

fn internal_vector_collection(
    request: &EnsureVectorCollection,
) -> Result<vyrm_vector::VectorCollectionDefinition> {
    let vectors = request
        .vectors
        .iter()
        .map(|vector| {
            let name = ProjectionId::new(vector.name.as_str()).map_err(core_vector)?;
            Ok((
                name.clone(),
                vyrm_vector::NamedVectorConfig {
                    name,
                    field: vector.field.as_str().into(),
                    kind: match vector.kind {
                        VectorValueKind::Dense => vyrm_vector::VectorValueKind::Dense,
                        VectorValueKind::Sparse => vyrm_vector::VectorValueKind::Sparse,
                        VectorValueKind::MultiDense => vyrm_vector::VectorValueKind::MultiDense,
                    },
                    dimensions: vector.dimensions,
                    metric: internal_vector_metric(vector.metric),
                    embedding_model: vector.embedding_model.as_ref().map(|model| {
                        vyrm_vector::EmbeddingModelBinding {
                            name: model.name.clone(),
                            digest: model.digest.clone(),
                        }
                    }),
                    memory_tier: match vector.memory_tier {
                        VectorMemoryTier::Pinned => vyrm_vector::VectorMemoryTier::Pinned,
                        VectorMemoryTier::Cached => vyrm_vector::VectorMemoryTier::Cached,
                        VectorMemoryTier::Cold => vyrm_vector::VectorMemoryTier::Cold,
                    },
                },
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    Ok(vyrm_vector::VectorCollectionDefinition {
        id: ProjectionId::new(request.collection_id.as_str()).map_err(core_vector)?,
        vectors,
    })
}

fn public_vector_collection(
    entry: &vyrm_vector::CollectionEntry,
) -> Result<VectorCollectionSnapshot> {
    Ok(VectorCollectionSnapshot {
        collection_id: CanonicalId::new(entry.definition.id.as_str())
            .map_err(|error| ServiceError::Vector(error.to_string()))?,
        vectors: entry
            .definition
            .vectors
            .values()
            .map(|vector| {
                Ok(NamedVectorDefinition {
                    name: CanonicalId::new(vector.name.as_str())
                        .map_err(|error| ServiceError::Vector(error.to_string()))?,
                    field: CanonicalId::new(&vector.field)
                        .map_err(|error| ServiceError::Vector(error.to_string()))?,
                    kind: match vector.kind {
                        vyrm_vector::VectorValueKind::Dense => VectorValueKind::Dense,
                        vyrm_vector::VectorValueKind::Sparse => VectorValueKind::Sparse,
                        vyrm_vector::VectorValueKind::MultiDense => VectorValueKind::MultiDense,
                    },
                    dimensions: vector.dimensions,
                    metric: public_vector_metric(vector.metric),
                    embedding_model: vector.embedding_model.as_ref().map(|model| {
                        VectorEmbeddingModel {
                            name: model.name.clone(),
                            digest: model.digest.clone(),
                        }
                    }),
                    memory_tier: match vector.memory_tier {
                        vyrm_vector::VectorMemoryTier::Pinned => VectorMemoryTier::Pinned,
                        vyrm_vector::VectorMemoryTier::Cached => VectorMemoryTier::Cached,
                        vyrm_vector::VectorMemoryTier::Cold => VectorMemoryTier::Cold,
                    },
                })
            })
            .collect::<Result<_>>()?,
        generation: entry.generation,
        created_at_unix_ms: entry.created_at,
        updated_at_unix_ms: entry.updated_at,
        configuration_sha256: entry.configuration_digest.clone(),
    })
}

pub(in crate::engine) fn validate_collection_query(
    config: &vyrm_vector::NamedVectorConfig,
    query: &VectorSearchQuery,
) -> Result<()> {
    let (kind, dimensions) = match query {
        VectorSearchQuery::Dense { values } => (
            vyrm_vector::VectorValueKind::Dense,
            u32::try_from(values.len())
                .map_err(|_| ServiceError::Vector("query dimensions exceed u32".into()))?,
        ),
        VectorSearchQuery::Sparse { dimensions, .. } => {
            (vyrm_vector::VectorValueKind::Sparse, *dimensions)
        }
        VectorSearchQuery::MultiDense { dimensions, .. } => {
            (vyrm_vector::VectorValueKind::MultiDense, *dimensions)
        }
    };
    if config.kind != kind || config.dimensions != dimensions {
        return Err(ServiceError::Vector(
            "query vector kind or dimensions differ from the named-vector contract".into(),
        ));
    }
    Ok(())
}
