//! DataFusion execution over one immutable, stamped RRD Arrow snapshot.

use crate::{
    record_batch_to_rows, ArrowSnapshot, BoundFilter, ComparisonOperator, Error, Projection,
    QueryRow, Result,
};
use async_trait::async_trait;
use datafusion::{
    common::ScalarValue,
    datasource::TableProvider,
    execution::{
        context::{SessionConfig, SessionContext},
        memory_pool::{FairSpillPool, MemoryPool, PeakRecordingPool},
        runtime_env::RuntimeEnvBuilder,
    },
    logical_expr::{Expr, TableProviderFilterPushDown, TableType},
    physical_plan::ExecutionPlan,
    prelude::{ident, lit},
};
use datafusion_datasource::{memory::MemorySourceConfig, source::DataSourceExec};
use futures::{future::Either, FutureExt, StreamExt};
use futures_timer::Delay;
use rrd_core::RuntimeValue;
use serde::{Deserialize, Serialize};
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

const SNAPSHOT_TABLE: &str = "rrd_snapshot";
const IDENTITY: &str = "__rrd_identity";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FusionBudget {
    pub max_batch_rows: usize,
    pub max_memory_bytes: usize,
    pub max_spill_bytes: usize,
    pub max_elapsed_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FusionAnalysis {
    pub engine: String,
    pub provider_scans: usize,
    pub input_rows: usize,
    pub input_batches: usize,
    pub input_memory_bytes: usize,
    pub output_batches: usize,
    pub projection_pushdown: String,
    pub filter_pushdown: String,
    pub limit_pushdown: String,
    pub physical_operators: usize,
    pub peak_memory_bytes: usize,
    pub spill_count: usize,
    pub spilled_bytes: usize,
    pub spilled_rows: usize,
    pub elapsed_micros: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FusionExecution {
    pub rows: Vec<QueryRow>,
    pub analysis: FusionAnalysis,
}

#[derive(Debug, Default)]
struct ProviderEvidence {
    scans: AtomicUsize,
    projected_columns: AtomicUsize,
    pushed_limit: AtomicUsize,
}

#[derive(Debug)]
struct RrdSnapshotTableProvider {
    snapshot: ArrowSnapshot,
    evidence: Arc<ProviderEvidence>,
}

impl RrdSnapshotTableProvider {
    fn try_new(snapshot: ArrowSnapshot, evidence: Arc<ProviderEvidence>) -> Result<Self> {
        snapshot.validate()?;
        Ok(Self { snapshot, evidence })
    }
}

#[async_trait]
impl TableProvider for RrdSnapshotTableProvider {
    fn schema(&self) -> datafusion::arrow::datatypes::SchemaRef {
        Arc::clone(&self.snapshot.schema)
    }

    fn table_type(&self) -> TableType {
        TableType::Base
    }

    async fn scan(
        &self,
        _state: &dyn datafusion_session::Session,
        projection: Option<&Vec<usize>>,
        filters: &[Expr],
        limit: Option<usize>,
    ) -> datafusion::common::Result<Arc<dyn ExecutionPlan>> {
        if !filters.is_empty() {
            return Err(datafusion::common::DataFusionError::Execution(
                "RRD snapshot provider received a filter it declared unsupported".into(),
            ));
        }
        self.evidence.scans.fetch_add(1, Ordering::Relaxed);
        self.evidence.projected_columns.store(
            projection.map_or(self.snapshot.schema.fields().len(), Vec::len),
            Ordering::Relaxed,
        );
        self.evidence
            .pushed_limit
            .store(limit.unwrap_or(usize::MAX), Ordering::Relaxed);
        let partitions = [self.snapshot.batches.clone()];
        let source = MemorySourceConfig::try_new(
            &partitions,
            Arc::clone(&self.snapshot.schema),
            projection.cloned(),
        )?
        .with_limit(limit);
        Ok(DataSourceExec::from_data_source(source))
    }

    fn supports_filters_pushdown(
        &self,
        filters: &[&Expr],
    ) -> datafusion::common::Result<Vec<TableProviderFilterPushDown>> {
        Ok(vec![
            TableProviderFilterPushDown::Unsupported;
            filters.len()
        ])
    }
}

/// Execute relational RRFlowQL operators through DataFusion's logical and
/// physical planners over the immutable Arrow snapshot captured for the read
/// stamp. MATCH is excluded because BM25 scoring owns its ranking and limit.
pub fn execute_snapshot(
    snapshot: ArrowSnapshot,
    filters: &[BoundFilter],
    projection: &Projection,
    limit: Option<usize>,
    defer_projection_and_limit: bool,
    budget: &FusionBudget,
) -> Result<FusionExecution> {
    let execute = move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| Error::Execution(format!("query runtime creation failed: {error}")))?
            .block_on(execute_snapshot_async(
                snapshot,
                filters,
                projection,
                limit,
                defer_projection_and_limit,
                budget,
            ))
    };
    if tokio::runtime::Handle::try_current().is_ok() {
        std::thread::scope(|scope| {
            scope
                .spawn(execute)
                .join()
                .map_err(|_| Error::Execution("DataFusion query runtime panicked".into()))?
        })
    } else {
        execute()
    }
}

async fn execute_snapshot_async(
    snapshot: ArrowSnapshot,
    filters: &[BoundFilter],
    projection: &Projection,
    limit: Option<usize>,
    defer_projection_and_limit: bool,
    budget: &FusionBudget,
) -> Result<FusionExecution> {
    if budget.max_batch_rows == 0
        || budget.max_memory_bytes == 0
        || budget.max_spill_bytes == 0
        || budget.max_elapsed_ms == 0
    {
        return Err(Error::Budget(
            "all DataFusion execution budgets must be greater than zero".into(),
        ));
    }
    snapshot.validate()?;
    let input_memory_bytes = snapshot.resident_bytes()?;
    let operator_memory_bytes = budget
        .max_memory_bytes
        .checked_sub(input_memory_bytes)
        .filter(|remaining| *remaining > 0)
        .ok_or_else(|| {
            Error::Budget(format!(
                "Arrow snapshot requires {input_memory_bytes} bytes, query memory budget allows {}",
                budget.max_memory_bytes
            ))
        })?;
    let started = Instant::now();
    let input_rows = snapshot.rows;
    let input_batches = snapshot.batches.len();
    let source_columns = snapshot.schema.fields().len();
    let evidence = Arc::new(ProviderEvidence::default());
    let table = RrdSnapshotTableProvider::try_new(snapshot, Arc::clone(&evidence))?;
    let recorder = Arc::new(PeakRecordingPool::new(Arc::new(FairSpillPool::new(
        operator_memory_bytes,
    ))));
    let memory_pool: Arc<dyn MemoryPool> = recorder.clone();
    let runtime = RuntimeEnvBuilder::new()
        .with_memory_pool(memory_pool)
        .with_max_temp_directory_size(
            u64::try_from(budget.max_spill_bytes)
                .map_err(|_| Error::Budget("spill budget exceeds u64".into()))?,
        )
        .build()
        .map_err(datafusion_error)?;
    let sort_reservation = (operator_memory_bytes / 4).clamp(1, 1024 * 1024);
    let config = SessionConfig::new()
        .with_batch_size(budget.max_batch_rows)
        .with_target_partitions(1)
        .with_sort_spill_reservation_bytes(sort_reservation);
    let context = SessionContext::new_with_config_rt(config, Arc::new(runtime));
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

    let task = Arc::new(frame.task_ctx());
    let physical = frame
        .create_physical_plan()
        .await
        .map_err(datafusion_error)?;
    let mut stream = datafusion::physical_plan::execute_stream(Arc::clone(&physical), task)
        .map_err(datafusion_error)?;
    let mut rows = Vec::new();
    let mut output_batches = 0usize;
    loop {
        let elapsed = started.elapsed();
        let allowed = Duration::from_millis(budget.max_elapsed_ms);
        let remaining = allowed.checked_sub(elapsed).ok_or_else(|| {
            Error::Budget(format!(
                "DataFusion execution exceeded the {} ms elapsed-time budget",
                budget.max_elapsed_ms
            ))
        })?;
        let next = stream.next().boxed();
        let timeout = Delay::new(remaining).boxed();
        match futures::future::select(next, timeout).await {
            Either::Left((Some(batch), _)) => {
                let batch = batch.map_err(datafusion_error)?;
                if batch.num_rows() > budget.max_batch_rows {
                    return Err(Error::Budget(format!(
                        "DataFusion emitted {} rows in one batch, budget allows {}",
                        batch.num_rows(),
                        budget.max_batch_rows
                    )));
                }
                output_batches = output_batches.saturating_add(1);
                rows.extend(record_batch_to_rows(&batch)?);
            }
            Either::Left((None, _)) => break,
            Either::Right(((), _)) => {
                return Err(Error::Budget(format!(
                    "DataFusion execution exceeded the {} ms elapsed-time budget",
                    budget.max_elapsed_ms
                )));
            }
        }
    }
    let metrics = physical_metrics(physical.as_ref());
    if metrics.spilled_bytes > budget.max_spill_bytes {
        return Err(Error::Budget(format!(
            "DataFusion spilled {} bytes, budget allows {}",
            metrics.spilled_bytes, budget.max_spill_bytes
        )));
    }
    let projected_columns = evidence.projected_columns.load(Ordering::Relaxed);
    let pushed_limit = evidence.pushed_limit.load(Ordering::Relaxed);
    Ok(FusionExecution {
        rows,
        analysis: FusionAnalysis {
            engine: "datafusion-55".into(),
            provider_scans: evidence.scans.load(Ordering::Relaxed),
            input_rows,
            input_batches,
            input_memory_bytes,
            output_batches,
            projection_pushdown: if projected_columns < source_columns {
                "exact".into()
            } else {
                "not_requested".into()
            },
            filter_pushdown: if filters.iter().any(|filter| !filter.comparison.is_match()) {
                "unsupported_exact_post_scan".into()
            } else {
                "not_requested".into()
            },
            limit_pushdown: if pushed_limit != usize::MAX {
                "exact".into()
            } else if limit.is_some() && !defer_projection_and_limit {
                "retained_above_scan".into()
            } else {
                "not_requested".into()
            },
            physical_operators: metrics.operators,
            peak_memory_bytes: input_memory_bytes.saturating_add(recorder.peak_reserved()),
            spill_count: metrics.spill_count,
            spilled_bytes: metrics.spilled_bytes,
            spilled_rows: metrics.spilled_rows,
            elapsed_micros: u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX),
        },
    })
}

#[derive(Default)]
struct PhysicalMetrics {
    operators: usize,
    spill_count: usize,
    spilled_bytes: usize,
    spilled_rows: usize,
}

fn physical_metrics(plan: &dyn ExecutionPlan) -> PhysicalMetrics {
    let mut result = PhysicalMetrics {
        operators: 1,
        ..PhysicalMetrics::default()
    };
    if let Some(metrics) = plan.metrics() {
        result.spill_count = metrics.spill_count().unwrap_or(0);
        result.spilled_bytes = metrics.spilled_bytes().unwrap_or(0);
        result.spilled_rows = metrics.spilled_rows().unwrap_or(0);
    }
    for child in plan.children() {
        let child = physical_metrics(child.as_ref());
        result.operators = result.operators.saturating_add(child.operators);
        result.spill_count = result.spill_count.saturating_add(child.spill_count);
        result.spilled_bytes = result.spilled_bytes.saturating_add(child.spilled_bytes);
        result.spilled_rows = result.spilled_rows.saturating_add(child.spilled_rows);
    }
    result
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

fn datafusion_error(error: datafusion::common::DataFusionError) -> Error {
    let message = format!("DataFusion execution failed: {error}");
    let exhausted = matches!(
        error.find_root(),
        datafusion::common::DataFusionError::ResourcesExhausted(_)
    ) || matches!(
        error.find_root(),
        datafusion::common::DataFusionError::IoError(io)
            if io.to_string().contains("exceeded the allowable limit")
    ) || message.contains("exceeded the allowable limit");
    if exhausted {
        Error::Budget(message)
    } else {
        Error::Execution(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{rows_to_arrow_snapshot, ArrowReadStamp, QueryFieldTypes};
    use rrd_core::RuntimeValueType;
    use std::collections::BTreeMap;

    fn large_snapshot() -> ArrowSnapshot {
        let rows = (0..8_192)
            .rev()
            .map(|index| QueryRow {
                identity: format!("record:fixture:{index:05}"),
                values: BTreeMap::from([(
                    "payload".into(),
                    RuntimeValue::String(format!("{index:05}-{}", "x".repeat(256))),
                )]),
            })
            .collect::<Vec<_>>();
        rows_to_arrow_snapshot(
            &rows,
            &QueryFieldTypes::from([("payload".into(), vec![RuntimeValueType::String])]),
            ArrowReadStamp {
                read_manifest: "manifest:spill-test".into(),
                scope: "instance:spill-test".into(),
                valid_at: 42,
                known_at_cursor: 7,
                source_cursor: 7,
                schema_revision: 1,
            },
            128,
        )
        .unwrap()
    }

    #[test]
    fn constrained_sort_spills_streams_and_matches_the_reference_order() {
        let snapshot = large_snapshot();
        let resident = snapshot.resident_bytes().unwrap();
        let budget = FusionBudget {
            max_batch_rows: 128,
            max_memory_bytes: resident + 256 * 1024,
            max_spill_bytes: 32 * 1024 * 1024,
            max_elapsed_ms: 30_000,
        };
        let execution = execute_snapshot(
            snapshot,
            &[],
            &Projection::Fields(vec!["payload".into()]),
            None,
            false,
            &budget,
        )
        .unwrap();
        let expected = (0..8_192)
            .map(|index| format!("record:fixture:{index:05}"))
            .collect::<Vec<_>>();
        assert_eq!(
            execution
                .rows
                .iter()
                .map(|row| row.identity.clone())
                .collect::<Vec<_>>(),
            expected
        );
        assert_eq!(execution.analysis.input_batches, 64);
        assert_eq!(execution.analysis.input_memory_bytes, resident);
        assert!(execution.analysis.output_batches > 1);
        assert!(execution.analysis.spill_count > 0);
        assert!(execution.analysis.spilled_bytes > 0);
        assert!(execution.analysis.spilled_bytes <= budget.max_spill_bytes);
        assert!(execution.analysis.peak_memory_bytes <= budget.max_memory_bytes);
    }

    #[test]
    fn elapsed_budget_cancels_before_a_large_sort_can_complete() {
        let snapshot = large_snapshot();
        let resident = snapshot.resident_bytes().unwrap();
        let error = execute_snapshot(
            snapshot,
            &[],
            &Projection::All,
            None,
            false,
            &FusionBudget {
                max_batch_rows: 128,
                max_memory_bytes: resident + 256 * 1024,
                max_spill_bytes: 32 * 1024 * 1024,
                max_elapsed_ms: 1,
            },
        )
        .unwrap_err();
        assert!(matches!(error, Error::Budget(reason) if reason.contains("elapsed-time")));
    }

    #[test]
    fn spill_budget_denies_before_temp_storage_can_exceed_its_cap() {
        let snapshot = large_snapshot();
        let resident = snapshot.resident_bytes().unwrap();
        let error = execute_snapshot(
            snapshot,
            &[],
            &Projection::All,
            None,
            false,
            &FusionBudget {
                max_batch_rows: 128,
                max_memory_bytes: resident + 256 * 1024,
                max_spill_bytes: 1,
                max_elapsed_ms: 30_000,
            },
        )
        .unwrap_err();
        match error {
            Error::Budget(reason) => assert!(
                reason.contains("allowable limit") || reason.contains("spill"),
                "unexpected spill-budget diagnostic: {reason}"
            ),
            error => panic!("spill-budget denial was misclassified: {error:?}"),
        }
    }

    #[test]
    fn synchronous_port_is_safe_inside_an_existing_async_runtime() {
        let snapshot = large_snapshot();
        let resident = snapshot.resident_bytes().unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let result = execute_snapshot(
                snapshot,
                &[],
                &Projection::All,
                Some(1),
                false,
                &FusionBudget {
                    max_batch_rows: 128,
                    max_memory_bytes: resident + 256 * 1024,
                    max_spill_bytes: 32 * 1024 * 1024,
                    max_elapsed_ms: 30_000,
                },
            )
            .unwrap();
            assert_eq!(result.rows.len(), 1);
        });
    }
}
