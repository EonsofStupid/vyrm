use super::*;

const MAX_DIAGNOSTIC_ASSEMBLY_ATTEMPTS: u8 = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
struct EngineDiagnosticStamp {
    claim_sequence: u64,
    control_sequence: u64,
    runtime: ReadStamp,
    retention_sha256: String,
}

impl RrdEngine {
    /// Assembles a bounded cross-model diagnostic view under one verified RRD
    /// read stamp. Concurrent mutations cause a retry; a response is never
    /// returned with coordinates from two different runtime states.
    #[allow(clippy::too_many_arguments)]
    pub fn read_diagnostic_snapshot(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        request: &ReadDiagnosticSnapshot,
        now: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<DiagnosticSnapshot> {
        request
            .validate()
            .map_err(|error| ServiceError::Contract(error.to_string()))?;
        self.authorize(
            session_id,
            token,
            SecurityAction::DiagnosticsRead,
            now,
            request_id,
            operation_id,
        )?;
        let scope = self.query_scope(&request.scope)?;

        for attempt in 1..=MAX_DIAGNOSTIC_ASSEMBLY_ATTEMPTS {
            let before = capture_diagnostic_stamp(self, &scope, now)?;
            let runtime_changes = match diagnostic_runtime_changes(
                self,
                &before.runtime,
                request.runtime_max_scanned_changes,
            ) {
                Ok(changes) => changes,
                Err(error) if error.retryable() => continue,
                Err(error) => return Err(error),
            };
            let schema = self
                .storage
                .runtime_schema(&scope)?
                .as_ref()
                .map(super::model::public_schema)
                .transpose()?;
            let models = diagnostic_models(&request.scope, schema.as_ref())?;
            let (graph, graph_difference) = match diagnostic_graph(
                &before.runtime,
                &runtime_changes,
                &request.scope,
                request.graph_valid_at_unix_ms,
                request.graph_known_at_cursor,
                request.graph_compare_cursor,
            ) {
                Ok(graph) => graph,
                Err(error) if error.retryable() => continue,
                Err(error) => return Err(error),
            };
            let vector_artifacts =
                diagnostic_vector_artifacts(&runtime_changes, &scope, &request.scope)?;
            let retention = diagnostic_retention(self, now)?;
            let retention_sha256 = diagnostic_retention_sha256(&retention)?;
            let query_indexes = diagnostic_query_indexes(self, &scope, &request.scope)?;
            let vector_collections = diagnostic_vector_collections(self, &scope, &request.scope)?;
            let estate = rrd_estate::EstateRepository::new(&self.storage, self.instance.clone())
                .load()?
                .as_ref()
                .map(rrd_estate::public_snapshot);
            let changes = self.read_changefeed_page(&ReadChangefeed {
                scope: request.scope.clone(),
                after_cursor: request.changes_after_cursor,
                limit: request.change_limit,
            })?;
            let audit = diagnostic_audit(
                self,
                request.audit_after_sequence,
                usize::from(request.audit_limit),
            )?;
            let product_capabilities = crate::product_capability_catalogue();
            let runtime_tools = crate::runtime_tool_contract_catalogue();
            let readiness = self.readiness(now)?;
            let after = capture_diagnostic_stamp(self, &scope, now)?;

            if before != after
                || readiness.claim_sequence != after.claim_sequence
                || readiness.runtime_cursor != after.runtime.commit_cursor
                || changes.head_cursor != after.runtime.commit_cursor
                || retention_sha256 != before.retention_sha256
                || retention_sha256 != after.retention_sha256
            {
                continue;
            }

            let sections = diagnostic_sections(
                after.runtime.commit_cursor,
                &product_capabilities,
                &runtime_tools,
                schema.as_ref(),
                &models,
                &graph,
                &graph_difference,
                &retention,
                &vector_artifacts,
                &query_indexes,
                &vector_collections,
                estate.as_ref(),
                &changes,
                &audit,
            )?;
            let snapshot = DiagnosticSnapshot {
                format_version: DIAGNOSTIC_SNAPSHOT_FORMAT_VERSION,
                observed_at_unix_ms: now,
                instance: ResourceId::new(ResourceKind::Instance, self.instance.as_str())
                    .expect("engine instance identity is canonical"),
                scope: request.scope.clone(),
                read: DiagnosticReadStamp {
                    claim_sequence: after.claim_sequence,
                    control_sequence: after.control_sequence,
                    runtime_cursor: after.runtime.commit_cursor,
                    schema_revision: after.runtime.schema_revision,
                    catalogue_revision: after.runtime.catalog_revision,
                    runtime_manifest_sha256: after.runtime.manifest_id,
                    retention_sha256: after.retention_sha256,
                    assembly_attempts: attempt,
                },
                readiness,
                product_capabilities,
                runtime_tools,
                schema,
                models,
                graph,
                graph_difference,
                retention,
                vector_artifacts,
                query_indexes,
                vector_collections,
                estate,
                changes,
                audit,
                sections,
            };
            snapshot
                .validate()
                .map_err(|error| ServiceError::Contract(error.to_string()))?;
            return Ok(snapshot);
        }

        Err(ServiceError::StorageConflict(format!(
            "diagnostic snapshot changed during all {MAX_DIAGNOSTIC_ASSEMBLY_ATTEMPTS} assembly attempts"
        )))
    }
}

#[allow(clippy::too_many_arguments)]
fn diagnostic_graph(
    read: &ReadStamp,
    changes: &[RuntimeChange],
    public_scope: &str,
    valid_at: u64,
    requested_known_at: Option<u64>,
    compare_cursor: u64,
) -> Result<(DiagnosticGraphSnapshot, DiagnosticGraphDifference)> {
    let known_at_cursor = requested_known_at.unwrap_or(read.commit_cursor);
    if known_at_cursor > read.commit_cursor {
        return Err(ServiceError::Contract(format!(
            "diagnostic graph cursor {known_at_cursor} exceeds captured head {}",
            read.commit_cursor
        )));
    }
    if compare_cursor > known_at_cursor {
        return Err(ServiceError::Contract(format!(
            "diagnostic graph comparison cursor {compare_cursor} exceeds target {known_at_cursor}"
        )));
    }
    let before =
        RuntimeGraphSnapshot::from_changes(changes, read.scope.clone(), valid_at, compare_cursor);
    let after =
        RuntimeGraphSnapshot::from_changes(changes, read.scope.clone(), valid_at, known_at_cursor);
    let difference = before.diff(&after);
    Ok((
        public_graph_snapshot(public_scope, &after)?,
        public_graph_difference(valid_at, &difference)?,
    ))
}

fn diagnostic_runtime_changes(
    engine: &RrdEngine,
    read: &ReadStamp,
    max_scanned_changes: u64,
) -> Result<Vec<RuntimeChange>> {
    if read.commit_cursor > max_scanned_changes {
        return Err(ServiceError::Contract(format!(
            "diagnostic runtime head {} exceeds the explicit replay budget {max_scanned_changes}; checkpointed projections are required for this range",
            read.commit_cursor
        )));
    }
    let mut after = 0;
    let mut changes = Vec::new();
    while after < read.commit_cursor {
        let remaining = read.commit_cursor - after;
        let limit = usize::try_from(remaining.min(1_024))
            .expect("diagnostic runtime page size is bounded by 1024");
        let page = engine.storage.runtime_read_changes(read, after, limit)?;
        changes.extend(page.changes);
        if page.through_cursor <= after {
            return Err(ServiceError::Storage(
                "diagnostic runtime replay made no cursor progress".into(),
            ));
        }
        after = page.through_cursor.min(read.commit_cursor);
    }
    Ok(changes)
}

fn diagnostic_vector_artifacts(
    changes: &[RuntimeChange],
    scope: &ScopeId,
    public_scope: &str,
) -> Result<DiagnosticVectorArtifactCatalogueSnapshot> {
    let entries = crate::runtime::vector_artifact_catalog_entries_from_changes(changes, scope)
        .map_err(|error| ServiceError::Vector(error.to_string()))?;
    let artifacts = entries
        .iter()
        .map(public_vector_artifact)
        .collect::<Result<Vec<_>>>()?;
    Ok(DiagnosticVectorArtifactCatalogueSnapshot {
        scope: public_scope.into(),
        revision: artifacts.last().map_or(0, |entry| entry.catalogue_revision),
        artifacts,
    })
}

fn public_vector_artifact(
    entry: &rrd_vector::VectorArtifactCatalogEntry,
) -> Result<DiagnosticVectorArtifactSnapshot> {
    let stamp = entry.descriptor.stamp();
    Ok(DiagnosticVectorArtifactSnapshot {
        catalogue_revision: entry.catalog_revision,
        kind: match entry.kind {
            rrd_vector::VectorArtifactKind::ExactSegment => {
                DiagnosticVectorArtifactKind::ExactSegment
            }
            rrd_vector::VectorArtifactKind::CompactDense => {
                DiagnosticVectorArtifactKind::CompactDense
            }
            rrd_vector::VectorArtifactKind::Hnsw => DiagnosticVectorArtifactKind::Hnsw,
            rrd_vector::VectorArtifactKind::TurboQuant => DiagnosticVectorArtifactKind::TurboQuant,
        },
        scope: entry.scope().as_str().into(),
        projection_id: stamp.id.as_str().into(),
        generation: stamp.generation,
        source_cursor: stamp.source_cursor,
        config_sha256: stamp.config_digest.clone(),
        artifact_sha256: stamp.artifact_digest.clone(),
        object: public_diagnostic_reference(&entry.object.reference),
        object_sha256: entry.object.sha256.clone(),
        object_length: entry.object.length,
        media_type: entry.object.media_type.clone(),
        receipt: DataObjectReceipt {
            backend: entry.object.receipt.backend.clone(),
            key: entry.object.receipt.key.clone(),
            version: entry.object.receipt.version.clone(),
            etag: entry.object.receipt.etag.clone(),
        },
        published_at_unix_ms: entry.published_at,
        entry_sha256: entry.entry_digest.clone(),
    })
}

fn public_graph_snapshot(
    public_scope: &str,
    snapshot: &RuntimeGraphSnapshot,
) -> Result<DiagnosticGraphSnapshot> {
    Ok(DiagnosticGraphSnapshot {
        scope: public_scope.into(),
        valid_at_unix_ms: snapshot.valid_at,
        known_at_cursor: snapshot.known_at_cursor,
        records: snapshot
            .records
            .iter()
            .map(public_graph_record)
            .collect::<Result<_>>()?,
        relations: snapshot
            .relations
            .iter()
            .map(public_graph_relation)
            .collect::<Result<_>>()?,
    })
}

fn public_graph_difference(
    valid_at: u64,
    difference: &rrd_core::RuntimeGraphDiff,
) -> Result<DiagnosticGraphDifference> {
    Ok(DiagnosticGraphDifference {
        valid_at_unix_ms: valid_at,
        from_cursor: difference.from_cursor,
        to_cursor: difference.to_cursor,
        added_records: difference
            .added_records
            .iter()
            .map(public_graph_record)
            .collect::<Result<_>>()?,
        removed_records: difference
            .removed_records
            .iter()
            .map(public_graph_record)
            .collect::<Result<_>>()?,
        changed_records: difference
            .changed_records
            .iter()
            .map(|change| {
                Ok(DiagnosticGraphRecordChange {
                    before: public_graph_record(&change.before)?,
                    after: public_graph_record(&change.after)?,
                })
            })
            .collect::<Result<_>>()?,
        added_relations: difference
            .added_relations
            .iter()
            .map(public_graph_relation)
            .collect::<Result<_>>()?,
        removed_relations: difference
            .removed_relations
            .iter()
            .map(public_graph_relation)
            .collect::<Result<_>>()?,
        changed_relations: difference
            .changed_relations
            .iter()
            .map(|change| {
                Ok(DiagnosticGraphRelationChange {
                    before: public_graph_relation(&change.before)?,
                    after: public_graph_relation(&change.after)?,
                })
            })
            .collect::<Result<_>>()?,
    })
}

fn public_graph_record(record: &RuntimeRecord) -> Result<DiagnosticGraphRecordSnapshot> {
    Ok(DiagnosticGraphRecordSnapshot {
        reference: public_diagnostic_reference(&record.reference),
        valid_from_unix_ms: record.valid_from,
        valid_to_unix_ms: record.valid_to,
        properties: super::model::public_properties(&record.properties)?,
    })
}

fn public_graph_relation(relation: &RuntimeRelation) -> Result<DiagnosticGraphRelationSnapshot> {
    Ok(DiagnosticGraphRelationSnapshot {
        reference: public_diagnostic_reference(&relation.reference),
        from: public_diagnostic_reference(&relation.from),
        to: public_diagnostic_reference(&relation.to),
        valid_from_unix_ms: relation.valid_from,
        valid_to_unix_ms: relation.valid_to,
        properties: super::model::public_properties(&relation.properties)?,
    })
}

fn public_diagnostic_reference(reference: &RuntimeRef) -> DiagnosticRuntimeReference {
    DiagnosticRuntimeReference {
        kind: reference.kind.as_str().into(),
        id: reference.id.as_str().into(),
    }
}

fn diagnostic_models(
    scope: &str,
    schema: Option<&DataSchemaRegistry>,
) -> Result<DiagnosticModelCatalogueSnapshot> {
    let mut models = Vec::new();
    if let Some(schema) = schema {
        for (id, model) in &schema.records {
            models.push(DiagnosticModelSnapshot {
                id: id.clone(),
                kind: DiagnosticModelKind::Record,
                property_count: diagnostic_count(model.properties.len())?,
                required_property_count: diagnostic_count(
                    model
                        .properties
                        .values()
                        .filter(|property| property.required)
                        .count(),
                )?,
                constraint_count: diagnostic_count(model.unique_properties.len())?,
                allow_additional_properties: model.allow_additional_properties,
            });
        }
        for (id, model) in &schema.relations {
            let constraint_count = model
                .from
                .len()
                .checked_add(model.to.len())
                .and_then(|count| count.checked_add(usize::from(model.unique_pair)))
                .and_then(|count| count.checked_add(usize::from(model.max_outgoing.is_some())))
                .and_then(|count| count.checked_add(usize::from(model.max_incoming.is_some())))
                .ok_or_else(|| ServiceError::Contract("model constraint count overflow".into()))?;
            models.push(DiagnosticModelSnapshot {
                id: id.clone(),
                kind: DiagnosticModelKind::Relation,
                property_count: diagnostic_count(model.properties.len())?,
                required_property_count: diagnostic_count(
                    model
                        .properties
                        .values()
                        .filter(|property| property.required)
                        .count(),
                )?,
                constraint_count: diagnostic_count(constraint_count)?,
                allow_additional_properties: model.allow_additional_properties,
            });
        }
        for (id, model) in &schema.events {
            let constraint_count = model
                .subject_types
                .len()
                .checked_add(usize::from(model.subject_required))
                .ok_or_else(|| ServiceError::Contract("model constraint count overflow".into()))?;
            models.push(DiagnosticModelSnapshot {
                id: id.clone(),
                kind: DiagnosticModelKind::Event,
                property_count: diagnostic_count(model.properties.len())?,
                required_property_count: diagnostic_count(
                    model
                        .properties
                        .values()
                        .filter(|property| property.required)
                        .count(),
                )?,
                constraint_count: diagnostic_count(constraint_count)?,
                allow_additional_properties: model.allow_additional_properties,
            });
        }
    }
    models.sort_by(|left, right| {
        left.kind
            .cmp(&right.kind)
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(DiagnosticModelCatalogueSnapshot {
        scope: scope.into(),
        schema_revision: schema.map(|schema| schema.revision),
        models,
    })
}

fn capture_diagnostic_stamp(
    engine: &RrdEngine,
    scope: &ScopeId,
    now: u64,
) -> Result<EngineDiagnosticStamp> {
    let retention = diagnostic_retention(engine, now)?;
    Ok(EngineDiagnosticStamp {
        control_sequence: engine.storage.control_sequence()?,
        claim_sequence: engine.storage.sequence()?,
        runtime: engine.storage.runtime_read_stamp(scope)?,
        retention_sha256: diagnostic_retention_sha256(&retention)?,
    })
}

fn diagnostic_retention(engine: &RrdEngine, now: u64) -> Result<DiagnosticRetentionSnapshot> {
    let handles = engine.storage.runtime_snapshots(now)?;
    let mut leases = Vec::with_capacity(handles.len());
    let mut pins = Vec::with_capacity(handles.len());
    for handle in handles {
        let pin = RetentionPin::from_snapshot(&handle)
            .map_err(|error| ServiceError::Storage(error.to_string()))?;
        leases.push(DiagnosticSnapshotLease {
            id_sha256: handle.id.as_str().into(),
            scope: handle.read.scope.as_str().into(),
            owner: handle.owner,
            created_at_unix_ms: handle.created_at,
            expires_at_unix_ms: handle.expires_at,
            runtime_cursor: handle.read.commit_cursor,
            schema_revision: handle.read.schema_revision,
            catalogue_revision: handle.read.catalog_revision,
            runtime_manifest_sha256: handle.read.manifest_id,
        });
        pins.push(DiagnosticRetentionPin {
            id_sha256: pin.id.as_str().into(),
            snapshot_id_sha256: pin.snapshot_id.as_str().into(),
            scope: pin.scope.as_str().into(),
            runtime_manifest_sha256: pin.manifest_id,
            minimum_cursor: pin.minimum_cursor,
            expires_at_unix_ms: pin.expires_at,
        });
    }
    leases.sort_by(|left, right| left.id_sha256.cmp(&right.id_sha256));
    pins.sort_by(|left, right| left.id_sha256.cmp(&right.id_sha256));
    let oldest_retained_cursor = pins.iter().map(|pin| pin.minimum_cursor).min();
    Ok(DiagnosticRetentionSnapshot {
        observed_at_unix_ms: now,
        leases,
        pins,
        oldest_retained_cursor,
    })
}

fn diagnostic_retention_sha256(retention: &DiagnosticRetentionSnapshot) -> Result<String> {
    let encoded =
        serde_json::to_vec(retention).map_err(|error| ServiceError::Contract(error.to_string()))?;
    Ok(digest::sha256_hex(&encoded))
}

fn diagnostic_query_indexes(
    engine: &RrdEngine,
    scope: &ScopeId,
    public_scope: &str,
) -> Result<QueryIndexCatalogueSnapshot> {
    let catalogue = rrd_query::IndexCatalogueRepository::new(&engine.storage, scope.clone())
        .load()
        .map_err(|error| ServiceError::Query(error.to_string()))?;
    Ok(QueryIndexCatalogueSnapshot {
        scope: public_scope.into(),
        revision: catalogue.revision,
        indexes: catalogue
            .entries
            .values()
            .map(super::query::public_query_index)
            .collect::<Result<_>>()?,
    })
}

fn diagnostic_vector_collections(
    engine: &RrdEngine,
    scope: &ScopeId,
    public_scope: &str,
) -> Result<VectorCollectionCatalogueSnapshot> {
    let catalogue = rrd_vector::VectorCollectionRepository::new(&engine.storage, scope.clone())
        .load()
        .map_err(super::vector::vector_collection_error)?;
    Ok(VectorCollectionCatalogueSnapshot {
        scope: public_scope.into(),
        revision: catalogue.revision,
        collections: catalogue
            .collections
            .values()
            .map(super::vector::public_vector_collection)
            .collect::<Result<_>>()?,
    })
}

fn diagnostic_audit(engine: &RrdEngine, after: u64, limit: usize) -> Result<AuditPage> {
    let page = rrd_security::SecurityRepository::new(&engine.storage, engine.instance.clone())
        .audit_since(after, limit)
        .map_err(|error| ServiceError::Contract(error.to_string()))?;
    Ok(AuditPage {
        requested_after_sequence: after,
        through_sequence: page.through_sequence,
        records: page
            .records
            .into_iter()
            .map(|(sequence, record)| AuditRecordSnapshot {
                sequence,
                audit_id: record.audit_id,
                at_unix_ms: record.at_unix_ms,
                principal_id: record.principal_id,
                action: record.action,
                resource: record.resource,
                request_id: record.request_id,
                operation_id: record.operation_id,
                phase: record.phase,
                decision: record.decision,
                status_code: record.status_code,
                request_sha256: record.request_sha256,
                response_sha256: record.response_sha256,
            })
            .collect(),
    })
}

#[allow(clippy::too_many_arguments)]
fn diagnostic_sections(
    known_at_cursor: u64,
    product_capabilities: &rrd_contract::ProductCapabilityCatalogue,
    runtime_tools: &rrd_contract::RuntimeToolCatalogue,
    schema: Option<&DataSchemaRegistry>,
    models: &DiagnosticModelCatalogueSnapshot,
    graph: &DiagnosticGraphSnapshot,
    graph_difference: &DiagnosticGraphDifference,
    retention: &DiagnosticRetentionSnapshot,
    vector_artifacts: &DiagnosticVectorArtifactCatalogueSnapshot,
    query_indexes: &QueryIndexCatalogueSnapshot,
    vector_collections: &VectorCollectionCatalogueSnapshot,
    estate: Option<&rrd_contract::EstateSnapshot>,
    changes: &ChangefeedPage,
    audit: &AuditPage,
) -> Result<Vec<DiagnosticSectionSnapshot>> {
    let mut sections = vec![
        section(
            "audit",
            DiagnosticAuthority::Authoritative,
            DiagnosticCoverage::Bounded,
            audit.records.len(),
            known_at_cursor,
            Some(audit.requested_after_sequence),
        )?,
        section(
            "changes",
            DiagnosticAuthority::Authoritative,
            DiagnosticCoverage::Bounded,
            changes.changes.len(),
            known_at_cursor,
            Some(changes.requested_after_cursor),
        )?,
        section(
            "estate",
            DiagnosticAuthority::Authoritative,
            DiagnosticCoverage::Complete,
            usize::from(estate.is_some()),
            known_at_cursor,
            None,
        )?,
        section(
            "graph",
            DiagnosticAuthority::Projection,
            DiagnosticCoverage::Complete,
            graph.records.len().saturating_add(graph.relations.len()),
            graph.known_at_cursor,
            None,
        )?,
        section(
            "graph-difference",
            DiagnosticAuthority::Projection,
            DiagnosticCoverage::Complete,
            graph_difference_row_count(graph_difference),
            graph_difference.to_cursor,
            Some(graph_difference.from_cursor),
        )?,
        section(
            "models",
            DiagnosticAuthority::Projection,
            DiagnosticCoverage::Complete,
            models.models.len(),
            known_at_cursor,
            None,
        )?,
        section(
            "product-capabilities",
            DiagnosticAuthority::Authoritative,
            DiagnosticCoverage::Complete,
            product_capabilities.capabilities.len(),
            known_at_cursor,
            None,
        )?,
        section(
            "query-indexes",
            DiagnosticAuthority::Projection,
            DiagnosticCoverage::Complete,
            query_indexes.indexes.len(),
            known_at_cursor,
            None,
        )?,
        section(
            "retention",
            DiagnosticAuthority::Authoritative,
            DiagnosticCoverage::Complete,
            retention.leases.len().saturating_add(retention.pins.len()),
            known_at_cursor,
            None,
        )?,
        section(
            "runtime-tools",
            DiagnosticAuthority::Authoritative,
            DiagnosticCoverage::Complete,
            runtime_tools.tools.len(),
            known_at_cursor,
            None,
        )?,
        section(
            "schema",
            DiagnosticAuthority::Authoritative,
            DiagnosticCoverage::Complete,
            usize::from(schema.is_some()),
            known_at_cursor,
            None,
        )?,
        section(
            "vector-artifacts",
            DiagnosticAuthority::Authoritative,
            DiagnosticCoverage::Complete,
            vector_artifacts.artifacts.len(),
            known_at_cursor,
            None,
        )?,
        section(
            "vector-collections",
            DiagnosticAuthority::Authoritative,
            DiagnosticCoverage::Complete,
            vector_collections.collections.len(),
            known_at_cursor,
            None,
        )?,
    ];
    sections.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(sections)
}

fn graph_difference_row_count(difference: &DiagnosticGraphDifference) -> usize {
    difference
        .added_records
        .len()
        .saturating_add(difference.removed_records.len())
        .saturating_add(difference.changed_records.len())
        .saturating_add(difference.added_relations.len())
        .saturating_add(difference.removed_relations.len())
        .saturating_add(difference.changed_relations.len())
}

fn diagnostic_count(value: usize) -> Result<u64> {
    u64::try_from(value).map_err(|_| ServiceError::Contract("diagnostic count exceeds u64".into()))
}

fn section(
    id: &str,
    authority: DiagnosticAuthority,
    coverage: DiagnosticCoverage,
    row_count: usize,
    known_at_cursor: u64,
    requested_after: Option<u64>,
) -> Result<DiagnosticSectionSnapshot> {
    Ok(DiagnosticSectionSnapshot {
        id: CanonicalId::new(id).map_err(|error| ServiceError::Contract(error.to_string()))?,
        authority,
        coverage,
        row_count: u64::try_from(row_count)
            .map_err(|_| ServiceError::Contract("diagnostic row count exceeds u64".into()))?,
        known_at_cursor,
        requested_after,
    })
}
