use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub const DEFAULT_SEGMENT_IO_REQUEST_BYTES: usize = 16 * 1024 * 1024;
const MIN_SEGMENT_IO_REQUEST_BYTES: usize = 4 * 1024;
const MAX_SEGMENT_IO_REQUEST_BYTES: usize = 64 * 1024 * 1024;

/// Physical access policy for immutable V3 segment blocks. Canonical decoding,
/// checksums, and the decoded block cache remain identical for every mode.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SegmentIoMode {
    /// Probe io_uring on Linux and otherwise use bounded positional reads.
    #[default]
    Auto,
    Mmap,
    IoUring,
    Bounded,
}

impl SegmentIoMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Mmap => "mmap",
            Self::IoUring => "io_uring",
            Self::Bounded => "bounded",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SegmentIoPolicy {
    pub mode: SegmentIoMode,
    pub allow_fallback: bool,
    pub max_request_bytes: usize,
}

impl Default for SegmentIoPolicy {
    fn default() -> Self {
        Self {
            mode: SegmentIoMode::Auto,
            allow_fallback: true,
            max_request_bytes: DEFAULT_SEGMENT_IO_REQUEST_BYTES,
        }
    }
}

impl SegmentIoPolicy {
    pub(crate) fn validate(self) -> Result<Self> {
        if !(MIN_SEGMENT_IO_REQUEST_BYTES..=MAX_SEGMENT_IO_REQUEST_BYTES)
            .contains(&self.max_request_bytes)
        {
            return Err(Error::InvalidConfiguration(format!(
                "segment I/O request bound must be in {MIN_SEGMENT_IO_REQUEST_BYTES}..={MAX_SEGMENT_IO_REQUEST_BYTES} bytes"
            )));
        }
        Ok(self)
    }
}

/// Process-local evidence. It never participates in canonical state or
/// backend selection after the database has opened.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SegmentIoStats {
    pub requested_mode: SegmentIoMode,
    pub configured_max_request_bytes: usize,
    pub mmap_segments: u64,
    pub io_uring_segments: u64,
    pub bounded_segments: u64,
    pub fallback_count: u64,
    pub last_fallback_reason: Option<String>,
    pub read_operations: u64,
    pub mmap_read_operations: u64,
    pub io_uring_read_operations: u64,
    pub bounded_read_operations: u64,
    pub bytes_read: u64,
    pub peak_request_bytes: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SelectedIo {
    Mmap,
    IoUring,
    Bounded,
}

pub(crate) type SharedIoContext = Arc<IoContext>;

pub(crate) struct IoContext {
    policy: SegmentIoPolicy,
    stats: Mutex<SegmentIoStats>,
    #[cfg(target_os = "linux")]
    ring: Option<Mutex<io_uring::IoUring>>,
    uring_disabled: AtomicBool,
    uring_unavailable: Option<String>,
}

impl std::fmt::Debug for IoContext {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("IoContext")
            .field("policy", &self.policy)
            .field("stats", &self.stats())
            .field("uring_unavailable", &self.uring_unavailable)
            .finish_non_exhaustive()
    }
}

impl IoContext {
    pub(crate) fn new(policy: SegmentIoPolicy) -> Result<SharedIoContext> {
        let policy = policy.validate()?;
        let (ring, unavailable) = probe_io_uring();
        #[cfg(not(target_os = "linux"))]
        let _ = ring;
        if policy.mode == SegmentIoMode::IoUring && ring.is_none() && !policy.allow_fallback {
            return Err(Error::InvalidConfiguration(format!(
                "io_uring was required but is unavailable: {}",
                unavailable.as_deref().unwrap_or("runtime probe failed")
            )));
        }
        Ok(Arc::new(Self {
            policy,
            stats: Mutex::new(SegmentIoStats {
                requested_mode: policy.mode,
                configured_max_request_bytes: policy.max_request_bytes,
                ..SegmentIoStats::default()
            }),
            #[cfg(target_os = "linux")]
            ring,
            uring_disabled: AtomicBool::new(false),
            uring_unavailable: unavailable,
        }))
    }

    pub(crate) fn policy(&self) -> SegmentIoPolicy {
        self.policy
    }

    pub(crate) fn file_backend(&self) -> Result<SelectedIo> {
        match self.policy.mode {
            SegmentIoMode::Mmap => Ok(SelectedIo::Mmap),
            SegmentIoMode::Bounded => Ok(SelectedIo::Bounded),
            SegmentIoMode::Auto | SegmentIoMode::IoUring => {
                if self.has_io_uring() {
                    Ok(SelectedIo::IoUring)
                } else if self.policy.allow_fallback {
                    self.record_fallback(
                        self.uring_unavailable
                            .as_deref()
                            .unwrap_or("io_uring runtime probe failed"),
                    );
                    Ok(SelectedIo::Bounded)
                } else {
                    Err(Error::InvalidConfiguration(
                        "io_uring is unavailable and fallback is disabled".into(),
                    ))
                }
            }
        }
    }

    pub(crate) fn record_segment(&self, selected: SelectedIo) {
        let mut stats = self.lock_stats();
        match selected {
            SelectedIo::Mmap => stats.mmap_segments = stats.mmap_segments.saturating_add(1),
            SelectedIo::IoUring => {
                stats.io_uring_segments = stats.io_uring_segments.saturating_add(1)
            }
            SelectedIo::Bounded => {
                stats.bounded_segments = stats.bounded_segments.saturating_add(1)
            }
        }
    }

    pub(crate) fn record_fallback(&self, reason: &str) {
        let mut stats = self.lock_stats();
        stats.fallback_count = stats.fallback_count.saturating_add(1);
        stats.last_fallback_reason = Some(reason.to_owned());
    }

    pub(crate) fn record_read(&self, selected: SelectedIo, bytes: usize) {
        let mut stats = self.lock_stats();
        stats.read_operations = stats.read_operations.saturating_add(1);
        match selected {
            SelectedIo::Mmap => {
                stats.mmap_read_operations = stats.mmap_read_operations.saturating_add(1)
            }
            SelectedIo::IoUring => {
                stats.io_uring_read_operations = stats.io_uring_read_operations.saturating_add(1)
            }
            SelectedIo::Bounded => {
                stats.bounded_read_operations = stats.bounded_read_operations.saturating_add(1)
            }
        }
        stats.bytes_read = stats.bytes_read.saturating_add(bytes as u64);
        stats.peak_request_bytes = stats.peak_request_bytes.max(bytes);
    }

    pub(crate) fn stats(&self) -> SegmentIoStats {
        self.lock_stats().clone()
    }

    pub(crate) fn read_exact_at(
        &self,
        selected: SelectedIo,
        file: &File,
        offset: u64,
        output: &mut [u8],
    ) -> Result<()> {
        if output.len() > self.policy.max_request_bytes {
            return Err(Error::InvalidSegment(format!(
                "segment block requests {} bytes beyond the configured {} byte I/O bound",
                output.len(),
                self.policy.max_request_bytes
            )));
        }
        let actual = match selected {
            SelectedIo::IoUring if !self.uring_disabled.load(Ordering::Acquire) => {
                match self.read_exact_io_uring(file, offset, output) {
                    Ok(()) => SelectedIo::IoUring,
                    Err(error) if self.policy.allow_fallback && is_uring_unavailable(&error) => {
                        self.uring_disabled.store(true, Ordering::Release);
                        self.record_fallback(&format!("io_uring read failed: {error}"));
                        positional_read_exact(file, offset, output)?;
                        SelectedIo::Bounded
                    }
                    Err(error) => return Err(error),
                }
            }
            SelectedIo::IoUring | SelectedIo::Bounded => {
                positional_read_exact(file, offset, output)?;
                SelectedIo::Bounded
            }
            SelectedIo::Mmap => {
                return Err(Error::InvalidSegment(
                    "mmap segment attempted positional I/O".into(),
                ))
            }
        };
        self.record_read(actual, output.len());
        Ok(())
    }

    fn lock_stats(&self) -> std::sync::MutexGuard<'_, SegmentIoStats> {
        self.stats
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[cfg(target_os = "linux")]
    fn has_io_uring(&self) -> bool {
        self.ring.is_some()
    }

    #[cfg(not(target_os = "linux"))]
    fn has_io_uring(&self) -> bool {
        false
    }

    #[cfg(target_os = "linux")]
    fn read_exact_io_uring(&self, file: &File, offset: u64, output: &mut [u8]) -> Result<()> {
        use io_uring::{opcode, types};
        use std::os::fd::AsRawFd;

        let length = u32::try_from(output.len()).map_err(|_| {
            Error::InvalidSegment("io_uring request length exceeds its u32 ABI".into())
        })?;
        let ring = self.ring.as_ref().ok_or_else(|| {
            Error::InvalidConfiguration("io_uring was selected without a runtime ring".into())
        })?;
        let mut ring = ring
            .lock()
            .map_err(|_| Error::InvalidSegment("io_uring lock poisoned".into()))?;
        let entry = opcode::Read::new(types::Fd(file.as_raw_fd()), output.as_mut_ptr(), length)
            .offset(offset)
            .build()
            .user_data(1);
        unsafe {
            ring.submission()
                .push(&entry)
                .map_err(|_| Error::Io(std::io::Error::other("io_uring submission queue full")))?;
        }
        ring.submit_and_wait(1)?;
        let completion = ring
            .completion()
            .next()
            .ok_or_else(|| Error::Io(std::io::Error::other("io_uring produced no completion")))?;
        let result = completion.result();
        if result < 0 {
            return Err(Error::Io(std::io::Error::from_raw_os_error(-result)));
        }
        if result as usize != output.len() {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                format!(
                    "io_uring read returned {result} of {} requested bytes",
                    output.len()
                ),
            )));
        }
        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    fn read_exact_io_uring(&self, _file: &File, _offset: u64, _output: &mut [u8]) -> Result<()> {
        Err(Error::InvalidConfiguration(
            "io_uring is available only on Linux".into(),
        ))
    }
}

#[cfg(target_os = "linux")]
fn probe_io_uring() -> (Option<Mutex<io_uring::IoUring>>, Option<String>) {
    use io_uring::{opcode, Probe};

    match io_uring::IoUring::new(8) {
        Ok(ring) => {
            let mut probe = Probe::new();
            if let Err(error) = ring.submitter().register_probe(&mut probe) {
                return (None, Some(format!("opcode probe failed: {error}")));
            }
            if !probe.is_supported(opcode::Read::CODE) {
                return (
                    None,
                    Some("kernel does not advertise IORING_OP_READ".into()),
                );
            }
            (Some(Mutex::new(ring)), None)
        }
        Err(error) => (None, Some(format!("ring setup failed: {error}"))),
    }
}

#[cfg(not(target_os = "linux"))]
fn probe_io_uring() -> (Option<()>, Option<String>) {
    (None, Some("io_uring is available only on Linux".into()))
}

fn is_uring_unavailable(error: &Error) -> bool {
    let Error::Io(error) = error else {
        return false;
    };
    error.raw_os_error().is_some_and(|code| {
        #[cfg(target_os = "linux")]
        {
            matches!(
                code,
                libc::EPERM | libc::EACCES | libc::EINVAL | libc::EOPNOTSUPP | libc::ENOSYS
            )
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = code;
            true
        }
    })
}

#[cfg(unix)]
fn positional_read_exact(file: &File, mut offset: u64, mut output: &mut [u8]) -> Result<()> {
    use std::os::unix::fs::FileExt;

    while !output.is_empty() {
        let read = file.read_at(output, offset)?;
        if read == 0 {
            return Err(Error::Io(std::io::Error::from(
                std::io::ErrorKind::UnexpectedEof,
            )));
        }
        offset = offset.saturating_add(read as u64);
        output = &mut output[read..];
    }
    Ok(())
}

#[cfg(windows)]
fn positional_read_exact(file: &File, mut offset: u64, mut output: &mut [u8]) -> Result<()> {
    use std::os::windows::fs::FileExt;

    while !output.is_empty() {
        let read = file.seek_read(output, offset)?;
        if read == 0 {
            return Err(Error::Io(std::io::Error::from(
                std::io::ErrorKind::UnexpectedEof,
            )));
        }
        offset = offset.saturating_add(read as u64);
        output = &mut output[read..];
    }
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn positional_read_exact(file: &File, offset: u64, output: &mut [u8]) -> Result<()> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = file.try_clone()?;
    file.seek(SeekFrom::Start(offset))?;
    file.read_exact(output)?;
    Ok(())
}
