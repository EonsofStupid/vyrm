use crate::{Error, IndexCatalogue, IndexCatalogueRepository, Result, Source};
use rrd_core::{
    Predicate, ReadStamp, RuntimeMutation, RuntimeSchemaRegistry, RuntimeType, ScopeId,
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
}

impl SourceWatermarks {
    fn observe(&mut self, cursor: u64, mutation: &RuntimeMutation) {
        match mutation {
            RuntimeMutation::Schema { .. } => self.schema = cursor,
            RuntimeMutation::Record { record } => {
                self.any_record = cursor;
                self.records.insert(record.reference.kind.clone(), cursor);
            }
            RuntimeMutation::Relation { relation } => {
                self.relations
                    .insert(relation.reference.kind.clone(), cursor);
            }
            RuntimeMutation::Event { event } => {
                self.events.insert(event.kind.clone(), cursor);
            }
            RuntimeMutation::SeriesSample { sample } => {
                self.series.insert(sample.series.kind.clone(), cursor);
            }
            RuntimeMutation::Geo { geo } => {
                self.geo.insert(geo.reference.kind.clone(), cursor);
            }
            RuntimeMutation::Claim { claim } => {
                self.claims.insert(claim.predicate.clone(), cursor);
            }
            RuntimeMutation::Vector { .. } | RuntimeMutation::Object { .. } => {}
        }
    }

    pub fn for_source(&self, source: &Source) -> u64 {
        let data = match source {
            Source::Record { kind } => self.records.get(kind).copied().unwrap_or(0),
            Source::Relation { kind } => self.relations.get(kind).copied().unwrap_or(0),
            Source::Event { kind } => self.events.get(kind).copied().unwrap_or(0),
            Source::Series { kind } => self.series.get(kind).copied().unwrap_or(0),
            Source::Geo { kind } => self.geo.get(kind).copied().unwrap_or(0),
            Source::Traversal { relation, .. } => self
                .relations
                .get(relation)
                .copied()
                .unwrap_or(0)
                .max(self.any_record),
            Source::Claim {
                predicate: Some(predicate),
            } => self.claims.get(predicate).copied().unwrap_or(0),
            Source::Claim { predicate: None } => self.claims.values().copied().max().unwrap_or(0),
        };
        data.max(self.schema)
    }
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
