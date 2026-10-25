use crate::CaptureReason;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy)]
pub(crate) struct Limits {
    pub files: usize,
    pub file_bytes: u64,
    pub bytes: u64,
    pub entries: usize,
    pub metadata: usize,
    pub duration: Duration,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            files: 4096,
            file_bytes: crate::CODE_LIMIT,
            bytes: 32 * 1024 * 1024,
            entries: 16_384,
            metadata: 4 * 1024 * 1024,
            duration: Duration::from_secs(2),
        }
    }
}
pub(crate) struct Budget {
    pub limits: Limits,
    pub bytes_read: u64,
    pub entries: usize,
    pub files: usize,
    metadata: usize,
    pub stopped: Option<CaptureReason>,
    cancel: CancellationToken,
    deadline: Instant,
}
impl Budget {
    pub fn new(limits: Limits, cancel: CancellationToken, started: Instant) -> Self {
        Self {
            limits,
            bytes_read: 0,
            entries: 0,
            files: 0,
            metadata: 0,
            stopped: None,
            cancel,
            deadline: started + limits.duration,
        }
    }
    pub fn stop(&mut self, reason: CaptureReason) -> CaptureReason {
        *self.stopped.get_or_insert(reason)
    }
    pub fn check(&mut self) -> Result<(), CaptureReason> {
        if let Some(reason) = self.stopped {
            return Err(reason);
        }
        if self.cancel.is_cancelled() {
            return Err(self.stop(CaptureReason::Cancelled));
        }
        if Instant::now() >= self.deadline {
            return Err(self.stop(CaptureReason::Deadline));
        }
        Ok(())
    }
    pub fn metadata(&mut self, bytes: usize) -> Result<(), CaptureReason> {
        self.check()?;
        if bytes > self.limits.metadata.saturating_sub(self.metadata) {
            return Err(self.stop(CaptureReason::MetadataLimit));
        }
        self.metadata += bytes;
        Ok(())
    }
    pub fn entry(&mut self) -> Result<(), CaptureReason> {
        self.check()?;
        if self.entries >= self.limits.entries {
            return Err(self.stop(CaptureReason::EntryLimit));
        }
        self.entries += 1;
        Ok(())
    }
    pub fn file(&mut self) -> Result<(), CaptureReason> {
        self.check()?;
        if self.files >= self.limits.files {
            return Err(self.stop(CaptureReason::FileLimit));
        }
        self.files += 1;
        Ok(())
    }
}
