use crate::{bind, execute, plan, Catalog, Error, ExecutionBudget, Parameters, QueryRow, Result};
use crate::{CursorExpr, Query};
use rrd_core::{digest, ScopeId};
use rrd_store::Engine;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveQueryBudget {
    pub execution: ExecutionBudget,
    pub max_delta_rows: usize,
}

impl Default for LiveQueryBudget {
    fn default() -> Self {
        Self {
            execution: ExecutionBudget::default(),
            max_delta_rows: 10_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveRowChange {
    pub before: QueryRow,
    pub after: QueryRow,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveQueryDelta {
    pub query_digest: String,
    pub from_cursor: u64,
    pub through_cursor: u64,
    pub head_cursor: u64,
    pub added: Vec<QueryRow>,
    pub updated: Vec<LiveRowChange>,
    pub removed: Vec<QueryRow>,
}

impl LiveQueryDelta {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.updated.is_empty() && self.removed.is_empty()
    }

    pub fn change_count(&self) -> usize {
        self.added.len() + self.updated.len() + self.removed.len()
    }
}

pub fn poll_live_query<E: Engine>(
    engine: &E,
    scope: &ScopeId,
    query: &Query,
    parameters: &Parameters,
    after_cursor: u64,
    budget: &LiveQueryBudget,
) -> Result<LiveQueryDelta> {
    if !matches!(query.temporal.known_at, CursorExpr::Head) {
        return Err(Error::Binding(
            "live queries require KNOWN HEAD; the resume cursor is supplied separately".into(),
        ));
    }
    if budget.max_delta_rows == 0 {
        return Err(Error::Budget(
            "live query delta budget must be greater than zero".into(),
        ));
    }
    let catalogue = Catalog::capture(engine, scope)?;
    let head_cursor = catalogue.read.commit_cursor;
    if after_cursor > head_cursor {
        return Err(Error::Binding(format!(
            "live query resume cursor {after_cursor} exceeds captured head {head_cursor}"
        )));
    }
    let before = if after_cursor == 0 || catalogue.schema_at(after_cursor).is_none() {
        BTreeMap::new()
    } else {
        execute_at(
            engine,
            &catalogue,
            query,
            parameters,
            after_cursor,
            &budget.execution,
        )?
    };
    let after = if head_cursor == 0 || catalogue.schema_at(head_cursor).is_none() {
        BTreeMap::new()
    } else {
        execute_at(
            engine,
            &catalogue,
            query,
            parameters,
            head_cursor,
            &budget.execution,
        )?
    };

    let added = after
        .iter()
        .filter(|(identity, _)| !before.contains_key(*identity))
        .map(|(_, row)| row.clone())
        .collect::<Vec<_>>();
    let updated = after
        .iter()
        .filter_map(|(identity, row)| {
            let prior = before.get(identity)?;
            (prior != row).then(|| LiveRowChange {
                before: prior.clone(),
                after: row.clone(),
            })
        })
        .collect::<Vec<_>>();
    let removed = before
        .iter()
        .filter(|(identity, _)| !after.contains_key(*identity))
        .map(|(_, row)| row.clone())
        .collect::<Vec<_>>();
    let change_count = added.len() + updated.len() + removed.len();
    if change_count > budget.max_delta_rows {
        return Err(Error::Budget(format!(
            "live query produced {change_count} row changes, budget allows {}",
            budget.max_delta_rows
        )));
    }
    let query_digest = digest::sha256_hex(&serde_json::to_vec(&(
        query.contract_version,
        query.canonical(),
        parameters,
    ))?);
    Ok(LiveQueryDelta {
        query_digest,
        from_cursor: after_cursor,
        through_cursor: head_cursor,
        head_cursor,
        added,
        updated,
        removed,
    })
}

fn execute_at<E: Engine>(
    engine: &E,
    catalogue: &Catalog,
    query: &Query,
    parameters: &Parameters,
    known_at_cursor: u64,
    budget: &ExecutionBudget,
) -> Result<BTreeMap<String, QueryRow>> {
    let mut query = query.clone();
    query.temporal.known_at = CursorExpr::Literal(known_at_cursor);
    let physical = plan(&bind(&query, parameters, catalogue)?)?;
    let execution = execute(engine, &physical, budget)?;
    if execution.truncated {
        return Err(Error::Budget(
            "live query snapshots must not be truncated".into(),
        ));
    }
    let mut rows = BTreeMap::new();
    for row in execution.batches.into_iter().flat_map(|batch| batch.rows) {
        if rows.insert(row.identity.clone(), row).is_some() {
            return Err(Error::Integrity(
                "live query snapshot contains duplicate row identities".into(),
            ));
        }
    }
    Ok(rows)
}
