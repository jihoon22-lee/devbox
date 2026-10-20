use crate::{ObserveError, R};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};

/// Linux proc root and monotonic sampling clock. Fixture injection never changes kill's live root.
#[derive(Clone)]
pub struct ProcFs {
    pub(crate) root: PathBuf,
    pub(crate) clock: Arc<dyn Fn() -> u64 + Send + Sync>,
    pub(crate) ticks_per_second: u64,
    pub(crate) page_size: u64,
}
impl Default for ProcFs {
    fn default() -> Self {
        let start = Instant::now();
        Self {
            root: "/proc".into(),
            clock: Arc::new(move || start.elapsed().as_millis() as u64),
            ticks_per_second: system_constant(nix::unistd::SysconfVar::CLK_TCK, 100),
            page_size: system_constant(nix::unistd::SysconfVar::PAGE_SIZE, 4096),
        }
    }
}
impl ProcFs {
    pub fn fixture(
        root: PathBuf,
        clock: Arc<dyn Fn() -> u64 + Send + Sync>,
        ticks_per_second: u64,
        page_size: u64,
    ) -> Self {
        assert!(ticks_per_second > 0 && page_size > 0);
        Self {
            root,
            clock,
            ticks_per_second,
            page_size,
        }
    }
    pub(crate) fn boot_ms(&self) -> R<u64> {
        read_text(&self.root.join("stat"), 1024 * 1024)
            .ok()
            .and_then(|s| {
                s.lines()
                    .find_map(|l| l.strip_prefix("btime ")?.trim().parse::<u64>().ok())
            })
            .and_then(|v| v.checked_mul(1000))
            .ok_or_else(|| ObserveError::SourceUnavailable.into())
    }
    pub(crate) fn start_ms(&self, boot_ms: u64, ticks: u64) -> u64 {
        boot_ms.saturating_add(
            (u128::from(ticks) * 1000 / u128::from(self.ticks_per_second)).min(u128::from(u64::MAX))
                as u64,
        )
    }
}
fn system_constant(var: nix::unistd::SysconfVar, fallback: u64) -> u64 {
    nix::unistd::sysconf(var)
        .ok()
        .flatten()
        .and_then(|v| u64::try_from(v).ok())
        .filter(|v| *v > 0)
        .unwrap_or(fallback)
}
pub(crate) fn read_text(path: &Path, limit: usize) -> std::io::Result<String> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "proc input exceeds limit",
        ));
    }
    String::from_utf8(bytes)
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid proc text"))
}
pub(crate) fn pids(root: &Path) -> R<Vec<u32>> {
    let entries = fs::read_dir(root).map_err(|_| ObserveError::SourceUnavailable)?;
    let mut pids = vec![];
    for (i, entry) in entries.enumerate() {
        if i >= 65_536 {
            return Err(ObserveError::LimitExceeded.into());
        }
        if let Ok(entry) = entry {
            if let Some(pid) = entry
                .file_name()
                .to_str()
                .and_then(|s| s.parse::<u32>().ok())
                .filter(|v| *v > 0 && *v <= i32::MAX as u32)
            {
                pids.push(pid);
            }
        }
    }
    pids.sort_unstable();
    Ok(pids)
}
