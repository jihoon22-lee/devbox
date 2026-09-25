//! Request counters preserve shared admission and reserved stop capacity.
pub(crate) use product_ipc::workspace::Lane;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tokio::sync::Semaphore;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LanePolicy {
    pub requests: usize,
    pub workers: usize,
}
pub(crate) const fn lane_policy(lane: Lane) -> LanePolicy {
    let (requests, workers) = match lane {
        Lane::Terminal | Lane::TerminalIo => (64, 4),
        Lane::TerminalStop => (64, 2),
        Lane::Engine => (24, 4),
        Lane::EngineStop => (32, 2),
        Lane::EngineBackground => (2, 4),
        Lane::Source | Lane::Files => (16, 2),
        Lane::FilesWatch => (1, 2),
        Lane::Lsp => (8, 2),
        Lane::LspStop => (10, 2),
        Lane::Metadata | Lane::Probes => (2, 0),
        Lane::Dialogs | Lane::Context => (1, 0),
    };
    LanePolicy { requests, workers }
}
const COUNT: usize = Lane::Context as usize + 1;
const fn request_group(lane: Lane) -> usize {
    match lane {
        Lane::EngineStop | Lane::EngineBackground => Lane::Engine as usize,
        Lane::FilesWatch => Lane::Files as usize,
        Lane::LspStop => Lane::Lsp as usize,
        _ => lane as usize,
    }
}
const fn worker_group(lane: Lane) -> usize {
    match lane {
        Lane::EngineBackground => Lane::Engine as usize,
        Lane::FilesWatch => Lane::Files as usize,
        Lane::LspStop => Lane::Lsp as usize,
        _ => lane as usize,
    }
}
const ALL: [Lane; COUNT] = [
    Lane::Terminal,
    Lane::TerminalIo,
    Lane::TerminalStop,
    Lane::Engine,
    Lane::EngineStop,
    Lane::EngineBackground,
    Lane::Source,
    Lane::Files,
    Lane::FilesWatch,
    Lane::Lsp,
    Lane::LspStop,
    Lane::Metadata,
    Lane::Probes,
    Lane::Dialogs,
    Lane::Context,
];
#[derive(Clone)]
pub(crate) struct Lanes {
    requests: [Arc<AtomicUsize>; COUNT],
    workers: [Arc<Semaphore>; COUNT],
}
impl Default for Lanes {
    fn default() -> Self {
        Self {
            requests: std::array::from_fn(|_| Arc::default()),
            workers: std::array::from_fn(|i| Arc::new(Semaphore::new(lane_policy(ALL[i]).workers))),
        }
    }
}
pub(crate) struct RequestPermit(Arc<AtomicUsize>);
impl Drop for RequestPermit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}
impl Lanes {
    pub(crate) fn try_enter(&self, lane: Lane) -> Result<RequestPermit, &'static str> {
        let counter = &self.requests[request_group(lane)];
        counter
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < lane_policy(lane).requests).then_some(n + 1)
            })
            .map_err(|_| "busy")?;
        Ok(RequestPermit(counter.clone()))
    }
    // Request ownership is acquired first. The host waits for context/filesystem
    // admission before taking a worker; dialogs and stop operations may bypass it.
    pub(crate) fn workers(&self, lane: Lane) -> Arc<Semaphore> {
        self.workers[worker_group(lane)].clone()
    }
    pub(crate) fn active(&self, lane: Lane) -> usize {
        self.requests[request_group(lane)].load(Ordering::Acquire)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn policies_preserve_live_limits_including_shared_stop_reserves() {
        for (lane, requests, workers) in [
            (Lane::Terminal, 64, 4),
            (Lane::TerminalIo, 64, 4),
            (Lane::TerminalStop, 64, 2),
            (Lane::Engine, 24, 4),
            (Lane::EngineStop, 32, 2),
            (Lane::EngineBackground, 2, 4),
            (Lane::Files, 16, 2),
            (Lane::FilesWatch, 1, 2),
            (Lane::Lsp, 8, 2),
            (Lane::LspStop, 10, 2),
            (Lane::Source, 16, 2),
            (Lane::Metadata, 2, 0),
            (Lane::Probes, 2, 0),
            (Lane::Dialogs, 1, 0),
            (Lane::Context, 1, 0),
        ] {
            assert_eq!(lane_policy(lane), LanePolicy { requests, workers });
        }
    }
    #[test]
    fn saturation_keeps_stop_capacity_without_widening_the_shared_pool() {
        for (normal, stop, normal_limit, stop_reserve) in [
            (Lane::Engine, Lane::EngineStop, 24, 8),
            (Lane::Lsp, Lane::LspStop, 8, 2),
        ] {
            let lanes = Lanes::default();
            let busy: Vec<_> = (0..normal_limit)
                .map(|_| lanes.try_enter(normal).unwrap())
                .collect();
            assert!(lanes.try_enter(normal).is_err());
            let stops: Vec<_> = (0..stop_reserve)
                .map(|_| lanes.try_enter(stop).unwrap())
                .collect();
            assert_eq!(lanes.active(normal), normal_limit + stop_reserve);
            assert!(lanes.try_enter(stop).is_err());
            drop(busy);
            drop(stops);
            assert_eq!(lanes.active(normal), 0);
        }
    }
    #[test]
    fn watchers_share_file_requests_and_workers_but_not_lsp_capacity() {
        let lanes = Lanes::default();
        let file = lanes.try_enter(Lane::Files).unwrap();
        assert!(lanes.try_enter(Lane::FilesWatch).is_err());
        assert!(lanes.try_enter(Lane::Lsp).is_ok());
        assert!(Arc::ptr_eq(
            &lanes.workers(Lane::Files),
            &lanes.workers(Lane::FilesWatch)
        ));
        drop(file);
        assert!(lanes.try_enter(Lane::FilesWatch).is_ok());
    }
    #[tokio::test]
    async fn terminal_restore_and_engine_workers_leave_stop_workers_available() {
        let lanes = Lanes::default();
        for (normal, stop) in [
            (Lane::Terminal, Lane::TerminalStop),
            (Lane::Engine, Lane::EngineStop),
        ] {
            let workers: Vec<_> = (0..lane_policy(normal).workers)
                .map(|_| lanes.workers(normal).try_acquire_owned().unwrap())
                .collect();
            assert!(lanes.workers(normal).try_acquire_owned().is_err());
            assert!(lanes.workers(stop).try_acquire_owned().is_ok());
            drop(workers);
        }
    }
}
