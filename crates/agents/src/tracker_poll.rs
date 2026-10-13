//! Background polling policy; explicit notify/resume reads bypass this state.
use super::{Binding, Diagnostic};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime},
};

const INTERVAL: Duration = Duration::from_secs(2);

#[derive(PartialEq, Eq)]
struct Observed {
    root: PathBuf,
    workdir: String,
    launch_ms: i64,
    launch_id: Option<String>,
    thread_id: String,
    path: PathBuf,
    len: u64,
    modified: SystemTime,
    created: Option<SystemTime>,
    #[cfg(unix)]
    identity: (u64, u64, i64, i64),
}

impl Observed {
    fn read(root: &Path, workdir: &str, binding: &Binding) -> Result<Option<Self>, Diagnostic> {
        let (Some(path), Some(thread)) = (&binding.rollout, &binding.thread_id) else {
            // Discovery has no single verified file to compare yet.
            return Ok(None);
        };
        super::safe_path(root)?;
        if !path.starts_with(root) {
            return Err(Diagnostic::UnsafePath);
        }
        super::safe_path(path)?;
        let metadata = std::fs::metadata(path).map_err(|_| Diagnostic::Unavailable)?;
        if !metadata.is_file() {
            return Err(Diagnostic::Unavailable);
        }
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        Ok(Some(Self {
            root: root.into(),
            workdir: workdir.into(),
            launch_ms: binding.launch_ms,
            launch_id: binding.launch_id.clone(),
            thread_id: thread.clone(),
            path: path.clone(),
            len: metadata.len(),
            modified: metadata.modified().map_err(|_| Diagnostic::Unavailable)?,
            created: metadata.created().ok(),
            #[cfg(unix)]
            identity: (
                metadata.dev(),
                metadata.ino(),
                metadata.ctime(),
                metadata.ctime_nsec(),
            ),
        }))
    }
}

#[derive(Default)]
pub(crate) struct Poll {
    attempted: Option<Instant>,
    verified: Option<Observed>,
}

impl Poll {
    pub(crate) fn invalidate(&mut self) {
        self.verified = None;
    }

    pub(crate) fn due(&self, now: Instant) -> bool {
        self.attempted
            .is_none_or(|at| now.saturating_duration_since(at) >= INTERVAL)
    }

    pub(crate) fn refresh(
        &mut self,
        now: Instant,
        root: &Path,
        workdir: &str,
        binding: &Binding,
    ) -> Result<Option<Binding>, Diagnostic> {
        self.refresh_with(now, root, workdir, binding, super::refresh)
    }

    fn refresh_with(
        &mut self,
        now: Instant,
        root: &Path,
        workdir: &str,
        binding: &Binding,
        read: impl FnOnce(&Path, &str, &Binding) -> Result<Binding, Diagnostic>,
    ) -> Result<Option<Binding>, Diagnostic> {
        if !self.due(now) {
            return Ok(None);
        }
        self.attempted = Some(now);
        let previous = self.verified.take();
        // Remember the observation before parsing. Appends/replacement during the
        // read must cause another attempt, not get cached as already consumed.
        let observed = Observed::read(root, workdir, binding)?;
        if observed.is_some() && previous == observed {
            self.verified = observed;
            return Ok(None);
        }
        let result = read(root, workdir, binding)?;
        // Failed reads remain retryable even if the file's metadata did not change.
        self.verified = observed;
        Ok(Some(result))
    }
}

#[cfg(test)]
#[path = "tracker_poll_tests.rs"]
mod tests;
