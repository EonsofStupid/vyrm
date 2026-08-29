use super::*;
use std::collections::BTreeSet;

impl RrdEngine {
    /// Ensures one durable approximate projection for a named dense vector.
    ///
    /// Canonical vectors remain transaction truth. The immutable graph bytes
    /// and their typed catalogue record publish atomically through the same RRD
    /// engine and object tier, then ordinary searches reopen that authority.
    pub fn ensure_vector_index(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        request: &EnsureVectorIndex,
        now: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<EnsureVectorIndexResult> {
        request
            .validate()
            .map_err(|error| ServiceError::Vector(error.to_string()))?;
        self.authorize(
            session_id,
            token,
            SecurityAction::VectorCollectionEnsure,
            now,
            request_id,
            operation_id,
        )?;
        let scope = self.query_scope(&request.scope)?;
        let catalogue = rrd_vector::VectorCollectionRepository::new(&self.storage, scope.clone())
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
        let vector = collection
            .definition
            .vectors
            .get(&ProjectionId::new(request.vector_name.as_str()).map_err(core_vector)?)
            .ok_or_else(|| {
                ServiceError::Vector(format!(
                    "unknown named vector {} in collection {}",
                    request.vector_name, request.collection_id
                ))
            })?;
        let filter_properties = match &request.configuration {
            VectorIndexConfiguration::Hnsw {
                filter_properties, ..
            }
            | VectorIndexConfiguration::TurboQuant {
                filter_properties, ..
            } => filter_properties,
        };
        for property in filter_properties {
            let field = ProjectionId::new(property.as_str()).map_err(core_vector)?;
            if !collection.payload_indexes.contains_key(&field) {
                return Err(ServiceError::Vector(format!(
                    "vector filter property {property} has no active payload index in collection {}",
                    request.collection_id
                )));
            }
        }
        if vector.kind != rrd_vector::VectorValueKind::Dense {
            return Err(ServiceError::Vector(
                "approximate indexing currently requires a dense named vector".into(),
            ));
        }

        let read = self.storage.runtime_read_stamp(&scope)?;
        let scan_limit = usize::try_from(request.max_scanned_changes)
            .map_err(|_| ServiceError::Vector("vector index scan budget exceeds usize".into()))?;
        let page = self.storage.runtime_read_changes(&read, 0, scan_limit)?;
        if page.through_cursor < page.head_cursor {
            return Err(ServiceError::Vector(format!(
                "vector index build requires more than {} retained changes",
                request.max_scanned_changes
            )));
        }
        let candidates = rrd_vector::candidates_from_changes(&page.changes, &scope)
            .into_iter()
            .filter(|candidate| {
                vector_matches_collection(
                    &candidate.vector,
                    &request.collection_id,
                    &request.vector_name,
                )
            })
            .collect::<Vec<_>>();
        let source_cursor = candidates
            .iter()
            .map(|candidate| candidate.source_cursor)
            .max()
            .unwrap_or(0);
        let data = rrd_store::DataRuntimeRef::new(&self.storage, &self.objects);
        let mut runtime = crate::reopen_vector_runtime(&data, &scope, candidates.clone())
            .map_err(|error| ServiceError::Vector(error.to_string()))?;
        let index_kind = match &request.configuration {
            VectorIndexConfiguration::Hnsw { .. } => "hnsw",
            VectorIndexConfiguration::TurboQuant { .. } => "turboquant",
        };
        let index_id = CanonicalId::new(format!(
            "{index_kind}-{}-{}",
            request.collection_id, request.vector_name
        ))
        .map_err(|error| ServiceError::Vector(error.to_string()))?;
        let projection_id = ProjectionId::new(index_id.as_str()).map_err(core_vector)?;
        let previous = runtime.catalog().entries.get(&projection_id).cloned();
        let generation = previous
            .as_ref()
            .map(|descriptor| descriptor.stamp().generation)
            .unwrap_or(1);
        let artifact = if let (VectorIndexConfiguration::Hnsw { .. }, Some(previous)) =
            (&request.configuration, previous.as_ref())
        {
            let active = runtime
                .artifact(&projection_id, previous.stamp().generation)
                .ok_or_else(|| {
                    ServiceError::Vector("active HNSW artifact bytes disappeared".into())
                })?;
            let rrd_vector::VectorArtifact::Hnsw(active) = active else {
                return Err(ServiceError::Vector(
                    "active vector index has the wrong artifact kind".into(),
                ));
            };
            let desired = hnsw_config(request, vector, scope.clone(), projection_id.clone())?;
            if active.config() == &desired {
                if source_cursor < active.descriptor().stamp.source_cursor {
                    return Err(ServiceError::Vector(
                        "canonical vector coverage regressed behind the active HNSW index".into(),
                    ));
                }
                if source_cursor == active.descriptor().stamp.source_cursor {
                    return replay_vector_index(
                        &self.storage,
                        active.descriptor().clone().into(),
                        &request.collection_id,
                        &request.vector_name,
                    );
                }
                let next_generation = generation.checked_add(1).ok_or_else(|| {
                    ServiceError::Vector("vector index generation overflowed".into())
                })?;
                active
                    .advance(
                        next_generation,
                        source_cursor,
                        candidates.into_iter().filter(|candidate| {
                            candidate.source_cursor > active.descriptor().stamp.source_cursor
                        }),
                    )
                    .map(Into::into)
                    .map_err(core_vector)?
            } else {
                build_index_artifact(
                    request,
                    vector,
                    scope,
                    projection_id,
                    generation.checked_add(1).ok_or_else(|| {
                        ServiceError::Vector("vector index generation overflowed".into())
                    })?,
                    source_cursor,
                    candidates,
                )?
            }
        } else {
            let artifact = build_index_artifact(
                request,
                vector,
                scope,
                projection_id,
                generation,
                source_cursor,
                candidates.clone(),
            )?;
            let descriptor = artifact.descriptor();
            if previous.as_ref() == Some(&descriptor) {
                return replay_vector_index(
                    &self.storage,
                    descriptor,
                    &request.collection_id,
                    &request.vector_name,
                );
            }
            if previous.is_some() {
                build_index_artifact(
                    request,
                    vector,
                    descriptor.scope().clone(),
                    descriptor.stamp().id.clone(),
                    generation.checked_add(1).ok_or_else(|| {
                        ServiceError::Vector("vector index generation overflowed".into())
                    })?,
                    source_cursor,
                    candidates,
                )?
            } else {
                artifact
            }
        };
        let expected_revision = runtime.catalog().revision;
        let publication = crate::publish_traced_vector_artifact(
            &data,
            &mut runtime,
            expected_revision,
            artifact,
            &format!("session:{}", session_id.as_str()),
            now,
        )
        .map_err(|error| ServiceError::Vector(error.to_string()))?;
        Ok(EnsureVectorIndexResult {
            index: public_vector_index(
                &publication.entry,
                &request.collection_id,
                &request.vector_name,
            )?,
            idempotent_replay: false,
        })
    }
}

fn replay_vector_index<E>(
    storage: &E,
    descriptor: rrd_vector::VectorProjectionDescriptor,
    collection_id: &CanonicalId,
    vector_name: &CanonicalId,
) -> Result<EnsureVectorIndexResult>
where
    E: rrd_store::Engine,
{
    let entry = crate::vector_artifact_catalog_entries(storage, descriptor.scope())
        .map_err(|error| ServiceError::Vector(error.to_string()))?
        .into_iter()
        .find(|entry| entry.descriptor == descriptor)
        .ok_or_else(|| {
            ServiceError::Vector("ready vector index catalog entry disappeared".into())
        })?;
    Ok(EnsureVectorIndexResult {
        index: public_vector_index(&entry, collection_id, vector_name)?,
        idempotent_replay: true,
    })
}

fn hnsw_config(
    request: &EnsureVectorIndex,
    vector: &rrd_vector::NamedVectorConfig,
    scope: ScopeId,
    id: ProjectionId,
) -> Result<rrd_vector::HnswConfig> {
    let dimensions = usize::try_from(vector.dimensions)
        .map_err(|_| ServiceError::Vector("vector dimensions exceed usize".into()))?;
    let VectorIndexConfiguration::Hnsw {
        m,
        ef_construction,
        max_level,
        seed,
        filter_properties,
    } = &request.configuration
    else {
        return Err(ServiceError::Vector(
            "HNSW configuration helper received another index kind".into(),
        ));
    };
    Ok(rrd_vector::HnswConfig {
        id,
        scope,
        field: vector.field.clone(),
        dimensions,
        metric: vector.metric,
        embedding_model: vector.embedding_model.clone(),
        m: usize::try_from(*m).map_err(|_| ServiceError::Vector("HNSW m exceeds usize".into()))?,
        ef_construction: usize::try_from(*ef_construction)
            .map_err(|_| ServiceError::Vector("HNSW ef_construction exceeds usize".into()))?,
        max_level: *max_level,
        seed: *seed,
        filter_properties: public_filter_properties(filter_properties),
    })
}

#[allow(clippy::too_many_arguments)]
fn build_index_artifact(
    request: &EnsureVectorIndex,
    vector: &rrd_vector::NamedVectorConfig,
    scope: ScopeId,
    id: ProjectionId,
    generation: u64,
    source_cursor: u64,
    candidates: Vec<rrd_vector::VectorCandidate>,
) -> Result<rrd_vector::VectorArtifact> {
    let dimensions = usize::try_from(vector.dimensions)
        .map_err(|_| ServiceError::Vector("vector dimensions exceed usize".into()))?;
    match &request.configuration {
        VectorIndexConfiguration::Hnsw { .. } => rrd_vector::HnswIndex::build(
            hnsw_config(request, vector, scope, id)?,
            generation,
            source_cursor,
            candidates,
        )
        .map(Into::into)
        .map_err(core_vector),
        VectorIndexConfiguration::TurboQuant {
            bits,
            seed,
            filter_properties,
        } => rrd_vector::TurboQuantSegment::build(
            rrd_vector::TurboQuantSegmentConfig {
                id,
                scope,
                field: vector.field.clone(),
                dimensions,
                metric: vector.metric,
                bits: match bits {
                    rrd_contract::VectorQuantizationBits::Bits4 => {
                        rrd_vector::TurboQuantBits::Bits4
                    }
                    rrd_contract::VectorQuantizationBits::Bits2 => {
                        rrd_vector::TurboQuantBits::Bits2
                    }
                    rrd_contract::VectorQuantizationBits::Bits1_5 => {
                        rrd_vector::TurboQuantBits::Bits1_5
                    }
                    rrd_contract::VectorQuantizationBits::Bits1 => {
                        rrd_vector::TurboQuantBits::Bits1
                    }
                },
                seed: *seed,
                embedding_model: vector.embedding_model.clone(),
                filter_properties: public_filter_properties(filter_properties),
            },
            generation,
            source_cursor,
            candidates,
        )
        .map(Into::into)
        .map_err(core_vector),
    }
}

fn public_filter_properties(properties: &[CanonicalId]) -> BTreeSet<String> {
    properties
        .iter()
        .map(|property| property.as_str().to_owned())
        .collect()
}

fn public_vector_index(
    entry: &rrd_vector::VectorArtifactCatalogEntry,
    collection_id: &CanonicalId,
    vector_name: &CanonicalId,
) -> Result<VectorIndexSnapshot> {
    let (
        stamp,
        kind,
        indexed_vectors,
        packed_vector_bytes,
        full_precision_vector_bytes,
        maintenance,
    ) = match &entry.descriptor {
        rrd_vector::VectorProjectionDescriptor::Hnsw { descriptor } => (
            &descriptor.stamp,
            "hnsw",
            descriptor.nodes,
            None,
            None,
            VectorIndexMaintenanceSnapshot {
                mode: match descriptor.maintenance {
                    rrd_vector::HnswMaintenanceKind::FullBuild => {
                        VectorIndexMaintenanceMode::FullBuild
                    }
                    rrd_vector::HnswMaintenanceKind::Incremental => {
                        VectorIndexMaintenanceMode::Incremental
                    }
                },
                previous_generation: descriptor.previous_generation,
                indexed_delta_vectors: u64::try_from(descriptor.indexed_delta_vectors)
                    .map_err(|_| ServiceError::Vector("HNSW delta count exceeds u64".into()))?,
            },
        ),
        rrd_vector::VectorProjectionDescriptor::TurboQuant { descriptor } => (
            &descriptor.stamp,
            "turboquant",
            descriptor.candidate_versions,
            Some(descriptor.packed_vector_bytes),
            Some(descriptor.full_precision_vector_bytes),
            VectorIndexMaintenanceSnapshot {
                mode: VectorIndexMaintenanceMode::FullBuild,
                previous_generation: None,
                indexed_delta_vectors: u64::try_from(descriptor.candidate_versions).map_err(
                    |_| ServiceError::Vector("TurboQuant delta count exceeds u64".into()),
                )?,
            },
        ),
        rrd_vector::VectorProjectionDescriptor::ExactSegment { .. } => {
            return Err(ServiceError::Vector(
                "vector index catalog entry is not an approximate index".into(),
            ));
        }
    };
    let indexed_vectors = u64::try_from(indexed_vectors)
        .map_err(|_| ServiceError::Vector("vector index count exceeds u64".into()))?;
    let packed_vector_bytes = packed_vector_bytes
        .map(|value| {
            u64::try_from(value)
                .map_err(|_| ServiceError::Vector("packed vector bytes exceed u64".into()))
        })
        .transpose()?;
    let full_precision_vector_bytes = full_precision_vector_bytes
        .map(|value| {
            u64::try_from(value)
                .map_err(|_| ServiceError::Vector("full-precision vector bytes exceed u64".into()))
        })
        .transpose()?;
    Ok(VectorIndexSnapshot {
        index_id: CanonicalId::new(stamp.id.as_str())
            .map_err(|error| ServiceError::Vector(error.to_string()))?,
        collection_id: collection_id.clone(),
        vector_name: vector_name.clone(),
        kind: CanonicalId::new(kind).map_err(|error| ServiceError::Vector(error.to_string()))?,
        generation: stamp.generation,
        source_cursor: stamp.source_cursor,
        indexed_vectors,
        maintenance,
        packed_vector_bytes,
        full_precision_vector_bytes,
        configuration_sha256: stamp.config_digest.clone(),
        artifact_sha256: stamp.artifact_digest.clone(),
        object_sha256: entry.object.sha256.clone(),
        catalogue_revision: entry.catalog_revision,
    })
}
