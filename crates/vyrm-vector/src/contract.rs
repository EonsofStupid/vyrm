use crate::FilterExpression;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use vyrm_core::{
    Error, Millis, ReadStamp, Result, RuntimeProperties, RuntimeRef, RuntimeVector, ScopeId,
    VectorValue,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScoreMetric {
    Cosine,
    Dot,
    Euclidean,
    Manhattan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MultiVectorComparator {
    /// Sum, over query rows, of the best score against any candidate row.
    MaxSim,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum SearchMode {
    Exact,
    AllowApproximate { exact_rerank: usize },
    RequireApproximate { exact_rerank: usize },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VectorQuery {
    Dense {
        values: Vec<f32>,
    },
    Sparse {
        dimensions: u32,
        indices: Vec<u32>,
        values: Vec<f32>,
    },
    MultiDense {
        dimensions: u32,
        vectors: Vec<Vec<f32>>,
        comparator: MultiVectorComparator,
    },
}

impl VectorQuery {
    pub fn validate(&self) -> Result<()> {
        let value = self.as_value();
        value.validate()?;
        Ok(())
    }

    pub(crate) fn as_value(&self) -> VectorValue {
        match self {
            Self::Dense { values } => VectorValue::Dense {
                values: values.clone(),
            },
            Self::Sparse {
                dimensions,
                indices,
                values,
            } => VectorValue::Sparse {
                dimensions: *dimensions,
                indices: indices.clone(),
                values: values.clone(),
            },
            Self::MultiDense {
                dimensions,
                vectors,
                ..
            } => VectorValue::MultiDense {
                dimensions: *dimensions,
                vectors: vectors.clone(),
            },
        }
    }

    pub fn dimensions(&self) -> usize {
        self.as_value().dimensions()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchRequest {
    pub scope: ScopeId,
    pub read: ReadStamp,
    pub valid_at: Millis,
    pub field: String,
    pub query: VectorQuery,
    pub metric: ScoreMetric,
    pub top_k: usize,
    pub mode: SearchMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<FilterExpression>,
}

impl SearchRequest {
    pub fn validate(&self) -> Result<()> {
        self.read.validate()?;
        if self.scope != self.read.scope {
            return invalid("vector request scope differs from its read stamp");
        }
        if self.field.trim().is_empty() || self.field.as_bytes().contains(&0) {
            return invalid("vector field must be non-empty and contain no NUL bytes");
        }
        if self.top_k == 0 || self.top_k > 100_000 {
            return invalid("vector top_k must be in 1..=100000");
        }
        match self.mode {
            SearchMode::Exact => {}
            SearchMode::AllowApproximate { exact_rerank }
            | SearchMode::RequireApproximate { exact_rerank } => {
                if exact_rerank < self.top_k || exact_rerank > 1_000_000 {
                    return invalid("vector exact_rerank must be in top_k..=1000000");
                }
            }
        }
        self.query.validate()?;
        if self.metric == ScoreMetric::Cosine && query_norm_is_zero(&self.query) {
            return invalid("cosine query must have non-zero norm");
        }
        if let Some(filter) = &self.filter {
            filter.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VectorCandidate {
    pub scope: ScopeId,
    pub source_cursor: u64,
    pub vector: RuntimeVector,
}

impl VectorCandidate {
    pub fn validate(&self) -> Result<()> {
        if self.source_cursor == 0 {
            return invalid("vector candidate source cursor must be greater than zero");
        }
        self.vector.validate()
    }

    pub(crate) fn filter_properties(&self) -> &RuntimeProperties {
        &self.vector.properties
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchHit {
    pub reference: RuntimeRef,
    pub subject: RuntimeRef,
    pub source_cursor: u64,
    /// Higher is always better. Euclidean/Manhattan expose negative distance.
    pub score: f64,
}

impl SearchHit {
    pub(crate) fn compare_best_first(left: &Self, right: &Self) -> Ordering {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.reference.cmp(&right.reference))
            .then_with(|| right.source_cursor.cmp(&left.source_cursor))
    }
}

fn query_norm_is_zero(query: &VectorQuery) -> bool {
    match query {
        VectorQuery::Dense { values } | VectorQuery::Sparse { values, .. } => {
            values.iter().all(|value| *value == 0.0)
        }
        VectorQuery::MultiDense { vectors, .. } => vectors
            .iter()
            .any(|vector| vector.iter().all(|value| *value == 0.0)),
    }
}

pub(crate) fn invalid<T>(reason: impl Into<String>) -> Result<T> {
    Err(Error::InvalidRuntime {
        reason: reason.into(),
    })
}
