//! Reversible typed Arrow representation of one immutable RRD query snapshot.

use crate::{Error, QueryFieldTypes, QueryRow, Result};
use datafusion::arrow::{
    array::{Array, ArrayRef, BinaryArray, BooleanArray, Int64Array, StringArray, UInt64Array},
    datatypes::{DataType, Field, Schema, SchemaRef},
    record_batch::RecordBatch,
};
use rrd_core::{RuntimeValue, RuntimeValueType};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

const IDENTITY: &str = "__rrd_identity";
const TYPE_METADATA: &str = "rrd.runtime_value_type";
const UNION_METADATA: &str = "rrd.runtime_union";
const READ_MANIFEST_METADATA: &str = "rrd.read_manifest";
const SCOPE_METADATA: &str = "rrd.scope";
const VALID_AT_METADATA: &str = "rrd.valid_at";
const KNOWN_AT_CURSOR_METADATA: &str = "rrd.known_at_cursor";
const SOURCE_CURSOR_METADATA: &str = "rrd.source_cursor";
const SCHEMA_REVISION_METADATA: &str = "rrd.schema_revision";

/// Stable RRD coordinates carried by every Arrow snapshot schema. Arrow is a
/// rebuildable execution representation; these coordinates bind it back to
/// the authoritative catalogue and authenticated read stamp.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArrowReadStamp {
    pub read_manifest: String,
    pub scope: String,
    pub valid_at: u64,
    pub known_at_cursor: u64,
    pub source_cursor: u64,
    pub schema_revision: u64,
}

#[derive(Debug, Clone)]
pub struct ArrowSnapshot {
    pub stamp: ArrowReadStamp,
    pub schema: SchemaRef,
    pub batches: Vec<RecordBatch>,
    pub rows: usize,
    pub max_batch_rows: usize,
}

impl ArrowSnapshot {
    pub fn validate(&self) -> Result<()> {
        if self.max_batch_rows == 0 {
            return Err(Error::Integrity(
                "Arrow snapshot has a zero batch-row bound".into(),
            ));
        }
        validate_stamp_metadata(self.schema.metadata(), &self.stamp)?;
        if self
            .batches
            .iter()
            .any(|batch| batch.schema() != self.schema || batch.num_rows() > self.max_batch_rows)
        {
            return Err(Error::Integrity(
                "Arrow snapshot batch violates its schema or row bound".into(),
            ));
        }
        let rows = self
            .batches
            .iter()
            .try_fold(0usize, |total, batch| total.checked_add(batch.num_rows()))
            .ok_or_else(|| Error::Integrity("Arrow snapshot row count overflow".into()))?;
        if rows != self.rows {
            return Err(Error::Integrity(format!(
                "Arrow snapshot contains {rows} rows but declares {}",
                self.rows
            )));
        }
        Ok(())
    }

    /// Resident Arrow array bytes held by the immutable snapshot. This is
    /// charged to the same query memory budget as DataFusion operators.
    pub fn resident_bytes(&self) -> Result<usize> {
        self.batches.iter().try_fold(0usize, |total, batch| {
            total
                .checked_add(batch.get_array_memory_size())
                .ok_or_else(|| Error::Budget("Arrow snapshot memory size overflow".into()))
        })
    }
}

pub fn rows_to_record_batch(rows: &[QueryRow], declared: &QueryFieldTypes) -> Result<RecordBatch> {
    let schema = row_schema(rows, declared, HashMap::new());
    rows_to_record_batch_with_schema(rows, schema)
}

pub fn rows_to_arrow_snapshot(
    rows: &[QueryRow],
    declared: &QueryFieldTypes,
    stamp: ArrowReadStamp,
    max_batch_rows: usize,
) -> Result<ArrowSnapshot> {
    if max_batch_rows == 0 {
        return Err(Error::Budget(
            "Arrow snapshot batch-row bound must be greater than zero".into(),
        ));
    }
    let schema = row_schema(rows, declared, stamp_metadata(&stamp));
    let batches = rows
        .chunks(max_batch_rows)
        .map(|rows| rows_to_record_batch_with_schema(rows, Arc::clone(&schema)))
        .collect::<Result<Vec<_>>>()?;
    let snapshot = ArrowSnapshot {
        stamp,
        schema,
        batches,
        rows: rows.len(),
        max_batch_rows,
    };
    snapshot.validate()?;
    Ok(snapshot)
}

fn row_schema(
    rows: &[QueryRow],
    declared: &QueryFieldTypes,
    metadata: HashMap<String, String>,
) -> SchemaRef {
    let mut names = declared.keys().cloned().collect::<BTreeSet<_>>();
    for row in rows {
        names.extend(row.values.keys().cloned());
    }

    let mut fields = vec![Field::new(IDENTITY, DataType::Utf8, false)];
    for name in names {
        let semantics = semantic_types(rows, declared.get(&name), &name);
        let representation = Representation::for_types(&semantics);
        fields.push(representation.field(&name));
    }
    Arc::new(Schema::new_with_metadata(fields, metadata))
}

fn rows_to_record_batch_with_schema(rows: &[QueryRow], schema: SchemaRef) -> Result<RecordBatch> {
    let mut arrays: Vec<ArrayRef> = vec![Arc::new(StringArray::from(
        rows.iter()
            .map(|row| Some(row.identity.as_str()))
            .collect::<Vec<_>>(),
    ))];
    for field in schema.fields().iter().skip(1) {
        arrays.push(Representation::from_field(field)?.array(rows, field.name())?);
    }
    RecordBatch::try_new(schema, arrays)
        .map_err(|error| Error::Execution(format!("Arrow batch construction failed: {error}")))
}

fn stamp_metadata(stamp: &ArrowReadStamp) -> HashMap<String, String> {
    HashMap::from([
        (READ_MANIFEST_METADATA.into(), stamp.read_manifest.clone()),
        (SCOPE_METADATA.into(), stamp.scope.clone()),
        (VALID_AT_METADATA.into(), stamp.valid_at.to_string()),
        (
            KNOWN_AT_CURSOR_METADATA.into(),
            stamp.known_at_cursor.to_string(),
        ),
        (
            SOURCE_CURSOR_METADATA.into(),
            stamp.source_cursor.to_string(),
        ),
        (
            SCHEMA_REVISION_METADATA.into(),
            stamp.schema_revision.to_string(),
        ),
    ])
}

fn validate_stamp_metadata(
    metadata: &HashMap<String, String>,
    stamp: &ArrowReadStamp,
) -> Result<()> {
    for (name, expected) in stamp_metadata(stamp) {
        if metadata.get(&name) != Some(&expected) {
            return Err(Error::Integrity(format!(
                "Arrow snapshot metadata {name:?} does not match its RRD read stamp"
            )));
        }
    }
    Ok(())
}

pub fn record_batch_to_rows(batch: &RecordBatch) -> Result<Vec<QueryRow>> {
    let identity = batch
        .column_by_name(IDENTITY)
        .ok_or_else(|| Error::Integrity("Arrow batch has no RRD identity column".into()))?
        .as_any()
        .downcast_ref::<StringArray>()
        .ok_or_else(|| Error::Integrity("Arrow RRD identity column is not Utf8".into()))?;
    let mut rows = (0..batch.num_rows())
        .map(|index| QueryRow {
            identity: identity.value(index).to_owned(),
            values: BTreeMap::new(),
        })
        .collect::<Vec<_>>();
    for (field, array) in batch.schema().fields().iter().zip(batch.columns()) {
        if field.name() == IDENTITY {
            continue;
        }
        let representation = Representation::from_field(field)?;
        for (index, row) in rows.iter_mut().enumerate() {
            row.values
                .insert(field.name().clone(), representation.value(array, index)?);
        }
    }
    Ok(rows)
}

fn semantic_types(
    rows: &[QueryRow],
    declared: Option<&Vec<RuntimeValueType>>,
    name: &str,
) -> BTreeSet<SemanticType> {
    let mut types = declared
        .into_iter()
        .flatten()
        .filter_map(|value| SemanticType::from_runtime_type(*value))
        .collect::<BTreeSet<_>>();
    for value in rows.iter().filter_map(|row| row.values.get(name)) {
        if let Some(value) = SemanticType::from_value(value) {
            types.insert(value);
        }
    }
    types
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum SemanticType {
    Bool,
    Integer,
    Unsigned,
    Decimal,
    String,
    Digest,
    List,
    Map,
}

impl SemanticType {
    fn from_runtime_type(value: RuntimeValueType) -> Option<Self> {
        match value {
            RuntimeValueType::Null => None,
            RuntimeValueType::Bool => Some(Self::Bool),
            RuntimeValueType::Integer => Some(Self::Integer),
            RuntimeValueType::Unsigned => Some(Self::Unsigned),
            RuntimeValueType::Decimal => Some(Self::Decimal),
            RuntimeValueType::String => Some(Self::String),
            RuntimeValueType::Digest => Some(Self::Digest),
            RuntimeValueType::List => Some(Self::List),
            RuntimeValueType::Map => Some(Self::Map),
        }
    }

    fn from_value(value: &RuntimeValue) -> Option<Self> {
        match value {
            RuntimeValue::Null => None,
            RuntimeValue::Bool(_) => Some(Self::Bool),
            RuntimeValue::Integer(_) => Some(Self::Integer),
            RuntimeValue::Unsigned(_) => Some(Self::Unsigned),
            RuntimeValue::Decimal(_) => Some(Self::Decimal),
            RuntimeValue::String(_) => Some(Self::String),
            RuntimeValue::Digest(_) => Some(Self::Digest),
            RuntimeValue::List(_) => Some(Self::List),
            RuntimeValue::Map(_) => Some(Self::Map),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Bool => "bool",
            Self::Integer => "integer",
            Self::Unsigned => "unsigned",
            Self::Decimal => "decimal",
            Self::String => "string",
            Self::Digest => "digest",
            Self::List => "list",
            Self::Map => "map",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "bool" => Some(Self::Bool),
            "integer" => Some(Self::Integer),
            "unsigned" => Some(Self::Unsigned),
            "decimal" => Some(Self::Decimal),
            "string" => Some(Self::String),
            "digest" => Some(Self::Digest),
            "list" => Some(Self::List),
            "map" => Some(Self::Map),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Representation {
    Bool,
    Integer,
    Unsigned,
    Text(SemanticType),
    Canonical(Vec<SemanticType>),
}

impl Representation {
    fn for_types(types: &BTreeSet<SemanticType>) -> Self {
        if types.len() != 1 {
            return Self::Canonical(types.iter().copied().collect());
        }
        match *types.first().expect("one type") {
            SemanticType::Bool => Self::Bool,
            SemanticType::Integer => Self::Integer,
            SemanticType::Unsigned => Self::Unsigned,
            value @ (SemanticType::Decimal | SemanticType::String | SemanticType::Digest) => {
                Self::Text(value)
            }
            value @ (SemanticType::List | SemanticType::Map) => Self::Canonical(vec![value]),
        }
    }

    fn field(&self, name: &str) -> Field {
        let (data_type, metadata) = match self {
            Self::Bool => (DataType::Boolean, type_metadata(SemanticType::Bool)),
            Self::Integer => (DataType::Int64, type_metadata(SemanticType::Integer)),
            Self::Unsigned => (DataType::UInt64, type_metadata(SemanticType::Unsigned)),
            Self::Text(semantic) => (DataType::Utf8, type_metadata(*semantic)),
            Self::Canonical(types) => (
                DataType::Binary,
                HashMap::from([(
                    UNION_METADATA.into(),
                    types
                        .iter()
                        .map(|value| value.name())
                        .collect::<Vec<_>>()
                        .join(","),
                )]),
            ),
        };
        Field::new(name, data_type, true).with_metadata(metadata)
    }

    fn from_field(field: &Field) -> Result<Self> {
        if let Some(value) = field.metadata().get(TYPE_METADATA) {
            let semantic = SemanticType::parse(value).ok_or_else(|| {
                Error::Integrity(format!("unknown Arrow semantic type {value:?}"))
            })?;
            return Ok(match semantic {
                SemanticType::Bool => Self::Bool,
                SemanticType::Integer => Self::Integer,
                SemanticType::Unsigned => Self::Unsigned,
                SemanticType::Decimal | SemanticType::String | SemanticType::Digest => {
                    Self::Text(semantic)
                }
                SemanticType::List | SemanticType::Map => Self::Canonical(vec![semantic]),
            });
        }
        let union = field
            .metadata()
            .get(UNION_METADATA)
            .ok_or_else(|| Error::Integrity("Arrow field lacks RRD type metadata".into()))?;
        let types = union
            .split(',')
            .filter(|value| !value.is_empty())
            .map(|value| {
                SemanticType::parse(value)
                    .ok_or_else(|| Error::Integrity(format!("unknown Arrow union type {value:?}")))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self::Canonical(types))
    }

    fn array(&self, rows: &[QueryRow], name: &str) -> Result<ArrayRef> {
        Ok(match self {
            Self::Bool => Arc::new(BooleanArray::from(
                rows.iter()
                    .map(|row| match row.values.get(name) {
                        Some(RuntimeValue::Bool(value)) => Ok(Some(*value)),
                        Some(RuntimeValue::Null) | None => Ok(None),
                        Some(_) => type_mismatch(name, "bool"),
                    })
                    .collect::<Result<Vec<_>>>()?,
            )),
            Self::Integer => Arc::new(Int64Array::from(
                rows.iter()
                    .map(|row| match row.values.get(name) {
                        Some(RuntimeValue::Integer(value)) => Ok(Some(*value)),
                        Some(RuntimeValue::Null) | None => Ok(None),
                        Some(_) => type_mismatch(name, "integer"),
                    })
                    .collect::<Result<Vec<_>>>()?,
            )),
            Self::Unsigned => Arc::new(UInt64Array::from(
                rows.iter()
                    .map(|row| match row.values.get(name) {
                        Some(RuntimeValue::Unsigned(value)) => Ok(Some(*value)),
                        Some(RuntimeValue::Null) | None => Ok(None),
                        Some(_) => type_mismatch(name, "unsigned"),
                    })
                    .collect::<Result<Vec<_>>>()?,
            )),
            Self::Text(semantic) => Arc::new(StringArray::from(
                rows.iter()
                    .map(|row| match (semantic, row.values.get(name)) {
                        (_, Some(RuntimeValue::Null) | None) => Ok(None),
                        (SemanticType::Decimal, Some(RuntimeValue::Decimal(value)))
                        | (SemanticType::String, Some(RuntimeValue::String(value)))
                        | (SemanticType::Digest, Some(RuntimeValue::Digest(value))) => {
                            Ok(Some(value.as_str()))
                        }
                        _ => type_mismatch(name, semantic.name()),
                    })
                    .collect::<Result<Vec<_>>>()?,
            )),
            Self::Canonical(_) => Arc::new(BinaryArray::from_opt_vec(
                rows.iter()
                    .map(|row| {
                        row.values
                            .get(name)
                            .filter(|value| !matches!(value, RuntimeValue::Null))
                            .map(serde_json::to_vec)
                            .transpose()
                    })
                    .collect::<std::result::Result<Vec<_>, _>>()?
                    .iter()
                    .map(|value| value.as_deref())
                    .collect::<Vec<_>>(),
            )),
        })
    }

    fn value(&self, array: &ArrayRef, index: usize) -> Result<RuntimeValue> {
        if array.is_null(index) {
            return Ok(RuntimeValue::Null);
        }
        match self {
            Self::Bool => downcast::<BooleanArray>(array, "Boolean")
                .map(|array| RuntimeValue::Bool(array.value(index))),
            Self::Integer => downcast::<Int64Array>(array, "Int64")
                .map(|array| RuntimeValue::Integer(array.value(index))),
            Self::Unsigned => downcast::<UInt64Array>(array, "UInt64")
                .map(|array| RuntimeValue::Unsigned(array.value(index))),
            Self::Text(semantic) => {
                let value = downcast::<StringArray>(array, "Utf8")?
                    .value(index)
                    .to_owned();
                Ok(match semantic {
                    SemanticType::Decimal => RuntimeValue::Decimal(value),
                    SemanticType::String => RuntimeValue::String(value),
                    SemanticType::Digest => RuntimeValue::Digest(value),
                    _ => unreachable!("text representation has text semantic"),
                })
            }
            Self::Canonical(_) => {
                serde_json::from_slice(downcast::<BinaryArray>(array, "Binary")?.value(index))
                    .map_err(Error::from)
            }
        }
    }
}

fn type_metadata(semantic: SemanticType) -> HashMap<String, String> {
    HashMap::from([(TYPE_METADATA.into(), semantic.name().into())])
}

fn type_mismatch<T>(name: &str, expected: &str) -> Result<T> {
    Err(Error::Integrity(format!(
        "RRD field {name:?} does not match Arrow {expected} representation"
    )))
}

fn downcast<'a, T: 'static>(array: &'a ArrayRef, expected: &str) -> Result<&'a T> {
    array.as_any().downcast_ref::<T>().ok_or_else(|| {
        Error::Integrity(format!(
            "Arrow array does not match expected {expected} type"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalar_union_and_nested_values_round_trip() {
        let rows = vec![
            QueryRow {
                identity: "record:doc:a".into(),
                values: BTreeMap::from([
                    ("active".into(), RuntimeValue::Bool(true)),
                    ("count".into(), RuntimeValue::Unsigned(7)),
                    ("title".into(), RuntimeValue::String("alpha".into())),
                    (
                        "mixed".into(),
                        RuntimeValue::List(vec![RuntimeValue::String("x".into())]),
                    ),
                ]),
            },
            QueryRow {
                identity: "record:doc:b".into(),
                values: BTreeMap::from([
                    ("active".into(), RuntimeValue::Null),
                    ("count".into(), RuntimeValue::Unsigned(9)),
                    ("title".into(), RuntimeValue::String("beta".into())),
                    (
                        "mixed".into(),
                        RuntimeValue::Map(BTreeMap::from([(
                            "key".into(),
                            RuntimeValue::Integer(1),
                        )])),
                    ),
                ]),
            },
        ];
        let batch = rows_to_record_batch(&rows, &QueryFieldTypes::new()).unwrap();
        assert_eq!(record_batch_to_rows(&batch).unwrap(), rows);

        let stamp = ArrowReadStamp {
            read_manifest: "a".repeat(64),
            scope: "instance:test".into(),
            valid_at: 100,
            known_at_cursor: 9,
            source_cursor: 8,
            schema_revision: 3,
        };
        let snapshot =
            rows_to_arrow_snapshot(&rows, &QueryFieldTypes::new(), stamp.clone(), 1).unwrap();
        assert_eq!(snapshot.batches.len(), 2);
        assert!(snapshot
            .batches
            .iter()
            .all(|batch| batch.num_rows() == 1 && batch.schema() == snapshot.schema));
        assert_eq!(
            snapshot
                .batches
                .iter()
                .flat_map(|batch| record_batch_to_rows(batch).unwrap())
                .collect::<Vec<_>>(),
            rows
        );
        let mut corrupted = snapshot;
        corrupted.stamp.known_at_cursor += 1;
        assert!(matches!(corrupted.validate(), Err(Error::Integrity(_))));
    }
}
