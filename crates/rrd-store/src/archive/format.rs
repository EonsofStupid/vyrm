use crate::{Error, Result};
use rrd_core::digest::Sha256;
use rrd_core::{AuditEnvelope, Claim, RuntimeCommit, RuntimeMutation};
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};

pub(super) const MAGIC: &[u8; 8] = b"RRDLAR01";
pub const LOGICAL_ARCHIVE_VERSION: u16 = 1;
pub(super) const RRD_CONTRACT_VERSION: u16 = 1;
const ACTION_TAG: u8 = 1;
const FOOTER_TAG: u8 = 0xff;
pub(super) const MAX_ACTION_BYTES: usize = 64 * 1024 * 1024;

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_audit_sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogicalRestoreReport {
    pub archive: PathBuf,
    pub target: PathBuf,
    pub inventory: LogicalArchiveInventory,
    pub reopened: bool,
    pub resumed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogicalArchiveOperation {
    Export,
    Restore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogicalArchiveCheckpoint {
    ActionDurable,
    ReceiptDurable,
    ArchiveFinalized,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogicalArchiveProgress {
    pub operation: LogicalArchiveOperation,
    pub checkpoint: LogicalArchiveCheckpoint,
    pub completed_actions: u64,
    pub action_count: u64,
    pub claim_sequence: u64,
    pub runtime_cursor: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum ArchiveAction {
    StandaloneClaim {
        claim: Claim,
    },
    RuntimeCommit {
        commit: RuntimeCommit,
        audit: Box<AuditEnvelope>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ArchiveHeader {
    pub claim_sequence: u64,
    pub runtime_cursor: u64,
    pub action_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PrefixInventory {
    pub header: ArchiveHeader,
    pub completed_actions: u64,
    pub standalone_claims: u64,
    pub runtime_commits: u64,
    pub runtime_mutations: u64,
    pub payload_bytes: u64,
    pub claim_sequence: u64,
    pub runtime_cursor: u64,
    pub runtime_audit_sha256: Option<String>,
    pub prefix_bytes: u64,
    pub prefix_sha256: String,
}

#[derive(Default)]
struct ReplayState {
    action_count: u64,
    standalone_claims: u64,
    runtime_commits: u64,
    runtime_mutations: u64,
    payload_bytes: u64,
    claim_sequence: u64,
    runtime_cursor: u64,
    runtime_audit_sha256: Option<String>,
}

impl ReplayState {
    fn accept(&mut self, action: &ArchiveAction, payload_bytes: usize) -> Result<()> {
        self.action_count = checked_increment(self.action_count, "archive action")?;
        self.payload_bytes = self
            .payload_bytes
            .checked_add(payload_bytes as u64)
            .ok_or_else(|| Error::Archive("archive payload counter overflow".into()))?;
        match action {
            ArchiveAction::StandaloneClaim { claim } => {
                claim.validate()?;
                self.claim_sequence = checked_increment(self.claim_sequence, "claim sequence")?;
                self.standalone_claims =
                    checked_increment(self.standalone_claims, "standalone claim")?;
            }
            ArchiveAction::RuntimeCommit { commit, audit } => {
                commit.validate()?;
                if commit.expected_cursor != self.runtime_cursor {
                    return Err(Error::Archive(
                        "runtime commit cursor is not replay-contiguous".into(),
                    ));
                }
                let mutations = commit.mutations.len() as u64;
                self.runtime_cursor = self
                    .runtime_cursor
                    .checked_add(mutations)
                    .ok_or(Error::SequenceOverflow)?;
                let claims = commit
                    .mutations
                    .iter()
                    .filter(|mutation| matches!(mutation, RuntimeMutation::Claim { .. }))
                    .count() as u64;
                self.claim_sequence = self
                    .claim_sequence
                    .checked_add(claims)
                    .ok_or(Error::SequenceOverflow)?;
                let commit_id = commit.digest();
                let expected_audit = AuditEnvelope::accepted_commit_at_read(
                    commit,
                    audit.read.as_ref(),
                    &commit_id,
                    self.runtime_cursor,
                    self.runtime_audit_sha256.clone(),
                )?;
                if audit.as_ref() != &expected_audit {
                    return Err(Error::Archive(format!(
                        "runtime commit {commit_id} audit envelope is invalid"
                    )));
                }
                self.runtime_audit_sha256 = Some(audit.digest.clone());
                self.runtime_commits = checked_increment(self.runtime_commits, "runtime commit")?;
                self.runtime_mutations = self
                    .runtime_mutations
                    .checked_add(mutations)
                    .ok_or_else(|| Error::Archive("runtime mutation counter overflow".into()))?;
            }
        }
        Ok(())
    }

    fn prefix(
        self,
        header: ArchiveHeader,
        prefix_bytes: u64,
        prefix_sha256: String,
    ) -> PrefixInventory {
        PrefixInventory {
            header,
            completed_actions: self.action_count,
            standalone_claims: self.standalone_claims,
            runtime_commits: self.runtime_commits,
            runtime_mutations: self.runtime_mutations,
            payload_bytes: self.payload_bytes,
            claim_sequence: self.claim_sequence,
            runtime_cursor: self.runtime_cursor,
            runtime_audit_sha256: self.runtime_audit_sha256,
            prefix_bytes,
            prefix_sha256,
        }
    }
}

pub(super) fn action_bytes(action: &ArchiveAction) -> Result<Vec<u8>> {
    let payload = serde_json::to_vec(action)?;
    if payload.len() > MAX_ACTION_BYTES {
        return Err(Error::Archive(
            "archive action exceeds the v1 size bound".into(),
        ));
    }
    Ok(payload)
}

pub(super) struct ArchiveWriter {
    file: File,
    digest: Sha256,
    header: ArchiveHeader,
    state: ReplayState,
}

impl ArchiveWriter {
    pub fn create(path: &Path, header: ArchiveHeader) -> Result<Self> {
        let mut writer = Self {
            file: OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
                .map_err(archive_io)?,
            digest: Sha256::new(),
            header,
            state: ReplayState::default(),
        };
        writer.hashed(MAGIC)?;
        writer.hashed(&LOGICAL_ARCHIVE_VERSION.to_be_bytes())?;
        writer.hashed(&RRD_CONTRACT_VERSION.to_be_bytes())?;
        writer.hashed(&header.claim_sequence.to_be_bytes())?;
        writer.hashed(&header.runtime_cursor.to_be_bytes())?;
        writer.hashed(&header.action_count.to_be_bytes())?;
        writer.file.sync_all().map_err(archive_io)?;
        Ok(writer)
    }

    pub fn resume(path: &Path, prefix: &PrefixInventory) -> Result<Self> {
        let mut source = File::open(path).map_err(archive_io)?;
        let mut digest = Sha256::new();
        let mut bytes = 0u64;
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let read = source.read(&mut buffer).map_err(archive_io)?;
            if read == 0 {
                break;
            }
            digest.update(&buffer[..read]);
            bytes = bytes
                .checked_add(read as u64)
                .ok_or_else(|| Error::Archive("archive prefix size overflow".into()))?;
        }
        let digest = digest.finalize();
        if bytes != prefix.prefix_bytes || hex(digest) != prefix.prefix_sha256 {
            return Err(Error::Archive(
                "logical archive partial bytes differ from their resume evidence".into(),
            ));
        }
        Ok(Self {
            file: OpenOptions::new()
                .append(true)
                .open(path)
                .map_err(archive_io)?,
            digest: Sha256::from_finalized_prefix(path)?,
            header: prefix.header,
            state: ReplayState {
                action_count: prefix.completed_actions,
                standalone_claims: prefix.standalone_claims,
                runtime_commits: prefix.runtime_commits,
                runtime_mutations: prefix.runtime_mutations,
                payload_bytes: prefix.payload_bytes,
                claim_sequence: prefix.claim_sequence,
                runtime_cursor: prefix.runtime_cursor,
                runtime_audit_sha256: prefix.runtime_audit_sha256.clone(),
            },
        })
    }

    pub fn action(&mut self, action: &ArchiveAction, payload: &[u8]) -> Result<()> {
        if payload.len() > MAX_ACTION_BYTES {
            return Err(Error::Archive(
                "archive action exceeds the v1 size bound".into(),
            ));
        }
        self.hashed(&[ACTION_TAG])?;
        self.hashed(&(payload.len() as u64).to_be_bytes())?;
        self.hashed(payload)?;
        self.state.accept(action, payload.len())
    }

    pub fn sync(&mut self) -> Result<()> {
        self.file.sync_all().map_err(archive_io)
    }

    pub fn prefix_inventory(&self) -> Result<PrefixInventory> {
        let prefix_bytes = self.file.metadata().map_err(archive_io)?.len();
        Ok(PrefixInventory {
            header: self.header,
            completed_actions: self.state.action_count,
            standalone_claims: self.state.standalone_claims,
            runtime_commits: self.state.runtime_commits,
            runtime_mutations: self.state.runtime_mutations,
            payload_bytes: self.state.payload_bytes,
            claim_sequence: self.state.claim_sequence,
            runtime_cursor: self.state.runtime_cursor,
            runtime_audit_sha256: self.state.runtime_audit_sha256.clone(),
            prefix_bytes,
            prefix_sha256: hex(self.digest.clone().finalize()),
        })
    }

    pub fn finish(mut self) -> Result<LogicalArchiveInventory> {
        self.validate_complete()?;
        let digest = self.digest.clone().finalize();
        self.file.write_all(&[FOOTER_TAG]).map_err(archive_io)?;
        for value in [
            self.state.standalone_claims,
            self.state.runtime_commits,
            self.state.runtime_mutations,
            self.state.payload_bytes,
        ] {
            self.file
                .write_all(&value.to_be_bytes())
                .map_err(archive_io)?;
        }
        self.file.write_all(&digest).map_err(archive_io)?;
        self.file.sync_all().map_err(archive_io)?;
        Ok(self.inventory(hex(digest)))
    }

    fn validate_complete(&self) -> Result<()> {
        if self.state.action_count != self.header.action_count
            || self.state.claim_sequence != self.header.claim_sequence
            || self.state.runtime_cursor != self.header.runtime_cursor
        {
            return Err(Error::Archive(
                "archive stream did not reach its declared action and watermark bounds".into(),
            ));
        }
        Ok(())
    }

    fn inventory(&self, archive_sha256: String) -> LogicalArchiveInventory {
        LogicalArchiveInventory {
            format_version: LOGICAL_ARCHIVE_VERSION,
            contract_version: RRD_CONTRACT_VERSION,
            archive_sha256,
            action_count: self.state.action_count,
            standalone_claims: self.state.standalone_claims,
            runtime_commits: self.state.runtime_commits,
            runtime_mutations: self.state.runtime_mutations,
            payload_bytes: self.state.payload_bytes,
            claim_sequence: self.state.claim_sequence,
            runtime_cursor: self.state.runtime_cursor,
            runtime_audit_sha256: self.state.runtime_audit_sha256.clone(),
        }
    }

    fn hashed(&mut self, bytes: &[u8]) -> Result<()> {
        self.file.write_all(bytes).map_err(archive_io)?;
        self.digest.update(bytes);
        Ok(())
    }
}

trait Sha256Prefix {
    fn from_finalized_prefix(path: &Path) -> Result<Sha256>;
}

impl Sha256Prefix for Sha256 {
    fn from_finalized_prefix(path: &Path) -> Result<Sha256> {
        let mut file = File::open(path).map_err(archive_io)?;
        let mut digest = Sha256::new();
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let read = file.read(&mut buffer).map_err(archive_io)?;
            if read == 0 {
                break;
            }
            digest.update(&buffer[..read]);
        }
        Ok(digest)
    }
}

pub(super) fn scan_archive_prefix(path: &Path) -> Result<PrefixInventory> {
    let mut file = File::open(path).map_err(archive_io)?;
    let mut digest = Sha256::new();
    let header = read_header(&mut file, &mut digest)?;
    let mut state = ReplayState::default();
    loop {
        let mut tag = [0u8; 1];
        match file.read(&mut tag).map_err(archive_io)? {
            0 => break,
            1 => {}
            _ => unreachable!("one-byte read returned more than one byte"),
        }
        if tag[0] != ACTION_TAG {
            return Err(Error::Archive(
                "logical archive partial contains a footer or unknown record".into(),
            ));
        }
        digest.update(&tag);
        let (action, payload_bytes) = read_action(&mut file, &mut digest)?;
        state.accept(&action, payload_bytes)?;
        if state.action_count > header.action_count {
            return Err(Error::Archive(
                "logical archive partial exceeds its declared action count".into(),
            ));
        }
    }
    let bytes = file.metadata().map_err(archive_io)?.len();
    Ok(state.prefix(header, bytes, hex(digest.finalize())))
}

pub(super) struct PrefixActionReader {
    file: File,
    remaining: u64,
    prefix_bytes: u64,
}

impl PrefixActionReader {
    pub fn open(path: &Path, expected: &PrefixInventory) -> Result<Self> {
        let mut file = File::open(path).map_err(archive_io)?;
        let mut digest = Sha256::new();
        if read_header(&mut file, &mut digest)? != expected.header {
            return Err(Error::Archive(
                "logical archive partial header differs from its receipt".into(),
            ));
        }
        Ok(Self {
            file,
            remaining: expected.completed_actions,
            prefix_bytes: expected.prefix_bytes,
        })
    }

    pub fn compare_or_absent(&mut self, action: &ArchiveAction, payload: &[u8]) -> Result<bool> {
        if self.remaining == 0 {
            return Ok(false);
        }
        let mut tag = [0u8; 1];
        self.file.read_exact(&mut tag).map_err(archive_read_error)?;
        if tag[0] != ACTION_TAG {
            return Err(Error::Archive(
                "logical archive partial action tag differs".into(),
            ));
        }
        let length = read_u64_raw(&mut self.file)?;
        let length = usize::try_from(length)
            .map_err(|_| Error::Archive("archive action length exceeds usize".into()))?;
        if length > MAX_ACTION_BYTES {
            return Err(Error::Archive(
                "archive action exceeds the v1 size bound".into(),
            ));
        }
        let mut existing = vec![0; length];
        self.file
            .read_exact(&mut existing)
            .map_err(archive_read_error)?;
        if existing != payload || serde_json::from_slice::<ArchiveAction>(&existing)? != *action {
            return Err(Error::Archive(
                "logical archive partial differs from the current source cut".into(),
            ));
        }
        self.remaining -= 1;
        Ok(true)
    }

    pub fn finish(mut self) -> Result<()> {
        if self.remaining != 0 {
            return Err(Error::Archive(
                "logical archive partial ended before its recorded action count".into(),
            ));
        }
        if self.file.stream_position().map_err(archive_io)? != self.prefix_bytes {
            return Err(Error::Archive(
                "logical archive partial prefix length differs from its resume evidence".into(),
            ));
        }
        Ok(())
    }
}

pub fn inspect_logical_archive(path: &Path) -> Result<LogicalArchiveInventory> {
    read_archive(path, |_, _| Ok(()))
}

pub(super) fn read_archive(
    path: &Path,
    mut accept: impl FnMut(u64, &ArchiveAction) -> Result<()>,
) -> Result<LogicalArchiveInventory> {
    let mut file = File::open(path).map_err(archive_io)?;
    let mut digest = Sha256::new();
    let header = read_header(&mut file, &mut digest)?;
    let mut state = ReplayState::default();
    for ordinal in 0..header.action_count {
        let mut tag = [0u8; 1];
        read_hashed(&mut file, &mut digest, &mut tag)?;
        if tag[0] != ACTION_TAG {
            return Err(Error::Archive(format!(
                "unknown logical archive record tag {}",
                tag[0]
            )));
        }
        let (action, payload_bytes) = read_action(&mut file, &mut digest)?;
        state.accept(&action, payload_bytes)?;
        accept(ordinal + 1, &action)?;
    }
    let mut footer_tag = [0u8; 1];
    file.read_exact(&mut footer_tag)
        .map_err(archive_read_error)?;
    if footer_tag[0] != FOOTER_TAG {
        return Err(Error::Archive("logical archive footer is absent".into()));
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
    if footer
        != [
            state.standalone_claims,
            state.runtime_commits,
            state.runtime_mutations,
            state.payload_bytes,
        ]
        || declared_digest != actual_digest
        || state.action_count != header.action_count
        || state.claim_sequence != header.claim_sequence
        || state.runtime_cursor != header.runtime_cursor
    {
        return Err(Error::Archive(
            "archive footer, digest, or replay watermarks do not match".into(),
        ));
    }
    Ok(LogicalArchiveInventory {
        format_version: LOGICAL_ARCHIVE_VERSION,
        contract_version: RRD_CONTRACT_VERSION,
        archive_sha256: hex(actual_digest),
        action_count: state.action_count,
        standalone_claims: state.standalone_claims,
        runtime_commits: state.runtime_commits,
        runtime_mutations: state.runtime_mutations,
        payload_bytes: state.payload_bytes,
        claim_sequence: state.claim_sequence,
        runtime_cursor: state.runtime_cursor,
        runtime_audit_sha256: state.runtime_audit_sha256,
    })
}

fn read_header(file: &mut File, digest: &mut Sha256) -> Result<ArchiveHeader> {
    let mut magic = [0u8; 8];
    read_hashed(file, digest, &mut magic)?;
    if &magic != MAGIC {
        return Err(Error::Archive("archive magic does not match".into()));
    }
    let version = read_u16_hashed(file, digest)?;
    let contract = read_u16_hashed(file, digest)?;
    if version != LOGICAL_ARCHIVE_VERSION || contract != RRD_CONTRACT_VERSION {
        return Err(Error::Archive(format!(
            "unsupported logical archive version {version} / contract {contract}"
        )));
    }
    Ok(ArchiveHeader {
        claim_sequence: read_u64_hashed(file, digest)?,
        runtime_cursor: read_u64_hashed(file, digest)?,
        action_count: read_u64_hashed(file, digest)?,
    })
}

fn read_action(file: &mut File, digest: &mut Sha256) -> Result<(ArchiveAction, usize)> {
    let length = read_u64_hashed(file, digest)?;
    let length = usize::try_from(length)
        .map_err(|_| Error::Archive("archive action length exceeds usize".into()))?;
    if length > MAX_ACTION_BYTES {
        return Err(Error::Archive(
            "archive action exceeds the v1 size bound".into(),
        ));
    }
    let mut payload = vec![0; length];
    read_hashed(file, digest, &mut payload)?;
    Ok((serde_json::from_slice(&payload)?, length))
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

fn checked_increment(value: u64, name: &str) -> Result<u64> {
    value
        .checked_add(1)
        .ok_or_else(|| Error::Archive(format!("{name} counter overflow")))
}

pub(super) fn archive_read_error(error: std::io::Error) -> Error {
    if error.kind() == std::io::ErrorKind::UnexpectedEof {
        Error::Archive("archive is truncated".into())
    } else {
        archive_io(error)
    }
}

pub(super) fn archive_io(error: std::io::Error) -> Error {
    Error::Archive(error.to_string())
}

pub(super) fn hex(digest: [u8; 32]) -> String {
    let mut output = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}
