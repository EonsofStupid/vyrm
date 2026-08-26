use super::super::*;
use super::lower::query_value;

pub(in crate::engine) fn public_runtime_change(
    change: &rrd_core::RuntimeChange,
) -> Result<RuntimeChangeSnapshot> {
    Ok(RuntimeChangeSnapshot {
        cursor: change.cursor,
        commit_sha256: change.commit_id.clone(),
        commit_ordinal: change.commit_ordinal,
        scope: change.scope.to_string(),
        at_unix_ms: change.at,
        actor: change.actor.clone(),
        mutation: match &change.mutation {
            RuntimeMutation::Claim { claim } => ChangeMutationSnapshot::Claim {
                claim: ClaimChangeSnapshot {
                    subject: claim.subject.as_str().into(),
                    predicate: claim.predicate.as_str().into(),
                    object: claim.object.clone(),
                    valid_from: claim.valid_from,
                    valid_to: claim.valid_to,
                    tx_time: claim.tx_time,
                    producer: claim.producer.actor.clone(),
                    on_behalf_of: claim.producer.on_behalf_of.clone(),
                    session: claim.producer.session.clone(),
                    confidence: claim.confidence,
                    supersedes_sha256: claim.supersedes.clone(),
                    signature: claim.signature.clone(),
                    tier: match claim.tier {
                        Tier::Local => ClaimTierSnapshot::Local,
                        Tier::Primary => ClaimTierSnapshot::Primary,
                        Tier::Tenant => ClaimTierSnapshot::Tenant,
                    },
                    promotion: match claim.promotion_state {
                        PromotionState::Unpromoted => ClaimPromotionSnapshot::Unpromoted,
                        PromotionState::Pending => ClaimPromotionSnapshot::Pending,
                        PromotionState::Promoted => ClaimPromotionSnapshot::Promoted,
                        PromotionState::Denied => ClaimPromotionSnapshot::Denied,
                    },
                },
            },
            mutation => ChangeMutationSnapshot::Data {
                mutation: public_data_mutation(mutation)?,
            },
        },
        previous_change_sha256: change.previous_digest.clone(),
        change_sha256: change.digest.clone(),
    })
}

pub(in crate::engine) fn public_data_mutation(
    mutation: &RuntimeMutation,
) -> Result<TransactionMutation> {
    Ok(match mutation {
        RuntimeMutation::Claim { .. } => {
            return Err(ServiceError::Changefeed(
                "claim must use the lossless claim snapshot".into(),
            ));
        }
        RuntimeMutation::Schema { registry } => TransactionMutation::PutSchema {
            registry: public_schema(registry)?,
        },
        RuntimeMutation::Record { record } => TransactionMutation::PutRecord {
            reference: public_change_ref(&record.reference)?,
            valid_from: record.valid_from,
            valid_to: record.valid_to,
            properties: public_properties(&record.properties)?,
        },
        RuntimeMutation::Relation { relation } => TransactionMutation::PutRelation {
            reference: public_change_ref(&relation.reference)?,
            from: public_change_ref(&relation.from)?,
            to: public_change_ref(&relation.to)?,
            valid_from: relation.valid_from,
            valid_to: relation.valid_to,
            properties: public_properties(&relation.properties)?,
        },
        RuntimeMutation::Event { event } => TransactionMutation::AppendEvent {
            kind: public_change_id(event.kind.as_str())?,
            subject: event.subject.as_ref().map(public_change_ref).transpose()?,
            properties: public_properties(&event.properties)?,
        },
        RuntimeMutation::Vector { vector } => TransactionMutation::PutVector {
            reference: public_change_ref(&vector.reference)?,
            subject: public_change_ref(&vector.subject)?,
            collection_id: None,
            vector_name: None,
            field: public_change_id(&vector.field)?,
            valid_from: vector.valid_from,
            valid_to: vector.valid_to,
            value: public_vector_value(&vector.value),
            provenance: vector
                .provenance
                .as_ref()
                .map(|value| -> Result<rrd_contract::DataEmbeddingProvenance> {
                    Ok(rrd_contract::DataEmbeddingProvenance {
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
                })
                .transpose()?,
            properties: public_properties(&vector.properties)?,
        },
        RuntimeMutation::SeriesSample { sample } => TransactionMutation::AppendSeriesSample {
            reference: public_change_ref(&sample.reference)?,
            series: public_change_ref(&sample.series)?,
            observed_at: sample.observed_at,
            value: match &sample.value {
                SeriesValue::Integer(value) => DataSeriesValue::Integer(*value),
                SeriesValue::Unsigned(value) => DataSeriesValue::Unsigned(*value),
                SeriesValue::Decimal(value) => DataSeriesValue::Decimal(value.clone()),
                SeriesValue::Bool(value) => DataSeriesValue::Bool(*value),
                SeriesValue::String(value) => DataSeriesValue::String(value.clone()),
            },
            properties: public_properties(&sample.properties)?,
        },
        RuntimeMutation::Geo { geo } => TransactionMutation::PutGeo {
            reference: public_change_ref(&geo.reference)?,
            subject: public_change_ref(&geo.subject)?,
            field: public_change_id(&geo.field)?,
            valid_from: geo.valid_from,
            valid_to: geo.valid_to,
            value: public_geo_value(&geo.value),
            properties: public_properties(&geo.properties)?,
        },
        RuntimeMutation::Object { object } => TransactionMutation::PublishObjectReference {
            reference: public_change_ref(&object.reference)?,
            subject: object.subject.as_ref().map(public_change_ref).transpose()?,
            sha256: object.sha256.clone(),
            length: object.length,
            media_type: object.media_type.clone(),
            receipt: DataObjectReceipt {
                backend: object.receipt.backend.clone(),
                key: object.receipt.key.clone(),
                version: object.receipt.version.clone(),
                etag: object.receipt.etag.clone(),
            },
            properties: public_properties(&object.properties)?,
        },
    })
}

pub(in crate::engine) fn public_change_id(value: &str) -> Result<CanonicalId> {
    CanonicalId::new(value).map_err(|error| ServiceError::Changefeed(error.to_string()))
}

pub(in crate::engine) fn public_change_ref(reference: &RuntimeRef) -> Result<DataReference> {
    Ok(DataReference {
        kind: public_change_id(reference.kind.as_str())?,
        id: public_change_id(reference.id.as_str())?,
    })
}

pub(in crate::engine) fn public_properties(
    properties: &RuntimeProperties,
) -> Result<DataProperties> {
    properties
        .iter()
        .map(|(name, value)| Ok((name.clone(), query_value(value)?)))
        .collect()
}

pub(in crate::engine) fn public_schema(
    registry: &RuntimeSchemaRegistry,
) -> Result<DataSchemaRegistry> {
    Ok(DataSchemaRegistry {
        revision: registry.revision,
        migration: registry.migration.clone(),
        records: registry
            .records
            .iter()
            .map(|(kind, schema)| {
                Ok((
                    public_change_id(kind.as_str())?,
                    DataRecordSchema {
                        properties: public_property_schemas(&schema.properties),
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
                    public_change_id(kind.as_str())?,
                    DataRelationSchema {
                        from: schema
                            .from
                            .iter()
                            .map(|kind| public_change_id(kind.as_str()))
                            .collect::<Result<_>>()?,
                        to: schema
                            .to
                            .iter()
                            .map(|kind| public_change_id(kind.as_str()))
                            .collect::<Result<_>>()?,
                        properties: public_property_schemas(&schema.properties),
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
                    public_change_id(kind.as_str())?,
                    DataEventSchema {
                        subject_required: schema.subject_required,
                        subject_types: schema
                            .subject_types
                            .iter()
                            .map(|kind| public_change_id(kind.as_str()))
                            .collect::<Result<_>>()?,
                        properties: public_property_schemas(&schema.properties),
                        allow_additional_properties: schema.allow_additional_properties,
                    },
                ))
            })
            .collect::<Result<_>>()?,
    })
}

pub(in crate::engine) fn public_property_schemas(
    properties: &BTreeMap<String, RuntimePropertySchema>,
) -> BTreeMap<String, DataPropertySchema> {
    properties
        .iter()
        .map(|(name, schema)| {
            (
                name.clone(),
                DataPropertySchema {
                    value_type: match schema.value_type {
                        RuntimeValueType::Null => DataValueType::Null,
                        RuntimeValueType::Bool => DataValueType::Bool,
                        RuntimeValueType::Integer => DataValueType::Integer,
                        RuntimeValueType::Unsigned => DataValueType::Unsigned,
                        RuntimeValueType::Decimal => DataValueType::Decimal,
                        RuntimeValueType::String => DataValueType::String,
                        RuntimeValueType::Digest => DataValueType::Digest,
                        RuntimeValueType::List => DataValueType::List,
                        RuntimeValueType::Map => DataValueType::Map,
                    },
                    required: schema.required,
                },
            )
        })
        .collect()
}

pub(in crate::engine) fn public_vector_value(value: &VectorValue) -> DataVectorValue {
    match value {
        VectorValue::Dense { values } => DataVectorValue::Dense {
            values: values.clone(),
        },
        VectorValue::Sparse {
            dimensions,
            indices,
            values,
        } => DataVectorValue::Sparse {
            dimensions: *dimensions,
            indices: indices.clone(),
            values: values.clone(),
        },
        VectorValue::MultiDense {
            dimensions,
            vectors,
        } => DataVectorValue::MultiDense {
            dimensions: *dimensions,
            vectors: vectors.clone(),
        },
    }
}

pub(in crate::engine) fn public_geo_value(value: &GeoValue) -> DataGeoValue {
    let point = |value: &GeoPoint| DataGeoPoint {
        longitude: value.longitude,
        latitude: value.latitude,
    };
    match value {
        GeoValue::Point { point: value } => DataGeoValue::Point {
            point: point(value),
        },
        GeoValue::BoundingBox {
            southwest,
            northeast,
        } => DataGeoValue::BoundingBox {
            southwest: point(southwest),
            northeast: point(northeast),
        },
    }
}
