use crate::{Error, IndexCatalogue, IndexCatalogueRepository, Result, Source};
use rrd_core::{
    Predicate, ReadStamp, RuntimeMutation, RuntimeRef, RuntimeSchemaRegistry, RuntimeType, ScopeId,
};
use rrd_store::Engine;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemaVersion {
    pub cursor: u64,
    pub registry: RuntimeSchemaRegistry,
}

/// Schema history and the immutable read stamp against which it was captured.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Catalog {
    pub read: ReadStamp,
    pub schemas: Vec<SchemaVersion>,
    pub indexes: IndexCatalogue,
    pub source_watermarks: SourceWatermarks,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceWatermarks {
    pub schema: u64,
    pub any_record: u64,
    pub records: BTreeMap<RuntimeType, u64>,
    pub relations: BTreeMap<RuntimeType, u64>,
    pub events: BTreeMap<RuntimeType, u64>,
    pub series: BTreeMap<RuntimeType, u64>,
    pub geo: BTreeMap<RuntimeType, u64>,
    pub claims: BTreeMap<Predicate, u64>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[doc(hidden)]
    pub series_targets: BTreeMap<RuntimeRef, RuntimeType>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[doc(hidden)]
    pub schema_history: Vec<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[doc(hidden)]
    pub any_record_history: Vec<u64>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[doc(hidden)]
    pub record_history: BTreeMap<RuntimeType, Vec<u64>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[doc(hidden)]
    pub relation_history: BTreeMap<RuntimeType, Vec<u64>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[doc(hidden)]
    pub event_history: BTreeMap<RuntimeType, Vec<u64>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[doc(hidden)]
    pub series_history: BTreeMap<RuntimeType, Vec<u64>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[doc(hidden)]
    pub geo_history: BTreeMap<RuntimeType, Vec<u64>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[doc(hidden)]
    pub claim_history: BTreeMap<Predicate, Vec<u64>>,
}

impl SourceWatermarks {
    fn observe(&mut self, cursor: u64, mutation: &RuntimeMutation) {
        match mutation {
            RuntimeMutation::Schema { .. } => {
                self.schema = cursor;
                self.schema_history.push(cursor);
            }
            RuntimeMutation::Record { record } => {
                self.any_record = cursor;
                self.records.insert(record.reference.kind.clone(), cursor);
                self.any_record_history.push(cursor);
                self.record_history
                    .entry(record.reference.kind.clone())
                    .or_default()
                    .push(cursor);
            }
            RuntimeMutation::Relation { relation } => {
                self.relations
                    .insert(relation.reference.kind.clone(), cursor);
                self.relation_history
                    .entry(relation.reference.kind.clone())
                    .or_default()
                    .push(cursor);
            }
            RuntimeMutation::Event { event } => {
                self.events.insert(event.kind.clone(), cursor);
                self.event_history
                    .entry(event.kind.clone())
                    .or_default()
                    .push(cursor);
            }
            RuntimeMutation::SeriesSample { sample } => {
                self.series_targets
                    .insert(sample.reference.clone(), sample.series.kind.clone());
                self.series.insert(sample.series.kind.clone(), cursor);
                self.series_history
                    .entry(sample.series.kind.clone())
                    .or_default()
                    .push(cursor);
            }
            RuntimeMutation::Geo { geo } => {
                self.geo.insert(geo.reference.kind.clone(), cursor);
                self.geo_history
                    .entry(geo.reference.kind.clone())
                    .or_default()
                    .push(cursor);
            }
            RuntimeMutation::Claim { claim } => {
                self.claims.insert(claim.predicate.clone(), cursor);
                self.claim_history
                    .entry(claim.predicate.clone())
                    .or_default()
                    .push(cursor);
            }
            RuntimeMutation::Retire { retirement } => {
                if retirement.model.is_record_like() {
                    self.any_record = cursor;
                    self.records
                        .insert(retirement.reference.kind.clone(), cursor);
                    self.any_record_history.push(cursor);
                    self.record_history
                        .entry(retirement.reference.kind.clone())
                        .or_default()
                        .push(cursor);
                } else if retirement.model == rrd_core::RuntimeLogicalModel::GraphRelation {
                    self.relations
                        .insert(retirement.reference.kind.clone(), cursor);
                    self.relation_history
                        .entry(retirement.reference.kind.clone())
                        .or_default()
                        .push(cursor);
                } else if retirement.model.is_event_like() {
                    self.events
                        .insert(retirement.reference.kind.clone(), cursor);
                    self.event_history
                        .entry(retirement.reference.kind.clone())
                        .or_default()
                        .push(cursor);
                } else if retirement.model == rrd_core::RuntimeLogicalModel::TimeSeries {
                    if let Some(kind) = self.series_targets.get(&retirement.reference).cloned() {
                        self.series.insert(kind.clone(), cursor);
                        self.series_history.entry(kind).or_default().push(cursor);
                    }
                } else if retirement.model == rrd_core::RuntimeLogicalModel::Geo {
                    self.geo.insert(retirement.reference.kind.clone(), cursor);
                    self.geo_history
                        .entry(retirement.reference.kind.clone())
                        .or_default()
                        .push(cursor);
                }
            }
            RuntimeMutation::Vector { .. } | RuntimeMutation::Object { .. } => {}
        }
    }

    pub fn for_source(&self, source: &Source) -> u64 {
        self.for_source_at(source, u64::MAX)
    }

    /// Last source or schema change visible at `known_at_cursor`.
    ///
    /// A latest-only watermark is not a valid historical coordinate: a source
    /// may have advanced after the requested cursor. Retaining the cursor
    /// history keeps `KNOWN n` planning and exact-index selection bounded by
    /// the same transaction-time prefix as authoritative replay.
    pub fn for_source_at(&self, source: &Source, known_at_cursor: u64) -> u64 {
        let data = match source {
            Source::Record { kind } => historical(
                self.record_history.get(kind),
                self.records.get(kind).copied(),
                known_at_cursor,
            ),
            Source::Relation { kind } => historical(
                self.relation_history.get(kind),
                self.relations.get(kind).copied(),
                known_at_cursor,
            ),
            Source::Event { kind } => historical(
                self.event_history.get(kind),
                self.events.get(kind).copied(),
                known_at_cursor,
            ),
            Source::Series { kind } => historical(
                self.series_history.get(kind),
                self.series.get(kind).copied(),
                known_at_cursor,
            ),
            Source::Geo { kind } => historical(
                self.geo_history.get(kind),
                self.geo.get(kind).copied(),
                known_at_cursor,
            ),
            Source::Traversal { relation, .. } => self
                .relation_history
                .get(relation)
                .map(|history| latest(history, known_at_cursor))
                .unwrap_or_else(|| {
                    self.relations
                        .get(relation)
                        .copied()
                        .filter(|cursor| *cursor <= known_at_cursor)
                        .unwrap_or(0)
                })
                .max(historical(
                    Some(&self.any_record_history),
                    Some(self.any_record),
                    known_at_cursor,
                )),
            Source::Claim {
                predicate: Some(predicate),
            } => historical(
                self.claim_history.get(predicate),
                self.claims.get(predicate).copied(),
                known_at_cursor,
            ),
            Source::Claim { predicate: None } => self
                .claim_history
                .values()
                .map(|history| latest(history, known_at_cursor))
                .max()
                .unwrap_or_else(|| {
                    self.claims
                        .values()
                        .copied()
                        .filter(|cursor| *cursor <= known_at_cursor)
                        .max()
                        .unwrap_or(0)
                }),
        };
        data.max(historical(
            Some(&self.schema_history),
            Some(self.schema),
            known_at_cursor,
        ))
    }
}

fn historical(history: Option<&Vec<u64>>, fallback: Option<u64>, known_at_cursor: u64) -> u64 {
    history
        .and_then(|history| {
            let cursor = latest(history, known_at_cursor);
            (cursor != 0).then_some(cursor)
        })
        .unwrap_or_else(|| {
            fallback
                .filter(|cursor| *cursor <= known_at_cursor)
                .unwrap_or(0)
        })
}

fn latest(history: &[u64], known_at_cursor: u64) -> u64 {
    history
        .iter()
        .rev()
        .copied()
        .find(|cursor| *cursor <= known_at_cursor)
        .unwrap_or(0)
}

impl Catalog {
    pub fn capture<E: Engine>(engine: &E, scope: &ScopeId) -> Result<Self> {
        let read = engine.runtime_read_stamp(scope)?;
        Self::capture_at(engine, read)
    }

    /// Captures schema history against a caller-owned immutable read stamp.
    /// This is the observer-safe path for instrumented execution: telemetry
    /// may advance the live head after `read` is captured without changing
    /// what `KNOWN HEAD` meant to the query.
    pub fn capture_at<E: Engine>(engine: &E, read: ReadStamp) -> Result<Self> {
        read.validate()
            .map_err(|error| Error::Catalog(error.to_string()))?;
        let limit = usize::try_from(read.commit_cursor).map_err(|_| {
            Error::Catalog("read cursor exceeds this platform's address space".into())
        })?;
        let changes = if limit == 0 {
            Vec::new()
        } else {
            let page = engine.runtime_read_changes(&read, 0, limit)?;
            if page.through_cursor != read.commit_cursor {
                return Err(Error::Catalog(format!(
                    "schema replay stopped at cursor {}, expected {}",
                    page.through_cursor, read.commit_cursor
                )));
            }
            page.changes
        };
        let mut source_watermarks = SourceWatermarks::default();
        let mut schemas = Vec::new();
        for change in changes {
            source_watermarks.observe(change.cursor, &change.mutation);
            if let RuntimeMutation::Schema { registry } = change.mutation {
                schemas.push(SchemaVersion {
                    cursor: change.cursor,
                    registry,
                });
            }
        }
        let indexes = IndexCatalogueRepository::new(engine, read.scope.clone()).load()?;
        // The catalogue is materialized outside the append-only runtime log.
        // Revalidate the same stamp after loading it so a concurrent index or
        // vector catalogue transition cannot produce a torn planning view.
        engine.runtime_read_changes(&read, read.commit_cursor, 1)?;
        Ok(Self {
            read,
            schemas,
            indexes,
            source_watermarks,
        })
    }

    pub fn schema_at(&self, cursor: u64) -> Option<&RuntimeSchemaRegistry> {
        self.schemas
            .iter()
            .rev()
            .find(|version| version.cursor <= cursor)
            .map(|version| &version.registry)
    }

    pub fn source_cursor(&self, source: &Source) -> u64 {
        self.source_watermarks.for_source(source)
    }
}
