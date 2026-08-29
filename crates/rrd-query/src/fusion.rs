//! DataFusion execution over one immutable, stamped RRD Arrow snapshot.

use crate::{
    record_batch_to_rows, BoundFilter, ComparisonOperator, Error, Projection, QueryRow, Result,
};
use datafusion::{
    arrow::record_batch::RecordBatch,
    common::ScalarValue,
    datasource::MemTable,
    logical_expr::Expr,
    prelude::{ident, lit, SessionContext},
};
use futures::executor::block_on;
use rrd_core::RuntimeValue;
use std::sync::Arc;

const SNAPSHOT_TABLE: &str = "rrd_snapshot";
const IDENTITY: &str = "__rrd_identity";

/// Execute relational RRFlowQL operators through DataFusion's logical and
/// physical planners over the immutable Arrow snapshot captured for the read
/// stamp. MATCH is excluded because BM25 scoring owns its ranking and limit.
pub fn execute_snapshot(
    batch: RecordBatch,
    filters: &[BoundFilter],
    projection: &Projection,
    limit: Option<usize>,
    defer_projection_and_limit: bool,
) -> Result<Vec<QueryRow>> {
    block_on(execute_snapshot_async(
        batch,
        filters,
        projection,
        limit,
        defer_projection_and_limit,
    ))
}

async fn execute_snapshot_async(
    batch: RecordBatch,
    filters: &[BoundFilter],
    projection: &Projection,
    limit: Option<usize>,
    defer_projection_and_limit: bool,
) -> Result<Vec<QueryRow>> {
    let schema = batch.schema();
    let table = MemTable::try_new(schema, vec![vec![batch]]).map_err(datafusion_error)?;
    let context = SessionContext::new();
    context
        .register_table(SNAPSHOT_TABLE, Arc::new(table))
        .map_err(datafusion_error)?;
    let mut frame = context
        .table(SNAPSHOT_TABLE)
        .await
        .map_err(datafusion_error)?;

    if let Some(predicate) = filters
        .iter()
        .filter(|filter| !filter.comparison.is_match())
        .map(filter_expression)
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .reduce(Expr::and)
    {
        frame = frame.filter(predicate).map_err(datafusion_error)?;
    }

    frame = frame
        .sort(vec![ident(IDENTITY).sort(true, false)])
        .map_err(datafusion_error)?;

    if !defer_projection_and_limit {
        if let Projection::Fields(fields) = projection {
            let mut columns = Vec::with_capacity(fields.len() + 1);
            columns.push(ident(IDENTITY));
            columns.extend(fields.iter().cloned().map(ident));
            frame = frame.select(columns).map_err(datafusion_error)?;
        }
        if let Some(limit) = limit {
            frame = frame.limit(0, Some(limit)).map_err(datafusion_error)?;
        }
    }

    let batches = frame.collect().await.map_err(datafusion_error)?;
    let mut rows = Vec::new();
    for batch in batches {
        rows.extend(record_batch_to_rows(&batch)?);
    }
    Ok(rows)
}

fn filter_expression(filter: &BoundFilter) -> Result<Expr> {
    let actual = ident(&filter.field);
    if matches!(filter.value, RuntimeValue::Null) {
        return match filter.comparison {
            ComparisonOperator::Equal => Ok(actual.is_null()),
            ComparisonOperator::NotEqual => Ok(actual.is_not_null()),
            ComparisonOperator::Match => unreachable!("MATCH is not lowered to DataFusion"),
            _ => Err(Error::Binding(format!(
                "ordering comparison against null is not supported for {:?}",
                filter.field
            ))),
        };
    }

    let expected = runtime_literal(&filter.value)?;
    Ok(match filter.comparison {
        ComparisonOperator::Equal => actual.eq(expected),
        // RRFlow's reference semantics consider a null field unequal to every
        // non-null value, while SQL's three-valued logic would drop that row.
        ComparisonOperator::NotEqual => actual.clone().is_null().or(actual.not_eq(expected)),
        ComparisonOperator::LessThan => actual.lt(expected),
        ComparisonOperator::LessThanOrEqual => actual.lt_eq(expected),
        ComparisonOperator::GreaterThan => actual.gt(expected),
        ComparisonOperator::GreaterThanOrEqual => actual.gt_eq(expected),
        ComparisonOperator::Match => unreachable!("MATCH is not lowered to DataFusion"),
    })
}

fn runtime_literal(value: &RuntimeValue) -> Result<Expr> {
    Ok(match value {
        RuntimeValue::Null => unreachable!("null predicates do not require a literal"),
        RuntimeValue::Bool(value) => lit(*value),
        RuntimeValue::Integer(value) => lit(*value),
        RuntimeValue::Unsigned(value) => lit(*value),
        RuntimeValue::Decimal(value)
        | RuntimeValue::String(value)
        | RuntimeValue::Digest(value) => lit(value.clone()),
        RuntimeValue::List(_) | RuntimeValue::Map(_) => {
            lit(ScalarValue::Binary(Some(serde_json::to_vec(value)?)))
        }
    })
}

fn datafusion_error(error: impl std::fmt::Display) -> Error {
    Error::Execution(format!("DataFusion execution failed: {error}"))
}
