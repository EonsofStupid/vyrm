use super::super::*;

pub(in crate::engine) fn runtime_value(value: &QueryValue) -> Result<RuntimeValue> {
    Ok(match value {
        QueryValue::Null => RuntimeValue::Null,
        QueryValue::Bool(value) => RuntimeValue::Bool(*value),
        QueryValue::Integer(value) => RuntimeValue::Integer(*value),
        QueryValue::Unsigned(value) => RuntimeValue::Unsigned(*value),
        QueryValue::Decimal(value) => RuntimeValue::Decimal(value.clone()),
        QueryValue::String(value) => RuntimeValue::String(value.clone()),
        QueryValue::Digest(value) => RuntimeValue::Digest(value.clone()),
        QueryValue::List(values) => RuntimeValue::List(
            values
                .iter()
                .map(runtime_value)
                .collect::<Result<Vec<_>>>()?,
        ),
        QueryValue::Map(values) => RuntimeValue::Map(
            values
                .iter()
                .map(|(name, value)| Ok((name.clone(), runtime_value(value)?)))
                .collect::<Result<_>>()?,
        ),
    })
}

pub(in crate::engine) fn query_value(value: &RuntimeValue) -> Result<QueryValue> {
    Ok(match value {
        RuntimeValue::Null => QueryValue::Null,
        RuntimeValue::Bool(value) => QueryValue::Bool(*value),
        RuntimeValue::Integer(value) => QueryValue::Integer(*value),
        RuntimeValue::Unsigned(value) => QueryValue::Unsigned(*value),
        RuntimeValue::Decimal(value) => QueryValue::Decimal(value.clone()),
        RuntimeValue::String(value) => QueryValue::String(value.clone()),
        RuntimeValue::Digest(value) => QueryValue::Digest(value.clone()),
        RuntimeValue::List(values) => {
            QueryValue::List(values.iter().map(query_value).collect::<Result<Vec<_>>>()?)
        }
        RuntimeValue::Map(values) => QueryValue::Map(
            values
                .iter()
                .map(|(name, value)| Ok((name.clone(), query_value(value)?)))
                .collect::<Result<_>>()?,
        ),
    })
}

pub(in crate::engine) fn public_runtime_commit(
    request: &CommitTransaction,
    session_id: &CorrelationId,
    instance: &CanonicalId,
    expected_cursor: u64,
    at: u64,
) -> Result<RuntimeCommit> {
    let mutations = request
        .mutations
        .iter()
        .map(|mutation| public_runtime_mutation(mutation, session_id))
        .collect::<Result<Vec<_>>>()?;
    let commit = RuntimeCommit {
        scope: ScopeId::new(format!("instance:{instance}")).map_err(core_contract)?,
        at,
        actor: format!("session:{}", session_id.as_str()),
        expected_cursor,
        mutations,
    };
    commit.validate().map_err(core_contract)?;
    Ok(commit)
}

pub(in crate::engine) fn public_runtime_mutation(
    mutation: &TransactionMutation,
    session_id: &CorrelationId,
) -> Result<RuntimeMutation> {
    Ok(match mutation {
        TransactionMutation::AssertClaim { .. } => RuntimeMutation::Claim {
            claim: public_claim(mutation, session_id)?,
        },
        TransactionMutation::PutSchema { registry } => RuntimeMutation::Schema {
            registry: runtime_schema(registry)?,
        },
        TransactionMutation::PutRecord {
            reference,
            valid_from,
            valid_to,
            properties,
        } => RuntimeMutation::Record {
            record: RuntimeRecord {
                reference: runtime_ref(reference)?,
                valid_from: *valid_from,
                valid_to: *valid_to,
                properties: runtime_properties(properties)?,
            },
        },
        TransactionMutation::PutRelation {
            reference,
            from,
            to,
            valid_from,
            valid_to,
            properties,
        } => RuntimeMutation::Relation {
            relation: RuntimeRelation {
                reference: runtime_ref(reference)?,
                from: runtime_ref(from)?,
                to: runtime_ref(to)?,
                valid_from: *valid_from,
                valid_to: *valid_to,
                properties: runtime_properties(properties)?,
            },
        },
        TransactionMutation::AppendEvent {
            kind,
            subject,
            properties,
        } => RuntimeMutation::Event {
            event: RuntimeEvent {
                kind: RuntimeType::new(kind.as_str()).map_err(core_contract)?,
                subject: subject.as_ref().map(runtime_ref).transpose()?,
                properties: runtime_properties(properties)?,
            },
        },
        TransactionMutation::PutVector {
            reference,
            subject,
            collection_id,
            vector_name,
            field,
            valid_from,
            valid_to,
            value,
            provenance,
            properties,
        } => RuntimeMutation::Vector {
            vector: RuntimeVector {
                reference: runtime_ref(reference)?,
                subject: runtime_ref(subject)?,
                collection: collection_id.as_ref().zip(vector_name.as_ref()).map(
                    |(collection_id, vector_name)| rrd_core::VectorCollectionAddress {
                        collection_id: collection_id.as_str().into(),
                        vector_name: vector_name.as_str().into(),
                    },
                ),
                field: field.as_str().into(),
                valid_from: *valid_from,
                valid_to: *valid_to,
                value: runtime_vector_value(value),
                provenance: provenance
                    .as_ref()
                    .map(|value| -> Result<EmbeddingProvenance> {
                        Ok(EmbeddingProvenance {
                            source_digest: value.source_sha256.clone(),
                            model: value.model.clone(),
                            model_digest: value.model_sha256.clone(),
                            dimensions: value.dimensions,
                            normalization: match value.normalization {
                                DataVectorNormalization::None => VectorNormalization::None,
                                DataVectorNormalization::UnitL2 => VectorNormalization::UnitL2,
                            },
                            generation_parameters: runtime_properties(
                                &value.generation_parameters,
                            )?,
                        })
                    })
                    .transpose()?,
                properties: runtime_properties(properties)?,
            },
        },
        TransactionMutation::AppendSeriesSample {
            reference,
            series,
            observed_at,
            value,
            properties,
        } => RuntimeMutation::SeriesSample {
            sample: RuntimeSeriesSample {
                reference: runtime_ref(reference)?,
                series: runtime_ref(series)?,
                observed_at: *observed_at,
                value: match value {
                    DataSeriesValue::Integer(value) => SeriesValue::Integer(*value),
                    DataSeriesValue::Unsigned(value) => SeriesValue::Unsigned(*value),
                    DataSeriesValue::Decimal(value) => SeriesValue::Decimal(value.clone()),
                    DataSeriesValue::Bool(value) => SeriesValue::Bool(*value),
                    DataSeriesValue::String(value) => SeriesValue::String(value.clone()),
                },
                properties: runtime_properties(properties)?,
            },
        },
        TransactionMutation::PutGeo {
            reference,
            subject,
            field,
            valid_from,
            valid_to,
            value,
            properties,
        } => RuntimeMutation::Geo {
            geo: RuntimeGeo {
                reference: runtime_ref(reference)?,
                subject: runtime_ref(subject)?,
                field: field.as_str().into(),
                valid_from: *valid_from,
                valid_to: *valid_to,
                value: runtime_geo_value(value),
                properties: runtime_properties(properties)?,
            },
        },
        TransactionMutation::PublishObjectReference {
            reference,
            subject,
            sha256,
            length,
            media_type,
            receipt,
            properties,
        } => RuntimeMutation::Object {
            object: ObjectReference {
                reference: runtime_ref(reference)?,
                subject: subject.as_ref().map(runtime_ref).transpose()?,
                sha256: sha256.clone(),
                length: *length,
                media_type: media_type.clone(),
                receipt: ObjectReceipt {
                    backend: receipt.backend.clone(),
                    key: receipt.key.clone(),
                    version: receipt.version.clone(),
                    etag: receipt.etag.clone(),
                },
                properties: runtime_properties(properties)?,
            },
        },
    })
}

pub(in crate::engine) fn runtime_ref(reference: &DataReference) -> Result<RuntimeRef> {
    RuntimeRef::new(reference.kind.as_str(), reference.id.as_str()).map_err(core_contract)
}

pub(in crate::engine) fn runtime_properties(
    properties: &DataProperties,
) -> Result<RuntimeProperties> {
    properties
        .iter()
        .map(|(name, value)| Ok((name.clone(), runtime_value(value)?)))
        .collect()
}

pub(in crate::engine) fn runtime_schema(
    registry: &DataSchemaRegistry,
) -> Result<RuntimeSchemaRegistry> {
    Ok(RuntimeSchemaRegistry {
        revision: registry.revision,
        migration: registry.migration.clone(),
        catalogue: RuntimeCatalogueIdentity {
            namespace: RuntimeType::new(registry.catalogue.namespace.as_str())
                .map_err(core_contract)?,
            database: RuntimeType::new(registry.catalogue.database.as_str())
                .map_err(core_contract)?,
        },
        tables: registry
            .tables
            .iter()
            .map(|(kind, table)| {
                Ok((
                    RuntimeType::new(kind.as_str()).map_err(core_contract)?,
                    RuntimeTableSchema {
                        model: runtime_logical_model(table.model),
                        mode: match table.mode {
                            DataSchemaMode::Strict => RuntimeSchemaMode::Strict,
                            DataSchemaMode::Schemaless => RuntimeSchemaMode::Schemaless,
                        },
                        properties: runtime_property_schemas(&table.properties),
                        allow_additional_properties: table.allow_additional_properties,
                    },
                ))
            })
            .collect::<Result<_>>()?,
        records: registry
            .records
            .iter()
            .map(|(kind, schema)| {
                Ok((
                    RuntimeType::new(kind.as_str()).map_err(core_contract)?,
                    RuntimeRecordSchema {
                        properties: runtime_property_schemas(&schema.properties),
                        allow_additional_properties: schema.allow_additional_properties,
                        unique_properties: schema.unique_properties.clone(),
                    },
                ))
            })
            .collect::<Result<_>>()?,
        relations: registry
            .relations
            .iter()
            .map(|(kind, schema)| {
                Ok((
                    RuntimeType::new(kind.as_str()).map_err(core_contract)?,
                    RuntimeRelationSchema {
                        from: schema
                            .from
                            .iter()
                            .map(|kind| RuntimeType::new(kind.as_str()).map_err(core_contract))
                            .collect::<Result<_>>()?,
                        to: schema
                            .to
                            .iter()
                            .map(|kind| RuntimeType::new(kind.as_str()).map_err(core_contract))
                            .collect::<Result<_>>()?,
                        properties: runtime_property_schemas(&schema.properties),
                        allow_additional_properties: schema.allow_additional_properties,
                        unique_pair: schema.unique_pair,
                        max_outgoing: schema.max_outgoing,
                        max_incoming: schema.max_incoming,
                    },
                ))
            })
            .collect::<Result<_>>()?,
        events: registry
            .events
            .iter()
            .map(|(kind, schema)| {
                Ok((
                    RuntimeType::new(kind.as_str()).map_err(core_contract)?,
                    RuntimeEventSchema {
                        subject_required: schema.subject_required,
                        subject_types: schema
                            .subject_types
                            .iter()
                            .map(|kind| RuntimeType::new(kind.as_str()).map_err(core_contract))
                            .collect::<Result<_>>()?,
                        properties: runtime_property_schemas(&schema.properties),
                        allow_additional_properties: schema.allow_additional_properties,
                    },
                ))
            })
            .collect::<Result<_>>()?,
    })
}

fn runtime_logical_model(model: DataLogicalModel) -> RuntimeLogicalModel {
    match model {
        DataLogicalModel::Document => RuntimeLogicalModel::Document,
        DataLogicalModel::Relational => RuntimeLogicalModel::Relational,
        DataLogicalModel::GraphNode => RuntimeLogicalModel::GraphNode,
        DataLogicalModel::GraphRelation => RuntimeLogicalModel::GraphRelation,
        DataLogicalModel::KeyValue => RuntimeLogicalModel::KeyValue,
        DataLogicalModel::Vector => RuntimeLogicalModel::Vector,
        DataLogicalModel::Event => RuntimeLogicalModel::Event,
        DataLogicalModel::TimeSeries => RuntimeLogicalModel::TimeSeries,
        DataLogicalModel::Geo => RuntimeLogicalModel::Geo,
        DataLogicalModel::Object => RuntimeLogicalModel::Object,
        DataLogicalModel::ReasoningClaim => RuntimeLogicalModel::ReasoningClaim,
        DataLogicalModel::ReasoningRecord => RuntimeLogicalModel::ReasoningRecord,
        DataLogicalModel::ReasoningEvent => RuntimeLogicalModel::ReasoningEvent,
        DataLogicalModel::LifecycleRecord => RuntimeLogicalModel::LifecycleRecord,
        DataLogicalModel::LifecycleEvent => RuntimeLogicalModel::LifecycleEvent,
    }
}

pub(in crate::engine) fn runtime_property_schemas(
    properties: &BTreeMap<String, rrd_contract::DataPropertySchema>,
) -> BTreeMap<String, RuntimePropertySchema> {
    properties
        .iter()
        .map(|(name, schema)| {
            (
                name.clone(),
                RuntimePropertySchema {
                    value_type: match schema.value_type {
                        DataValueType::Null => RuntimeValueType::Null,
                        DataValueType::Bool => RuntimeValueType::Bool,
                        DataValueType::Integer => RuntimeValueType::Integer,
                        DataValueType::Unsigned => RuntimeValueType::Unsigned,
                        DataValueType::Decimal => RuntimeValueType::Decimal,
                        DataValueType::String => RuntimeValueType::String,
                        DataValueType::Digest => RuntimeValueType::Digest,
                        DataValueType::List => RuntimeValueType::List,
                        DataValueType::Map => RuntimeValueType::Map,
                    },
                    required: schema.required,
                },
            )
        })
        .collect()
}

pub(in crate::engine) fn runtime_vector_value(value: &DataVectorValue) -> VectorValue {
    match value {
        DataVectorValue::Dense { values } => VectorValue::Dense {
            values: values.clone(),
        },
        DataVectorValue::Sparse {
            dimensions,
            indices,
            values,
        } => VectorValue::Sparse {
            dimensions: *dimensions,
            indices: indices.clone(),
            values: values.clone(),
        },
        DataVectorValue::MultiDense {
            dimensions,
            vectors,
        } => VectorValue::MultiDense {
            dimensions: *dimensions,
            vectors: vectors.clone(),
        },
    }
}

pub(in crate::engine) fn runtime_geo_value(value: &DataGeoValue) -> GeoValue {
    let point = |value: &DataGeoPoint| GeoPoint {
        longitude: value.longitude,
        latitude: value.latitude,
    };
    match value {
        DataGeoValue::Point { point: value } => GeoValue::Point {
            point: point(value),
        },
        DataGeoValue::BoundingBox {
            southwest,
            northeast,
        } => GeoValue::BoundingBox {
            southwest: point(southwest),
            northeast: point(northeast),
        },
    }
}

pub(in crate::engine) fn core_contract(error: rrd_core::Error) -> ServiceError {
    ServiceError::Contract(error.to_string())
}

pub(in crate::engine) fn core_changefeed(error: rrd_core::Error) -> ServiceError {
    ServiceError::Changefeed(error.to_string())
}

pub(in crate::engine) fn public_claim(
    mutation: &TransactionMutation,
    session_id: &CorrelationId,
) -> Result<Claim> {
    let TransactionMutation::AssertClaim {
        subject,
        predicate,
        object,
        valid_from,
        tx_time,
        producer,
        confidence,
    } = mutation
    else {
        return Err(ServiceError::Contract(
            "expected an assert_claim mutation".into(),
        ));
    };
    let mut claim = Claim::new(
        Subject::new(subject.as_str()).map_err(core_contract)?,
        Predicate::new(predicate.as_str()).map_err(core_contract)?,
        object,
        *valid_from,
        *tx_time,
        Producer {
            actor: producer.as_str().into(),
            on_behalf_of: None,
            session: Some(session_id.as_str().into()),
        },
    );
    claim.confidence = *confidence;
    Ok(claim)
}

pub(in crate::engine) fn runtime_receipt(
    transaction_id: &CorrelationId,
    operation_sha256: &str,
    accepted: &rrd_core::RuntimeCommitOutcome,
    idempotent_replay: bool,
) -> CommitReceipt {
    let claim_count = accepted
        .last_claim_sequence
        .zip(accepted.first_claim_sequence)
        .map_or(0, |(last, first)| last - first + 1);
    CommitReceipt {
        transaction_id: transaction_id.clone(),
        operation_sha256: operation_sha256.into(),
        first_claim_sequence: accepted.first_claim_sequence.unwrap_or(0),
        last_claim_sequence: accepted.last_claim_sequence.unwrap_or(0),
        mutation_count: accepted.count as u64,
        runtime_commit_sha256: Some(accepted.commit_id.clone()),
        first_runtime_cursor: Some(accepted.first_cursor),
        last_runtime_cursor: Some(accepted.last_cursor),
        claim_mutation_count: Some(claim_count),
        idempotent_replay,
    }
}
