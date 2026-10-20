use crate::{
    host::{ProcFs, pids, read_text},
    ports::unit_from_cgroup,
    *,
};
use nix::{
    sys::signal::{Signal, kill},
    unistd::{Pid, Uid},
};
use std::{collections::HashMap, fs, os::unix::fs::MetadataExt, path::Path};

#[derive(Clone)]
pub(crate) struct Stat {
    pub name: String,
    pub cpu_ticks: u64,
    pub start_ticks: u64,
    pub rss_pages: u64,
}
pub(crate) fn parse_stat(text: &str) -> Option<Stat> {
    let begin = text.find('(')?;
    let end = text.rfind(')')?;
    let fields: Vec<_> = text.get(end + 1..)?.split_whitespace().collect();
    Some(Stat {
        name: text.get(begin + 1..end)?.to_owned(),
        cpu_ticks: fields
            .get(11)?
            .parse::<u64>()
            .ok()?
            .saturating_add(fields.get(12)?.parse().ok()?),
        start_ticks: fields.get(19)?.parse().ok()?,
        rss_pages: fields.get(21)?.parse::<i64>().ok()?.max(0) as u64,
    })
}
pub(crate) fn project_for<'a>(cwd: Option<&Path>, projects: &'a [ProjectRoot]) -> Option<&'a str> {
    let cwd = cwd?;
    projects
        .iter()
        .filter(|p| cwd.starts_with(&p.path))
        .max_by_key(|p| p.path.components().count())
        .map(|p| p.id.as_str())
}
#[derive(Default)]
pub(crate) struct ProcessSampler {
    previous: HashMap<u32, (u64, u64)>,
    sampled_ms: Option<u64>,
}
impl ProcessSampler {
    pub(crate) fn sample(
        &mut self,
        proc: &ProcFs,
        projects: &[ProjectRoot],
        runs: &[UnitRun],
        selected: Option<&str>,
    ) -> R<Vec<ProcessInfo>> {
        if selected.is_some_and(|id| !projects.iter().any(|p| p.id == id)) {
            return Err(ObserveError::ProjectNotFound.into());
        }
        let boot = proc.boot_ms()?;
        let now = (proc.clock)();
        let elapsed = self
            .sampled_ms
            .map(|before| now.saturating_sub(before))
            .unwrap_or(0);
        let mut next = HashMap::new();
        let mut result = vec![];
        for pid in pids(&proc.root)? {
            let dir = proc.root.join(pid.to_string());
            let Some(stat) = read_text(&dir.join("stat"), 65536)
                .ok()
                .and_then(|s| parse_stat(&s))
            else {
                continue;
            };
            let Ok(meta) = fs::metadata(&dir) else {
                continue;
            };
            let cwd = fs::read_link(dir.join("cwd")).ok();
            let unit = read_text(&dir.join("cgroup"), 65536)
                .ok()
                .and_then(|s| unit_from_cgroup(&s));
            let run = runs
                .iter()
                .find(|r| Some(r.unit.as_str()) == unit.as_deref());
            let delta = self
                .previous
                .get(&pid)
                .filter(|(start, _)| *start == stat.start_ticks)
                .map_or(0, |(_, ticks)| stat.cpu_ticks.saturating_sub(*ticks));
            let cpu_percent = if elapsed == 0 {
                0.0
            } else {
                delta as f64 * 100_000.0 / proc.ticks_per_second as f64 / elapsed as f64
            };
            let project_id = run
                .map(|r| r.project_id.clone())
                .or_else(|| project_for(cwd.as_deref(), projects).map(str::to_owned));
            next.insert(pid, (stat.start_ticks, stat.cpu_ticks));
            result.push(ProcessInfo {
                pid,
                name: stat.name,
                uid: meta.uid(),
                cwd: cwd.map(|p| p.to_string_lossy().into_owned()),
                start_time_ms: proc.start_ms(boot, stat.start_ticks),
                cpu_percent,
                rss_bytes: stat.rss_pages.saturating_mul(proc.page_size),
                unit,
                run_id: run.map(|r| r.run_id.clone()),
                project_id,
            });
        }
        self.previous = next;
        self.sampled_ms = Some(now);
        let prioritized = |p: &ProcessInfo| {
            selected.is_some_and(|id| {
                p.project_id.as_deref() == Some(id)
                    || projects.iter().any(|root| {
                        root.id == id
                            && p.cwd
                                .as_deref()
                                .is_some_and(|cwd| Path::new(cwd).starts_with(&root.path))
                    })
            })
        };
        result.sort_by(|a, b| {
            prioritized(b)
                .cmp(&prioritized(a))
                .then_with(|| b.cpu_percent.total_cmp(&a.cpu_percent))
                .then(a.pid.cmp(&b.pid))
        });
        let project_count = result.iter().take_while(|p| prioritized(p)).count();
        result.truncate(project_count.saturating_add(50));
        // Keep query responses below the existing transport budget; never silently drop project rows.
        if serde_json::to_vec(&result)
            .map_err(|e| devbox_protocol::Fail::Internal(e.to_string()))?
            .len()
            > 768 * 1024
        {
            return Err(ObserveError::LimitExceeded.into());
        }
        Ok(result)
    }
}
pub(crate) struct Identity {
    pub start_time_ms: u64,
    start_ticks: u64,
    uid: u32,
}
pub(crate) fn identity(pid: u32) -> R<Identity> {
    let proc = ProcFs::default();
    let path = proc.root.join(pid.to_string());
    let stat = read_text(&path.join("stat"), 65536)
        .ok()
        .and_then(|s| parse_stat(&s))
        .ok_or(ObserveError::StateChanged)?;
    let uid = fs::metadata(path)
        .map_err(|_| ObserveError::StateChanged)?
        .uid();
    Ok(Identity {
        start_time_ms: proc.start_ms(proc.boot_ms()?, stat.start_ticks),
        start_ticks: stat.start_ticks,
        uid,
    })
}
pub(crate) fn kill_process(p: KillProcess) -> R<()> {
    if p.pid == 0 || p.pid > i32::MAX as u32 {
        return Err(ObserveError::InvalidInput.into());
    }
    // Never signal init, even when the daemon itself happens to run as root.
    if p.pid == 1 {
        return Err(ObserveError::PermissionDenied.into());
    }
    let before = identity(p.pid)?;
    if before.uid != Uid::effective().as_raw() {
        return Err(ObserveError::PermissionDenied.into());
    }
    if before.start_time_ms != p.expect.start_time_ms {
        return Err(ObserveError::StateChanged.into());
    }
    let signal = match p.signal {
        KillSignal::Term => Signal::SIGTERM,
        KillSignal::Kill => Signal::SIGKILL,
    };
    // Re-read live proc immediately before signaling. Fixture roots cannot redirect this check.
    let current = identity(p.pid)?;
    if current.uid != Uid::effective().as_raw() {
        return Err(ObserveError::PermissionDenied.into());
    }
    if current.start_ticks != before.start_ticks || current.start_time_ms != p.expect.start_time_ms
    {
        return Err(ObserveError::StateChanged.into());
    }
    kill(Pid::from_raw(p.pid as i32), signal).map_err(|e| match e {
        nix::errno::Errno::ESRCH => ObserveError::StateChanged,
        nix::errno::Errno::EPERM => ObserveError::PermissionDenied,
        _ => ObserveError::SourceUnavailable,
    })?;
    Ok(())
}
