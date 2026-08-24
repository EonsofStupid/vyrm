use crate::{Error, IndexCatalogue, IndexCatalogueRepository, Result};
use rrd_store::Engine;
use serde::{Deserialize, Serialize};
use vyrm_core::{ReadStamp, RuntimeMutation, RuntimeSchemaRegistry, ScopeId};

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
        let schemas = changes
            .into_iter()
            .filter_map(|change| match change.mutation {
                RuntimeMutation::Schema { registry } => Some(SchemaVersion {
                    cursor: change.cursor,
                    registry,
                }),
                _ => None,
            })
            .collect();
        let indexes = IndexCatalogueRepository::new(engine, read.scope.clone()).load()?;
        // The catalogue is materialized outside the append-only runtime log.
        // Revalidate the same stamp after loading it so a concurrent index or
        // vector catalogue transition cannot produce a torn planning view.
        engine.runtime_read_changes(&read, read.commit_cursor, 1)?;
        Ok(Self {
            read,
            schemas,
            indexes,
        })
    }

    pub fn schema_at(&self, cursor: u64) -> Option<&RuntimeSchemaRegistry> {
        self.schemas
            .iter()
            .rev()
            .find(|version| version.cursor <= cursor)
            .map(|version| &version.registry)
    }
}
