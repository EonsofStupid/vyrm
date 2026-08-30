use super::*;
use std::collections::BTreeSet;

impl RrdEngine {
    pub fn build_vector_quantization_artifact(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        request: &BuildVectorQuantizationArtifact,
        now: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<BuildVectorQuantizationArtifactResult> {
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
        self.build_vector_quantization_artifact_authorized(session_id, request, now, false)
    }

    fn build_vector_quantization_artifact_authorized(
        &self,
        session_id: &CorrelationId,
        request: &BuildVectorQuantizationArtifact,
        now: u64,
        reuse_matching: bool,
    ) -> Result<BuildVectorQuantizationArtifactResult> {
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
        if vector.kind != rrd_vector::VectorValueKind::Dense {
            return Err(ServiceError::Vector(
                "quantization artifacts require a dense named vector".into(),
            ));
        }
        for property in &request.filter_properties {
            let field = ProjectionId::new(property.as_str()).map_err(core_vector)?;
            if !collection.payload_indexes.contains_key(&field) {
                return Err(ServiceError::Vector(format!(
                    "quantization filter property {property} has no active payload index in collection {}",
                    request.collection_id
                )));
            }
        }

        let read = self.storage.runtime_read_stamp(&scope)?;
        let scan_limit = usize::try_from(request.max_scanned_changes)
            .map_err(|_| ServiceError::Vector("quantization scan budget exceeds usize".into()))?;
        let page = self.storage.runtime_read_changes(&read, 0, scan_limit)?;
        if page.through_cursor < page.head_cursor {
            return Err(ServiceError::Vector(format!(
                "quantization build requires more than {} retained changes",
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
        let method_name = match request.method {
            VectorQuantizationMethod::Scalar => "scalar",
            VectorQuantizationMethod::Product { .. } => "product",
            VectorQuantizationMethod::Binary => "binary",
            VectorQuantizationMethod::TurboQuant { .. } => "turboquant",
        };
        let artifact_id =
            quantization_artifact_id(method_name, &request.collection_id, &request.vector_name)?;
        let lifecycle = crate::quantization_artifact_catalogue(&self.storage, &scope)
            .map_err(|error| ServiceError::Vector(error.to_string()))?;
        let generation = lifecycle
            .artifacts
            .keys()
            .filter(|(id, _)| id == &artifact_id)
            .map(|(_, generation)| *generation)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| ServiceError::Vector("quantization generation overflowed".into()))?;
        let dimensions = usize::try_from(vector.dimensions)
            .map_err(|_| ServiceError::Vector("vector dimensions exceed usize".into()))?;
        let filter_properties = request
            .filter_properties
            .iter()
            .map(|property| property.as_str().to_owned())
            .collect::<BTreeSet<_>>();
        let artifact: rrd_vector::VectorArtifact = match request.method {
            VectorQuantizationMethod::Scalar
            | VectorQuantizationMethod::Product { .. }
            | VectorQuantizationMethod::Binary => rrd_vector::QuantizedSegment::build(
                rrd_vector::QuantizedSegmentConfig {
                    id: artifact_id.clone(),
                    scope: scope.clone(),
                    field: vector.field.clone(),
                    dimensions,
                    metric: vector.metric,
                    method: internal_quantization_method(&request.method)?,
                    embedding_model: vector.embedding_model.clone(),
                    filter_properties,
                },
                generation,
                source_cursor,
                candidates,
            )
            .map(Into::into)
            .map_err(core_vector)?,
            VectorQuantizationMethod::TurboQuant { bits, seed } => {
                rrd_vector::TurboQuantSegment::build(
                    rrd_vector::TurboQuantSegmentConfig {
                        id: artifact_id.clone(),
                        scope: scope.clone(),
                        field: vector.field.clone(),
                        dimensions,
                        metric: vector.metric,
                        bits: internal_turboquant_bits(bits),
                        seed,
                        embedding_model: vector.embedding_model.clone(),
                        filter_properties,
                    },
                    generation,
                    source_cursor,
                    candidates,
                )
                .map(Into::into)
                .map_err(core_vector)?
            }
        };
        if reuse_matching {
            let newest_generation = lifecycle
                .artifacts
                .keys()
                .filter(|(id, _)| id == &artifact_id)
                .map(|(_, generation)| *generation)
                .max();
            if let Some(generation) = newest_generation {
                let existing = &lifecycle.artifacts[&(artifact_id.clone(), generation)];
                let desired = artifact.descriptor();
                let existing_stamp = existing.entry.descriptor.stamp();
                let desired_stamp = desired.stamp();
                let activation_eligible = match existing.state {
                    rrd_vector::QuantizationArtifactState::Active => true,
                    rrd_vector::QuantizationArtifactState::Ready => lifecycle
                        .active
                        .get(&artifact_id)
                        .is_none_or(|active| generation > *active),
                    rrd_vector::QuantizationArtifactState::Retired => false,
                };
                if activation_eligible
                    && existing.entry.kind == artifact.kind()
                    && existing_stamp.config_digest == desired_stamp.config_digest
                    && existing_stamp.source_cursor == desired_stamp.source_cursor
                {
                    return Ok(BuildVectorQuantizationArtifactResult {
                        artifact: public_quantization_artifact(
                            &existing.entry,
                            existing.state,
                            lifecycle.revision,
                        )?,
                    });
                }
            }
        }
        let data = rrd_store::DataRuntimeRef::new(&self.storage, &self.objects);
        let publication = crate::build_traced_quantization_artifact(
            &data,
            ProjectionId::new(request.collection_id.as_str()).map_err(core_vector)?,
            ProjectionId::new(request.vector_name.as_str()).map_err(core_vector)?,
            artifact,
            &format!("session:{}", session_id.as_str()),
            now,
        )
        .map_err(|error| ServiceError::Vector(error.to_string()))?;
        Ok(BuildVectorQuantizationArtifactResult {
            artifact: public_quantization_artifact(
                &publication.entry,
                publication.state,
                publication.lifecycle_revision,
            )?,
        })
    }

    /// Compatibility adapter for the historical `ensure_vector_index`
    /// TurboQuant request. It uses the same ready/activate lifecycle as every
    /// other quantization method and resumes a matching ready generation after
    /// an interrupted build instead of creating a second catalogue authority.
    pub(super) fn ensure_turboquant_index_authorized(
        &self,
        session_id: &CorrelationId,
        request: &EnsureVectorIndex,
        now: u64,
    ) -> Result<EnsureVectorIndexResult> {
        let VectorIndexConfiguration::TurboQuant {
            bits,
            seed,
            filter_properties,
        } = &request.configuration
        else {
            return Err(ServiceError::Vector(
                "TurboQuant compatibility adapter received another index kind".into(),
            ));
        };
        let build = BuildVectorQuantizationArtifact {
            scope: request.scope.clone(),
            collection_id: request.collection_id.clone(),
            vector_name: request.vector_name.clone(),
            method: VectorQuantizationMethod::TurboQuant {
                bits: *bits,
                seed: *seed,
            },
            filter_properties: filter_properties.clone(),
            max_scanned_changes: request.max_scanned_changes,
        };
        let built = self
            .build_vector_quantization_artifact_authorized(session_id, &build, now, true)?
            .artifact;
        if built.state == VectorQuantizationArtifactState::Active {
            return Ok(EnsureVectorIndexResult {
                index: public_turboquant_index(&built)?,
                idempotent_replay: true,
            });
        }
        if built.state != VectorQuantizationArtifactState::Ready {
            return Err(ServiceError::Vector(
                "TurboQuant ensure cannot activate a retired artifact".into(),
            ));
        }
        let scope = self.query_scope(&request.scope)?;
        let data = rrd_store::DataRuntimeRef::new(&self.storage, &self.objects);
        let transition = crate::transition_traced_quantization_artifact(
            &data,
            &scope,
            &ProjectionId::new(built.artifact_id.as_str()).map_err(core_vector)?,
            built.generation,
            rrd_vector::QuantizationLifecycleAction::Activate,
            &format!("session:{}", session_id.as_str()),
            now,
        )
        .map_err(|error| ServiceError::Vector(error.to_string()))?;
        let active = public_quantization_artifact(
            &transition.entry,
            transition.state,
            transition.lifecycle_revision,
        )?;
        Ok(EnsureVectorIndexResult {
            index: public_turboquant_index(&active)?,
            idempotent_replay: false,
        })
    }

    pub fn list_vector_quantization_artifacts(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        request: &ListVectorQuantizationArtifacts,
        now: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<ListVectorQuantizationArtifactsResult> {
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
        let catalogue = crate::quantization_artifact_catalogue(&self.storage, &scope)
            .map_err(|error| ServiceError::Vector(error.to_string()))?;
        let limit = usize::try_from(request.max_artifacts)
            .map_err(|_| ServiceError::Vector("quantization list bound exceeds usize".into()))?;
        let artifacts = catalogue
            .artifacts
            .values()
            .filter(|artifact| {
                request.collection_id.as_ref().is_none_or(|collection| {
                    artifact.entry.collection_id.as_str() == collection.as_str()
                }) && request
                    .vector_name
                    .as_ref()
                    .is_none_or(|vector| artifact.entry.vector_name.as_str() == vector.as_str())
            })
            .take(limit)
            .map(|artifact| {
                public_quantization_artifact(&artifact.entry, artifact.state, catalogue.revision)
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(ListVectorQuantizationArtifactsResult {
            scope: request.scope.clone(),
            lifecycle_revision: catalogue.revision,
            artifacts,
        })
    }

    pub fn activate_vector_quantization_artifact(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        request: &ActivateVectorQuantizationArtifact,
        now: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<ActivateVectorQuantizationArtifactResult> {
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
        let data = rrd_store::DataRuntimeRef::new(&self.storage, &self.objects);
        let transition = crate::transition_traced_quantization_artifact(
            &data,
            &scope,
            &ProjectionId::new(request.artifact_id.as_str()).map_err(core_vector)?,
            request.generation,
            rrd_vector::QuantizationLifecycleAction::Activate,
            &format!("session:{}", session_id.as_str()),
            now,
        )
        .map_err(|error| ServiceError::Vector(error.to_string()))?;
        Ok(ActivateVectorQuantizationArtifactResult {
            artifact: public_quantization_artifact(
                &transition.entry,
                transition.state,
                transition.lifecycle_revision,
            )?,
        })
    }

    pub fn retire_vector_quantization_artifact(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        request: &RetireVectorQuantizationArtifact,
        now: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<RetireVectorQuantizationArtifactResult> {
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
        let data = rrd_store::DataRuntimeRef::new(&self.storage, &self.objects);
        let transition = crate::transition_traced_quantization_artifact(
            &data,
            &scope,
            &ProjectionId::new(request.artifact_id.as_str()).map_err(core_vector)?,
            request.generation,
            rrd_vector::QuantizationLifecycleAction::Retire,
            &format!("session:{}", session_id.as_str()),
            now,
        )
        .map_err(|error| ServiceError::Vector(error.to_string()))?;
        Ok(RetireVectorQuantizationArtifactResult {
            artifact: public_quantization_artifact(
                &transition.entry,
                transition.state,
                transition.lifecycle_revision,
            )?,
        })
    }
}

pub(super) fn quantization_artifact_id(
    method: &str,
    collection_id: &CanonicalId,
    vector_name: &CanonicalId,
) -> Result<ProjectionId> {
    ProjectionId::new(format!("quant-{method}-{collection_id}-{vector_name}")).map_err(core_vector)
}

fn internal_quantization_method(
    method: &VectorQuantizationMethod,
) -> Result<rrd_vector::QuantizationMethod> {
    Ok(match method {
        VectorQuantizationMethod::Scalar => rrd_vector::QuantizationMethod::Scalar,
        VectorQuantizationMethod::Product { compression } => {
            rrd_vector::QuantizationMethod::Product {
                compression: match compression {
                    VectorProductCompression::X4 => rrd_vector::ProductCompression::X4,
                    VectorProductCompression::X8 => rrd_vector::ProductCompression::X8,
                    VectorProductCompression::X16 => rrd_vector::ProductCompression::X16,
                    VectorProductCompression::X32 => rrd_vector::ProductCompression::X32,
                    VectorProductCompression::X64 => rrd_vector::ProductCompression::X64,
                },
            }
        }
        VectorQuantizationMethod::Binary => rrd_vector::QuantizationMethod::Binary,
        VectorQuantizationMethod::TurboQuant { .. } => {
            return Err(ServiceError::Vector(
                "TurboQuant does not use the scalar/product/binary codec".into(),
            ))
        }
    })
}

fn internal_turboquant_bits(bits: VectorQuantizationBits) -> rrd_vector::TurboQuantBits {
    match bits {
        VectorQuantizationBits::Bits4 => rrd_vector::TurboQuantBits::Bits4,
        VectorQuantizationBits::Bits2 => rrd_vector::TurboQuantBits::Bits2,
        VectorQuantizationBits::Bits1_5 => rrd_vector::TurboQuantBits::Bits1_5,
        VectorQuantizationBits::Bits1 => rrd_vector::TurboQuantBits::Bits1,
    }
}

fn public_quantization_artifact(
    entry: &rrd_vector::QuantizationArtifactEntry,
    state: rrd_vector::QuantizationArtifactState,
    lifecycle_revision: u64,
) -> Result<VectorQuantizationArtifactSnapshot> {
    let stamp = entry.descriptor.stamp();
    let (method, indexed_vectors, packed, full, auxiliary) = match &entry.descriptor {
        rrd_vector::VectorProjectionDescriptor::Quantized { descriptor } => (
            match descriptor.method {
                rrd_vector::QuantizationMethod::Scalar => VectorQuantizationMethod::Scalar,
                rrd_vector::QuantizationMethod::Product { compression } => {
                    VectorQuantizationMethod::Product {
                        compression: match compression {
                            rrd_vector::ProductCompression::X4 => VectorProductCompression::X4,
                            rrd_vector::ProductCompression::X8 => VectorProductCompression::X8,
                            rrd_vector::ProductCompression::X16 => VectorProductCompression::X16,
                            rrd_vector::ProductCompression::X32 => VectorProductCompression::X32,
                            rrd_vector::ProductCompression::X64 => VectorProductCompression::X64,
                        },
                    }
                }
                rrd_vector::QuantizationMethod::Binary => VectorQuantizationMethod::Binary,
            },
            descriptor.candidate_versions,
            descriptor.packed_vector_bytes,
            descriptor.full_precision_vector_bytes,
            descriptor.auxiliary_bytes,
        ),
        rrd_vector::VectorProjectionDescriptor::TurboQuant { descriptor } => (
            VectorQuantizationMethod::TurboQuant {
                bits: match descriptor.bits {
                    rrd_vector::TurboQuantBits::Bits4 => VectorQuantizationBits::Bits4,
                    rrd_vector::TurboQuantBits::Bits2 => VectorQuantizationBits::Bits2,
                    rrd_vector::TurboQuantBits::Bits1_5 => VectorQuantizationBits::Bits1_5,
                    rrd_vector::TurboQuantBits::Bits1 => VectorQuantizationBits::Bits1,
                },
                seed: descriptor.seed,
            },
            descriptor.candidate_versions,
            descriptor.packed_vector_bytes,
            descriptor.full_precision_vector_bytes,
            0,
        ),
        _ => {
            return Err(ServiceError::Vector(
                "quantization lifecycle entry has a non-quantized descriptor".into(),
            ))
        }
    };
    let as_u64 = |value: usize, label: &str| {
        u64::try_from(value)
            .map_err(|_| ServiceError::Vector(format!("quantization {label} exceeds u64")))
    };
    Ok(VectorQuantizationArtifactSnapshot {
        artifact_id: CanonicalId::new(stamp.id.as_str())
            .map_err(|error| ServiceError::Vector(error.to_string()))?,
        collection_id: CanonicalId::new(entry.collection_id.as_str())
            .map_err(|error| ServiceError::Vector(error.to_string()))?,
        vector_name: CanonicalId::new(entry.vector_name.as_str())
            .map_err(|error| ServiceError::Vector(error.to_string()))?,
        maximum_compression_ratio: method.maximum_compression_ratio(),
        method,
        state: match state {
            rrd_vector::QuantizationArtifactState::Ready => VectorQuantizationArtifactState::Ready,
            rrd_vector::QuantizationArtifactState::Active => {
                VectorQuantizationArtifactState::Active
            }
            rrd_vector::QuantizationArtifactState::Retired => {
                VectorQuantizationArtifactState::Retired
            }
        },
        generation: stamp.generation,
        source_cursor: stamp.source_cursor,
        indexed_vectors: as_u64(indexed_vectors, "indexed vectors")?,
        packed_vector_bytes: as_u64(packed, "packed bytes")?,
        full_precision_vector_bytes: as_u64(full, "full-precision bytes")?,
        auxiliary_bytes: as_u64(auxiliary, "auxiliary bytes")?,
        configuration_sha256: stamp.config_digest.clone(),
        artifact_sha256: stamp.artifact_digest.clone(),
        object_sha256: entry.object.sha256.clone(),
        object_length: entry.object.length,
        built_at_unix_ms: entry.built_at,
        lifecycle_revision,
    })
}

fn public_turboquant_index(
    artifact: &VectorQuantizationArtifactSnapshot,
) -> Result<VectorIndexSnapshot> {
    if !matches!(artifact.method, VectorQuantizationMethod::TurboQuant { .. })
        || artifact.state != VectorQuantizationArtifactState::Active
    {
        return Err(ServiceError::Vector(
            "TurboQuant index snapshot requires one active lifecycle artifact".into(),
        ));
    }
    Ok(VectorIndexSnapshot {
        index_id: artifact.artifact_id.clone(),
        collection_id: artifact.collection_id.clone(),
        vector_name: artifact.vector_name.clone(),
        kind: CanonicalId::new("turboquant")
            .map_err(|error| ServiceError::Vector(error.to_string()))?,
        generation: artifact.generation,
        source_cursor: artifact.source_cursor,
        indexed_vectors: artifact.indexed_vectors,
        maintenance: VectorIndexMaintenanceSnapshot {
            mode: VectorIndexMaintenanceMode::FullBuild,
            previous_generation: None,
            indexed_delta_vectors: artifact.indexed_vectors,
        },
        packed_vector_bytes: Some(artifact.packed_vector_bytes),
        full_precision_vector_bytes: Some(artifact.full_precision_vector_bytes),
        build_evidence: None,
        configuration_sha256: artifact.configuration_sha256.clone(),
        artifact_sha256: artifact.artifact_sha256.clone(),
        object_sha256: artifact.object_sha256.clone(),
        catalogue_revision: artifact.lifecycle_revision,
    })
}
