use super::*;

impl RrflowEngine {
    pub fn scroll_vector_points(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        request: &ScrollVectorPoints,
        now: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<VectorPointPage> {
        request
            .validate()
            .map_err(|error| ServiceError::Vector(error.to_string()))?;
        self.authorize(
            session_id,
            token,
            SecurityAction::VectorPointScroll,
            now,
            request_id,
            operation_id,
        )?;
        let scope = self.query_scope(&request.scope)?;
        let catalogue = vyrm_vector::VectorCollectionRepository::new(&self.storage, scope.clone())
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
        let config = collection
            .definition
            .vectors
            .get(&ProjectionId::new(request.vector_name.as_str()).map_err(core_vector)?)
            .ok_or_else(|| {
                ServiceError::Vector(format!(
                    "unknown named vector {} in collection {}",
                    request.vector_name, request.collection_id
                ))
            })?;
        let read = self.storage.runtime_read_stamp(&scope)?;
        let scan_limit = usize::try_from(request.max_scanned_changes)
            .map_err(|_| ServiceError::Vector("vector point scan budget exceeds usize".into()))?;
        let page = self.storage.runtime_read_changes(&read, 0, scan_limit)?;
        if page.through_cursor < page.head_cursor {
            return Err(ServiceError::Vector(format!(
                "vector point scroll requires more than {} retained changes",
                request.max_scanned_changes
            )));
        }
        let visibility = vyrm_vector::VectorVisibilityRequest {
            scope: scope.clone(),
            read: read.clone(),
            valid_at: request.valid_at,
            field: config.field.clone(),
            embedding_model: config.embedding_model.clone(),
            filter: request
                .filter
                .as_ref()
                .map(internal_vector_filter)
                .transpose()?,
        };
        let mut candidates = vyrm_vector::materialize_visible(
            &visibility,
            vyrm_vector::candidates_from_changes(&page.changes, &scope),
        )
        .map_err(core_vector)?;
        candidates.sort_by(|left, right| left.vector.reference.cmp(&right.vector.reference));
        if let Some(after) = &request.after_reference {
            let after = runtime_ref(after)?;
            candidates.retain(|candidate| candidate.vector.reference > after);
        }
        let limit = usize::try_from(request.limit)
            .map_err(|_| ServiceError::Vector("vector point page limit exceeds usize".into()))?;
        let truncated = candidates.len() > limit;
        candidates.truncate(limit);
        let points = candidates
            .iter()
            .map(public_vector_point)
            .collect::<Result<Vec<_>>>()?;
        let next_after = truncated
            .then(|| points.last().map(|point| point.reference.clone()))
            .flatten();
        Ok(VectorPointPage {
            scope: request.scope.clone(),
            collection_id: request.collection_id.clone(),
            vector_name: request.vector_name.clone(),
            read_manifest_sha256: read.manifest_id,
            known_at_cursor: read.commit_cursor,
            scanned_changes: page.validation.change_reads,
            points,
            next_after,
            truncated,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn retrieve_vector_points(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        request: &RetrieveVectorPoints,
        now: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<VectorPointBatch> {
        request
            .validate()
            .map_err(|error| ServiceError::Vector(error.to_string()))?;
        self.authorize(
            session_id,
            token,
            SecurityAction::VectorPointRetrieve,
            now,
            request_id,
            operation_id,
        )?;
        let scope = self.query_scope(&request.scope)?;
        let catalogue = vyrm_vector::VectorCollectionRepository::new(&self.storage, scope.clone())
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
        let config = collection
            .definition
            .vectors
            .get(&ProjectionId::new(request.vector_name.as_str()).map_err(core_vector)?)
            .ok_or_else(|| {
                ServiceError::Vector(format!(
                    "unknown named vector {} in collection {}",
                    request.vector_name, request.collection_id
                ))
            })?;
        let read = self.storage.runtime_read_stamp(&scope)?;
        let scan_limit = usize::try_from(request.max_scanned_changes)
            .map_err(|_| ServiceError::Vector("vector point scan budget exceeds usize".into()))?;
        let page = self.storage.runtime_read_changes(&read, 0, scan_limit)?;
        if page.through_cursor < page.head_cursor {
            return Err(ServiceError::Vector(format!(
                "vector point retrieve requires more than {} retained changes",
                request.max_scanned_changes
            )));
        }
        let visibility = vyrm_vector::VectorVisibilityRequest {
            scope: scope.clone(),
            read: read.clone(),
            valid_at: request.valid_at,
            field: config.field.clone(),
            embedding_model: config.embedding_model.clone(),
            filter: None,
        };
        let visible = vyrm_vector::materialize_visible(
            &visibility,
            vyrm_vector::candidates_from_changes(&page.changes, &scope),
        )
        .map_err(core_vector)?
        .into_iter()
        .map(|candidate| (candidate.vector.reference.clone(), candidate))
        .collect::<BTreeMap<_, _>>();
        let mut points = Vec::new();
        let mut missing = Vec::new();
        for reference in &request.references {
            let runtime_reference = runtime_ref(reference)?;
            match visible.get(&runtime_reference) {
                Some(candidate) => points.push(public_vector_point(candidate)?),
                None => missing.push(reference.clone()),
            }
        }
        Ok(VectorPointBatch {
            scope: request.scope.clone(),
            collection_id: request.collection_id.clone(),
            vector_name: request.vector_name.clone(),
            read_manifest_sha256: read.manifest_id,
            known_at_cursor: read.commit_cursor,
            scanned_changes: page.validation.change_reads,
            points,
            missing,
        })
    }
}

pub(in crate::engine) fn public_data_ref(reference: &RuntimeRef) -> Result<DataReference> {
    Ok(DataReference {
        kind: CanonicalId::new(reference.kind.as_str())
            .map_err(|error| ServiceError::Vector(error.to_string()))?,
        id: CanonicalId::new(reference.id.as_str())
            .map_err(|error| ServiceError::Vector(error.to_string()))?,
    })
}

fn public_provenance(value: &EmbeddingProvenance) -> Result<DataEmbeddingProvenance> {
    Ok(DataEmbeddingProvenance {
        source_sha256: value.source_digest.clone(),
        model: value.model.clone(),
        model_sha256: value.model_digest.clone(),
        dimensions: value.dimensions,
        normalization: match value.normalization {
            VectorNormalization::None => DataVectorNormalization::None,
            VectorNormalization::UnitL2 => DataVectorNormalization::UnitL2,
        },
        generation_parameters: public_properties(&value.generation_parameters)?,
    })
}

fn public_vector_point(candidate: &vyrm_vector::VectorCandidate) -> Result<VectorPointSnapshot> {
    Ok(VectorPointSnapshot {
        reference: public_data_ref(&candidate.vector.reference)?,
        subject: public_data_ref(&candidate.vector.subject)?,
        source_cursor: candidate.source_cursor,
        value: public_vector_value(&candidate.vector.value),
        provenance: candidate
            .vector
            .provenance
            .as_ref()
            .map(public_provenance)
            .transpose()?,
        payload: public_properties(&candidate.vector.properties)?,
    })
}
