use super::format::{
    action_bytes, inspect_logical_archive, read_archive, ArchiveAction, ArchiveHeader,
    LogicalArchiveCheckpoint, LogicalArchiveInventory, LogicalArchiveOperation,
    LogicalArchiveProgress, LogicalRestoreReport, PrefixInventory,
};
use super::receipt::{load_receipt, remove_receipt, restore_paths, write_receipt, RestoreReceipt};
use crate::{Engine, Error, NativeEngine, Result};
use rrd_core::{RuntimeCommitOutcome, RuntimeMutation};
use std::fs;
use std::path::Path;

pub fn restore_logical_archive_to_new_root(
    archive: &Path,
    target: &Path,
    at: u64,
) -> Result<LogicalRestoreReport> {
    restore_logical_archive_to_new_root_with_progress(archive, target, at, |_| Ok(()), |_| Ok(()))
}

pub fn restore_logical_archive_to_new_root_with(
    archive: &Path,
    target: &Path,
    at: u64,
    populate_staging: impl FnOnce(&Path) -> Result<()>,
) -> Result<LogicalRestoreReport> {
    restore_logical_archive_to_new_root_with_progress(archive, target, at, populate_staging, |_| {
        Ok(())
    })
}

/// Restores through a durable private staging root. The progress callback is
/// invoked once after an action is durable and again after its checksummed
/// receipt is durable. An interruption at either boundary is reconciled by
/// inspecting the staging database on the next invocation.
pub fn restore_logical_archive_to_new_root_with_progress(
    archive: &Path,
    target: &Path,
    at: u64,
    populate_staging: impl FnOnce(&Path) -> Result<()>,
    mut progress: impl FnMut(&LogicalArchiveProgress) -> Result<()>,
) -> Result<LogicalRestoreReport> {
    let expected = inspect_logical_archive(archive)?;
    if target.exists() {
        return Err(Error::Archive(format!(
            "restore target already exists: {}",
            target.display()
        )));
    }
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(archive_io)?;
    let (staging, receipt_path) = restore_paths(target, &expected.archive_sha256)?;
    if receipt_path.exists() && !staging.exists() {
        return Err(Error::Archive(
            "logical restore receipt exists without its staging database".into(),
        ));
    }
    let resumed = staging.exists();
    let engine = NativeEngine::open(&staging)?;
    let mut prefix = reconcile_staging_prefix(archive, &expected, &engine)?;
    if receipt_path.exists() {
        let receipt = load_receipt::<RestoreReceipt>(&receipt_path)?;
        receipt.validate_fixed(target, &expected.archive_sha256, expected.action_count)?;
        if receipt.completed_actions > prefix.completed_actions
            || (receipt.completed_actions == prefix.completed_actions
                && (receipt.claim_sequence != prefix.claim_sequence
                    || receipt.runtime_cursor != prefix.runtime_cursor
                    || receipt.runtime_audit_sha256 != prefix.runtime_audit_sha256))
        {
            return Err(Error::Archive(
                "logical restore receipt is ahead of or differs from staging".into(),
            ));
        }
    }
    write_receipt(
        &receipt_path,
        &RestoreReceipt::new(
            target,
            expected.archive_sha256.clone(),
            expected.action_count,
            &prefix,
        )?,
    )?;

    let replayed = read_archive(archive, |ordinal, action| {
        if ordinal <= prefix.completed_actions {
            return Ok(());
        }
        match action {
            ArchiveAction::StandaloneClaim { claim } => {
                engine.append_batch(std::slice::from_ref(claim))?;
            }
            ArchiveAction::RuntimeCommit { commit, audit } => {
                let outcome = engine.restore_runtime_commit(commit, audit)?;
                if outcome.commit_id != commit.digest() {
                    return Err(Error::Archive(
                        "restored runtime commit identity diverged".into(),
                    ));
                }
            }
        }
        advance_prefix(&mut prefix, action)?;
        let checkpoint = LogicalArchiveProgress {
            operation: LogicalArchiveOperation::Restore,
            checkpoint: LogicalArchiveCheckpoint::ActionDurable,
            completed_actions: prefix.completed_actions,
            action_count: expected.action_count,
            claim_sequence: prefix.claim_sequence,
            runtime_cursor: prefix.runtime_cursor,
        };
        progress(&checkpoint)?;
        write_receipt(
            &receipt_path,
            &RestoreReceipt::new(
                target,
                expected.archive_sha256.clone(),
                expected.action_count,
                &prefix,
            )?,
        )?;
        progress(&LogicalArchiveProgress {
            checkpoint: LogicalArchiveCheckpoint::ReceiptDurable,
            ..checkpoint
        })
    })?;
    if replayed != expected {
        return Err(Error::Archive(
            "archive changed between validation and replay".into(),
        ));
    }
    verify_watermarks(&engine, &expected)?;
    engine.flush(at)?;
    drop(engine);

    let reopened = NativeEngine::open(&staging)?;
    verify_watermarks(&reopened, &expected)?;
    drop(reopened);
    populate_staging(&staging)?;
    let reopened = NativeEngine::open(&staging)?;
    verify_watermarks(&reopened, &expected)?;
    drop(reopened);
    if target.exists() {
        return Err(Error::Archive(format!(
            "restore target appeared before publication: {}",
            target.display()
        )));
    }

    // Removing the receipt before rename is crash-safe because an extant
    // staging database can reconstruct its exact prefix from the archive.
    remove_receipt(&receipt_path)?;
    rrd_lsm::publish_rename(parent, &staging, target).map_err(archive_io)?;
    Ok(LogicalRestoreReport {
        archive: archive.to_owned(),
        target: target.to_owned(),
        inventory: expected,
        reopened: true,
        resumed,
    })
}

fn reconcile_staging_prefix(
    archive: &Path,
    expected: &LogicalArchiveInventory,
    engine: &NativeEngine,
) -> Result<PrefixInventory> {
    let actual_claims = engine.sequence()?;
    let actual_cursor = engine.runtime_cursor()?;
    if actual_claims > expected.claim_sequence || actual_cursor > expected.runtime_cursor {
        return Err(Error::Archive(
            "logical restore staging is ahead of the selected archive".into(),
        ));
    }
    let header = ArchiveHeader {
        claim_sequence: expected.claim_sequence,
        runtime_cursor: expected.runtime_cursor,
        action_count: expected.action_count,
    };
    let mut prefix = PrefixInventory {
        header,
        completed_actions: 0,
        standalone_claims: 0,
        runtime_commits: 0,
        runtime_mutations: 0,
        payload_bytes: 0,
        claim_sequence: 0,
        runtime_cursor: 0,
        runtime_audit_sha256: None,
        prefix_bytes: 0,
        prefix_sha256: String::new(),
    };
    read_archive(archive, |_, action| {
        if prefix.claim_sequence == actual_claims && prefix.runtime_cursor == actual_cursor {
            return Ok(());
        }
        let (next_claims, next_cursor) =
            action_watermarks(action, prefix.claim_sequence, prefix.runtime_cursor)?;
        if next_claims > actual_claims || next_cursor > actual_cursor {
            return Ok(());
        }
        verify_action_present(engine, action, prefix.claim_sequence, prefix.runtime_cursor)?;
        advance_prefix(&mut prefix, action)
    })?;
    if prefix.claim_sequence != actual_claims || prefix.runtime_cursor != actual_cursor {
        return Err(Error::Archive(
            "logical restore staging does not end at an archive action boundary".into(),
        ));
    }
    Ok(prefix)
}

fn verify_action_present(
    engine: &NativeEngine,
    action: &ArchiveAction,
    claim_sequence: u64,
    runtime_cursor: u64,
) -> Result<()> {
    match action {
        ArchiveAction::StandaloneClaim { claim } => {
            let actual = engine.claims_in_range(claim_sequence, claim_sequence + 1)?;
            if actual.as_slice() != std::slice::from_ref(claim) {
                return Err(Error::Archive(
                    "restore staging standalone claim differs from archive".into(),
                ));
            }
        }
        ArchiveAction::RuntimeCommit { commit, audit } => {
            let count = commit.mutations.len();
            let page = engine.runtime_changes_since(runtime_cursor, count, None)?;
            if page.changes.len() != count
                || page.through_cursor != runtime_cursor + count as u64
                || page.changes.iter().enumerate().any(|(ordinal, change)| {
                    change.commit_id != commit.digest()
                        || change.commit_ordinal != ordinal as u64
                        || change.mutation != commit.mutations[ordinal]
                })
            {
                return Err(Error::Archive(
                    "restore staging runtime commit differs from archive".into(),
                ));
            }
            if engine.runtime_audit(&commit.digest())?.as_ref() != Some(audit) {
                return Err(Error::Archive(
                    "restore staging runtime audit differs from archive".into(),
                ));
            }
            let claim_mutations = commit
                .mutations
                .iter()
                .filter_map(|mutation| match mutation {
                    RuntimeMutation::Claim { claim } => Some(claim),
                    _ => None,
                })
                .collect::<Vec<_>>();
            if !claim_mutations.is_empty() {
                let actual = engine.claims_in_range(
                    claim_sequence,
                    claim_sequence + claim_mutations.len() as u64,
                )?;
                if actual.len() != claim_mutations.len()
                    || actual
                        .iter()
                        .zip(claim_mutations.iter())
                        .any(|(left, right)| left != *right)
                {
                    return Err(Error::Archive(
                        "restore staging runtime claims differ from archive".into(),
                    ));
                }
            }
            let expected_outcome = RuntimeCommitOutcome {
                commit_id: commit.digest(),
                first_cursor: runtime_cursor + 1,
                last_cursor: runtime_cursor + count as u64,
                count,
                first_claim_sequence: (!claim_mutations.is_empty()).then_some(claim_sequence + 1),
                last_claim_sequence: (!claim_mutations.is_empty())
                    .then_some(claim_sequence + claim_mutations.len() as u64),
                outbox_count: commit
                    .mutations
                    .iter()
                    .filter(|mutation| rrd_core::projection_family(mutation).is_some())
                    .count(),
            };
            if engine.runtime_commit_outcome(&commit.digest())?.as_ref() != Some(&expected_outcome)
            {
                return Err(Error::Archive(
                    "restore staging runtime outcome differs from archive".into(),
                ));
            }
        }
    }
    Ok(())
}

fn advance_prefix(prefix: &mut PrefixInventory, action: &ArchiveAction) -> Result<()> {
    let payload = action_bytes(action)?;
    prefix.completed_actions = prefix
        .completed_actions
        .checked_add(1)
        .ok_or_else(|| Error::Archive("restore action count overflow".into()))?;
    prefix.payload_bytes = prefix
        .payload_bytes
        .checked_add(payload.len() as u64)
        .ok_or_else(|| Error::Archive("restore payload count overflow".into()))?;
    let (claims, cursor) = action_watermarks(action, prefix.claim_sequence, prefix.runtime_cursor)?;
    prefix.claim_sequence = claims;
    prefix.runtime_cursor = cursor;
    match action {
        ArchiveAction::StandaloneClaim { .. } => prefix.standalone_claims += 1,
        ArchiveAction::RuntimeCommit { commit, audit } => {
            prefix.runtime_commits += 1;
            prefix.runtime_mutations = prefix
                .runtime_mutations
                .checked_add(commit.mutations.len() as u64)
                .ok_or_else(|| Error::Archive("restore mutation count overflow".into()))?;
            prefix.runtime_audit_sha256 = Some(audit.digest.clone());
        }
    }
    Ok(())
}

fn action_watermarks(
    action: &ArchiveAction,
    claim_sequence: u64,
    runtime_cursor: u64,
) -> Result<(u64, u64)> {
    match action {
        ArchiveAction::StandaloneClaim { .. } => Ok((
            claim_sequence
                .checked_add(1)
                .ok_or(Error::SequenceOverflow)?,
            runtime_cursor,
        )),
        ArchiveAction::RuntimeCommit { commit, .. } => {
            let claims = commit
                .mutations
                .iter()
                .filter(|mutation| matches!(mutation, RuntimeMutation::Claim { .. }))
                .count() as u64;
            Ok((
                claim_sequence
                    .checked_add(claims)
                    .ok_or(Error::SequenceOverflow)?,
                runtime_cursor
                    .checked_add(commit.mutations.len() as u64)
                    .ok_or(Error::SequenceOverflow)?,
            ))
        }
    }
}

fn verify_watermarks(engine: &impl Engine, expected: &LogicalArchiveInventory) -> Result<()> {
    let sequence = engine.sequence()?;
    let cursor = engine.runtime_cursor()?;
    if sequence != expected.claim_sequence || cursor != expected.runtime_cursor {
        return Err(Error::Archive(format!(
            "restored watermarks diverged: claims {sequence}/{}, runtime {cursor}/{}",
            expected.claim_sequence, expected.runtime_cursor
        )));
    }
    if expected.runtime_cursor > 0 {
        let page = engine.runtime_changes_since(expected.runtime_cursor - 1, 1, None)?;
        let commit_id = page
            .changes
            .first()
            .ok_or_else(|| Error::Archive("restored runtime tail is absent".into()))?
            .commit_id
            .clone();
        let audit = engine
            .runtime_audit(&commit_id)?
            .ok_or_else(|| Error::Archive("restored runtime tail audit is absent".into()))?;
        if expected.runtime_audit_sha256.as_deref() != Some(audit.digest.as_str()) {
            return Err(Error::Archive(
                "restored runtime audit head differs from archive".into(),
            ));
        }
    } else if expected.runtime_audit_sha256.is_some() {
        return Err(Error::Archive(
            "empty runtime archive declares an audit head".into(),
        ));
    }
    Ok(())
}

fn archive_io(error: std::io::Error) -> Error {
    Error::Archive(error.to_string())
}
