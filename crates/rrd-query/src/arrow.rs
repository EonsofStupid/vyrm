//! Reversible typed Arrow representation of one immutable RRD query snapshot.

use crate::{Error, QueryFieldTypes, QueryRow, Result};
use datafusion::arrow::{
    array::{Array, ArrayRef, BinaryArray, BooleanArray, Int64Array, StringArray, UInt64Array},
    datatypes::{DataType, Field, Schema},
    record_batch::RecordBatch,
};
use rrd_core::{RuntimeValue, RuntimeValueType};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

const IDENTITY: &str = "__rrd_identity";
const TYPE_METADATA: &str = "rrd.runtime_value_type";
const UNION_METADATA: &str = "rrd.runtime_union";

pub fn rows_to_record_batch(rows: &[QueryRow], declared: &QueryFieldTypes) -> Result<RecordBatch> {
    let mut names = declared.keys().cloned().collect::<BTreeSet<_>>();
    for row in rows {
        names.extend(row.values.keys().cloned());
    }

    let mut fields = vec![Field::new(IDENTITY, DataType::Utf8, false)];
    let mut arrays: Vec<ArrayRef> = vec![Arc::new(StringArray::from(
        rows.iter()
            .map(|row| Some(row.identity.as_str()))
            .collect::<Vec<_>>(),
    ))];
    for name in names {
        let semantics = semantic_types(rows, declared.get(&name), &name);
        let representation = Representation::for_types(&semantics);
        fields.push(representation.field(&name));
        arrays.push(representation.array(rows, &name)?);
    }
    RecordBatch::try_new(Arc::new(Schema::new(fields)), arrays)
        .map_err(|error| Error::Execution(format!("Arrow batch construction failed: {error}")))
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
    }
}
