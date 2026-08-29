use super::*;

impl RrdEngine {
    /// Read every logical model through one catalogue revision, valid-time
    /// instant, authenticated log root, and transaction cursor.
    #[allow(clippy::too_many_arguments)]
    pub fn read_data_snapshot(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        request: &ReadDataSnapshot,
        now: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<DataSnapshot> {
        request
            .validate()
            .map_err(|error| ServiceError::Contract(error.to_string()))?;
        self.authorize(
            session_id,
            token,
            SecurityAction::QueryExecute,
            now,
            request_id,
            operation_id,
        )?;
        let scope = ScopeId::new(format!("instance:{}", self.instance)).map_err(core_contract)?;
        let replay_limit = usize::try_from(request.max_scanned_changes)
            .map_err(|_| ServiceError::Query("data snapshot replay limit exceeds usize".into()))?;
        let (read, snapshot) =
            self.storage
                .runtime_data_snapshot(&scope, request.valid_at, replay_limit)?;

        let mut entries = Vec::with_capacity(
            snapshot.records.len()
                + snapshot.relations.len()
                + snapshot.events.len()
                + snapshot.vectors.len()
                + snapshot.series.len()
                + snapshot.geo.len()
                + snapshot.objects.len(),
        );
        for entry in snapshot.records {
            let reference = entry.value.reference.clone();
            entries.push(snapshot_entry(
                entry.model,
                reference,
                RuntimeMutation::Record {
                    record: entry.value,
                },
            )?);
        }
        for entry in snapshot.relations {
            let reference = entry.value.reference.clone();
            entries.push(snapshot_entry(
                entry.model,
                reference,
                RuntimeMutation::Relation {
                    relation: entry.value,
                },
            )?);
        }
        for entry in snapshot.events {
            let reference = entry.value.reference.clone();
            entries.push(snapshot_entry(
                entry.model,
                reference,
                RuntimeMutation::Event {
                    event: entry.value.event,
                },
            )?);
        }
        for entry in snapshot.vectors {
            let reference = entry.value.reference.clone();
            entries.push(snapshot_entry(
                entry.model,
                reference,
                RuntimeMutation::Vector {
                    vector: entry.value,
                },
            )?);
        }
        for entry in snapshot.series {
            let reference = entry.value.reference.clone();
            entries.push(snapshot_entry(
                entry.model,
                reference,
                RuntimeMutation::SeriesSample {
                    sample: entry.value,
                },
            )?);
        }
        for entry in snapshot.geo {
            let reference = entry.value.reference.clone();
            entries.push(snapshot_entry(
                entry.model,
                reference,
                RuntimeMutation::Geo { geo: entry.value },
            )?);
        }
        for entry in snapshot.objects {
            let reference = entry.value.reference.clone();
            entries.push(snapshot_entry(
                entry.model,
                reference,
                RuntimeMutation::Object {
                    object: entry.value,
                },
            )?);
        }

        Ok(DataSnapshot {
            scope: snapshot.scope.to_string(),
            valid_at: snapshot.valid_at,
            known_at_cursor: snapshot.known_at_cursor,
            schema_revision: snapshot.schema_revision,
            read_manifest_sha256: read.manifest_id,
            entries,
        })
    }
}

fn snapshot_entry(
    model: RuntimeLogicalModel,
    reference: RuntimeRef,
    mutation: RuntimeMutation,
) -> Result<DataSnapshotEntry> {
    Ok(DataSnapshotEntry {
        model: public_logical_model(model),
        target: public_data_target(model, &reference)?,
        value: public_data_mutation(&mutation)?,
    })
}
