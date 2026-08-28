use super::*;

const BACKUP_OPERATION_FORMAT: u16 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BackupOperationState {
    format_version: u16,
    operation_sha256: String,
    storage_label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    result: Option<CreateInstanceBackupResult>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RestoreOperationState {
    format_version: u16,
    operation_sha256: String,
    backup_sha256: String,
    restore_id: CanonicalId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    result: Option<RestoreInstanceBackupResult>,
}

impl RrdEngine {
    #[allow(clippy::too_many_arguments)]
    pub fn create_instance_backup(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        idempotency_key: &CorrelationId,
        request: &CreateInstanceBackup,
        now: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<CreateInstanceBackupResult> {
        request
            .validate()
            .map_err(|error| ServiceError::Contract(error.to_string()))?;
        self.authorize(
            session_id,
            token,
            SecurityAction::BackupCreate,
            now,
            request_id,
            operation_id,
        )?;
        let operation_sha256 = operation_digest(request)?;
        let key = backup_operation_key(&self.instance, session_id, idempotency_key);
        let existing = self.storage.control_record(&key)?;
        let (prepared_bytes, mut state) = if let Some(bytes) = existing {
            let state: BackupOperationState =
                serde_json::from_slice(&bytes).map_err(contract_json)?;
            if state.format_version != BACKUP_OPERATION_FORMAT
                || state.operation_sha256 != operation_sha256
            {
                return Err(ServiceError::IdempotencyConflict);
            }
            if let Some(mut result) = state.result {
                result.idempotent_replay = true;
                return Ok(result);
            }
            (bytes, state)
        } else {
            let state = BackupOperationState {
                format_version: BACKUP_OPERATION_FORMAT,
                operation_sha256: operation_sha256.clone(),
                storage_label: format!("{}--{}", request.label, &operation_sha256[..16]),
                result: None,
            };
            let bytes = serde_json::to_vec(&state).map_err(contract_json)?;
            self.storage.commit_control_transition(&ControlTransition {
                key: key.clone(),
                expected: None,
                replacement: Some(bytes.clone()),
                at: now,
                actor: format!("session:{}", session_id.as_str()),
                action: "backup.prepared".into(),
                request_id: request_id.into(),
                operation_id: operation_id.into(),
            })?;
            (bytes, state)
        };

        let root = self.backup_root();
        let catalogue = rrd_store::verify_backup_catalogue(&root)?;
        let matching = catalogue
            .backups
            .iter()
            .filter(|entry| entry.label == state.storage_label)
            .collect::<Vec<_>>();
        if matching.len() > 1 {
            return Err(ServiceError::Backup(
                "backup operation label resolved to multiple catalogue entries".into(),
            ));
        }
        let entry = match matching.first() {
            Some(entry) => (*entry).clone(),
            None => rrd_store::create_application_backup(
                &self.storage,
                &self.objects,
                &root,
                &state.storage_label,
                request.created_at_unix_ms,
            )?,
        };
        let catalogue = rrd_store::verify_backup_catalogue(&root)?;
        let result = CreateInstanceBackupResult {
            backup: public_backup(&entry),
            catalogue_revision: catalogue.revision,
            catalogue_sha256: catalogue.catalogue_sha256,
            idempotent_replay: false,
        };
        state.result = Some(result.clone());
        commit_engine_control(
            &self.storage,
            key,
            Some(prepared_bytes),
            &state,
            EngineControlContext {
                now,
                session_id,
                action: "backup.completed",
                request_id,
                operation_id,
            },
        )?;
        Ok(result)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn list_instance_backups(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        request: &ListInstanceBackups,
        now: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<InstanceBackupCatalogueSnapshot> {
        self.authorize(
            session_id,
            token,
            SecurityAction::BackupList,
            now,
            request_id,
            operation_id,
        )?;
        let catalogue = if request.verify_archives {
            rrd_store::verify_backup_catalogue(&self.backup_root())?
        } else {
            rrd_store::load_backup_catalogue(&self.backup_root())?
        };
        Ok(public_backup_catalogue(&catalogue, request.verify_archives))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn restore_instance_backup(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        idempotency_key: &CorrelationId,
        request: &RestoreInstanceBackup,
        now: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<RestoreInstanceBackupResult> {
        request
            .validate()
            .map_err(|error| ServiceError::Contract(error.to_string()))?;
        self.authorize(
            session_id,
            token,
            SecurityAction::RestoreCreate,
            now,
            request_id,
            operation_id,
        )?;
        let operation_sha256 = operation_digest(request)?;
        let key = restore_operation_key(&self.instance, session_id, idempotency_key);
        let existing = self.storage.control_record(&key)?;
        let (prepared_bytes, mut state) = if let Some(bytes) = existing {
            let state: RestoreOperationState =
                serde_json::from_slice(&bytes).map_err(contract_json)?;
            if state.format_version != BACKUP_OPERATION_FORMAT
                || state.operation_sha256 != operation_sha256
                || state.backup_sha256 != request.backup_sha256
                || state.restore_id != request.restore_id
            {
                return Err(ServiceError::IdempotencyConflict);
            }
            if let Some(mut result) = state.result {
                result.idempotent_replay = true;
                return Ok(result);
            }
            (bytes, state)
        } else {
            let state = RestoreOperationState {
                format_version: BACKUP_OPERATION_FORMAT,
                operation_sha256,
                backup_sha256: request.backup_sha256.clone(),
                restore_id: request.restore_id.clone(),
                result: None,
            };
            let bytes = serde_json::to_vec(&state).map_err(contract_json)?;
            self.storage.commit_control_transition(&ControlTransition {
                key: key.clone(),
                expected: None,
                replacement: Some(bytes.clone()),
                at: now,
                actor: format!("session:{}", session_id.as_str()),
                action: "restore.prepared".into(),
                request_id: request_id.into(),
                operation_id: operation_id.into(),
            })?;
            (bytes, state)
        };

        let catalogue = rrd_store::verify_backup_catalogue(&self.backup_root())?;
        let backup = catalogue
            .backups
            .iter()
            .find(|entry| entry.backup_id == request.backup_sha256)
            .ok_or_else(|| ServiceError::Backup("backup is not catalogued".into()))?;
        let target = self.restore_root().join(request.restore_id.as_str());
        let (inventory, reopened, recovered) = if target.exists() {
            let restored = rrd_store::PersistentEngine::open(&target)?;
            if restored.sequence()? != backup.archive.claim_sequence
                || restored.runtime_cursor()? != backup.archive.runtime_cursor
            {
                return Err(ServiceError::Backup(
                    "existing restore target watermarks differ from the backup".into(),
                ));
            }
            rrd_store::verify_restored_backup_objects(
                &self.backup_root(),
                &request.backup_sha256,
                &target,
            )?;
            (backup.archive.clone(), true, true)
        } else {
            let report = rrd_store::restore_catalogued_backup(
                &self.backup_root(),
                &request.backup_sha256,
                &target,
                request.restored_at_unix_ms,
            )?;
            (report.inventory, report.reopened, false)
        };
        let result = RestoreInstanceBackupResult {
            backup_sha256: request.backup_sha256.clone(),
            restore_id: request.restore_id.clone(),
            inventory: public_archive(&inventory),
            reopened,
            idempotent_replay: recovered,
        };
        state.result = Some(result.clone());
        commit_engine_control(
            &self.storage,
            key,
            Some(prepared_bytes),
            &state,
            EngineControlContext {
                now,
                session_id,
                action: "restore.completed",
                request_id,
                operation_id,
            },
        )?;
        Ok(result)
    }

    fn operation_root(&self) -> PathBuf {
        let parent = self
            .storage
            .path()
            .parent()
            .unwrap_or_else(|| Path::new("."));
        parent.join("rrd-service").join(self.instance.as_str())
    }

    fn backup_root(&self) -> PathBuf {
        self.operation_root().join("backups")
    }

    fn restore_root(&self) -> PathBuf {
        self.operation_root().join("restores")
    }
}

fn backup_operation_key(
    instance: &CanonicalId,
    session: &CorrelationId,
    key: &CorrelationId,
) -> String {
    let identity =
        digest::sha256_hex(format!("backup\0{}\0{}", session.as_str(), key.as_str()).as_bytes());
    format!("server/state/{instance}/backup/{identity}")
}

fn restore_operation_key(
    instance: &CanonicalId,
    session: &CorrelationId,
    key: &CorrelationId,
) -> String {
    let identity =
        digest::sha256_hex(format!("restore\0{}\0{}", session.as_str(), key.as_str()).as_bytes());
    format!("server/state/{instance}/restore/{identity}")
}

fn public_backup_catalogue(
    catalogue: &rrd_store::BackupCatalogue,
    archives_verified: bool,
) -> InstanceBackupCatalogueSnapshot {
    InstanceBackupCatalogueSnapshot {
        format_version: catalogue.format_version,
        revision: catalogue.revision,
        catalogue_sha256: catalogue.catalogue_sha256.clone(),
        archives_verified,
        backups: catalogue.backups.iter().map(public_backup).collect(),
    }
}

fn public_backup(entry: &rrd_store::BackupEntry) -> InstanceBackupSnapshot {
    InstanceBackupSnapshot {
        backup_sha256: entry.backup_id.clone(),
        label: entry.label.clone(),
        created_at_unix_ms: entry.created_at,
        archive: public_archive(&entry.archive),
        claims: public_backup_coverage(entry.claims.clone()),
        typed_runtime: public_backup_coverage(entry.typed_runtime.clone()),
        catalogues: public_backup_coverage(entry.catalogues.clone()),
        object_payloads: public_backup_coverage(entry.object_payloads.clone()),
        projections: public_backup_coverage(entry.projections.clone()),
        invocation_telemetry: public_backup_coverage(entry.invocation_telemetry.clone()),
        snapshot_leases: public_backup_coverage(entry.snapshot_leases.clone()),
        application_complete: entry.application_complete,
    }
}

fn public_backup_coverage(
    value: rrd_store::BackupCoverage,
) -> rrd_contract::BackupCoverageSnapshot {
    match value {
        rrd_store::BackupCoverage::Included => rrd_contract::BackupCoverageSnapshot::Included,
        rrd_store::BackupCoverage::ReferencedOnly => {
            rrd_contract::BackupCoverageSnapshot::ReferencedOnly
        }
        rrd_store::BackupCoverage::RebuildRequired => {
            rrd_contract::BackupCoverageSnapshot::RebuildRequired
        }
        rrd_store::BackupCoverage::Excluded => rrd_contract::BackupCoverageSnapshot::Excluded,
    }
}

fn public_archive(inventory: &rrd_store::LogicalArchiveInventory) -> LogicalArchiveSnapshot {
    LogicalArchiveSnapshot {
        format_version: inventory.format_version,
        contract_version: inventory.contract_version,
        archive_sha256: inventory.archive_sha256.clone(),
        action_count: inventory.action_count,
        standalone_claims: inventory.standalone_claims,
        runtime_commits: inventory.runtime_commits,
        runtime_mutations: inventory.runtime_mutations,
        payload_bytes: inventory.payload_bytes,
        claim_sequence: inventory.claim_sequence,
        runtime_cursor: inventory.runtime_cursor,
        runtime_audit_sha256: inventory.runtime_audit_sha256.clone(),
    }
}
