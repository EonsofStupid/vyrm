//! One persisted catalogue and schema contract for every logical runtime model.

use crate::{
    Error, Result, RuntimeEvent, RuntimeMutation, RuntimeProperties, RuntimeRecord,
    RuntimeRelation, RuntimeType, RuntimeValue,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeValueType {
    Null,
    Bool,
    Integer,
    Unsigned,
    Decimal,
    String,
    Digest,
    List,
    Map,
}

impl RuntimeValueType {
    pub fn matches(self, value: &RuntimeValue) -> bool {
        matches!(
            (self, value),
            (Self::Null, RuntimeValue::Null)
                | (Self::Bool, RuntimeValue::Bool(_))
                | (Self::Integer, RuntimeValue::Integer(_))
                | (Self::Unsigned, RuntimeValue::Unsigned(_))
                | (Self::Decimal, RuntimeValue::Decimal(_))
                | (Self::String, RuntimeValue::String(_))
                | (Self::Digest, RuntimeValue::Digest(_))
                | (Self::List, RuntimeValue::List(_))
                | (Self::Map, RuntimeValue::Map(_))
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimePropertySchema {
    pub value_type: RuntimeValueType,
    #[serde(default)]
    pub required: bool,
}

impl RuntimePropertySchema {
    pub const fn required(value_type: RuntimeValueType) -> Self {
        Self {
            value_type,
            required: true,
        }
    }

    pub const fn optional(value_type: RuntimeValueType) -> Self {
        Self {
            value_type,
            required: false,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeRecordSchema {
    #[serde(default)]
    pub properties: BTreeMap<String, RuntimePropertySchema>,
    #[serde(default)]
    pub allow_additional_properties: bool,
    #[serde(default)]
    pub unique_properties: BTreeSet<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeRelationSchema {
    #[serde(default)]
    pub from: BTreeSet<RuntimeType>,
    #[serde(default)]
    pub to: BTreeSet<RuntimeType>,
    #[serde(default)]
    pub properties: BTreeMap<String, RuntimePropertySchema>,
    #[serde(default)]
    pub allow_additional_properties: bool,
    #[serde(default)]
    pub unique_pair: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_outgoing: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_incoming: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeEventSchema {
    #[serde(default)]
    pub subject_required: bool,
    #[serde(default)]
    pub subject_types: BTreeSet<RuntimeType>,
    #[serde(default)]
    pub properties: BTreeMap<String, RuntimePropertySchema>,
    #[serde(default)]
    pub allow_additional_properties: bool,
}

/// Logical meaning of one table identity inside a database catalogue.
///
/// Record-like models deliberately share the canonical [`RuntimeRecord`]
/// identity. Their distinct catalogue kind prevents document, relational,
/// graph, key-value, reasoning, and lifecycle APIs from creating competing
/// persisted truths for the same table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeLogicalModel {
    Document,
    Relational,
    GraphNode,
    GraphRelation,
    KeyValue,
    Vector,
    Event,
    TimeSeries,
    Geo,
    Object,
    ReasoningClaim,
    ReasoningRecord,
    ReasoningEvent,
    LifecycleRecord,
    LifecycleEvent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeSchemaMode {
    Strict,
    Schemaless,
}

/// A single table entry in the revisioned logical catalogue.
///
/// Record, relation, and event tables keep their richer constraints in the
/// existing specialized maps below. The property contract here governs the
/// structurally typed vector, time-series, geo, and object families. In
/// schemaless mode these fields must remain empty so mode cannot be inferred or
/// contradicted by a second boolean contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeTableSchema {
    pub model: RuntimeLogicalModel,
    pub mode: RuntimeSchemaMode,
    #[serde(default)]
    pub properties: BTreeMap<String, RuntimePropertySchema>,
    #[serde(default)]
    pub allow_additional_properties: bool,
}

impl RuntimeTableSchema {
    pub fn strict(model: RuntimeLogicalModel) -> Self {
        Self {
            model,
            mode: RuntimeSchemaMode::Strict,
            properties: BTreeMap::new(),
            allow_additional_properties: false,
        }
    }

    pub fn schemaless(model: RuntimeLogicalModel) -> Self {
        Self {
            model,
            mode: RuntimeSchemaMode::Schemaless,
            properties: BTreeMap::new(),
            allow_additional_properties: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeCatalogueIdentity {
    pub namespace: RuntimeType,
    pub database: RuntimeType,
}

impl Default for RuntimeCatalogueIdentity {
    fn default() -> Self {
        Self {
            namespace: RuntimeType::new("default").expect("static namespace is valid"),
            database: RuntimeType::new("default").expect("static database is valid"),
        }
    }
}

impl RuntimeCatalogueIdentity {
    fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeSchemaRegistry {
    pub revision: u64,
    pub migration: String,
    #[serde(default, skip_serializing_if = "RuntimeCatalogueIdentity::is_default")]
    pub catalogue: RuntimeCatalogueIdentity,
    /// Empty only for the persisted pre-G03 migration form. In that form the
    /// specialized record/relation/event maps derive strict entries. Once any
    /// explicit table is published, every non-claim mutation must resolve
    /// through this map.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tables: BTreeMap<RuntimeType, RuntimeTableSchema>,
    #[serde(default)]
    pub records: BTreeMap<RuntimeType, RuntimeRecordSchema>,
    #[serde(default)]
    pub relations: BTreeMap<RuntimeType, RuntimeRelationSchema>,
    #[serde(default)]
    pub events: BTreeMap<RuntimeType, RuntimeEventSchema>,
}

impl RuntimeSchemaRegistry {
    pub fn empty(revision: u64, migration: impl Into<String>) -> Self {
        Self {
            revision,
            migration: migration.into(),
            catalogue: RuntimeCatalogueIdentity::default(),
            tables: BTreeMap::new(),
            records: BTreeMap::new(),
            relations: BTreeMap::new(),
            events: BTreeMap::new(),
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.revision == 0 {
            return invalid("runtime schema revision must be greater than zero");
        }
        if self.migration.trim().is_empty() {
            return invalid("runtime schema migration description must not be empty");
        }
        if self.tables.is_empty()
            && self.records.is_empty()
            && self.relations.is_empty()
            && self.events.is_empty()
        {
            return invalid("runtime schema must declare at least one governed type");
        }
        self.catalogue_tables()?;
        for (kind, schema) in &self.records {
            validate_property_schema(kind, &schema.properties, schema.allow_additional_properties)?;
            for property in &schema.unique_properties {
                if !schema.properties.contains_key(property) {
                    return invalid(format!(
                        "record type {kind} declares unknown unique property {property:?}"
                    ));
                }
            }
        }
        for (kind, schema) in &self.relations {
            if schema.from.is_empty() || schema.to.is_empty() {
                return invalid(format!(
                    "relation type {kind} must declare at least one from and to endpoint type"
                ));
            }
            if schema.max_outgoing == Some(0) || schema.max_incoming == Some(0) {
                return invalid(format!(
                    "relation type {kind} cardinality limits must be greater than zero"
                ));
            }
            validate_property_schema(kind, &schema.properties, schema.allow_additional_properties)?;
        }
        for (kind, schema) in &self.events {
            validate_property_schema(kind, &schema.properties, schema.allow_additional_properties)?;
        }
        Ok(())
    }

    /// Complete table catalogue, including strict entries derived for legacy
    /// record/relation/event registries. A returned map is therefore stable for
    /// diagnostics and query planning even before a scope performs its G03
    /// migration.
    pub fn catalogue_tables(&self) -> Result<BTreeMap<RuntimeType, RuntimeTableSchema>> {
        let mut tables = self.tables.clone();
        for kind in self.records.keys() {
            install_specialized_table(
                &mut tables,
                kind,
                RuntimeMutationFamily::Record,
                RuntimeLogicalModel::Relational,
            )?;
        }
        for kind in self.relations.keys() {
            install_specialized_table(
                &mut tables,
                kind,
                RuntimeMutationFamily::Relation,
                RuntimeLogicalModel::GraphRelation,
            )?;
        }
        for kind in self.events.keys() {
            install_specialized_table(
                &mut tables,
                kind,
                RuntimeMutationFamily::Event,
                RuntimeLogicalModel::Event,
            )?;
        }
        for (kind, table) in &tables {
            validate_table(kind, table)?;
            let specialized = match table.model.family() {
                RuntimeMutationFamily::Record => self.records.contains_key(kind),
                RuntimeMutationFamily::Relation => self.relations.contains_key(kind),
                RuntimeMutationFamily::Event => self.events.contains_key(kind),
                RuntimeMutationFamily::Vector
                | RuntimeMutationFamily::Series
                | RuntimeMutationFamily::Geo
                | RuntimeMutationFamily::Object
                | RuntimeMutationFamily::Claim => false,
            };
            match (table.mode, table.model.family(), specialized) {
                (
                    RuntimeSchemaMode::Strict,
                    RuntimeMutationFamily::Record
                    | RuntimeMutationFamily::Relation
                    | RuntimeMutationFamily::Event,
                    false,
                ) => {
                    return invalid(format!(
                        "strict table type {kind} lacks its specialized {:?} schema",
                        table.model.family()
                    ));
                }
                (
                    RuntimeSchemaMode::Schemaless,
                    RuntimeMutationFamily::Record
                    | RuntimeMutationFamily::Relation
                    | RuntimeMutationFamily::Event,
                    true,
                ) => {
                    return invalid(format!(
                        "schemaless table type {kind} also declares a strict specialized schema"
                    ));
                }
                _ => {}
            }
        }
        Ok(tables)
    }

    pub fn validate_objects<'a>(
        &self,
        mutations: impl IntoIterator<Item = &'a RuntimeMutation>,
        existing_records: impl IntoIterator<Item = &'a RuntimeRecord>,
        existing_relations: impl IntoIterator<Item = &'a RuntimeRelation>,
    ) -> Result<()> {
        self.validate()?;
        let tables = self.catalogue_tables()?;
        let unified = !self.tables.is_empty();
        let mutations = mutations.into_iter().collect::<Vec<_>>();
        for mutation in &mutations {
            match mutation {
                RuntimeMutation::Claim { .. } | RuntimeMutation::Schema { .. } => {}
                RuntimeMutation::Record { record } => self.validate_record(record, &tables)?,
                RuntimeMutation::Relation { relation } => {
                    self.validate_relation(relation, &tables)?
                }
                RuntimeMutation::Event { event } => self.validate_event(event, &tables)?,
                RuntimeMutation::Vector { vector } if unified => validate_structural_table(
                    "vector",
                    &vector.reference.kind,
                    &vector.properties,
                    RuntimeMutationFamily::Vector,
                    &tables,
                )?,
                RuntimeMutation::SeriesSample { sample } if unified => validate_structural_table(
                    "time-series sample",
                    &sample.reference.kind,
                    &sample.properties,
                    RuntimeMutationFamily::Series,
                    &tables,
                )?,
                RuntimeMutation::Geo { geo } if unified => validate_structural_table(
                    "geo value",
                    &geo.reference.kind,
                    &geo.properties,
                    RuntimeMutationFamily::Geo,
                    &tables,
                )?,
                RuntimeMutation::Object { object } if unified => validate_structural_table(
                    "object",
                    &object.reference.kind,
                    &object.properties,
                    RuntimeMutationFamily::Object,
                    &tables,
                )?,
                RuntimeMutation::Vector { .. }
                | RuntimeMutation::SeriesSample { .. }
                | RuntimeMutation::Geo { .. }
                | RuntimeMutation::Object { .. } => {}
            }
        }

        let mut records = existing_records
            .into_iter()
            .map(|record| (record.reference.clone(), record.clone()))
            .collect::<BTreeMap<_, _>>();
        let mut relations = existing_relations
            .into_iter()
            .map(|relation| (relation.reference.clone(), relation.clone()))
            .collect::<BTreeMap<_, _>>();
        for mutation in mutations {
            match mutation {
                RuntimeMutation::Record { record } => {
                    records.insert(record.reference.clone(), record.clone());
                }
                RuntimeMutation::Relation { relation } => {
                    relations.insert(relation.reference.clone(), relation.clone());
                }
                RuntimeMutation::Claim { .. }
                | RuntimeMutation::Event { .. }
                | RuntimeMutation::Schema { .. }
                | RuntimeMutation::Vector { .. }
                | RuntimeMutation::SeriesSample { .. }
                | RuntimeMutation::Geo { .. }
                | RuntimeMutation::Object { .. } => {}
            }
        }
        self.validate_record_uniqueness(records.values())?;
        self.validate_relation_cardinality(relations.values())?;
        Ok(())
    }

    fn validate_record(
        &self,
        record: &RuntimeRecord,
        tables: &BTreeMap<RuntimeType, RuntimeTableSchema>,
    ) -> Result<()> {
        let table = require_table(
            &record.reference.kind,
            RuntimeMutationFamily::Record,
            tables,
        )?;
        if table.mode == RuntimeSchemaMode::Schemaless {
            return Ok(());
        }
        let schema =
            self.records
                .get(&record.reference.kind)
                .ok_or_else(|| Error::InvalidRuntime {
                    reason: format!("record type {} is not registered", record.reference.kind),
                })?;
        validate_properties(
            "record",
            &record.reference.kind,
            &record.properties,
            &schema.properties,
            schema.allow_additional_properties,
        )
    }

    fn validate_relation(
        &self,
        relation: &RuntimeRelation,
        tables: &BTreeMap<RuntimeType, RuntimeTableSchema>,
    ) -> Result<()> {
        let table = require_table(
            &relation.reference.kind,
            RuntimeMutationFamily::Relation,
            tables,
        )?;
        if table.mode == RuntimeSchemaMode::Schemaless {
            return Ok(());
        }
        let schema = self
            .relations
            .get(&relation.reference.kind)
            .ok_or_else(|| Error::InvalidRuntime {
                reason: format!(
                    "relation type {} is not registered",
                    relation.reference.kind
                ),
            })?;
        if !schema.from.contains(&relation.from.kind) || !schema.to.contains(&relation.to.kind) {
            return invalid(format!(
                "relation type {} rejects endpoint {} -> {}",
                relation.reference.kind, relation.from.kind, relation.to.kind
            ));
        }
        validate_properties(
            "relation",
            &relation.reference.kind,
            &relation.properties,
            &schema.properties,
            schema.allow_additional_properties,
        )
    }

    fn validate_event(
        &self,
        event: &RuntimeEvent,
        tables: &BTreeMap<RuntimeType, RuntimeTableSchema>,
    ) -> Result<()> {
        let table = require_table(&event.kind, RuntimeMutationFamily::Event, tables)?;
        if table.mode == RuntimeSchemaMode::Schemaless {
            return Ok(());
        }
        let schema = self
            .events
            .get(&event.kind)
            .ok_or_else(|| Error::InvalidRuntime {
                reason: format!("event type {} is not registered", event.kind),
            })?;
        if schema.subject_required && event.subject.is_none() {
            return invalid(format!("event type {} requires a subject", event.kind));
        }
        if let Some(subject) = &event.subject {
            if !schema.subject_types.is_empty() && !schema.subject_types.contains(&subject.kind) {
                return invalid(format!(
                    "event type {} rejects subject type {}",
                    event.kind, subject.kind
                ));
            }
        }
        validate_properties(
            "event",
            &event.kind,
            &event.properties,
            &schema.properties,
            schema.allow_additional_properties,
        )
    }

    fn validate_record_uniqueness<'a>(
        &self,
        records: impl IntoIterator<Item = &'a RuntimeRecord>,
    ) -> Result<()> {
        let records = records.into_iter().collect::<Vec<_>>();
        for (kind, schema) in &self.records {
            for property in &schema.unique_properties {
                let candidates = records
                    .iter()
                    .filter(|record| &record.reference.kind == kind)
                    .filter_map(|record| {
                        record
                            .properties
                            .get(property)
                            .map(|value| (*record, value))
                    })
                    .collect::<Vec<_>>();
                for (index, (left, value)) in candidates.iter().enumerate() {
                    if candidates[index + 1..].iter().any(|(right, other)| {
                        *value == *other
                            && windows_overlap(
                                left.valid_from,
                                left.valid_to,
                                right.valid_from,
                                right.valid_to,
                            )
                    }) {
                        return invalid(format!(
                            "record type {kind} property {property:?} is not unique for overlapping validity windows"
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    fn validate_relation_cardinality<'a>(
        &self,
        relations: impl IntoIterator<Item = &'a RuntimeRelation>,
    ) -> Result<()> {
        let relations = relations.into_iter().collect::<Vec<_>>();
        for (kind, schema) in &self.relations {
            let typed = relations
                .iter()
                .filter(|relation| &relation.reference.kind == kind)
                .copied()
                .collect::<Vec<_>>();
            if schema.unique_pair {
                let mut groups = BTreeMap::new();
                for relation in &typed {
                    groups
                        .entry((relation.from.clone(), relation.to.clone()))
                        .or_insert_with(Vec::new)
                        .push(*relation);
                }
                for ((from, to), group) in groups {
                    if max_overlap(&group) > 1 {
                        return invalid(format!(
                            "relation type {kind} requires a unique pair for {}/{} -> {}/{}",
                            from.kind, from.id, to.kind, to.id
                        ));
                    }
                }
            }
            if let Some(limit) = schema.max_outgoing {
                let mut groups = BTreeMap::new();
                for relation in &typed {
                    groups
                        .entry(relation.from.clone())
                        .or_insert_with(Vec::new)
                        .push(*relation);
                }
                for (from, group) in groups {
                    if max_overlap(&group) > limit {
                        return invalid(format!(
                            "relation type {kind} exceeds max_outgoing={limit} at {}/{}",
                            from.kind, from.id
                        ));
                    }
                }
            }
            if let Some(limit) = schema.max_incoming {
                let mut groups = BTreeMap::new();
                for relation in &typed {
                    groups
                        .entry(relation.to.clone())
                        .or_insert_with(Vec::new)
                        .push(*relation);
                }
                for (to, group) in groups {
                    if max_overlap(&group) > limit {
                        return invalid(format!(
                            "relation type {kind} exceeds max_incoming={limit} at {}/{}",
                            to.kind, to.id
                        ));
                    }
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimeMutationFamily {
    Record,
    Relation,
    Event,
    Vector,
    Series,
    Geo,
    Object,
    Claim,
}

impl RuntimeLogicalModel {
    fn family(self) -> RuntimeMutationFamily {
        match self {
            Self::Document
            | Self::Relational
            | Self::GraphNode
            | Self::KeyValue
            | Self::ReasoningRecord
            | Self::LifecycleRecord => RuntimeMutationFamily::Record,
            Self::GraphRelation => RuntimeMutationFamily::Relation,
            Self::Event | Self::ReasoningEvent | Self::LifecycleEvent => {
                RuntimeMutationFamily::Event
            }
            Self::Vector => RuntimeMutationFamily::Vector,
            Self::TimeSeries => RuntimeMutationFamily::Series,
            Self::Geo => RuntimeMutationFamily::Geo,
            Self::Object => RuntimeMutationFamily::Object,
            Self::ReasoningClaim => RuntimeMutationFamily::Claim,
        }
    }
}

fn install_specialized_table(
    tables: &mut BTreeMap<RuntimeType, RuntimeTableSchema>,
    kind: &RuntimeType,
    family: RuntimeMutationFamily,
    inferred_model: RuntimeLogicalModel,
) -> Result<()> {
    if let Some(table) = tables.get(kind) {
        if table.model.family() != family {
            return invalid(format!(
                "table type {kind} is catalogued as {:?} but also has a {:?} schema",
                table.model, family
            ));
        }
        return Ok(());
    }
    tables.insert(kind.clone(), RuntimeTableSchema::strict(inferred_model));
    Ok(())
}

fn validate_table(kind: &RuntimeType, table: &RuntimeTableSchema) -> Result<()> {
    validate_property_schema(kind, &table.properties, table.allow_additional_properties)?;
    if table.mode == RuntimeSchemaMode::Schemaless
        && (!table.properties.is_empty() || table.allow_additional_properties)
    {
        return invalid(format!(
            "schemaless table type {kind} cannot also declare a strict property contract"
        ));
    }
    if matches!(
        table.model.family(),
        RuntimeMutationFamily::Record
            | RuntimeMutationFamily::Relation
            | RuntimeMutationFamily::Event
    ) && (!table.properties.is_empty() || table.allow_additional_properties)
    {
        return invalid(format!(
            "table type {kind} must keep record/relation/event properties in its specialized schema"
        ));
    }
    Ok(())
}

fn require_table<'a>(
    kind: &RuntimeType,
    family: RuntimeMutationFamily,
    tables: &'a BTreeMap<RuntimeType, RuntimeTableSchema>,
) -> Result<&'a RuntimeTableSchema> {
    let table = tables.get(kind).ok_or_else(|| Error::InvalidRuntime {
        reason: format!("{family:?} table type {kind} is not registered in the catalogue"),
    })?;
    if table.model.family() != family {
        return invalid(format!(
            "table type {kind} is catalogued as {:?}, not {family:?}",
            table.model
        ));
    }
    Ok(table)
}

fn validate_structural_table(
    object: &str,
    kind: &RuntimeType,
    properties: &RuntimeProperties,
    family: RuntimeMutationFamily,
    tables: &BTreeMap<RuntimeType, RuntimeTableSchema>,
) -> Result<()> {
    let table = require_table(kind, family, tables)?;
    if table.mode == RuntimeSchemaMode::Schemaless {
        return Ok(());
    }
    validate_properties(
        object,
        kind,
        properties,
        &table.properties,
        table.allow_additional_properties,
    )
}

fn validate_property_schema(
    kind: &RuntimeType,
    properties: &BTreeMap<String, RuntimePropertySchema>,
    _allow_additional: bool,
) -> Result<()> {
    for name in properties.keys() {
        if name.is_empty() || name.as_bytes().contains(&0) {
            return invalid(format!(
                "type {kind} contains invalid property name {name:?}"
            ));
        }
    }
    Ok(())
}

fn validate_properties(
    object: &str,
    kind: &RuntimeType,
    actual: &RuntimeProperties,
    declared: &BTreeMap<String, RuntimePropertySchema>,
    allow_additional: bool,
) -> Result<()> {
    for (name, property) in declared {
        match actual.get(name) {
            Some(value) if property.value_type.matches(value) => {}
            Some(_) => {
                return invalid(format!(
                    "{object} type {kind} property {name:?} has the wrong value type"
                ));
            }
            None if property.required => {
                return invalid(format!(
                    "{object} type {kind} is missing required property {name:?}"
                ));
            }
            None => {}
        }
    }
    if !allow_additional {
        if let Some(name) = actual.keys().find(|name| !declared.contains_key(*name)) {
            return invalid(format!(
                "{object} type {kind} contains undeclared property {name:?}"
            ));
        }
    }
    Ok(())
}

fn max_overlap(relations: &[&RuntimeRelation]) -> u64 {
    let mut points = Vec::with_capacity(relations.len() * 2);
    for relation in relations {
        points.push((relation.valid_from, 1_i8));
        if let Some(end) = relation.valid_to {
            points.push((end, -1_i8));
        }
    }
    // Half-open intervals: ends are processed before starts at the same time.
    points.sort_by_key(|(at, delta)| (*at, *delta));
    let mut current = 0_i64;
    let mut maximum = 0_i64;
    for (_, delta) in points {
        current += i64::from(delta);
        maximum = maximum.max(current);
    }
    maximum as u64
}

fn windows_overlap(
    left_from: u64,
    left_to: Option<u64>,
    right_from: u64,
    right_to: Option<u64>,
) -> bool {
    left_from < right_to.unwrap_or(u64::MAX) && right_from < left_to.unwrap_or(u64::MAX)
}

fn invalid<T>(reason: impl Into<String>) -> Result<T> {
    Err(Error::InvalidRuntime {
        reason: reason.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RuntimeId, RuntimeRef};

    fn reference(kind: &str, id: &str) -> RuntimeRef {
        RuntimeRef {
            kind: RuntimeType::new(kind).unwrap(),
            id: RuntimeId::new(id).unwrap(),
        }
    }

    #[test]
    fn required_properties_and_endpoint_types_fail_closed() {
        let mut registry = RuntimeSchemaRegistry::empty(1, "bootstrap");
        registry.records.insert(
            RuntimeType::new("prompt").unwrap(),
            RuntimeRecordSchema {
                properties: BTreeMap::from([(
                    "text".into(),
                    RuntimePropertySchema::required(RuntimeValueType::String),
                )]),
                ..RuntimeRecordSchema::default()
            },
        );
        registry.relations.insert(
            RuntimeType::new("caused").unwrap(),
            RuntimeRelationSchema {
                from: BTreeSet::from([RuntimeType::new("prompt").unwrap()]),
                to: BTreeSet::from([RuntimeType::new("prompt").unwrap()]),
                unique_pair: true,
                ..RuntimeRelationSchema::default()
            },
        );
        let record = RuntimeRecord {
            reference: reference("prompt", "p1"),
            valid_from: 1,
            valid_to: None,
            properties: RuntimeProperties::new(),
        };
        assert!(registry
            .validate_objects(
                [&RuntimeMutation::Record { record }],
                std::iter::empty(),
                std::iter::empty()
            )
            .unwrap_err()
            .to_string()
            .contains("missing required property"));
    }

    #[test]
    fn cardinality_is_measured_across_temporal_overlap() {
        let mut registry = RuntimeSchemaRegistry::empty(1, "bootstrap");
        registry.records.insert(
            RuntimeType::new("prompt").unwrap(),
            RuntimeRecordSchema::default(),
        );
        registry.relations.insert(
            RuntimeType::new("next").unwrap(),
            RuntimeRelationSchema {
                from: BTreeSet::from([RuntimeType::new("prompt").unwrap()]),
                to: BTreeSet::from([RuntimeType::new("prompt").unwrap()]),
                max_outgoing: Some(1),
                ..RuntimeRelationSchema::default()
            },
        );
        let relation = |id: &str, to: &str, from: u64, until: Option<u64>| RuntimeRelation {
            reference: reference("next", id),
            from: reference("prompt", "p1"),
            to: reference("prompt", to),
            valid_from: from,
            valid_to: until,
            properties: RuntimeProperties::new(),
        };
        let first = relation("a", "p2", 1, Some(5));
        let non_overlapping = relation("b", "p3", 5, None);
        registry
            .validate_objects(
                std::iter::empty(),
                std::iter::empty(),
                [&first, &non_overlapping],
            )
            .unwrap();
        let overlapping = relation("c", "p4", 4, None);
        assert!(registry
            .validate_objects(
                std::iter::empty(),
                std::iter::empty(),
                [&first, &overlapping]
            )
            .is_err());
    }

    #[test]
    fn one_catalogue_names_every_logical_model_and_schema_mode() {
        let mut registry = RuntimeSchemaRegistry::empty(1, "freeze unified catalogue");
        registry.catalogue = RuntimeCatalogueIdentity {
            namespace: RuntimeType::new("project").unwrap(),
            database: RuntimeType::new("runtime").unwrap(),
        };
        let models = [
            ("documents", RuntimeLogicalModel::Document),
            ("rows", RuntimeLogicalModel::Relational),
            ("nodes", RuntimeLogicalModel::GraphNode),
            ("edges", RuntimeLogicalModel::GraphRelation),
            ("entries", RuntimeLogicalModel::KeyValue),
            ("vectors", RuntimeLogicalModel::Vector),
            ("events", RuntimeLogicalModel::Event),
            ("samples", RuntimeLogicalModel::TimeSeries),
            ("places", RuntimeLogicalModel::Geo),
            ("objects", RuntimeLogicalModel::Object),
            ("claims", RuntimeLogicalModel::ReasoningClaim),
            ("reasoning", RuntimeLogicalModel::ReasoningRecord),
            ("reasoning_events", RuntimeLogicalModel::ReasoningEvent),
            ("lifecycles", RuntimeLogicalModel::LifecycleRecord),
            ("lifecycle_events", RuntimeLogicalModel::LifecycleEvent),
        ];
        for (kind, model) in models {
            registry.tables.insert(
                RuntimeType::new(kind).unwrap(),
                RuntimeTableSchema::schemaless(model),
            );
        }
        registry.validate().unwrap();
        assert_eq!(registry.catalogue_tables().unwrap().len(), models.len());
        assert_eq!(
            registry.tables[&RuntimeType::new("documents").unwrap()].mode,
            RuntimeSchemaMode::Schemaless
        );
    }

    #[test]
    fn strict_and_schemaless_or_conflicting_model_families_cannot_be_confused() {
        let prompt = RuntimeType::new("prompt").unwrap();
        let mut registry = RuntimeSchemaRegistry::empty(1, "reject ambiguous schema");
        registry
            .records
            .insert(prompt.clone(), RuntimeRecordSchema::default());
        registry.tables.insert(
            prompt.clone(),
            RuntimeTableSchema::schemaless(RuntimeLogicalModel::Document),
        );
        assert!(registry
            .validate()
            .unwrap_err()
            .to_string()
            .contains("schemaless"));

        registry.tables.insert(
            prompt,
            RuntimeTableSchema::strict(RuntimeLogicalModel::GraphRelation),
        );
        assert!(registry
            .validate()
            .unwrap_err()
            .to_string()
            .contains("also has a Record schema"));
    }
}
