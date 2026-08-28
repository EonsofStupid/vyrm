use super::format::{
    action_bytes, scan_archive_prefix, ArchiveAction, ArchiveHeader, ArchiveWriter,
    LogicalArchiveCheckpoint, LogicalArchiveInventory, LogicalArchiveOperation,
    LogicalArchiveProgress, PrefixActionReader, MAX_ACTION_BYTES,
};
use super::receipt::{
    export_paths, load_receipt, remove_export_artifacts, remove_receipt, write_receipt,
    ExportReceipt,
};
use crate::{Engine, Error, Result};
use rrd_core::{AuditEnvelope, Claim, RuntimeChange, RuntimeCommit, RuntimeMutation};
use std::collections::VecDeque;
use std::fs;
use std::path::Path;

const PAGE_SIZE: usize = 1_024;

#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceSummary {
    claim_sequence: u64,
    runtime_cursor: u64,
    action_count: u64,
    standalone_claims: u64,
    runtime_commits: u64,
    runtime_mutations: u64,
    payload_bytes: u64,
    runtime_audit_sha256: Option<String>,
}

pub fn export_logical_archive<E: Engine>(
    engine: &E,
    destination: &Path,
) -> Result<LogicalArchiveInventory> {
    export_logical_archive_with_progress(engine, destination, |_| Ok(()))
}

/// Exports a stable logical cut while publishing durable action/receipt
/// checkpoints to `progress`. Returning an error from the callback simulates
/// interruption and intentionally leaves the exact private partial/receipt
/// artifacts for the next call to reconcile and resume.
pub fn export_logical_archive_with_progress<E: Engine>(
    engine: &E,
    destination: &Path,
    mut progress: impl FnMut(&LogicalArchiveProgress) -> Result<()>,
) -> Result<LogicalArchiveInventory> {
    if destination.exists() {
        return Err(Error::Archive(format!(
            "archive target already exists: {}",
            destination.display()
        )));
    }
    let parent = destination.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(archive_io)?;

    // The first bounded pass validates the complete source and determines the
    // immutable header without retaining the log. The second pass either
    // compares or appends each deterministic frame.
    let expected = scan_source(engine)?;
    let header = ArchiveHeader {
        claim_sequence: expected.claim_sequence,
        runtime_cursor: expected.runtime_cursor,
        action_count: expected.action_count,
    };
    let (partial_path, receipt_path) = export_paths(destination)?;
    if receipt_path.exists() && !partial_path.exists() {
        return Err(Error::Archive(
            "logical export receipt exists without its partial archive".into(),
        ));
    }
    if partial_path.exists() {
        if let Ok(inventory) = super::format::inspect_logical_archive(&partial_path) {
            if !inventory_matches_source(&inventory, &expected) {
                remove_export_artifacts(&partial_path, &receipt_path)?;
                return Err(Error::Archive(
                    "finalized logical export no longer matches the source cut".into(),
                ));
            }
            rrd_lsm::publish_rename(parent, &partial_path, destination).map_err(archive_io)?;
            remove_receipt(&receipt_path)?;
            return Ok(inventory);
        }
    }
    if !partial_path.exists() {
        let mut writer = ArchiveWriter::create(&partial_path, header)?;
        writer.sync()?;
        let prefix = writer.prefix_inventory()?;
        drop(writer);
        write_receipt(
            &receipt_path,
            &ExportReceipt::from_prefix(
                destination,
                expected.runtime_audit_sha256.clone(),
                &prefix,
            )?,
        )?;
    }

    let prefix = scan_archive_prefix(&partial_path)?;
    if prefix.header != header {
        remove_export_artifacts(&partial_path, &receipt_path)?;
        return Err(Error::Archive(
            "logical export source watermarks changed; discarded the stale partial cut".into(),
        ));
    }
    if !receipt_path.exists() {
        write_receipt(
            &receipt_path,
            &ExportReceipt::from_prefix(
                destination,
                expected.runtime_audit_sha256.clone(),
                &prefix,
            )?,
        )?;
    }
    let receipt = load_receipt::<ExportReceipt>(&receipt_path)?;
    receipt.validate_fixed(
        destination,
        header,
        expected.runtime_audit_sha256.as_deref(),
    )?;
    receipt.validate_prefix(&prefix)?;
    if receipt.completed_actions != prefix.completed_actions {
        write_receipt(
            &receipt_path,
            &ExportReceipt::from_prefix(
                destination,
                expected.runtime_audit_sha256.clone(),
                &prefix,
            )?,
        )?;
    }

    if engine.sequence()? != header.claim_sequence
        || engine.runtime_cursor()? != header.runtime_cursor
    {
        remove_export_artifacts(&partial_path, &receipt_path)?;
        return Err(Error::Archive(
            "logical export source advanced before resume; discarded the stale partial cut".into(),
        ));
    }

    let mut prefix_reader = PrefixActionReader::open(&partial_path, &prefix)?;
    let mut writer = ArchiveWriter::resume(&partial_path, &prefix)?;
    let emitted = stream_source(engine, |action, payload| {
        if prefix_reader.compare_or_absent(action, payload)? {
            return Ok(());
        }
        writer.action(action, payload)?;
        writer.sync()?;
        let durable = writer.prefix_inventory()?;
        let checkpoint = LogicalArchiveProgress {
            operation: LogicalArchiveOperation::Export,
            checkpoint: LogicalArchiveCheckpoint::ActionDurable,
            completed_actions: durable.completed_actions,
            action_count: header.action_count,
            claim_sequence: durable.claim_sequence,
            runtime_cursor: durable.runtime_cursor,
        };
        progress(&checkpoint)?;
        let receipt = ExportReceipt::from_prefix(
            destination,
            expected.runtime_audit_sha256.clone(),
            &durable,
        )?;
        write_receipt(&receipt_path, &receipt)?;
        progress(&LogicalArchiveProgress {
            checkpoint: LogicalArchiveCheckpoint::ReceiptDurable,
            ..checkpoint
        })
    });
    let emitted = match emitted {
        Ok(value) => value,
        Err(error) => {
            if engine.sequence()? != header.claim_sequence
                || engine.runtime_cursor()? != header.runtime_cursor
            {
                remove_export_artifacts(&partial_path, &receipt_path)?;
            }
            return Err(error);
        }
    };
    prefix_reader.finish()?;
    if emitted != expected {
        remove_export_artifacts(&partial_path, &receipt_path)?;
        return Err(Error::Archive(
            "logical export source changed between validation and publication".into(),
        ));
    }
    let inventory = writer.finish()?;
    if inventory.runtime_audit_sha256 != expected.runtime_audit_sha256 {
        remove_export_artifacts(&partial_path, &receipt_path)?;
        return Err(Error::Archive(
            "logical export audit head differs from the validated source".into(),
        ));
    }
    progress(&LogicalArchiveProgress {
        operation: LogicalArchiveOperation::Export,
        checkpoint: LogicalArchiveCheckpoint::ArchiveFinalized,
        completed_actions: inventory.action_count,
        action_count: inventory.action_count,
        claim_sequence: inventory.claim_sequence,
        runtime_cursor: inventory.runtime_cursor,
    })?;
    rrd_lsm::publish_rename(parent, &partial_path, destination).map_err(archive_io)?;
    remove_receipt(&receipt_path)?;
    Ok(inventory)
}

fn inventory_matches_source(inventory: &LogicalArchiveInventory, source: &SourceSummary) -> bool {
    inventory.format_version == 1
        && inventory.contract_version == 1
        && inventory.action_count == source.action_count
        && inventory.standalone_claims == source.standalone_claims
        && inventory.runtime_commits == source.runtime_commits
        && inventory.runtime_mutations == source.runtime_mutations
        && inventory.payload_bytes == source.payload_bytes
        && inventory.claim_sequence == source.claim_sequence
        && inventory.runtime_cursor == source.runtime_cursor
        && inventory.runtime_audit_sha256 == source.runtime_audit_sha256
}

fn scan_source(engine: &impl Engine) -> Result<SourceSummary> {
    stream_source(engine, |_, _| Ok(()))
}

fn stream_source(
    engine: &impl Engine,
    mut emit: impl FnMut(&ArchiveAction, &[u8]) -> Result<()>,
) -> Result<SourceSummary> {
    let claim_sequence = engine.sequence()?;
    let runtime_cursor = engine.runtime_cursor()?;
    let mut claims = ClaimPager::new(engine, claim_sequence);
    let mut commits = RuntimeCommitPager::new(engine, runtime_cursor);
    let mut summary = SourceSummary {
        claim_sequence,
        runtime_cursor,
        action_count: 0,
        standalone_claims: 0,
        runtime_commits: 0,
        runtime_mutations: 0,
        payload_bytes: 0,
        runtime_audit_sha256: None,
    };

    while let Some(action) = commits.next_commit()? {
        let ArchiveAction::RuntimeCommit { commit, .. } = &action else {
            unreachable!("runtime pager only emits runtime commits");
        };
        let committed_claims = commit
            .mutations
            .iter()
            .filter_map(|mutation| match mutation {
                RuntimeMutation::Claim { claim } => Some(claim),
                _ => None,
            })
            .collect::<Vec<_>>();
        if let Some(first) = committed_claims.first() {
            loop {
                let actual = claims.next_claim()?.ok_or_else(|| {
                    Error::Archive("runtime claim is absent from the claim sequence log".into())
                })?;
                if actual.digest() == first.digest() {
                    break;
                }
                emit_counted(
                    &mut summary,
                    ArchiveAction::StandaloneClaim { claim: actual },
                    &mut emit,
                )?;
            }
            for expected in committed_claims.into_iter().skip(1) {
                let actual = claims.next_claim()?.ok_or_else(|| {
                    Error::Archive("runtime claim is absent from the claim sequence log".into())
                })?;
                if actual.digest() != expected.digest() {
                    return Err(Error::Archive(
                        "claim mutations in one runtime commit are not contiguous in sequence log"
                            .into(),
                    ));
                }
            }
        }
        emit_counted(&mut summary, action, &mut emit)?;
    }
    while let Some(claim) = claims.next_claim()? {
        emit_counted(
            &mut summary,
            ArchiveAction::StandaloneClaim { claim },
            &mut emit,
        )?;
    }
    claims.finish()?;
    commits.finish()?;
    if engine.sequence()? != claim_sequence || engine.runtime_cursor()? != runtime_cursor {
        return Err(Error::Archive(
            "source watermarks changed during logical export; retry from a stable cut".into(),
        ));
    }
    Ok(summary)
}

fn emit_counted(
    summary: &mut SourceSummary,
    action: ArchiveAction,
    emit: &mut impl FnMut(&ArchiveAction, &[u8]) -> Result<()>,
) -> Result<()> {
    let payload = action_bytes(&action)?;
    emit(&action, &payload)?;
    summary.action_count = summary
        .action_count
        .checked_add(1)
        .ok_or_else(|| Error::Archive("archive action count overflow".into()))?;
    summary.payload_bytes = summary
        .payload_bytes
        .checked_add(payload.len() as u64)
        .ok_or_else(|| Error::Archive("archive payload count overflow".into()))?;
    match action {
        ArchiveAction::StandaloneClaim { .. } => {
            summary.standalone_claims += 1;
        }
        ArchiveAction::RuntimeCommit { commit, audit } => {
            summary.runtime_commits += 1;
            summary.runtime_mutations = summary
                .runtime_mutations
                .checked_add(commit.mutations.len() as u64)
                .ok_or_else(|| Error::Archive("runtime mutation count overflow".into()))?;
            summary.runtime_audit_sha256 = Some(audit.digest);
        }
    }
    Ok(())
}

struct ClaimPager<'a, E> {
    engine: &'a E,
    head: u64,
    after: u64,
    page: VecDeque<Claim>,
}

impl<'a, E: Engine> ClaimPager<'a, E> {
    fn new(engine: &'a E, head: u64) -> Self {
        Self {
            engine,
            head,
            after: 0,
            page: VecDeque::new(),
        }
    }

    fn next_claim(&mut self) -> Result<Option<Claim>> {
        if self.page.is_empty() && self.after < self.head {
            let through = self.head.min(self.after.saturating_add(PAGE_SIZE as u64));
            let page = self.engine.claims_in_range(self.after, through)?;
            let expected = usize::try_from(through - self.after)
                .map_err(|_| Error::Archive("claim page exceeds usize".into()))?;
            if page.len() != expected {
                return Err(Error::Archive(format!(
                    "claim log is discontinuous in ({}, {through}]",
                    self.after
                )));
            }
            self.after = through;
            self.page.extend(page);
        }
        Ok(self.page.pop_front())
    }

    fn finish(&self) -> Result<()> {
        if self.after != self.head || !self.page.is_empty() {
            return Err(Error::Archive(
                "claim export did not consume its captured watermark".into(),
            ));
        }
        Ok(())
    }
}

struct RuntimeCommitPager<'a, E> {
    engine: &'a E,
    head: u64,
    fetched_through: u64,
    validated_through: u64,
    page: VecDeque<RuntimeChange>,
    pending: Option<RuntimeChange>,
    previous_change_sha256: Option<String>,
    previous_audit_sha256: Option<String>,
}

impl<'a, E: Engine> RuntimeCommitPager<'a, E> {
    fn new(engine: &'a E, head: u64) -> Self {
        Self {
            engine,
            head,
            fetched_through: 0,
            validated_through: 0,
            page: VecDeque::new(),
            pending: None,
            previous_change_sha256: None,
            previous_audit_sha256: None,
        }
    }

    fn next_commit(&mut self) -> Result<Option<ArchiveAction>> {
        let Some(first) = self.next_change()? else {
            return Ok(None);
        };
        if first.commit_ordinal != 0 {
            return Err(Error::Archive(
                "runtime commit begins at a non-zero ordinal".into(),
            ));
        }
        let commit_id = first.commit_id.clone();
        let scope = first.scope.clone();
        let at = first.at;
        let actor = first.actor.clone();
        let expected_cursor = first.cursor - 1;
        let mut mutations = Vec::new();
        let mut encoded_bound = 0usize;
        let mut change = first;
        loop {
            let ordinal = mutations.len() as u64;
            if change.cursor != self.validated_through + 1
                || change.commit_ordinal != ordinal
                || change.commit_id != commit_id
                || change.scope != scope
                || change.at != at
                || change.actor != actor
                || change.previous_digest != self.previous_change_sha256
                || !change.verify_digest()
            {
                return Err(Error::Archive(format!(
                    "runtime change {} failed chain or commit validation",
                    change.cursor
                )));
            }
            encoded_bound = encoded_bound
                .checked_add(serde_json::to_vec(&change.mutation)?.len())
                .ok_or_else(|| Error::Archive("runtime commit size overflow".into()))?;
            if encoded_bound > MAX_ACTION_BYTES {
                return Err(Error::Archive(
                    "runtime commit exceeds the archive action memory bound".into(),
                ));
            }
            self.validated_through = change.cursor;
            self.previous_change_sha256 = Some(change.digest.clone());
            mutations.push(change.mutation);
            match self.next_change()? {
                Some(next) if next.commit_id == commit_id => change = next,
                Some(next) => {
                    self.pending = Some(next);
                    break;
                }
                None => break,
            }
        }
        let commit = RuntimeCommit {
            scope,
            at,
            actor,
            expected_cursor,
            mutations,
        };
        commit.validate()?;
        if commit.digest() != commit_id {
            return Err(Error::Archive(format!(
                "runtime commit at cursor {} has a mismatched content identity",
                expected_cursor + 1
            )));
        }
        let audit = self.engine.runtime_audit(&commit_id)?.ok_or_else(|| {
            Error::Archive(format!("runtime commit {commit_id} has no audit envelope"))
        })?;
        let expected_audit = AuditEnvelope::accepted_commit_at_read(
            &commit,
            audit.read.as_ref(),
            &commit_id,
            self.validated_through,
            self.previous_audit_sha256.clone(),
        )?;
        if audit != expected_audit {
            return Err(Error::Archive(format!(
                "runtime commit {commit_id} audit envelope is missing or divergent"
            )));
        }
        self.previous_audit_sha256 = Some(audit.digest.clone());
        Ok(Some(ArchiveAction::RuntimeCommit {
            commit,
            audit: Box::new(audit),
        }))
    }

    fn next_change(&mut self) -> Result<Option<RuntimeChange>> {
        if let Some(change) = self.pending.take() {
            return Ok(Some(change));
        }
        if self.page.is_empty() && self.fetched_through < self.head {
            let page = self
                .engine
                .runtime_changes_since(self.fetched_through, PAGE_SIZE, None)?;
            if page.head_cursor != self.head
                || page.requested_after != self.fetched_through
                || page.through_cursor <= self.fetched_through
                || page.through_cursor > self.head
                || page.changes.is_empty()
            {
                return Err(Error::Archive(
                    "runtime watermark or page continuity changed during export".into(),
                ));
            }
            self.fetched_through = page.through_cursor;
            self.page.extend(page.changes);
        }
        Ok(self.page.pop_front())
    }

    fn finish(&self) -> Result<()> {
        if self.fetched_through != self.head
            || self.validated_through != self.head
            || !self.page.is_empty()
            || self.pending.is_some()
        {
            return Err(Error::Archive(
                "runtime export did not consume its captured watermark".into(),
            ));
        }
        Ok(())
    }
}

fn archive_io(error: std::io::Error) -> Error {
    Error::Archive(error.to_string())
}
