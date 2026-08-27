use super::*;

pub struct RrdEngine {
    pub(crate) storage: PersistentEngine,
    pub(crate) objects: rrd_store::LocalObjectStore,
    pub(in crate::engine) instance: CanonicalId,
    pub(crate) token_key: [u8; 32],
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
            storage,
            objects,
            instance,
            token_key,
        })
    }

    pub fn instance_id(&self) -> &CanonicalId {
        &self.instance
    }

    pub fn readiness(&self, observed_at_unix_ms: u64) -> Result<Readiness> {
        Ok(Readiness {
            observed_at_unix_ms,
            claim_sequence: self.storage.sequence()?,
            runtime_cursor: self.storage.runtime_cursor()?,
            backend: CanonicalId::new(self.storage.backend().as_str())
                .map_err(|error| ServiceError::Contract(error.to_string()))?,
        })
    }
}
