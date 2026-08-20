use super::{
    content_digest, invalid, quote_identifier, validate_name, validate_pg_identifier,
    validate_version, IterativeScanMode, OperatorAccessPath, OperatorAdapterDescriptor,
    OperatorKnowledgeAdapter, OperatorKnowledgeBinding, OperatorKnowledgeWriter,
    OperatorPlanEvidence, OperatorSearchHit, OperatorSearchRequest, OperatorSearchResult,
    OperatorSourceRevision, OperatorSyncOperation, OperatorSyncReceipt, OperatorSyncWork,
    OperatorVectorKind, PgvectorRelation, OPERATOR_KNOWLEDGE_CONTRACT_VERSION,
};
use native_tls::{Certificate, TlsConnector};
use postgres::{config::SslMode, Client, Config, GenericClient, IsolationLevel, Transaction};
use postgres_native_tls::MakeTlsConnector;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;
use std::time::Instant;
use vyrm_core::{digest, Error, Result};
use vyrm_vector::{ScoreMetric, VectorQuery};

pub const PGVECTOR_LIVE_IMPLEMENTATION_VERSION: &str = "vyrm-pgvector-live-v1";

/// Non-secret deployment identity for one pgvector relation and Vyrm control
/// schema. Connection strings, passwords, and root certificates never enter
/// this serializable contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PgvectorDeployment {
    pub contract_version: u16,
    pub relation: PgvectorRelation,
    pub model_digest_column: String,
    pub source_cursor_column: String,
    pub control_schema: String,
    pub metadata_table: String,
    pub revision_table: String,
    pub applied_work_table: String,
}

impl PgvectorDeployment {
    pub fn validate(&self) -> Result<()> {
        validate_version(self.contract_version)?;
        self.relation.validate()?;
        for (kind, value) in [
            ("pgvector model digest column", &self.model_digest_column),
            ("pgvector source cursor column", &self.source_cursor_column),
            ("pgvector control schema", &self.control_schema),
            ("pgvector metadata table", &self.metadata_table),
            ("pgvector revision table", &self.revision_table),
            ("pgvector applied-work table", &self.applied_work_table),
        ] {
            validate_pg_identifier(kind, value)?;
        }
        let columns = [
            self.relation.id_column.as_str(),
            self.relation.subject_column.as_str(),
            self.relation.vector_column.as_str(),
            self.relation.tenant_column.as_str(),
            self.model_digest_column.as_str(),
            self.source_cursor_column.as_str(),
        ];
        if columns
            .iter()
            .enumerate()
            .any(|(index, value)| columns[..index].contains(value))
        {
            return invalid("pgvector deployment columns must be distinct");
        }
        let tables = [
            self.metadata_table.as_str(),
            self.revision_table.as_str(),
            self.applied_work_table.as_str(),
        ];
        if tables
            .iter()
            .enumerate()
            .any(|(index, value)| tables[..index].contains(value))
        {
            return invalid("pgvector control tables must be distinct");
        }
        Ok(())
    }

    pub fn digest(&self) -> Result<String> {
        self.validate()?;
        content_digest(b"vyrm-pgvector-deployment-v1\0", self)
    }
}

/// Canonical payload for applying one vector-family outbox item to pgvector.
/// The surrounding `OperatorSyncWork` content-addresses these exact bytes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PgvectorSyncPayload {
    pub contract_version: u16,
    pub external_id: String,
    pub subject_id: String,
    pub source_cursor: u64,
    pub vector: Vec<f32>,
}

impl PgvectorSyncPayload {
    pub fn validate(&self, dimensions: u32) -> Result<()> {
        validate_version(self.contract_version)?;
        validate_name("pgvector external identity", &self.external_id)?;
        validate_name("pgvector subject identity", &self.subject_id)?;
        if self.source_cursor == 0 {
            return invalid("pgvector sync source cursor must be non-zero");
        }
        if self.vector.len() != dimensions as usize
            || self.vector.is_empty()
            || self.vector.iter().any(|value| !value.is_finite())
        {
            return invalid("pgvector sync vector differs from the bound finite model space");
        }
        Ok(())
    }

    pub fn canonical_bytes(&self, dimensions: u32) -> Result<Vec<u8>> {
        self.validate(dimensions)?;
        serde_json::to_vec(self).map_err(|error| Error::InvalidRuntime {
            reason: format!("encode canonical pgvector sync payload: {error}"),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PgvectorDeletePayload {
    pub contract_version: u16,
    pub external_id: String,
    pub source_cursor: u64,
}

impl PgvectorDeletePayload {
    pub fn validate(&self) -> Result<()> {
        validate_version(self.contract_version)?;
        validate_name("pgvector delete identity", &self.external_id)?;
        if self.source_cursor == 0 {
            return invalid("pgvector delete source cursor must be non-zero");
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>> {
        self.validate()?;
        serde_json::to_vec(self).map_err(|error| Error::InvalidRuntime {
            reason: format!("encode canonical pgvector delete payload: {error}"),
        })
    }
}

/// A synchronous live adapter. It deliberately owns one PostgreSQL client so
/// each trait call has one transaction and cannot accidentally interleave
/// another project's statements on the same session.
pub struct PgvectorLiveAdapter {
    client: Client,
    deployment: PgvectorDeployment,
    binding: OperatorKnowledgeBinding,
    binding_digest: String,
    tenant: String,
    descriptor: OperatorAdapterDescriptor,
}

impl PgvectorLiveAdapter {
    /// Connects with certificate- and hostname-validating native TLS. The
    /// connection configuration must explicitly require TLS; `prefer` is
    /// rejected because it can downgrade to plaintext.
    pub fn connect_tls(
        params: &str,
        root_certificate_pem: Option<&[u8]>,
        deployment: PgvectorDeployment,
        binding: OperatorKnowledgeBinding,
        tenant: impl Into<String>,
    ) -> Result<Self> {
        let config = require_tls_config(params)?;
        let mut connector = TlsConnector::builder();
        if let Some(pem) = root_certificate_pem {
            let certificate = Certificate::from_pem(pem)
                .map_err(|error| runtime_error("parse pgvector root certificate", error))?;
            connector.add_root_certificate(certificate);
        }
        let connector = connector
            .build()
            .map_err(|error| runtime_error("build pgvector TLS connector", error))?;
        let client = config
            .connect(MakeTlsConnector::new(connector))
            .map_err(|error| pg_error("connect pgvector TLS session", error))?;
        Self::from_client(client, deployment, binding, tenant)
    }

    /// Adopts an already-authenticated client. This is also the explicit seam
    /// used by local endpoint tests; production callers should prefer
    /// `connect_tls`.
    pub fn from_client(
        mut client: Client,
        deployment: PgvectorDeployment,
        binding: OperatorKnowledgeBinding,
        tenant: impl Into<String>,
    ) -> Result<Self> {
        deployment.validate()?;
        binding.validate()?;
        let tenant = tenant.into();
        validate_name("pgvector tenant value", &tenant)?;
        if binding.adapter != "pgvector"
            || deployment.digest()? != binding.config_digest
            || deployment.relation.digest()? != binding.relation_digest
            || digest::sha256_hex(tenant.as_bytes()) != binding.tenant_digest
        {
            return invalid("pgvector deployment or tenant differs from the immutable binding");
        }
        verify_live_schema(&mut client, &deployment, &binding)?;
        let descriptor = descriptor();
        let binding_digest = binding.digest()?;
        Ok(Self {
            client,
            deployment,
            binding,
            binding_digest,
            tenant,
            descriptor,
        })
    }

    /// Installs only Vyrm's control schema and the project revision row. The
    /// operator knowledge relation remains application-owned. Existing source
    /// identity cannot be overwritten by a different deployment.
    pub fn install_control_schema(
        client: &mut Client,
        deployment: &PgvectorDeployment,
        binding: &OperatorKnowledgeBinding,
    ) -> Result<()> {
        deployment.validate()?;
        binding.validate()?;
        if binding.adapter != "pgvector"
            || deployment.digest()? != binding.config_digest
            || deployment.relation.digest()? != binding.relation_digest
        {
            return invalid("pgvector control migration differs from the immutable binding");
        }
        let schema = quote_identifier(&deployment.control_schema);
        let metadata = qualified(&deployment.control_schema, &deployment.metadata_table);
        let revision = qualified(&deployment.control_schema, &deployment.revision_table);
        let applied = qualified(&deployment.control_schema, &deployment.applied_work_table);
        let ddl = format!(
            "CREATE SCHEMA IF NOT EXISTS {schema};\
             CREATE TABLE IF NOT EXISTS {metadata} (singleton boolean PRIMARY KEY CHECK (singleton), contract_version integer NOT NULL, source_identity_digest text NOT NULL);\
             CREATE TABLE IF NOT EXISTS {revision} (project_digest text PRIMARY KEY, stable_revision bigint NOT NULL CHECK (stable_revision >= 0));\
             CREATE TABLE IF NOT EXISTS {applied} (work_id text PRIMARY KEY, project_digest text NOT NULL, payload_digest text NOT NULL, source_cursor bigint NOT NULL CHECK (source_cursor > 0), revision jsonb NOT NULL);"
        );
        let mut transaction = client
            .build_transaction()
            .isolation_level(IsolationLevel::Serializable)
            .start()
            .map_err(|error| pg_error("start pgvector control migration", error))?;
        transaction
            .batch_execute(&ddl)
            .map_err(|error| pg_error("install pgvector control schema", error))?;
        let metadata_sql = format!(
            "INSERT INTO {metadata} (singleton, contract_version, source_identity_digest) VALUES (true, $1, $2) ON CONFLICT (singleton) DO NOTHING"
        );
        transaction
            .execute(
                &metadata_sql,
                &[
                    &i32::from(OPERATOR_KNOWLEDGE_CONTRACT_VERSION),
                    &binding.source_identity_digest,
                ],
            )
            .map_err(|error| pg_error("seed pgvector source identity", error))?;
        verify_source_identity(&mut transaction, deployment, binding)?;
        let revision_sql = format!(
            "INSERT INTO {revision} (project_digest, stable_revision) VALUES ($1, 0) ON CONFLICT (project_digest) DO NOTHING"
        );
        transaction
            .execute(&revision_sql, &[&project_digest(binding)])
            .map_err(|error| pg_error("seed pgvector project revision", error))?;
        transaction
            .commit()
            .map_err(|error| pg_error("commit pgvector control migration", error))
    }

    pub fn into_client(self) -> Client {
        self.client
    }
}

impl OperatorKnowledgeAdapter for PgvectorLiveAdapter {
    fn descriptor(&self) -> &OperatorAdapterDescriptor {
        &self.descriptor
    }

    fn search(
        &mut self,
        binding: &OperatorKnowledgeBinding,
        request: &OperatorSearchRequest,
    ) -> Result<OperatorSearchResult> {
        if binding.digest()? != self.binding_digest {
            return invalid("live pgvector adapter is bound to another project");
        }
        let vector = match &request.search.query {
            VectorQuery::Dense { values } => vector_literal(values),
            _ => return invalid("live pgvector v1 accepts dense vectors only"),
        };
        let started = Instant::now();
        let deployment = self.deployment.clone();
        let stored_binding = self.binding.clone();
        let tenant = self.tenant.clone();
        let mut transaction = self
            .client
            .build_transaction()
            .isolation_level(IsolationLevel::RepeatableRead)
            .read_only(true)
            .start()
            .map_err(|error| pg_error("start pgvector repeatable-read search", error))?;
        verify_source_identity(&mut transaction, &deployment, &stored_binding)?;
        let portable_plan = deployment
            .relation
            .build_search(request.search.metric, &request.controls)?;
        for (name, value) in &portable_plan.settings {
            transaction
                .query_one("SELECT set_config($1, $2, true)", &[name, value])
                .map_err(|error| pg_error("apply pgvector local search control", error))?;
        }
        let sql = live_search_sql(&deployment, request.search.metric);
        let explain_sql = format!("EXPLAIN (FORMAT JSON) {sql}");
        let top_k = i64::try_from(request.search.top_k).map_err(|_| Error::InvalidRuntime {
            reason: "pgvector top_k does not fit bigint".into(),
        })?;
        let read_cursor = i64::try_from(request.search.read.commit_cursor).map_err(|_| {
            Error::InvalidRuntime {
                reason: "pgvector read cursor does not fit bigint".into(),
            }
        })?;
        let parameters: [&(dyn postgres::types::ToSql + Sync); 5] = [
            &vector,
            &tenant,
            &stored_binding.model.digest,
            &read_cursor,
            &top_k,
        ];
        let explain: Value = transaction
            .query_one(&explain_sql, &parameters)
            .map_err(|error| pg_error("explain pgvector search", error))?
            .get(0);
        let catalog = capture_catalog(&mut transaction, &deployment)?;
        let (selected_path, selected_index) = selected_path(&explain, &catalog);
        let rows = transaction
            .query(&sql, &parameters)
            .map_err(|error| pg_error("execute pgvector search", error))?;
        let revision = capture_revision(&mut transaction, &deployment, &stored_binding, &catalog)?;
        transaction
            .commit()
            .map_err(|error| pg_error("commit pgvector search snapshot", error))?;
        let fallback_reason_digest =
            (selected_path != request.controls.requested_path).then(|| {
                digest::sha256_hex(
                    format!(
                        "pgvector planner selected {selected_path:?} instead of {:?}",
                        request.controls.requested_path
                    )
                    .as_bytes(),
                )
            });
        let index_digest =
            selected_index.map(|index| digest::sha256_hex(index.definition.as_bytes()));
        let plan_digest = content_digest(b"vyrm-pgvector-explain-v1\0", &explain)?;
        Ok(OperatorSearchResult {
            contract_version: OPERATOR_KNOWLEDGE_CONTRACT_VERSION,
            request_digest: request.digest()?,
            revision,
            plan: OperatorPlanEvidence {
                selected_path,
                plan_digest,
                index_digest,
                fallback_reason_digest,
                controls: request.controls.clone(),
                filter_applied_after_ann: selected_path != OperatorAccessPath::Exact,
                ordering_exact: selected_path == OperatorAccessPath::Exact
                    || request.controls.iterative_scan != IterativeScanMode::RelaxedOrder,
                candidates_examined: None,
            },
            hits: rows
                .into_iter()
                .map(|row| OperatorSearchHit {
                    external_id: row.get(0),
                    subject_id: row.get(1),
                    score: row.get(2),
                })
                .collect(),
            elapsed_micros: started.elapsed().as_micros().try_into().unwrap_or(u64::MAX),
        })
    }
}

impl OperatorKnowledgeWriter for PgvectorLiveAdapter {
    fn descriptor(&self) -> &OperatorAdapterDescriptor {
        &self.descriptor
    }

    fn apply(
        &mut self,
        binding: &OperatorKnowledgeBinding,
        work: &OperatorSyncWork,
        payload: &[u8],
    ) -> Result<OperatorSyncReceipt> {
        if binding.digest()? != self.binding_digest {
            return invalid("live pgvector writer is bound to another project");
        }
        let (external_id, source_cursor, operation) = match work.operation {
            OperatorSyncOperation::UpsertVector => {
                let payload: PgvectorSyncPayload =
                    serde_json::from_slice(payload).map_err(|error| Error::InvalidRuntime {
                        reason: format!("decode canonical pgvector sync payload: {error}"),
                    })?;
                payload.validate(binding.dimensions)?;
                (
                    payload.external_id.clone(),
                    payload.source_cursor,
                    LiveSyncOperation::Upsert(payload),
                )
            }
            OperatorSyncOperation::DeleteVector => {
                let payload: PgvectorDeletePayload =
                    serde_json::from_slice(payload).map_err(|error| Error::InvalidRuntime {
                        reason: format!("decode canonical pgvector delete payload: {error}"),
                    })?;
                payload.validate()?;
                (
                    payload.external_id.clone(),
                    payload.source_cursor,
                    LiveSyncOperation::Delete,
                )
            }
        };
        if source_cursor != work.source_cursor {
            return invalid("pgvector payload cursor differs from its outbox work");
        }
        let deployment = self.deployment.clone();
        let stored_binding = self.binding.clone();
        let tenant = self.tenant.clone();
        let mut transaction = self
            .client
            .build_transaction()
            .isolation_level(IsolationLevel::Serializable)
            .start()
            .map_err(|error| pg_error("start pgvector sync transaction", error))?;
        verify_source_identity(&mut transaction, &deployment, &stored_binding)?;
        transaction
            .query_one(
                "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
                &[&project_digest(&stored_binding)],
            )
            .map_err(|error| pg_error("lock pgvector project sync lane", error))?;
        if let Some(revision) =
            load_applied_revision(&mut transaction, &deployment, &stored_binding, work)?
        {
            transaction
                .commit()
                .map_err(|error| pg_error("commit pgvector idempotent replay", error))?;
            return Ok(OperatorSyncReceipt {
                contract_version: OPERATOR_KNOWLEDGE_CONTRACT_VERSION,
                work_id: work.id.clone(),
                revision,
                applied_now: false,
                idempotent_replay: true,
            });
        }
        deny_newer_row(
            &mut transaction,
            &deployment,
            &tenant,
            &external_id,
            work.source_cursor,
        )?;
        match operation {
            LiveSyncOperation::Upsert(payload) => apply_vector(
                &mut transaction,
                &deployment,
                &stored_binding,
                &tenant,
                &payload,
            )?,
            LiveSyncOperation::Delete => delete_vector(
                &mut transaction,
                &deployment,
                &tenant,
                &external_id,
                source_cursor,
            )?,
        }
        bump_project_revision(&mut transaction, &deployment, &stored_binding)?;
        let catalog = capture_catalog(&mut transaction, &deployment)?;
        let revision = capture_revision(&mut transaction, &deployment, &stored_binding, &catalog)?;
        store_applied_revision(
            &mut transaction,
            &deployment,
            &stored_binding,
            work,
            &revision,
        )?;
        transaction
            .commit()
            .map_err(|error| pg_error("commit pgvector sync transaction", error))?;
        Ok(OperatorSyncReceipt {
            contract_version: OPERATOR_KNOWLEDGE_CONTRACT_VERSION,
            work_id: work.id.clone(),
            revision,
            applied_now: true,
            idempotent_replay: false,
        })
    }
}

#[derive(Serialize)]
struct CatalogEvidence {
    extension_version: String,
    relation_oid: u32,
    columns: Vec<(i16, String, String)>,
    indexes: Vec<IndexEvidence>,
}

enum LiveSyncOperation {
    Upsert(PgvectorSyncPayload),
    Delete,
}

#[derive(Serialize)]
struct IndexEvidence {
    name: String,
    access_method: String,
    definition: String,
}

fn descriptor() -> OperatorAdapterDescriptor {
    let all = BTreeSet::from([
        ScoreMetric::Cosine,
        ScoreMetric::Dot,
        ScoreMetric::Euclidean,
        ScoreMetric::Manhattan,
    ]);
    OperatorAdapterDescriptor {
        contract_version: OPERATOR_KNOWLEDGE_CONTRACT_VERSION,
        adapter: "pgvector".into(),
        implementation_digest: digest::sha256_hex(PGVECTOR_LIVE_IMPLEMENTATION_VERSION.as_bytes()),
        max_dimensions: 2_000,
        vector_kinds: BTreeSet::from([OperatorVectorKind::Dense]),
        search_capabilities: BTreeMap::from([
            (OperatorAccessPath::Exact, all.clone()),
            (OperatorAccessPath::Hnsw, all),
            (
                OperatorAccessPath::IvfFlat,
                BTreeSet::from([
                    ScoreMetric::Cosine,
                    ScoreMetric::Dot,
                    ScoreMetric::Euclidean,
                ]),
            ),
        ]),
        supports_tenant_filter: true,
        supports_payload_filter: false,
        supports_stable_revision: true,
    }
}

fn verify_live_schema<C: GenericClient>(
    client: &mut C,
    deployment: &PgvectorDeployment,
    binding: &OperatorKnowledgeBinding,
) -> Result<()> {
    verify_source_identity(client, deployment, binding)?;
    let catalog = capture_catalog(client, deployment)?;
    let vector_type = catalog
        .columns
        .iter()
        .find(|(_, name, _)| name == &deployment.relation.vector_column)
        .map(|(_, _, kind)| kind.as_str());
    let cursor_type = catalog
        .columns
        .iter()
        .find(|(_, name, _)| name == &deployment.source_cursor_column)
        .map(|(_, _, kind)| kind.as_str());
    let required = [
        &deployment.relation.id_column,
        &deployment.relation.subject_column,
        &deployment.relation.tenant_column,
        &deployment.model_digest_column,
    ];
    if !matches!(vector_type, Some(kind) if kind.starts_with("vector("))
        || cursor_type != Some("bigint")
        || required
            .iter()
            .any(|name| !catalog.columns.iter().any(|(_, column, _)| column == *name))
    {
        return invalid(
            "pgvector live relation is missing its bound vector/model/tenant/cursor shape",
        );
    }
    let revision_sql = format!(
        "SELECT stable_revision FROM {} WHERE project_digest = $1",
        qualified(&deployment.control_schema, &deployment.revision_table)
    );
    if client
        .query_opt(&revision_sql, &[&project_digest(binding)])
        .map_err(|error| pg_error("verify pgvector project revision", error))?
        .is_none()
    {
        return invalid("pgvector project revision is not installed");
    }
    Ok(())
}

fn verify_source_identity<C: GenericClient>(
    client: &mut C,
    deployment: &PgvectorDeployment,
    binding: &OperatorKnowledgeBinding,
) -> Result<()> {
    let sql = format!(
        "SELECT contract_version, source_identity_digest FROM {} WHERE singleton = true",
        qualified(&deployment.control_schema, &deployment.metadata_table)
    );
    let row = client
        .query_opt(&sql, &[])
        .map_err(|error| pg_error("read pgvector source identity", error))?
        .ok_or_else(|| Error::InvalidRuntime {
            reason: "pgvector source identity is not installed".into(),
        })?;
    let version: i32 = row.get(0);
    let source: String = row.get(1);
    if version != i32::from(OPERATOR_KNOWLEDGE_CONTRACT_VERSION)
        || source != binding.source_identity_digest
    {
        return invalid("pgvector live source identity differs from the project binding");
    }
    Ok(())
}

fn capture_catalog<C: GenericClient>(
    client: &mut C,
    deployment: &PgvectorDeployment,
) -> Result<CatalogEvidence> {
    let extension_version: String = client
        .query_opt(
            "SELECT extversion FROM pg_extension WHERE extname = 'vector'",
            &[],
        )
        .map_err(|error| pg_error("read pgvector extension version", error))?
        .ok_or_else(|| Error::InvalidRuntime {
            reason: "pgvector extension is not installed".into(),
        })?
        .get(0);
    require_pgvector_08(&extension_version)?;
    let relation = client
        .query_opt(
            "SELECT c.oid FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = $1 AND c.relname = $2",
            &[&deployment.relation.schema, &deployment.relation.relation],
        )
        .map_err(|error| pg_error("resolve pgvector relation", error))?
        .ok_or_else(|| Error::InvalidRuntime {
            reason: "bound pgvector relation does not exist".into(),
        })?;
    let relation_oid: u32 = relation.get(0);
    let columns = client
        .query(
            "SELECT attnum, attname, format_type(atttypid, atttypmod) FROM pg_attribute WHERE attrelid = $1 AND attnum > 0 AND NOT attisdropped ORDER BY attnum",
            &[&relation_oid],
        )
        .map_err(|error| pg_error("inspect pgvector columns", error))?
        .into_iter()
        .map(|row| (row.get(0), row.get(1), row.get(2)))
        .collect();
    let indexes = client
        .query(
            "SELECT ic.relname, am.amname, pg_get_indexdef(i.indexrelid) FROM pg_index i JOIN pg_class ic ON ic.oid = i.indexrelid JOIN pg_am am ON am.oid = ic.relam WHERE i.indrelid = $1 ORDER BY ic.relname",
            &[&relation_oid],
        )
        .map_err(|error| pg_error("inspect pgvector indexes", error))?
        .into_iter()
        .map(|row| IndexEvidence {
            name: row.get(0),
            access_method: row.get(1),
            definition: row.get(2),
        })
        .collect();
    Ok(CatalogEvidence {
        extension_version,
        relation_oid,
        columns,
        indexes,
    })
}

fn capture_revision<C: GenericClient>(
    client: &mut C,
    deployment: &PgvectorDeployment,
    binding: &OperatorKnowledgeBinding,
    catalog: &CatalogEvidence,
) -> Result<OperatorSourceRevision> {
    let row = client
        .query_one(
            "SELECT pg_current_snapshot()::text, pg_current_wal_lsn()::text",
            &[],
        )
        .map_err(|error| pg_error("capture pgvector snapshot", error))?;
    let snapshot: String = row.get(0);
    let wal_lsn: String = row.get(1);
    let revision_sql = format!(
        "SELECT stable_revision FROM {} WHERE project_digest = $1",
        qualified(&deployment.control_schema, &deployment.revision_table)
    );
    let stable_revision: i64 = client
        .query_one(&revision_sql, &[&project_digest(binding)])
        .map_err(|error| pg_error("capture pgvector stable revision", error))?
        .get(0);
    Ok(OperatorSourceRevision {
        contract_version: OPERATOR_KNOWLEDGE_CONTRACT_VERSION,
        adapter: binding.adapter.clone(),
        project_id: binding.project_id.clone(),
        source_identity_digest: binding.source_identity_digest.clone(),
        snapshot_digest: digest::sha256_hex(snapshot.as_bytes()),
        catalog_digest: content_digest(b"vyrm-pgvector-catalog-v1\0", catalog)?,
        stable_revision: Some(stable_revision.to_string()),
        wal_lsn_digest: Some(digest::sha256_hex(wal_lsn.as_bytes())),
    })
}

fn live_search_sql(deployment: &PgvectorDeployment, metric: ScoreMetric) -> String {
    let relation = qualified(&deployment.relation.schema, &deployment.relation.relation);
    let id = quote_identifier(&deployment.relation.id_column);
    let subject = quote_identifier(&deployment.relation.subject_column);
    let vector = quote_identifier(&deployment.relation.vector_column);
    let tenant = quote_identifier(&deployment.relation.tenant_column);
    let model = quote_identifier(&deployment.model_digest_column);
    let cursor = quote_identifier(&deployment.source_cursor_column);
    let (operator, score) = match metric {
        ScoreMetric::Cosine => ("<=>", "1.0 - distance"),
        ScoreMetric::Dot => ("<#>", "distance * -1.0"),
        ScoreMetric::Euclidean => ("<->", "distance * -1.0"),
        ScoreMetric::Manhattan => ("<+>", "distance * -1.0"),
    };
    format!(
        "SELECT external_id, subject_id, {score} AS score FROM (SELECT {id}::text AS external_id, {subject}::text AS subject_id, {vector} {operator} $1::text::vector AS distance FROM {relation} WHERE {tenant}::text = $2 AND {model}::text = $3 AND {cursor} <= $4 ORDER BY {vector} {operator} $1::text::vector LIMIT $5) AS vyrm_ranked ORDER BY distance ASC, external_id ASC"
    )
}

fn selected_path<'a>(
    explain: &Value,
    catalog: &'a CatalogEvidence,
) -> (OperatorAccessPath, Option<&'a IndexEvidence>) {
    for index in &catalog.indexes {
        if explain_contains_index(explain, &index.name) {
            return match index.access_method.as_str() {
                "hnsw" => (OperatorAccessPath::Hnsw, Some(index)),
                "ivfflat" => (OperatorAccessPath::IvfFlat, Some(index)),
                _ => continue,
            };
        }
    }
    (OperatorAccessPath::Exact, None)
}

fn explain_contains_index(value: &Value, expected: &str) -> bool {
    match value {
        Value::Object(map) => {
            map.get("Index Name").and_then(Value::as_str) == Some(expected)
                || map
                    .values()
                    .any(|value| explain_contains_index(value, expected))
        }
        Value::Array(values) => values
            .iter()
            .any(|value| explain_contains_index(value, expected)),
        _ => false,
    }
}

fn deny_newer_row(
    transaction: &mut Transaction<'_>,
    deployment: &PgvectorDeployment,
    tenant: &str,
    external_id: &str,
    source_cursor: u64,
) -> Result<()> {
    let sql = format!(
        "SELECT {} FROM {} WHERE {}::text = $1 AND {}::text = $2 FOR UPDATE",
        quote_identifier(&deployment.source_cursor_column),
        qualified(&deployment.relation.schema, &deployment.relation.relation),
        quote_identifier(&deployment.relation.tenant_column),
        quote_identifier(&deployment.relation.id_column),
    );
    let current: Option<i64> = transaction
        .query_opt(&sql, &[&tenant, &external_id])
        .map_err(|error| pg_error("check pgvector row freshness", error))?
        .map(|row| row.get(0));
    let cursor = i64::try_from(source_cursor).map_err(|_| Error::InvalidRuntime {
        reason: "pgvector source cursor does not fit bigint".into(),
    })?;
    if current.is_some_and(|current| current > cursor) {
        return invalid("pgvector sync work is older than the external row");
    }
    Ok(())
}

fn apply_vector(
    transaction: &mut Transaction<'_>,
    deployment: &PgvectorDeployment,
    binding: &OperatorKnowledgeBinding,
    tenant_value: &str,
    payload: &PgvectorSyncPayload,
) -> Result<()> {
    let tenant = quote_identifier(&deployment.relation.tenant_column);
    let id = quote_identifier(&deployment.relation.id_column);
    let subject = quote_identifier(&deployment.relation.subject_column);
    let vector = quote_identifier(&deployment.relation.vector_column);
    let model = quote_identifier(&deployment.model_digest_column);
    let cursor = quote_identifier(&deployment.source_cursor_column);
    let relation = qualified(&deployment.relation.schema, &deployment.relation.relation);
    let sql = format!(
        "INSERT INTO {relation} ({tenant}, {id}, {subject}, {vector}, {model}, {cursor}) VALUES ($1, $2, $3, $4::text::vector, $5, $6) ON CONFLICT ({tenant}, {id}) DO UPDATE SET {subject} = EXCLUDED.{subject}, {vector} = EXCLUDED.{vector}, {model} = EXCLUDED.{model}, {cursor} = EXCLUDED.{cursor}"
    );
    let vector = vector_literal(&payload.vector);
    let source_cursor =
        i64::try_from(payload.source_cursor).map_err(|_| Error::InvalidRuntime {
            reason: "pgvector source cursor does not fit bigint".into(),
        })?;
    transaction
        .execute(
            &sql,
            &[
                &tenant_value,
                &payload.external_id,
                &payload.subject_id,
                &vector,
                &binding.model.digest,
                &source_cursor,
            ],
        )
        .map_err(|error| pg_error("apply pgvector row", error))?;
    Ok(())
}

fn delete_vector(
    transaction: &mut Transaction<'_>,
    deployment: &PgvectorDeployment,
    tenant_value: &str,
    external_id: &str,
    source_cursor: u64,
) -> Result<()> {
    let relation = qualified(&deployment.relation.schema, &deployment.relation.relation);
    let tenant = quote_identifier(&deployment.relation.tenant_column);
    let id = quote_identifier(&deployment.relation.id_column);
    let cursor = quote_identifier(&deployment.source_cursor_column);
    let sql = format!(
        "DELETE FROM {relation} WHERE {tenant}::text = $1 AND {id}::text = $2 AND {cursor} <= $3"
    );
    let source_cursor = i64::try_from(source_cursor).map_err(|_| Error::InvalidRuntime {
        reason: "pgvector source cursor does not fit bigint".into(),
    })?;
    transaction
        .execute(&sql, &[&tenant_value, &external_id, &source_cursor])
        .map_err(|error| pg_error("delete pgvector row", error))?;
    Ok(())
}

fn bump_project_revision(
    transaction: &mut Transaction<'_>,
    deployment: &PgvectorDeployment,
    binding: &OperatorKnowledgeBinding,
) -> Result<i64> {
    let sql = format!(
        "UPDATE {} SET stable_revision = stable_revision + 1 WHERE project_digest = $1 RETURNING stable_revision",
        qualified(&deployment.control_schema, &deployment.revision_table)
    );
    transaction
        .query_opt(&sql, &[&project_digest(binding)])
        .map_err(|error| pg_error("advance pgvector project revision", error))?
        .map(|row| row.get(0))
        .ok_or_else(|| Error::InvalidRuntime {
            reason: "pgvector project revision disappeared".into(),
        })
}

fn load_applied_revision(
    transaction: &mut Transaction<'_>,
    deployment: &PgvectorDeployment,
    binding: &OperatorKnowledgeBinding,
    work: &OperatorSyncWork,
) -> Result<Option<OperatorSourceRevision>> {
    let sql = format!(
        "SELECT project_digest, payload_digest, source_cursor, revision FROM {} WHERE work_id = $1",
        qualified(&deployment.control_schema, &deployment.applied_work_table)
    );
    let Some(row) = transaction
        .query_opt(&sql, &[&work.id])
        .map_err(|error| pg_error("read pgvector applied work", error))?
    else {
        return Ok(None);
    };
    let project: String = row.get(0);
    let payload: String = row.get(1);
    let source_cursor: i64 = row.get(2);
    let revision: Value = row.get(3);
    if project != project_digest(binding)
        || payload != work.payload_digest
        || source_cursor != i64::try_from(work.source_cursor).unwrap_or(-1)
    {
        return invalid("pgvector idempotency identity collides with different work");
    }
    let revision = serde_json::from_value(revision).map_err(|error| Error::InvalidRuntime {
        reason: format!("decode pgvector stored revision: {error}"),
    })?;
    Ok(Some(revision))
}

fn store_applied_revision(
    transaction: &mut Transaction<'_>,
    deployment: &PgvectorDeployment,
    binding: &OperatorKnowledgeBinding,
    work: &OperatorSyncWork,
    revision: &OperatorSourceRevision,
) -> Result<()> {
    let sql = format!(
        "INSERT INTO {} (work_id, project_digest, payload_digest, source_cursor, revision) VALUES ($1, $2, $3, $4, $5)",
        qualified(&deployment.control_schema, &deployment.applied_work_table)
    );
    let source_cursor = i64::try_from(work.source_cursor).map_err(|_| Error::InvalidRuntime {
        reason: "pgvector source cursor does not fit bigint".into(),
    })?;
    let revision = serde_json::to_value(revision).map_err(|error| Error::InvalidRuntime {
        reason: format!("encode pgvector stored revision: {error}"),
    })?;
    transaction
        .execute(
            &sql,
            &[
                &work.id,
                &project_digest(binding),
                &work.payload_digest,
                &source_cursor,
                &revision,
            ],
        )
        .map_err(|error| pg_error("record pgvector applied work", error))?;
    Ok(())
}

fn require_pgvector_08(version: &str) -> Result<()> {
    let mut parts = version.split('.');
    let major = parts.next().and_then(|part| part.parse::<u32>().ok());
    let minor = parts.next().and_then(|part| part.parse::<u32>().ok());
    if !matches!((major, minor), (Some(major), Some(minor)) if major > 0 || minor >= 8) {
        return invalid("live adapter requires pgvector 0.8 or newer");
    }
    Ok(())
}

fn require_tls_config(params: &str) -> Result<Config> {
    let config = Config::from_str(params).map_err(|error| pg_error("parse TLS config", error))?;
    if config.get_ssl_mode() != SslMode::Require {
        return invalid("pgvector TLS connection must set sslmode=require");
    }
    Ok(config)
}

fn vector_literal(values: &[f32]) -> String {
    let mut literal = String::from("[");
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            literal.push(',');
        }
        literal.push_str(&value.to_string());
    }
    literal.push(']');
    literal
}

fn project_digest(binding: &OperatorKnowledgeBinding) -> String {
    digest::sha256_hex(binding.project_id.as_bytes())
}

fn qualified(schema: &str, relation: &str) -> String {
    format!(
        "{}.{}",
        quote_identifier(schema),
        quote_identifier(relation)
    )
}

fn pg_error(context: &'static str, error: postgres::Error) -> Error {
    if let Some(database) = error.as_db_error() {
        return Error::InvalidRuntime {
            reason: format!(
                "{context}: PostgreSQL {}: {}",
                database.code().code(),
                database.message()
            ),
        };
    }
    runtime_error(context, error)
}

fn runtime_error(context: &'static str, error: impl std::fmt::Display) -> Error {
    Error::InvalidRuntime {
        reason: format!("{context}: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deployment_rejects_ambiguous_columns_and_old_extensions() {
        let deployment = PgvectorDeployment {
            contract_version: OPERATOR_KNOWLEDGE_CONTRACT_VERSION,
            relation: PgvectorRelation {
                schema: "public".into(),
                relation: "knowledge".into(),
                id_column: "id".into(),
                subject_column: "subject".into(),
                vector_column: "embedding".into(),
                tenant_column: "tenant".into(),
            },
            model_digest_column: "embedding".into(),
            source_cursor_column: "source_cursor".into(),
            control_schema: "vyrm".into(),
            metadata_table: "metadata".into(),
            revision_table: "revision".into(),
            applied_work_table: "applied".into(),
        };
        assert!(deployment.validate().is_err());
        assert!(require_pgvector_08("0.7.4").is_err());
        assert!(require_pgvector_08("0.8.0").is_ok());
        assert!(require_pgvector_08("1.0.0").is_ok());
    }

    #[test]
    fn explain_path_detection_ignores_non_vector_indexes() {
        let explain =
            serde_json::json!([{"Plan": {"Node Type": "Index Scan", "Index Name": "tenant_idx"}}]);
        let catalog = CatalogEvidence {
            extension_version: "0.8.1".into(),
            relation_oid: 42,
            columns: Vec::new(),
            indexes: vec![
                IndexEvidence {
                    name: "tenant_idx".into(),
                    access_method: "btree".into(),
                    definition: "CREATE INDEX tenant_idx".into(),
                },
                IndexEvidence {
                    name: "knowledge_hnsw".into(),
                    access_method: "hnsw".into(),
                    definition: "CREATE INDEX knowledge_hnsw".into(),
                },
            ],
        };
        assert_eq!(
            selected_path(&explain, &catalog).0,
            OperatorAccessPath::Exact
        );
    }

    #[test]
    fn tls_connection_contract_rejects_downgrade_modes_before_connecting() {
        assert!(require_tls_config("host=localhost sslmode=disable").is_err());
        assert!(require_tls_config("host=localhost sslmode=prefer").is_err());
        assert_eq!(
            require_tls_config("host=localhost sslmode=require")
                .unwrap()
                .get_ssl_mode(),
            SslMode::Require
        );
    }
}
