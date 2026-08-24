use crate::{bind, Catalog, Error, Parameters, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use vyrm_core::{
    digest, ProjectionId, ProjectionStamp, ProjectionState, ScopeId, DATA_RUNTIME_CONTRACT_VERSION,
};
use vyrm_ql::{CursorExpr, Projection, Query, Source, TemporalSelector, TimeExpr};
use vyrm_store::{ControlTransition, Engine};

pub const INDEX_CATALOGUE_CONTRACT_VERSION: u16 = 1;
const MAX_INDEX_FIELDS: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexDefinition {
    pub id: ProjectionId,
    pub source: Source,
    pub fields: Vec<String>,
    #[serde(default)]
    pub unique: bool,
}

impl IndexDefinition {
    pub fn validate(&self) -> Result<()> {
        if matches!(self.source, Source::Traversal { .. }) {
            return Err(Error::Catalog(
                "recursive traversal cannot be an index source".into(),
            ));
        }
        if self.fields.is_empty() || self.fields.len() > MAX_INDEX_FIELDS {
            return Err(Error::Catalog(format!(
                "an index must contain 1..={MAX_INDEX_FIELDS} fields"
            )));
        }
        let unique = self.fields.iter().collect::<BTreeSet<_>>();
        if unique.len() != self.fields.len() {
            return Err(Error::Catalog("index fields must be unique".into()));
        }
        Ok(())
    }

    pub fn config_digest(&self) -> Result<String> {
        self.validate()?;
        Ok(digest::sha256_hex(&serde_json::to_vec(self)?))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexEntry {
    pub definition: IndexDefinition,
    pub stamp: ProjectionStamp,
}

impl IndexEntry {
    pub fn is_usable_at(&self, source_cursor: u64) -> bool {
        self.stamp.state == ProjectionState::Ready
            && self.stamp.source_cursor >= source_cursor
            && self
                .definition
                .config_digest()
                .is_ok_and(|digest| self.stamp.config_digest == digest)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexCatalogue {
    pub contract_version: u16,
    pub scope: ScopeId,
    pub revision: u64,
    pub entries: BTreeMap<ProjectionId, IndexEntry>,
}

impl IndexCatalogue {
    pub(crate) fn empty(scope: ScopeId) -> Self {
        Self {
            contract_version: INDEX_CATALOGUE_CONTRACT_VERSION,
            scope,
            revision: 0,
            entries: BTreeMap::new(),
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.contract_version != INDEX_CATALOGUE_CONTRACT_VERSION {
            return Err(Error::Integrity(format!(
                "unsupported index catalogue contract version {}",
                self.contract_version
            )));
        }
        for (id, entry) in &self.entries {
            entry.definition.validate()?;
            entry.stamp.validate().map_err(|error| {
                Error::Integrity(format!("invalid index stamp for {id}: {error}"))
            })?;
            if id != &entry.definition.id
                || id != &entry.stamp.id
                || entry.stamp.config_digest != entry.definition.config_digest()?
            {
                return Err(Error::Integrity(format!(
                    "index identity or configuration digest disagrees for {id}"
                )));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexMutationContext {
    pub at: u64,
    pub actor: String,
    pub request_id: String,
    pub operation_id: String,
}

impl IndexMutationContext {
    fn validate(&self) -> Result<()> {
        if self.at == 0
            || self.actor.is_empty()
            || self.request_id.is_empty()
            || self.operation_id.is_empty()
        {
            return Err(Error::Catalog(
                "index mutation context requires time, actor, request, and operation IDs".into(),
            ));
        }
        Ok(())
    }
}

pub struct IndexCatalogueRepository<'a, E: Engine + ?Sized> {
    engine: &'a E,
    scope: ScopeId,
    key: String,
}

impl<'a, E: Engine + ?Sized> IndexCatalogueRepository<'a, E> {
    pub fn new(engine: &'a E, scope: ScopeId) -> Self {
        let key = format!("server/state/index-catalogue/{scope}");
        Self { engine, scope, key }
    }

    pub fn load(&self) -> Result<IndexCatalogue> {
        let Some(bytes) = self.engine.control_record(&self.key)? else {
            return Ok(IndexCatalogue::empty(self.scope.clone()));
        };
        let catalogue: IndexCatalogue = serde_json::from_slice(&bytes)?;
        if catalogue.scope != self.scope {
            return Err(Error::Integrity(
                "index catalogue scope differs from its control key".into(),
            ));
        }
        catalogue.validate()?;
        Ok(catalogue)
    }

    pub fn create(
        &self,
        context: &IndexMutationContext,
        query_catalogue: &Catalog,
        definition: IndexDefinition,
    ) -> Result<IndexCatalogue> {
        context.validate()?;
        definition.validate()?;
        if query_catalogue.read.scope != self.scope {
            return Err(Error::Catalog(
                "query and index catalogues belong to different scopes".into(),
            ));
        }
        validate_fields(query_catalogue, &definition)?;
        self.update(context, "index.created", move |catalogue| {
            if catalogue.entries.contains_key(&definition.id) {
                return Err(Error::Catalog(format!(
                    "index {} already exists",
                    definition.id
                )));
            }
            let config_digest = definition.config_digest()?;
            let id = definition.id.clone();
            catalogue.entries.insert(
                id.clone(),
                IndexEntry {
                    definition,
                    stamp: ProjectionStamp {
                        contract_version: DATA_RUNTIME_CONTRACT_VERSION,
                        id,
                        generation: 1,
                        source_cursor: 0,
                        config_digest,
                        artifact_digest: digest::sha256_hex(&[]),
                        state: ProjectionState::Building,
                    },
                },
            );
            Ok(())
        })
    }

    pub fn begin_rebuild(
        &self,
        context: &IndexMutationContext,
        id: &ProjectionId,
    ) -> Result<IndexCatalogue> {
        context.validate()?;
        let id = id.clone();
        self.update(context, "index.rebuild_started", move |catalogue| {
            let entry = catalogue
                .entries
                .get_mut(&id)
                .ok_or_else(|| Error::Catalog(format!("unknown index {id}")))?;
            entry.stamp.generation = entry
                .stamp
                .generation
                .checked_add(1)
                .ok_or_else(|| Error::Integrity("index generation overflow".into()))?;
            entry.stamp.source_cursor = 0;
            entry.stamp.artifact_digest = digest::sha256_hex(&[]);
            entry.stamp.state = ProjectionState::Building;
            Ok(())
        })
    }

    pub fn publish_ready(
        &self,
        context: &IndexMutationContext,
        id: &ProjectionId,
        generation: u64,
        source_cursor: u64,
        artifact_digest: String,
    ) -> Result<IndexCatalogue> {
        context.validate()?;
        let scope_head = self.engine.runtime_read_stamp(&self.scope)?.commit_cursor;
        if source_cursor == 0 || source_cursor > scope_head {
            return Err(Error::Catalog(
                "index source cursor must name an existing authoritative change".into(),
            ));
        }
        let id = id.clone();
        self.update(context, "index.ready", move |catalogue| {
            let entry = catalogue
                .entries
                .get_mut(&id)
                .ok_or_else(|| Error::Catalog(format!("unknown index {id}")))?;
            if entry.stamp.generation != generation
                || entry.stamp.state != ProjectionState::Building
            {
                return Err(Error::Catalog(
                    "index publication is stale or not building".into(),
                ));
            }
            entry.stamp.source_cursor = source_cursor;
            entry.stamp.artifact_digest = artifact_digest;
            entry.stamp.state = ProjectionState::Ready;
            entry
                .stamp
                .validate()
                .map_err(|error| Error::Catalog(format!("invalid ready index stamp: {error}")))?;
            Ok(())
        })
    }

    pub fn quarantine(
        &self,
        context: &IndexMutationContext,
        id: &ProjectionId,
    ) -> Result<IndexCatalogue> {
        self.set_state(
            context,
            id,
            ProjectionState::Quarantined,
            "index.quarantined",
        )
    }

    pub fn retire(
        &self,
        context: &IndexMutationContext,
        id: &ProjectionId,
    ) -> Result<IndexCatalogue> {
        self.set_state(context, id, ProjectionState::Retiring, "index.retiring")
    }

    fn set_state(
        &self,
        context: &IndexMutationContext,
        id: &ProjectionId,
        state: ProjectionState,
        action: &str,
    ) -> Result<IndexCatalogue> {
        context.validate()?;
        let id = id.clone();
        self.update(context, action, move |catalogue| {
            let entry = catalogue
                .entries
                .get_mut(&id)
                .ok_or_else(|| Error::Catalog(format!("unknown index {id}")))?;
            entry.stamp.state = state;
            Ok(())
        })
    }

    fn update(
        &self,
        context: &IndexMutationContext,
        action: &str,
        mutate: impl FnOnce(&mut IndexCatalogue) -> Result<()>,
    ) -> Result<IndexCatalogue> {
        let expected = self.engine.control_record(&self.key)?;
        let mut catalogue = match &expected {
            Some(bytes) => serde_json::from_slice(bytes)?,
            None => IndexCatalogue::empty(self.scope.clone()),
        };
        catalogue.validate()?;
        mutate(&mut catalogue)?;
        catalogue.revision = catalogue
            .revision
            .checked_add(1)
            .ok_or_else(|| Error::Integrity("index catalogue revision overflow".into()))?;
        catalogue.validate()?;
        let replacement = serde_json::to_vec(&catalogue)?;
        self.engine.commit_control_transition(&ControlTransition {
            key: self.key.clone(),
            expected,
            replacement: Some(replacement),
            at: context.at,
            actor: context.actor.clone(),
            action: action.into(),
            request_id: context.request_id.clone(),
            operation_id: context.operation_id.clone(),
        })?;
        Ok(catalogue)
    }
}

fn validate_fields(catalogue: &Catalog, definition: &IndexDefinition) -> Result<()> {
    let mut query = Query::new(
        definition.source.clone(),
        TemporalSelector {
            valid_at: TimeExpr::Literal(0),
            known_at: CursorExpr::Head,
        },
    );
    query.projection = Projection::Fields(definition.fields.clone());
    let _ = bind(&query, &Parameters::new(), catalogue)?;
    Ok(())
}
