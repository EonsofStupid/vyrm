use super::*;

impl RrdEngine {
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
        let (session_bytes, mut session) = self.load_authenticated(session_id, token)?;
        self.authorize_session_policy(&session, SecurityAction::VectorCollectionEnsure, now)?;
        let scope = self.query_scope(&request.scope)?;
        let operation_digest = operation_digest(request)?;
        let repository = rrd_vector::VectorCollectionRepository::new(&self.storage, scope);
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
        // Exact completed operations remain replayable after lease expiry;
        // every new collection mutation still requires an active session.
        self.require_active_or_expire(
            session_id,
            session_bytes,
            &mut session,
            now,
            context.request_id.as_str(),
            context.operation_id.as_str(),
        )?;
        let definition = internal_vector_collection(request)?;
        let context = rrd_vector::CollectionMutationContext {
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
        let catalogue = rrd_vector::VectorCollectionRepository::new(&self.storage, scope)
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

    pub fn ensure_vector_payload_index(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        request: &EnsureVectorPayloadIndex,
        context: &RequestContext,
        now: u64,
    ) -> Result<EnsureVectorPayloadIndexResult> {
        context
            .validate(true)
            .map_err(|error| ServiceError::Contract(error.to_string()))?;
        let idempotency_key = context.idempotency_key.as_ref().ok_or_else(|| {
            ServiceError::Contract("vector payload-index ensure requires idempotency".into())
        })?;
        request
            .validate()
            .map_err(|error| ServiceError::Vector(error.to_string()))?;
        let (session_bytes, mut session) = self.load_authenticated(session_id, token)?;
        self.authorize_session_policy(&session, SecurityAction::VectorPayloadIndexEnsure, now)?;
        let scope = self.query_scope(&request.scope)?;
        let operation_digest = operation_digest(request)?;
        let repository = rrd_vector::VectorCollectionRepository::new(&self.storage, scope);
        if let Some(receipt) = repository
            .payload_operation_receipt(idempotency_key.as_str(), &operation_digest)
            .map_err(vector_collection_error)?
        {
            if receipt.deleted {
                return Err(ServiceError::IdempotencyConflict);
            }
            let catalogue = repository.load().map_err(vector_collection_error)?;
            return Ok(EnsureVectorPayloadIndexResult {
                collection: public_vector_collection(&receipt.collection)?,
                index: public_payload_index(&receipt.index)?,
                catalogue_revision: catalogue.revision,
                idempotent_replay: true,
            });
        }
        self.require_active_or_expire(
            session_id,
            session_bytes,
            &mut session,
            now,
            context.request_id.as_str(),
            context.operation_id.as_str(),
        )?;
        let collection_id =
            ProjectionId::new(request.collection_id.as_str()).map_err(core_vector)?;
        let definition = rrd_vector::PayloadIndexDefinition {
            field: ProjectionId::new(request.field.as_str()).map_err(core_vector)?,
            kind: internal_payload_index_kind(request.kind),
        };
        let mutation = collection_mutation_context(session_id, context, now);
        let (catalogue, index, idempotent_replay) = repository
            .ensure_payload_index(
                &mutation,
                idempotency_key.as_str().into(),
                operation_digest,
                &collection_id,
                definition,
            )
            .map_err(vector_collection_error)?;
        let collection = catalogue
            .collections
            .get(&collection_id)
            .ok_or_else(|| ServiceError::Vector("payload-index collection disappeared".into()))?;
        Ok(EnsureVectorPayloadIndexResult {
            collection: public_vector_collection(collection)?,
            index: public_payload_index(&index)?,
            catalogue_revision: catalogue.revision,
            idempotent_replay,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn list_vector_payload_indexes(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        request: &ListVectorPayloadIndexes,
        now: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<VectorPayloadIndexCatalogueSnapshot> {
        request
            .validate()
            .map_err(|error| ServiceError::Vector(error.to_string()))?;
        self.authorize(
            session_id,
            token,
            SecurityAction::VectorPayloadIndexList,
            now,
            request_id,
            operation_id,
        )?;
        let scope = self.query_scope(&request.scope)?;
        let catalogue = rrd_vector::VectorCollectionRepository::new(&self.storage, scope)
            .load()
            .map_err(vector_collection_error)?;
        let collection = catalogue
            .collections
            .get(&ProjectionId::new(request.collection_id.as_str()).map_err(core_vector)?)
            .ok_or_else(|| {
                ServiceError::Vector(format!(
                    "unknown vector collection {}",
                    request.collection_id
                ))
            })?;
        Ok(VectorPayloadIndexCatalogueSnapshot {
            scope: request.scope.clone(),
            collection_id: request.collection_id.clone(),
            collection_generation: collection.generation,
            catalogue_revision: catalogue.revision,
            indexes: collection
                .payload_indexes
                .values()
                .map(public_payload_index)
                .collect::<Result<_>>()?,
        })
    }

    pub fn delete_vector_payload_index(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        request: &DeleteVectorPayloadIndex,
        context: &RequestContext,
        now: u64,
    ) -> Result<DeleteVectorPayloadIndexResult> {
        context
            .validate(true)
            .map_err(|error| ServiceError::Contract(error.to_string()))?;
        let idempotency_key = context.idempotency_key.as_ref().ok_or_else(|| {
            ServiceError::Contract("vector payload-index delete requires idempotency".into())
        })?;
        request
            .validate()
            .map_err(|error| ServiceError::Vector(error.to_string()))?;
        let (session_bytes, mut session) = self.load_authenticated(session_id, token)?;
        self.authorize_session_policy(&session, SecurityAction::VectorPayloadIndexDelete, now)?;
        let scope = self.query_scope(&request.scope)?;
        let operation_digest = operation_digest(request)?;
        let repository = rrd_vector::VectorCollectionRepository::new(&self.storage, scope);
        if let Some(receipt) = repository
            .payload_operation_receipt(idempotency_key.as_str(), &operation_digest)
            .map_err(vector_collection_error)?
        {
            if !receipt.deleted {
                return Err(ServiceError::IdempotencyConflict);
            }
            let catalogue = repository.load().map_err(vector_collection_error)?;
            return Ok(DeleteVectorPayloadIndexResult {
                collection: public_vector_collection(&receipt.collection)?,
                deleted_index: public_payload_index(&receipt.index)?,
                catalogue_revision: catalogue.revision,
                idempotent_replay: true,
            });
        }
        self.require_active_or_expire(
            session_id,
            session_bytes,
            &mut session,
            now,
            context.request_id.as_str(),
            context.operation_id.as_str(),
        )?;
        let collection_id =
            ProjectionId::new(request.collection_id.as_str()).map_err(core_vector)?;
        let field = ProjectionId::new(request.field.as_str()).map_err(core_vector)?;
        let catalogue = repository.load().map_err(vector_collection_error)?;
        let collection = catalogue.collections.get(&collection_id).ok_or_else(|| {
            ServiceError::Vector(format!(
                "unknown vector collection {}",
                request.collection_id
            ))
        })?;
        require_payload_index_not_in_use(
            self,
            &self.query_scope(&request.scope)?,
            &request.collection_id,
            collection,
            request.field.as_str(),
        )?;
        let mutation = collection_mutation_context(session_id, context, now);
        let (catalogue, deleted_index, idempotent_replay) = repository
            .delete_payload_index(
                &mutation,
                idempotency_key.as_str().into(),
                operation_digest,
                &collection_id,
                &field,
            )
            .map_err(vector_collection_error)?;
        let collection = catalogue
            .collections
            .get(&collection_id)
            .ok_or_else(|| ServiceError::Vector("payload-index collection disappeared".into()))?;
        Ok(DeleteVectorPayloadIndexResult {
            collection: public_vector_collection(collection)?,
            deleted_index: public_payload_index(&deleted_index)?,
            catalogue_revision: catalogue.revision,
            idempotent_replay,
        })
    }

    pub fn delete_vector_collection(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        request: &DeleteVectorCollection,
        context: &RequestContext,
        now: u64,
    ) -> Result<DeleteVectorCollectionResult> {
        context
            .validate(true)
            .map_err(|error| ServiceError::Contract(error.to_string()))?;
        let idempotency_key = context.idempotency_key.as_ref().ok_or_else(|| {
            ServiceError::Contract("vector collection delete requires idempotency".into())
        })?;
        request
            .validate()
            .map_err(|error| ServiceError::Vector(error.to_string()))?;
        let (session_bytes, mut session) = self.load_authenticated(session_id, token)?;
        self.authorize_session_policy(&session, SecurityAction::VectorCollectionDelete, now)?;
        let scope = self.query_scope(&request.scope)?;
        let operation_digest = operation_digest(request)?;
        let repository = rrd_vector::VectorCollectionRepository::new(&self.storage, scope.clone());
        if let Some(receipt) = repository
            .deletion_receipt(idempotency_key.as_str(), &operation_digest)
            .map_err(vector_collection_error)?
        {
            let catalogue = repository.load().map_err(vector_collection_error)?;
            return Ok(DeleteVectorCollectionResult {
                deleted_collection: public_vector_collection(&receipt.deleted_entry)?,
                catalogue_revision: catalogue.revision,
                idempotent_replay: true,
            });
        }
        self.require_active_or_expire(
            session_id,
            session_bytes,
            &mut session,
            now,
            context.request_id.as_str(),
            context.operation_id.as_str(),
        )?;
        let catalogue = repository.load().map_err(vector_collection_error)?;
        let collection_id =
            ProjectionId::new(request.collection_id.as_str()).map_err(core_vector)?;
        let collection = catalogue.collections.get(&collection_id).ok_or_else(|| {
            ServiceError::Vector(format!(
                "unknown vector collection {}",
                request.collection_id
            ))
        })?;
        self.require_empty_collection(request, &scope, collection)?;
        require_no_collection_artifacts(self, &scope, &request.collection_id, collection)?;
        let mutation = collection_mutation_context(session_id, context, now);
        let (catalogue, deleted_collection, idempotent_replay) = repository
            .delete(
                &mutation,
                idempotency_key.as_str().into(),
                operation_digest,
                &collection_id,
            )
            .map_err(vector_collection_error)?;
        Ok(DeleteVectorCollectionResult {
            deleted_collection: public_vector_collection(&deleted_collection)?,
            catalogue_revision: catalogue.revision,
            idempotent_replay,
        })
    }

    fn require_empty_collection(
        &self,
        request: &DeleteVectorCollection,
        scope: &ScopeId,
        collection: &rrd_vector::CollectionEntry,
    ) -> Result<()> {
        let read = self.storage.runtime_read_stamp(scope)?;
        let limit = usize::try_from(request.max_scanned_changes).map_err(|_| {
            ServiceError::Vector("collection delete scan budget exceeds usize".into())
        })?;
        let page = self.storage.runtime_read_changes(&read, 0, limit)?;
        if page.through_cursor < page.head_cursor {
            return Err(ServiceError::Vector(format!(
                "vector collection delete requires more than {} retained changes",
                request.max_scanned_changes
            )));
        }
        let mut latest = BTreeMap::new();
        for candidate in rrd_vector::candidates_from_changes(&page.changes, scope) {
            let addressed = candidate.vector.collection.as_ref().is_some_and(|address| {
                address.collection_id == request.collection_id.as_str()
                    && collection
                        .definition
                        .vectors
                        .keys()
                        .any(|name| name.as_str() == address.vector_name)
            });
            if !addressed {
                continue;
            }
            candidate.validate().map_err(core_vector)?;
            let identity = candidate.vector.reference.clone();
            if latest
                .get(&identity)
                .is_none_or(|current: &rrd_vector::VectorCandidate| {
                    current.source_cursor < candidate.source_cursor
                })
            {
                latest.insert(identity, candidate);
            }
        }
        if latest.values().any(|candidate| {
            candidate
                .vector
                .valid_to
                .is_none_or(|valid_to| valid_to > request.valid_at)
        }) {
            return Err(ServiceError::Vector(format!(
                "vector collection {} still contains live or future points; retire them through one transaction at or before the deletion time",
                request.collection_id
            )));
        }
        Ok(())
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
        let catalogue = rrd_vector::VectorCollectionRepository::new(&self.storage, scope)
            .load()
            .map_err(vector_collection_error)?;
        for mutation in &request.mutations {
            let TransactionMutation::PutVector {
                collection_id: Some(collection_id),
                vector_name: Some(vector_name),
                field,
                value,
                provenance,
                properties,
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
            validate_payload_index_values(collection, properties)?;
        }
        Ok(())
    }
}

fn validate_payload_index_values(
    collection: &rrd_vector::CollectionEntry,
    properties: &DataProperties,
) -> Result<()> {
    for index in collection.payload_indexes.values() {
        let Some(value) = properties.get(index.definition.field.as_str()) else {
            continue;
        };
        let valid = matches!(
            (index.definition.kind, value),
            (rrd_vector::PayloadIndexKind::Boolean, QueryValue::Bool(_))
                | (
                    rrd_vector::PayloadIndexKind::Integer,
                    QueryValue::Integer(_)
                )
                | (
                    rrd_vector::PayloadIndexKind::Unsigned,
                    QueryValue::Unsigned(_)
                )
                | (
                    rrd_vector::PayloadIndexKind::Decimal,
                    QueryValue::Decimal(_)
                )
                | (rrd_vector::PayloadIndexKind::Keyword, QueryValue::String(_))
                | (rrd_vector::PayloadIndexKind::Digest, QueryValue::Digest(_))
        );
        if !valid {
            return Err(ServiceError::Vector(format!(
                "payload property {} differs from its {:?} index contract",
                index.definition.field, index.definition.kind
            )));
        }
    }
    Ok(())
}

pub(in crate::engine) fn vector_collection_error(
    error: rrd_vector::CollectionError,
) -> ServiceError {
    if matches!(error, rrd_vector::CollectionError::IdempotencyConflict) {
        ServiceError::IdempotencyConflict
    } else {
        ServiceError::Vector(error.to_string())
    }
}

pub(in crate::engine) fn internal_vector_metric(
    metric: VectorSearchMetric,
) -> rrd_vector::ScoreMetric {
    match metric {
        VectorSearchMetric::Cosine => rrd_vector::ScoreMetric::Cosine,
        VectorSearchMetric::Dot => rrd_vector::ScoreMetric::Dot,
        VectorSearchMetric::Euclidean => rrd_vector::ScoreMetric::Euclidean,
        VectorSearchMetric::Manhattan => rrd_vector::ScoreMetric::Manhattan,
    }
}

fn public_vector_metric(metric: rrd_vector::ScoreMetric) -> VectorSearchMetric {
    match metric {
        rrd_vector::ScoreMetric::Cosine => VectorSearchMetric::Cosine,
        rrd_vector::ScoreMetric::Dot => VectorSearchMetric::Dot,
        rrd_vector::ScoreMetric::Euclidean => VectorSearchMetric::Euclidean,
        rrd_vector::ScoreMetric::Manhattan => VectorSearchMetric::Manhattan,
    }
}

fn internal_vector_collection(
    request: &EnsureVectorCollection,
) -> Result<rrd_vector::VectorCollectionDefinition> {
    let vectors = request
        .vectors
        .iter()
        .map(|vector| {
            let name = ProjectionId::new(vector.name.as_str()).map_err(core_vector)?;
            Ok((
                name.clone(),
                rrd_vector::NamedVectorConfig {
                    name,
                    field: vector.field.as_str().into(),
                    kind: match vector.kind {
                        VectorValueKind::Dense => rrd_vector::VectorValueKind::Dense,
                        VectorValueKind::Sparse => rrd_vector::VectorValueKind::Sparse,
                        VectorValueKind::MultiDense => rrd_vector::VectorValueKind::MultiDense,
                    },
                    dimensions: vector.dimensions,
                    metric: internal_vector_metric(vector.metric),
                    embedding_model: vector.embedding_model.as_ref().map(|model| {
                        rrd_vector::EmbeddingModelBinding {
                            name: model.name.clone(),
                            digest: model.digest.clone(),
                        }
                    }),
                    memory_tier: match vector.memory_tier {
                        VectorMemoryTier::Pinned => rrd_vector::VectorMemoryTier::Pinned,
                        VectorMemoryTier::Cached => rrd_vector::VectorMemoryTier::Cached,
                        VectorMemoryTier::Cold => rrd_vector::VectorMemoryTier::Cold,
                    },
                },
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    Ok(rrd_vector::VectorCollectionDefinition {
        id: ProjectionId::new(request.collection_id.as_str()).map_err(core_vector)?,
        vectors,
    })
}

pub(in crate::engine) fn public_vector_collection(
    entry: &rrd_vector::CollectionEntry,
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
                        rrd_vector::VectorValueKind::Dense => VectorValueKind::Dense,
                        rrd_vector::VectorValueKind::Sparse => VectorValueKind::Sparse,
                        rrd_vector::VectorValueKind::MultiDense => VectorValueKind::MultiDense,
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
                        rrd_vector::VectorMemoryTier::Pinned => VectorMemoryTier::Pinned,
                        rrd_vector::VectorMemoryTier::Cached => VectorMemoryTier::Cached,
                        rrd_vector::VectorMemoryTier::Cold => VectorMemoryTier::Cold,
                    },
                })
            })
            .collect::<Result<_>>()?,
        payload_indexes: entry
            .payload_indexes
            .values()
            .map(public_payload_index)
            .collect::<Result<_>>()?,
        generation: entry.generation,
        created_at_unix_ms: entry.created_at,
        updated_at_unix_ms: entry.updated_at,
        configuration_sha256: entry.configuration_digest.clone(),
    })
}

fn collection_mutation_context(
    session_id: &CorrelationId,
    context: &RequestContext,
    now: u64,
) -> rrd_vector::CollectionMutationContext {
    rrd_vector::CollectionMutationContext {
        at: now,
        actor: format!("session:{}", session_id.as_str()),
        request_id: context.request_id.as_str().into(),
        operation_id: context.operation_id.as_str().into(),
    }
}

fn internal_payload_index_kind(kind: VectorPayloadIndexKind) -> rrd_vector::PayloadIndexKind {
    match kind {
        VectorPayloadIndexKind::Boolean => rrd_vector::PayloadIndexKind::Boolean,
        VectorPayloadIndexKind::Integer => rrd_vector::PayloadIndexKind::Integer,
        VectorPayloadIndexKind::Unsigned => rrd_vector::PayloadIndexKind::Unsigned,
        VectorPayloadIndexKind::Decimal => rrd_vector::PayloadIndexKind::Decimal,
        VectorPayloadIndexKind::Keyword => rrd_vector::PayloadIndexKind::Keyword,
        VectorPayloadIndexKind::Digest => rrd_vector::PayloadIndexKind::Digest,
    }
}

fn public_payload_index_kind(kind: rrd_vector::PayloadIndexKind) -> VectorPayloadIndexKind {
    match kind {
        rrd_vector::PayloadIndexKind::Boolean => VectorPayloadIndexKind::Boolean,
        rrd_vector::PayloadIndexKind::Integer => VectorPayloadIndexKind::Integer,
        rrd_vector::PayloadIndexKind::Unsigned => VectorPayloadIndexKind::Unsigned,
        rrd_vector::PayloadIndexKind::Decimal => VectorPayloadIndexKind::Decimal,
        rrd_vector::PayloadIndexKind::Keyword => VectorPayloadIndexKind::Keyword,
        rrd_vector::PayloadIndexKind::Digest => VectorPayloadIndexKind::Digest,
    }
}

fn public_payload_index(
    entry: &rrd_vector::PayloadIndexEntry,
) -> Result<VectorPayloadIndexSnapshot> {
    Ok(VectorPayloadIndexSnapshot {
        field: CanonicalId::new(entry.definition.field.as_str())
            .map_err(|error| ServiceError::Vector(error.to_string()))?,
        kind: public_payload_index_kind(entry.definition.kind),
        generation: entry.generation,
        created_at_unix_ms: entry.created_at,
        updated_at_unix_ms: entry.updated_at,
        configuration_sha256: entry.configuration_digest.clone(),
    })
}

fn require_no_collection_artifacts(
    engine: &RrdEngine,
    scope: &ScopeId,
    collection_id: &CanonicalId,
    collection: &rrd_vector::CollectionEntry,
) -> Result<()> {
    let entries = crate::vector_artifact_catalog_entries(&engine.storage, scope)
        .map_err(|error| ServiceError::Vector(error.to_string()))?;
    for vector in collection.definition.vectors.values() {
        for kind in ["hnsw", "turboquant"] {
            let expected = format!("{kind}-{collection_id}-{}", vector.name);
            if entries
                .iter()
                .any(|entry| entry.descriptor.stamp().id.as_str() == expected)
            {
                return Err(ServiceError::Vector(format!(
                    "vector collection {collection_id} still owns active {kind} artifacts; retire them before deletion"
                )));
            }
        }
    }
    let quantization = crate::quantization_artifact_catalogue(&engine.storage, scope)
        .map_err(|error| ServiceError::Vector(error.to_string()))?;
    if quantization.artifacts.values().any(|artifact| {
        artifact.entry.collection_id.as_str() == collection_id.as_str()
            && artifact.state != rrd_vector::QuantizationArtifactState::Retired
    }) {
        return Err(ServiceError::Vector(format!(
            "vector collection {collection_id} still owns ready or active quantization artifacts; retire them before deletion"
        )));
    }
    Ok(())
}

fn require_payload_index_not_in_use(
    engine: &RrdEngine,
    scope: &ScopeId,
    collection_id: &CanonicalId,
    collection: &rrd_vector::CollectionEntry,
    field: &str,
) -> Result<()> {
    let entries = crate::vector_artifact_catalog_entries(&engine.storage, scope)
        .map_err(|error| ServiceError::Vector(error.to_string()))?;
    for vector in collection.definition.vectors.values() {
        for kind in ["hnsw", "turboquant"] {
            let expected = format!("{kind}-{collection_id}-{}", vector.name);
            let Some(entry) = entries
                .iter()
                .find(|entry| entry.descriptor.stamp().id.as_str() == expected)
            else {
                continue;
            };
            let properties = match &entry.descriptor {
                rrd_vector::VectorProjectionDescriptor::Hnsw { descriptor } => {
                    &descriptor.filter_properties
                }
                rrd_vector::VectorProjectionDescriptor::TurboQuant { descriptor } => {
                    &descriptor.filter_properties
                }
                rrd_vector::VectorProjectionDescriptor::Quantized { descriptor } => {
                    &descriptor.filter_properties
                }
                rrd_vector::VectorProjectionDescriptor::ExactSegment { .. } => continue,
            };
            if properties.contains(field) {
                return Err(ServiceError::Vector(format!(
                    "payload index {field} is required by active {kind} artifact {}; retire or replace the artifact before deletion",
                    entry.descriptor.stamp().id
                )));
            }
        }
    }
    let quantization = crate::quantization_artifact_catalogue(&engine.storage, scope)
        .map_err(|error| ServiceError::Vector(error.to_string()))?;
    for artifact in quantization.artifacts.values().filter(|artifact| {
        artifact.entry.collection_id.as_str() == collection_id.as_str()
            && artifact.state != rrd_vector::QuantizationArtifactState::Retired
    }) {
        let properties = match &artifact.entry.descriptor {
            rrd_vector::VectorProjectionDescriptor::Quantized { descriptor } => {
                &descriptor.filter_properties
            }
            rrd_vector::VectorProjectionDescriptor::TurboQuant { descriptor } => {
                &descriptor.filter_properties
            }
            _ => continue,
        };
        if properties.contains(field) {
            return Err(ServiceError::Vector(format!(
                "payload index {field} is required by quantization artifact {}; retire it before deletion",
                artifact.entry.descriptor.stamp().id
            )));
        }
    }
    Ok(())
}

pub(in crate::engine) fn validate_collection_query(
    config: &rrd_vector::NamedVectorConfig,
    query: &VectorSearchQuery,
) -> Result<()> {
    let (kind, dimensions) = match query {
        VectorSearchQuery::Dense { values } => (
            rrd_vector::VectorValueKind::Dense,
            u32::try_from(values.len())
                .map_err(|_| ServiceError::Vector("query dimensions exceed u32".into()))?,
        ),
        VectorSearchQuery::Sparse { dimensions, .. } => {
            (rrd_vector::VectorValueKind::Sparse, *dimensions)
        }
        VectorSearchQuery::MultiDense { dimensions, .. } => {
            (rrd_vector::VectorValueKind::MultiDense, *dimensions)
        }
    };
    if config.kind != kind || config.dimensions != dimensions {
        return Err(ServiceError::Vector(
            "query vector kind or dimensions differ from the named-vector contract".into(),
        ));
    }
    Ok(())
}

pub(in crate::engine) fn vector_matches_collection(
    vector: &RuntimeVector,
    collection_id: &CanonicalId,
    vector_name: &CanonicalId,
) -> bool {
    vector.collection.as_ref().is_some_and(|address| {
        address.collection_id == collection_id.as_str()
            && address.vector_name == vector_name.as_str()
    })
}
