use super::*;

pub struct RrflowEngine {
    pub(in crate::engine) storage: PersistentEngine,
    pub(in crate::engine) instance: CanonicalId,
    pub(in crate::engine) token_key: [u8; 32],
}
impl RrflowEngine {
    pub(in crate::engine) fn query_scope(&self, requested: &str) -> Result<ScopeId> {
        let expected_scope = format!("instance:{}", self.instance);
        if requested != expected_scope {
            return Err(ServiceError::WrongScope);
        }
        ScopeId::new(requested.to_owned()).map_err(|error| ServiceError::Query(error.to_string()))
    }

    pub fn open(root: &Path, instance: CanonicalId, token_key: [u8; 32]) -> Result<Self> {
        Ok(Self {
            storage: PersistentEngine::open(root)?,
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
