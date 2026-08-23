//! Backend-independent, content-authenticated logical archive and native restore.

use crate::{Engine, Error, NativeEngine, Result};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use vyrm_core::digest::Sha256;
use vyrm_core::{Claim, RuntimeChange, RuntimeCommit, RuntimeMutation};

const MAGIC: &[u8; 8] = b"RRDLAR01";
pub const LOGICAL_ARCHIVE_VERSION: u16 = 1;
const RRD_CONTRACT_VERSION: u16 = 1;
const ACTION_TAG: u8 = 1;
const FOOTER_TAG: u8 = 0xff;
const PAGE_SIZE: usize = 1_024;
const MAX_ACTION_BYTES: usize = 64 * 1024 * 1024;
static TEMP_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogicalArchiveInventory {
    pub format_version: u16,
    pub contract_version: u16,
    pub archive_sha256: String,
    pub action_count: u64,
    pub standalone_claims: u64,
    pub runtime_commits: u64,
    pub runtime_mutations: u64,
    pub payload_bytes: u64,
    pub claim_sequence: u64,
    pub runtime_cursor: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogicalRestoreReport {
    pub archive: PathBuf,
    pub target: PathBuf,
    pub inventory: LogicalArchiveInventory,
    pub reopened: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum ArchiveAction {
    StandaloneClaim { claim: Claim },
    RuntimeCommit { commit: RuntimeCommit },
}

/// Exports one stable logical cut. A concurrent append rejects the export and
/// leaves no final archive at `path`.
pub fn export_logical_archive<E: Engine>(
    engine: &E,
    path: &Path,
) -> Result<LogicalArchiveInventory> {
    let claim_sequence = engine.sequence()?;
    let runtime_cursor = engine.runtime_cursor()?;
    let claims = read_claims(engine, claim_sequence)?;
    let changes = read_changes(engine, runtime_cursor)?;
    if engine.sequence()? != claim_sequence || engine.runtime_cursor()? != runtime_cursor {
        return Err(Error::Archive(
            "source watermarks changed during logical export; retry from a stable cut".into(),
        ));
    }

    let commits = reconstruct_commits(&changes, runtime_cursor)?;
    let actions = interleave_actions(claims, commits)?;
    write_archive(path, claim_sequence, runtime_cursor, &actions)
}

/// Validates the complete stream and its replay coordinates without opening a
/// database or mutating a restore destination.
pub fn inspect_logical_archive(path: &Path) -> Result<LogicalArchiveInventory> {
    read_archive(path, |_| Ok(()))
}

/// Restores only into an absent root. Partial work remains hidden in a unique
/// sibling staging directory and is removed after any failed attempt.
pub fn restore_logical_archive_to_new_root(
    archive: &Path,
    target: &Path,
    at: u64,
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
    let name = target
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| Error::Archive("restore target must have a UTF-8 file name".into()))?;
    let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
    let staging = parent.join(format!(
        ".{name}.rrd-restore-{}-{id}.tmp",
        std::process::id()
    ));

    let attempt = (|| {
        let engine = NativeEngine::open(&staging)?;
        let actual = read_archive(archive, |action| match action {
            ArchiveAction::StandaloneClaim { claim } => {
                engine.append_batch(std::slice::from_ref(claim))?;
                Ok(())
            }
            ArchiveAction::RuntimeCommit { commit } => {
                let outcome = engine.commit_runtime(commit)?;
                if outcome.commit_id != commit.digest() {
                    return Err(Error::Archive(
                        "restored runtime commit identity diverged".into(),
                    ));
                }
                Ok(())
            }
        })?;
        if actual != expected {
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
        if target.exists() {
            return Err(Error::Archive(format!(
                "restore target appeared before publication: {}",
                target.display()
            )));
        }
        vyrm_kv::publish_rename(parent, &staging, target).map_err(archive_io)?;
        Ok(LogicalRestoreReport {
            archive: archive.to_owned(),
            target: target.to_owned(),
            inventory: expected.clone(),
            reopened: true,
        })
    })();

    if attempt.is_err() && staging.exists() {
        fs::remove_dir_all(&staging).map_err(archive_io)?;
    }
    attempt
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
    Ok(())
}

fn read_claims(engine: &impl Engine, head: u64) -> Result<Vec<Claim>> {
    let capacity = usize::try_from(head)
        .map_err(|_| Error::Archive("claim log is too large for this exporter".into()))?;
    let mut claims = Vec::with_capacity(capacity);
    let mut after = 0u64;
    while after < head {
        let through = head.min(after.saturating_add(PAGE_SIZE as u64));
        let page = engine.claims_in_range(after, through)?;
        let expected = usize::try_from(through - after)
            .map_err(|_| Error::Archive("claim page exceeds usize".into()))?;
        if page.len() != expected {
            return Err(Error::Archive(format!(
                "claim log is discontinuous in ({after}, {through}]"
            )));
        }
        claims.extend(page);
        after = through;
    }
    Ok(claims)
}

fn read_changes(engine: &impl Engine, head: u64) -> Result<Vec<RuntimeChange>> {
    let capacity = usize::try_from(head)
        .map_err(|_| Error::Archive("runtime log is too large for this exporter".into()))?;
    let mut changes = Vec::with_capacity(capacity);
    let mut after = 0u64;
    while after < head {
        let page = engine.runtime_changes_since(after, PAGE_SIZE, None)?;
        if page.head_cursor != head || page.requested_after != after {
            return Err(Error::Archive(
                "runtime watermark changed during logical export".into(),
            ));
        }
        if page.through_cursor <= after || page.through_cursor > head {
            return Err(Error::Archive(
                "runtime page did not advance canonically".into(),
            ));
        }
        changes.extend(page.changes);
        after = page.through_cursor;
    }
    Ok(changes)
}

fn reconstruct_commits(changes: &[RuntimeChange], head: u64) -> Result<Vec<RuntimeCommit>> {
    if changes.len() as u64 != head {
        return Err(Error::Archive(format!(
            "runtime log contains {} changes for cursor {head}",
            changes.len()
        )));
    }
    let mut commits = Vec::new();
    let mut index = 0usize;
    let mut previous_digest: Option<&str> = None;
    while index < changes.len() {
        let first = &changes[index];
        if first.cursor != index as u64 + 1 || first.commit_ordinal != 0 {
            return Err(Error::Archive(
                "runtime cursor or commit ordinal is discontinuous".into(),
            ));
        }
        let commit_id = first.commit_id.clone();
        let start = index;
        while index < changes.len() && changes[index].commit_id == commit_id {
            let change = &changes[index];
            if change.cursor != index as u64 + 1
                || change.commit_ordinal != (index - start) as u64
                || change.scope != first.scope
                || change.at != first.at
                || change.actor != first.actor
                || change.previous_digest.as_deref() != previous_digest
                || !change.verify_digest()
            {
                return Err(Error::Archive(format!(
                    "runtime change {} failed chain or commit validation",
                    change.cursor
                )));
            }
            previous_digest = Some(&change.digest);
            index += 1;
        }
        let commit = RuntimeCommit {
            scope: first.scope.clone(),
            at: first.at,
            actor: first.actor.clone(),
            expected_cursor: first.cursor - 1,
            mutations: changes[start..index]
                .iter()
                .map(|change| change.mutation.clone())
                .collect(),
        };
        commit.validate()?;
        if commit.digest() != commit_id {
            return Err(Error::Archive(format!(
                "runtime commit at cursor {} has a mismatched content identity",
                first.cursor
            )));
        }
        commits.push(commit);
    }
    Ok(commits)
}

fn interleave_actions(
    claims: Vec<Claim>,
    commits: Vec<RuntimeCommit>,
) -> Result<Vec<ArchiveAction>> {
    let mut actions = Vec::with_capacity(claims.len().saturating_add(commits.len()));
    let mut claim_index = 0usize;
    for commit in commits {
        let committed_claims: Vec<&Claim> = commit
            .mutations
            .iter()
            .filter_map(|mutation| match mutation {
                RuntimeMutation::Claim { claim } => Some(claim),
                _ => None,
            })
            .collect();
        if let Some(first) = committed_claims.first() {
            while claim_index < claims.len() && claims[claim_index].digest() != first.digest() {
                actions.push(ArchiveAction::StandaloneClaim {
                    claim: claims[claim_index].clone(),
                });
                claim_index += 1;
            }
            for expected in committed_claims {
                let actual = claims.get(claim_index).ok_or_else(|| {
                    Error::Archive("runtime claim is absent from the claim sequence log".into())
                })?;
                if actual.digest() != expected.digest() {
                    return Err(Error::Archive(
                        "claim mutations in one runtime commit are not contiguous in sequence log"
                            .into(),
                    ));
                }
                claim_index += 1;
            }
        }
        actions.push(ArchiveAction::RuntimeCommit { commit });
    }
    for claim in claims.into_iter().skip(claim_index) {
        actions.push(ArchiveAction::StandaloneClaim { claim });
    }
    Ok(actions)
}

fn write_archive(
    path: &Path,
    claim_sequence: u64,
    runtime_cursor: u64,
    actions: &[ArchiveAction],
) -> Result<LogicalArchiveInventory> {
    if path.exists() {
        return Err(Error::Archive(format!(
            "archive target already exists: {}",
            path.display()
        )));
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(archive_io)?;
    let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(
        ".rrd-logical-export-{}-{id}.tmp",
        std::process::id()
    ));
    let action_count = u64::try_from(actions.len())
        .map_err(|_| Error::Archive("archive action count exceeds u64".into()))?;
    let result = (|| {
        let mut writer = ArchiveWriter::create(&temporary)?;
        writer.hashed(MAGIC)?;
        writer.hashed(&LOGICAL_ARCHIVE_VERSION.to_be_bytes())?;
        writer.hashed(&RRD_CONTRACT_VERSION.to_be_bytes())?;
        writer.hashed(&claim_sequence.to_be_bytes())?;
        writer.hashed(&runtime_cursor.to_be_bytes())?;
        writer.hashed(&action_count.to_be_bytes())?;
        for action in actions {
            writer.action(action)?;
        }
        let inventory = writer.finish(claim_sequence, runtime_cursor)?;
        vyrm_kv::publish_rename(parent, &temporary, path).map_err(archive_io)?;
        Ok(inventory)
    })();
    if result.is_err() && temporary.exists() {
        fs::remove_file(&temporary).map_err(archive_io)?;
    }
    result
}

struct ArchiveWriter {
    file: File,
    digest: Sha256,
    standalone_claims: u64,
    runtime_commits: u64,
    runtime_mutations: u64,
    payload_bytes: u64,
}

impl ArchiveWriter {
    fn create(path: &Path) -> Result<Self> {
        Ok(Self {
            file: OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
                .map_err(archive_io)?,
            digest: Sha256::new(),
            standalone_claims: 0,
            runtime_commits: 0,
            runtime_mutations: 0,
            payload_bytes: 0,
        })
    }

    fn action(&mut self, action: &ArchiveAction) -> Result<()> {
        let payload = serde_json::to_vec(action)?;
        if payload.len() > MAX_ACTION_BYTES {
            return Err(Error::Archive(
                "archive action exceeds the v1 size bound".into(),
            ));
        }
        self.hashed(&[ACTION_TAG])?;
        self.hashed(&(payload.len() as u64).to_be_bytes())?;
        self.hashed(&payload)?;
        self.payload_bytes = self
            .payload_bytes
            .checked_add(payload.len() as u64)
            .ok_or_else(|| Error::Archive("archive payload counter overflow".into()))?;
        match action {
            ArchiveAction::StandaloneClaim { .. } => {
                self.standalone_claims = self
                    .standalone_claims
                    .checked_add(1)
                    .ok_or_else(|| Error::Archive("standalone claim counter overflow".into()))?;
            }
            ArchiveAction::RuntimeCommit { commit } => {
                self.runtime_commits = self
                    .runtime_commits
                    .checked_add(1)
                    .ok_or_else(|| Error::Archive("runtime commit counter overflow".into()))?;
                self.runtime_mutations = self
                    .runtime_mutations
                    .checked_add(commit.mutations.len() as u64)
                    .ok_or_else(|| Error::Archive("runtime mutation counter overflow".into()))?;
            }
        }
        Ok(())
    }

    fn finish(
        mut self,
        claim_sequence: u64,
        runtime_cursor: u64,
    ) -> Result<LogicalArchiveInventory> {
        let action_count = self
            .standalone_claims
            .checked_add(self.runtime_commits)
            .ok_or_else(|| Error::Archive("archive action counter overflow".into()))?;
        let digest = self.digest.clone().finalize();
        self.file.write_all(&[FOOTER_TAG]).map_err(archive_io)?;
        for value in [
            self.standalone_claims,
            self.runtime_commits,
            self.runtime_mutations,
            self.payload_bytes,
        ] {
            self.file
                .write_all(&value.to_be_bytes())
                .map_err(archive_io)?;
        }
        self.file.write_all(&digest).map_err(archive_io)?;
        self.file.sync_all().map_err(archive_io)?;
        Ok(LogicalArchiveInventory {
            format_version: LOGICAL_ARCHIVE_VERSION,
            contract_version: RRD_CONTRACT_VERSION,
            archive_sha256: hex(digest),
            action_count,
            standalone_claims: self.standalone_claims,
            runtime_commits: self.runtime_commits,
            runtime_mutations: self.runtime_mutations,
            payload_bytes: self.payload_bytes,
            claim_sequence,
            runtime_cursor,
        })
    }

    fn hashed(&mut self, bytes: &[u8]) -> Result<()> {
        self.file.write_all(bytes).map_err(archive_io)?;
        self.digest.update(bytes);
        Ok(())
    }
}

fn read_archive(
    path: &Path,
    mut accept: impl FnMut(&ArchiveAction) -> Result<()>,
) -> Result<LogicalArchiveInventory> {
    let mut file = File::open(path).map_err(archive_io)?;
    let mut digest = Sha256::new();
    let mut magic = [0u8; 8];
    read_hashed(&mut file, &mut digest, &mut magic)?;
    if &magic != MAGIC {
        return Err(Error::Archive("archive magic does not match".into()));
    }
    let version = read_u16_hashed(&mut file, &mut digest)?;
    let contract_version = read_u16_hashed(&mut file, &mut digest)?;
    if version != LOGICAL_ARCHIVE_VERSION || contract_version != RRD_CONTRACT_VERSION {
        return Err(Error::Archive(format!(
            "unsupported logical archive version {version} / contract {contract_version}"
        )));
    }
    let claim_sequence = read_u64_hashed(&mut file, &mut digest)?;
    let runtime_cursor = read_u64_hashed(&mut file, &mut digest)?;
    let declared_actions = read_u64_hashed(&mut file, &mut digest)?;
    let mut standalone_claims = 0u64;
    let mut runtime_commits = 0u64;
    let mut runtime_mutations = 0u64;
    let mut payload_bytes = 0u64;
    let mut replay_sequence = 0u64;
    let mut replay_cursor = 0u64;
    loop {
        let mut tag = [0u8; 1];
        file.read_exact(&mut tag).map_err(archive_read_error)?;
        if tag[0] == FOOTER_TAG {
            break;
        }
        if tag[0] != ACTION_TAG {
            return Err(Error::Archive(format!(
                "unknown logical archive record tag {}",
                tag[0]
            )));
        }
        digest.update(&tag);
        let length = read_u64_hashed(&mut file, &mut digest)?;
        let length = usize::try_from(length)
            .map_err(|_| Error::Archive("archive action length exceeds usize".into()))?;
        if length > MAX_ACTION_BYTES {
            return Err(Error::Archive(
                "archive action exceeds the v1 size bound".into(),
            ));
        }
        let mut payload = vec![0; length];
        read_hashed(&mut file, &mut digest, &mut payload)?;
        let action: ArchiveAction = serde_json::from_slice(&payload)?;
        payload_bytes = payload_bytes
            .checked_add(length as u64)
            .ok_or_else(|| Error::Archive("archive payload counter overflow".into()))?;
        match &action {
            ArchiveAction::StandaloneClaim { claim } => {
                claim.validate()?;
                replay_sequence = replay_sequence
                    .checked_add(1)
                    .ok_or(Error::SequenceOverflow)?;
                standalone_claims += 1;
            }
            ArchiveAction::RuntimeCommit { commit } => {
                commit.validate()?;
                if commit.expected_cursor != replay_cursor {
                    return Err(Error::Archive(
                        "runtime commit cursor is not replay-contiguous".into(),
                    ));
                }
                let mutations = commit.mutations.len() as u64;
                replay_cursor = replay_cursor
                    .checked_add(mutations)
                    .ok_or(Error::SequenceOverflow)?;
                let claims = commit
                    .mutations
                    .iter()
                    .filter(|mutation| matches!(mutation, RuntimeMutation::Claim { .. }))
                    .count() as u64;
                replay_sequence = replay_sequence
                    .checked_add(claims)
                    .ok_or(Error::SequenceOverflow)?;
                runtime_commits += 1;
                runtime_mutations = runtime_mutations
                    .checked_add(mutations)
                    .ok_or_else(|| Error::Archive("runtime mutation counter overflow".into()))?;
            }
        }
        accept(&action)?;
    }

    let footer = [
        read_u64_raw(&mut file)?,
        read_u64_raw(&mut file)?,
        read_u64_raw(&mut file)?,
        read_u64_raw(&mut file)?,
    ];
    let mut declared_digest = [0u8; 32];
    file.read_exact(&mut declared_digest)
        .map_err(archive_read_error)?;
    let mut trailing = [0u8; 1];
    if file.read(&mut trailing).map_err(archive_io)? != 0 {
        return Err(Error::Archive("archive carries trailing bytes".into()));
    }
    let actual_digest = digest.finalize();
    let action_count = standalone_claims
        .checked_add(runtime_commits)
        .ok_or_else(|| Error::Archive("archive action counter overflow".into()))?;
    if footer
        != [
            standalone_claims,
            runtime_commits,
            runtime_mutations,
            payload_bytes,
        ]
        || declared_actions != action_count
        || declared_digest != actual_digest
        || replay_sequence != claim_sequence
        || replay_cursor != runtime_cursor
    {
        return Err(Error::Archive(
            "archive footer, digest, or replay watermarks do not match".into(),
        ));
    }
    Ok(LogicalArchiveInventory {
        format_version: version,
        contract_version,
        archive_sha256: hex(actual_digest),
        action_count,
        standalone_claims,
        runtime_commits,
        runtime_mutations,
        payload_bytes,
        claim_sequence,
        runtime_cursor,
    })
}

fn read_hashed(file: &mut File, digest: &mut Sha256, bytes: &mut [u8]) -> Result<()> {
    file.read_exact(bytes).map_err(archive_read_error)?;
    digest.update(bytes);
    Ok(())
}

fn read_u16_hashed(file: &mut File, digest: &mut Sha256) -> Result<u16> {
    let mut bytes = [0u8; 2];
    read_hashed(file, digest, &mut bytes)?;
    Ok(u16::from_be_bytes(bytes))
}

fn read_u64_hashed(file: &mut File, digest: &mut Sha256) -> Result<u64> {
    let mut bytes = [0u8; 8];
    read_hashed(file, digest, &mut bytes)?;
    Ok(u64::from_be_bytes(bytes))
}

fn read_u64_raw(file: &mut File) -> Result<u64> {
    let mut bytes = [0u8; 8];
    file.read_exact(&mut bytes).map_err(archive_read_error)?;
    Ok(u64::from_be_bytes(bytes))
}

fn archive_read_error(error: std::io::Error) -> Error {
    if error.kind() == std::io::ErrorKind::UnexpectedEof {
        Error::Archive("archive is truncated".into())
    } else {
        archive_io(error)
    }
}

fn archive_io(error: std::io::Error) -> Error {
    Error::Archive(error.to_string())
}

fn hex(digest: [u8; 32]) -> String {
    let mut output = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}
