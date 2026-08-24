use super::*;

impl RrflowEngine {
    pub fn read_estate(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        estate_id: CanonicalId,
        now: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<Option<rrd_contract::EstateSnapshot>> {
        self.authorize(
            session_id,
            token,
            SecurityAction::EstateRead,
            now,
            request_id,
            operation_id,
        )?;
        let repository = rrd_estate::EstateRepository::new(&self.storage, estate_id);
        repository
            .load()
            .map(|document| document.as_ref().map(rrd_estate::public_snapshot))
            .map_err(Into::into)
    }
}
