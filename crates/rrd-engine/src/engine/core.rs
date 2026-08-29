use super::*;

pub struct RrdEngine {
    pub(crate) storage: EngineBox,
    pub(crate) objects: ObjectStoreBox,
    pub(crate) storage_root: Option<PathBuf>,
    pub(in crate::engine) instance: CanonicalId,
    pub(crate) token_key: [u8; 32],
    /// Serializes the local definition/validation/commit boundary. The native
    /// store already owns the cross-process writer lock; this closes the
    /// in-process race between publishing a unique index and committing data.
    pub(crate) transaction_gate: Mutex<()>,
    /// Tracks invocation reservations currently executing through a supported
    /// adapter. Session-backed methods use this to distinguish a correctly
    /// wrapped call from a direct embedded call and make the latter leave a
    /// durable authorization or denial record as well.
    pub(crate) active_invocations: Mutex<BTreeMap<(String, String), std::thread::ThreadId>>,
}
impl RrdEngine {
    /// Opens a local engine authority for engine-owned control/bootstrap
    /// operations that cannot yet rely on a project manifest. The constructor
    /// remains private to `rrd-engine`; outward adapters call typed operations.
    pub(in crate::engine) fn open_local_authority(
        root: &Path,
        instance: CanonicalId,
    ) -> Result<Self> {
        let mut engine = Self::open(root, instance, [0_u8; TOKEN_KEY_BYTES])?;
        engine.token_key = load_or_create_token_key(&root.join("RRD.SECRET"))
            .map_err(|error| ServiceError::Storage(error.to_string()))?;
        Ok(engine)
    }

    pub(in crate::engine) fn query_scope(&self, requested: &str) -> Result<ScopeId> {
        let expected_scope = format!("instance:{}", self.instance);
        if requested != expected_scope {
            return Err(ServiceError::WrongScope);
        }
        ScopeId::new(requested.to_owned()).map_err(|error| ServiceError::Query(error.to_string()))
    }

    pub fn open(root: &Path, instance: CanonicalId, token_key: [u8; 32]) -> Result<Self> {
        let storage = PersistentEngine::open(root)?;
        let objects = rrd_store::LocalObjectStore::open(root.join("immutable"))?;
        Ok(Self {
            storage: EngineBox::persistent(storage),
            objects: ObjectStoreBox::new(objects),
            storage_root: Some(root.to_path_buf()),
            instance,
            token_key,
            transaction_gate: Mutex::new(()),
            active_invocations: Mutex::new(BTreeMap::new()),
        })
    }

    /// Creates one process-local RRD authority with no persistent root. The
    /// full composition remains intact: sessions, transactions, policy,
    /// queries, audit, changefeeds, and subscriptions use the same engine
    /// methods as embedded and daemon profiles. Durability-only operations
    /// fail explicitly because this mode has no storage path.
    pub fn memory(instance: CanonicalId, token_key: [u8; 32]) -> Self {
        Self {
            storage: EngineBox::memory(),
            objects: ObjectStoreBox::new(MemoryObjectStore::new()),
            storage_root: None,
            instance,
            token_key,
            transaction_gate: Mutex::new(()),
            active_invocations: Mutex::new(BTreeMap::new()),
        }
    }

    pub fn has_persistent_root(&self) -> bool {
        self.storage_root.is_some()
    }

    pub fn deployment_mode(&self) -> DeploymentMode {
        if self.has_persistent_root() {
            DeploymentMode::Embedded
        } else {
            DeploymentMode::Memory
        }
    }

    pub(in crate::engine) fn require_persistent_root(&self, operation: &str) -> Result<&Path> {
        self.storage_root.as_deref().ok_or_else(|| {
            ServiceError::Backup(format!(
                "{operation} requires a persistent RRD root; memory mode is non-durable"
            ))
        })
    }

    pub fn instance_id(&self) -> &CanonicalId {
        &self.instance
    }

    pub fn readiness(&self, observed_at_unix_ms: u64) -> Result<Readiness> {
        let backend = self.storage.physical_store_evidence()?.backend;
        Ok(Readiness {
            observed_at_unix_ms,
            claim_sequence: self.storage.sequence()?,
            runtime_cursor: self.storage.runtime_cursor()?,
            backend: CanonicalId::new(backend)
                .map_err(|error| ServiceError::Contract(error.to_string()))?,
        })
    }
}
