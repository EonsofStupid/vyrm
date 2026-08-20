//! Backend-neutral immutable object hydration for a grounded replica transfer.

use crate::{
    ArtifactReplicaObjectReceipt, ArtifactTransferManifest, ArtifactTransferReceipt, ClusterError,
    Result,
};
use std::collections::{BTreeMap, BTreeSet};
use vyrm_core::{RuntimeMutation, ScopeId};
use vyrm_store::{Engine, Error as StoreError, ImmutableObjectStore};

const REPLAY_PAGE: usize = 4_096;

/// Captures every immutable object reference visible at one exact project read
/// and binds its digest closure into the replica transfer plan.
pub fn prepare_artifact_transfer<E: Engine>(
    mut plan: crate::ReplicaTransferPlan,
    engine: &E,
    scope: &ScopeId,
) -> Result<ArtifactTransferManifest> {
    plan.validate()?;
    let read = engine
        .runtime_read_stamp(scope)
        .map_err(cluster_store_error)?;
    let mut cursor = 0;
    let mut objects = BTreeMap::new();
    loop {
        let page = engine
            .runtime_read_changes(&read, cursor, REPLAY_PAGE)
            .map_err(cluster_store_error)?;
        for change in page.changes {
            if !change.verify_digest() {
                return Err(ClusterError::Invalid(
                    "artifact transfer replay encountered a corrupt change digest".into(),
                ));
            }
            if let RuntimeMutation::Object { object } = change.mutation {
                objects.insert(object.reference.clone(), object);
            }
        }
        if page.through_cursor == read.commit_cursor {
            break;
        }
        if page.through_cursor <= cursor {
            return Err(ClusterError::Unavailable(
                "artifact transfer replay did not reach its project read stamp".into(),
            ));
        }
        cursor = page.through_cursor;
    }
    let objects = objects.into_values().collect::<Vec<_>>();
    plan.artifact_digests = objects
        .iter()
        .map(|object| object.sha256.clone())
        .collect::<BTreeSet<_>>();
    ArtifactTransferManifest::new(plan, scope.clone(), read, objects)
}

/// Hydrates a target object tier from a verified source. Existing verified
/// content is reused; partial success remains harmless unreachable content and
/// no completion receipt exists until the entire manifest verifies.
pub fn transfer_artifacts<S, T>(
    source: &S,
    target: &T,
    manifest: &ArtifactTransferManifest,
    completed_at: u64,
) -> Result<ArtifactTransferReceipt>
where
    S: ImmutableObjectStore,
    T: ImmutableObjectStore,
{
    manifest.validate()?;
    let mut receipts_by_digest = BTreeMap::new();
    let mut transferred_by_digest = BTreeMap::new();
    for object in &manifest.objects {
        if receipts_by_digest.contains_key(&object.sha256) {
            continue;
        }
        let existing = target.verify(&object.sha256);
        let (verified, transferred) = match existing {
            Ok(verified) if verified.length == object.length => (verified, false),
            Ok(verified) => {
                return Err(ClusterError::Invalid(format!(
                    "target object {} has length {}, expected {}",
                    object.sha256, verified.length, object.length
                )))
            }
            Err(StoreError::ObjectMissing(_)) | Err(StoreError::ObjectCorrupt { .. }) => {
                let mut reader = source.open_verified(object).map_err(cluster_store_error)?;
                let verified = target
                    .put_verified_stream(&object.sha256, object.length, reader.as_mut())
                    .map_err(cluster_store_error)?;
                (verified, true)
            }
            Err(error) => return Err(cluster_store_error(error)),
        };
        if verified.sha256 != object.sha256 || verified.length != object.length {
            return Err(ClusterError::Invalid(
                "target object verification differs from the transfer manifest".into(),
            ));
        }
        transferred_by_digest.insert(object.sha256.clone(), transferred);
        receipts_by_digest.insert(object.sha256.clone(), verified.receipt);
    }
    let mut accounted = BTreeSet::new();
    let receipts = manifest
        .objects
        .iter()
        .map(|object| ArtifactReplicaObjectReceipt {
            reference: object.reference.clone(),
            sha256: object.sha256.clone(),
            length: object.length,
            target: receipts_by_digest[&object.sha256].clone(),
            transferred: transferred_by_digest[&object.sha256]
                && accounted.insert(object.sha256.clone()),
        })
        .collect();
    ArtifactTransferReceipt::new(manifest, receipts, completed_at)
}

fn cluster_store_error(error: StoreError) -> ClusterError {
    match error {
        StoreError::ObjectMissing(digest) => ClusterError::NotFound(digest),
        error => ClusterError::Unavailable(error.to_string()),
    }
}
