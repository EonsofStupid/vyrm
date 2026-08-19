use crate::{Catalog, Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use vyrm_core::{digest, ReadStamp, RuntimeSchemaRegistry, RuntimeValue, RuntimeValueType};
use vyrm_ql::{CursorExpr, Projection, Query, Source, TimeExpr, ValueExpr, QUERY_CONTRACT_VERSION};

pub type Parameters = BTreeMap<String, RuntimeValue>;
type FieldTypes = Vec<RuntimeValueType>;
type FieldCatalog = BTreeMap<String, FieldTypes>;
type BuiltinFields = &'static [(&'static str, &'static [RuntimeValueType])];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoundFilter {
    pub field: String,
    pub value: RuntimeValue,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoundQuery {
    pub contract_version: u16,
    pub read: ReadStamp,
    pub source: Source,
    pub valid_at: u64,
    pub known_at_cursor: u64,
    pub filters: Vec<BoundFilter>,
    pub projection: Projection,
    pub limit: Option<usize>,
    pub explain_contract: bool,
    pub schema_revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operator", rename_all = "snake_case")]
pub enum LogicalOperator {
    Scan { source: Source },
    Temporal { valid_at: u64, known_at_cursor: u64 },
    Filter { predicates: Vec<BoundFilter> },
    Project { projection: Projection },
    Limit { rows: usize },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogicalPlan {
    pub contract_version: u16,
    pub read: ReadStamp,
    pub schema_revision: u64,
    pub operators: Vec<LogicalOperator>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operator", rename_all = "snake_case")]
pub enum PhysicalOperator {
    AuthoritativeLogScan {
        through_cursor: u64,
        exact: bool,
        stable_order: String,
    },
    ReferenceEvaluate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidatePath {
    pub name: String,
    pub selected: bool,
    pub exact: bool,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionContract {
    pub read_manifest: String,
    pub scope: String,
    pub valid_at: u64,
    pub known_at_cursor: u64,
    pub schema_revision: u64,
    pub exact: bool,
    pub deterministic_order: String,
    pub network_required: bool,
    pub gpu_required: bool,
    pub authorization_boundary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanExplanation {
    pub contract: ExecutionContract,
    pub candidates: Vec<CandidatePath>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhysicalPlan {
    pub logical: LogicalPlan,
    pub operators: Vec<PhysicalOperator>,
    pub explanation: PlanExplanation,
    pub digest: String,
}

pub fn bind(query: &Query, parameters: &Parameters, catalog: &Catalog) -> Result<BoundQuery> {
    query
        .validate()
        .map_err(|error| Error::Binding(error.to_string()))?;
    let valid_at = resolve_u64_time(&query.temporal.valid_at, parameters, "valid time")?;
    let known_at_cursor = match &query.temporal.known_at {
        CursorExpr::Head => catalog.read.commit_cursor,
        CursorExpr::Literal(value) => *value,
        CursorExpr::Parameter(name) => parameter_u64(parameters, name, "known cursor")?,
    };
    if known_at_cursor > catalog.read.commit_cursor {
        return Err(Error::Binding(format!(
            "known cursor {known_at_cursor} exceeds captured head {}",
            catalog.read.commit_cursor
        )));
    }
    let schema = catalog.schema_at(known_at_cursor).ok_or_else(|| {
        Error::Binding(format!(
            "no schema was visible at known cursor {known_at_cursor}"
        ))
    })?;
    let fields = fields_for_source(&query.source, schema)?;
    for filter in &query.filters {
        let accepted = ensure_field(&filter.field, &fields)?;
        if let ValueExpr::Literal(value) = &filter.value {
            ensure_value_type(&filter.field, value, accepted)?;
        }
    }
    if let Projection::Fields(projected) = &query.projection {
        for field in projected {
            let _ = ensure_field(field, &fields)?;
        }
    }
    let filters = query
        .filters
        .iter()
        .map(|filter| {
            let value = match &filter.value {
                ValueExpr::Literal(value) => value.clone(),
                ValueExpr::Parameter(name) => parameters
                    .get(name)
                    .cloned()
                    .ok_or_else(|| Error::Binding(format!("missing query parameter ${name}")))?,
            };
            ensure_value_type(
                &filter.field,
                &value,
                fields
                    .get(&filter.field)
                    .expect("filter field was checked before parameter resolution"),
            )?;
            Ok(BoundFilter {
                field: filter.field.clone(),
                value,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(BoundQuery {
        contract_version: QUERY_CONTRACT_VERSION,
        read: catalog.read.clone(),
        source: query.source.clone(),
        valid_at,
        known_at_cursor,
        filters,
        projection: query.projection.clone(),
        limit: query.limit,
        explain_contract: query.explain_contract,
        schema_revision: schema.revision,
    })
}

pub fn plan(bound: &BoundQuery) -> Result<PhysicalPlan> {
    let mut operators = vec![
        LogicalOperator::Scan {
            source: bound.source.clone(),
        },
        LogicalOperator::Temporal {
            valid_at: bound.valid_at,
            known_at_cursor: bound.known_at_cursor,
        },
    ];
    if !bound.filters.is_empty() {
        operators.push(LogicalOperator::Filter {
            predicates: bound.filters.clone(),
        });
    }
    operators.push(LogicalOperator::Project {
        projection: bound.projection.clone(),
    });
    if let Some(rows) = bound.limit {
        operators.push(LogicalOperator::Limit { rows });
    }
    let logical = LogicalPlan {
        contract_version: bound.contract_version,
        read: bound.read.clone(),
        schema_revision: bound.schema_revision,
        operators,
    };
    let explanation = PlanExplanation {
        contract: ExecutionContract {
            read_manifest: bound.read.manifest_id.clone(),
            scope: bound.read.scope.to_string(),
            valid_at: bound.valid_at,
            known_at_cursor: bound.known_at_cursor,
            schema_revision: bound.schema_revision,
            exact: true,
            deterministic_order: "identity_ascending".into(),
            network_required: false,
            gpu_required: false,
            authorization_boundary: format!("scope:{}", bound.read.scope),
        },
        candidates: vec![
            CandidatePath {
                name: "authoritative_log_scan".into(),
                selected: true,
                exact: true,
                reason: "replays the immutable log at the captured read stamp".into(),
            },
            CandidatePath {
                name: "derived_projection".into(),
                selected: false,
                exact: false,
                reason: "rejected: no projection generation and grounding proof supplied".into(),
            },
        ],
    };
    let physical_operators = vec![
        PhysicalOperator::AuthoritativeLogScan {
            through_cursor: bound.known_at_cursor,
            exact: true,
            stable_order: "global_cursor".into(),
        },
        PhysicalOperator::ReferenceEvaluate,
    ];
    let digest = plan_digest(&logical, &physical_operators, &explanation)?;
    Ok(PhysicalPlan {
        logical,
        operators: physical_operators,
        explanation,
        digest,
    })
}

impl PhysicalPlan {
    pub fn verify(&self) -> Result<()> {
        if self.logical.contract_version != QUERY_CONTRACT_VERSION {
            return Err(Error::Integrity(format!(
                "unsupported query contract version {}",
                self.logical.contract_version
            )));
        }
        self.logical
            .read
            .validate()
            .map_err(|error| Error::Integrity(error.to_string()))?;
        let expected = plan_digest(&self.logical, &self.operators, &self.explanation)?;
        if self.digest != expected {
            return Err(Error::Integrity(
                "physical plan digest does not match its contract".into(),
            ));
        }
        if !self.explanation.contract.exact {
            return Err(Error::Integrity(
                "reference executor requires an exact plan".into(),
            ));
        }
        Ok(())
    }
}

fn plan_digest(
    logical: &LogicalPlan,
    physical: &[PhysicalOperator],
    explanation: &PlanExplanation,
) -> Result<String> {
    let bytes = serde_json::to_vec(&(logical, physical, explanation))?;
    Ok(digest::sha256_hex(&bytes))
}

fn resolve_u64_time(value: &TimeExpr, parameters: &Parameters, label: &str) -> Result<u64> {
    match value {
        TimeExpr::Literal(value) => Ok(*value),
        TimeExpr::Parameter(name) => parameter_u64(parameters, name, label),
    }
}

fn parameter_u64(parameters: &Parameters, name: &str, label: &str) -> Result<u64> {
    match parameters.get(name) {
        Some(RuntimeValue::Unsigned(value)) => Ok(*value),
        Some(_) => Err(Error::Binding(format!(
            "{label} parameter ${name} must be unsigned"
        ))),
        None => Err(Error::Binding(format!("missing query parameter ${name}"))),
    }
}

fn ensure_field<'a>(field: &str, fields: &'a FieldCatalog) -> Result<&'a [RuntimeValueType]> {
    fields.get(field).map(Vec::as_slice).ok_or_else(|| {
        Error::Binding(format!(
            "field {field:?} is not present in the selected source"
        ))
    })
}

fn ensure_value_type(
    field: &str,
    value: &RuntimeValue,
    accepted: &[RuntimeValueType],
) -> Result<()> {
    if accepted.iter().any(|expected| expected.matches(value)) {
        Ok(())
    } else {
        Err(Error::Binding(format!(
            "filter value for {field:?} does not match accepted types {accepted:?}"
        )))
    }
}

fn fields_for_source(source: &Source, schema: &RuntimeSchemaRegistry) -> Result<FieldCatalog> {
    let (builtins, properties): (BuiltinFields, Vec<(String, RuntimeValueType)>) = match source {
        Source::Record { kind } => {
            let definition = schema.records.get(kind).ok_or_else(|| {
                Error::Binding(format!(
                    "record type {kind} is not registered at this cursor"
                ))
            })?;
            (
                &[
                    ("id", &[RuntimeValueType::String]),
                    ("kind", &[RuntimeValueType::String]),
                    ("valid_from", &[RuntimeValueType::Unsigned]),
                    (
                        "valid_to",
                        &[RuntimeValueType::Null, RuntimeValueType::Unsigned],
                    ),
                ],
                definition
                    .properties
                    .iter()
                    .map(|(name, rule)| (name.clone(), rule.value_type))
                    .collect(),
            )
        }
        Source::Relation { kind } => {
            let definition = schema.relations.get(kind).ok_or_else(|| {
                Error::Binding(format!(
                    "relation type {kind} is not registered at this cursor"
                ))
            })?;
            (
                &[
                    ("id", &[RuntimeValueType::String]),
                    ("kind", &[RuntimeValueType::String]),
                    ("from_kind", &[RuntimeValueType::String]),
                    ("from_id", &[RuntimeValueType::String]),
                    ("to_kind", &[RuntimeValueType::String]),
                    ("to_id", &[RuntimeValueType::String]),
                    ("valid_from", &[RuntimeValueType::Unsigned]),
                    (
                        "valid_to",
                        &[RuntimeValueType::Null, RuntimeValueType::Unsigned],
                    ),
                ],
                definition
                    .properties
                    .iter()
                    .map(|(name, rule)| (name.clone(), rule.value_type))
                    .collect(),
            )
        }
        Source::Event { kind } => {
            let definition = schema.events.get(kind).ok_or_else(|| {
                Error::Binding(format!(
                    "event type {kind} is not registered at this cursor"
                ))
            })?;
            (
                &[
                    ("cursor", &[RuntimeValueType::Unsigned]),
                    ("kind", &[RuntimeValueType::String]),
                    (
                        "subject_kind",
                        &[RuntimeValueType::Null, RuntimeValueType::String],
                    ),
                    (
                        "subject_id",
                        &[RuntimeValueType::Null, RuntimeValueType::String],
                    ),
                    ("at", &[RuntimeValueType::Unsigned]),
                    ("actor", &[RuntimeValueType::String]),
                ],
                definition
                    .properties
                    .iter()
                    .map(|(name, rule)| (name.clone(), rule.value_type))
                    .collect(),
            )
        }
        Source::Claim { .. } => (
            &[
                ("subject", &[RuntimeValueType::String]),
                ("predicate", &[RuntimeValueType::String]),
                ("object", &[RuntimeValueType::String]),
                ("valid_from", &[RuntimeValueType::Unsigned]),
                (
                    "valid_to",
                    &[RuntimeValueType::Null, RuntimeValueType::Unsigned],
                ),
                ("tx_time", &[RuntimeValueType::Unsigned]),
                ("actor", &[RuntimeValueType::String]),
            ],
            Vec::new(),
        ),
    };
    Ok(builtins
        .iter()
        .map(|(field, accepted)| ((*field).to_owned(), accepted.to_vec()))
        .chain(
            properties
                .into_iter()
                .map(|(field, accepted)| (field, vec![accepted])),
        )
        .collect())
}
