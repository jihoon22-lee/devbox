//! SC2 acceptance timing on the native monotonic clock only; renderer clocks are never subtracted.
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

pub const PENDING_LIMIT: usize = 16;

#[derive(Default)]
pub struct PaletteTraces {
    last: u64,
    pending: VecDeque<(u64, Instant)>,
}
#[derive(Debug, PartialEq, Eq)]
pub enum TraceEnd {
    Elapsed(Duration),
    /// Allocated earlier but already acknowledged or dropped to keep the store bounded.
    Stale,
    Unknown,
}
impl PaletteTraces {
    /// Returns the new trace ID and the oldest pending ID dropped to stay within the limit.
    pub fn allocate(&mut self, now: Instant) -> (u64, Option<u64>) {
        self.last = self.last.saturating_add(1);
        let dropped = if self.pending.len() == PENDING_LIMIT {
            self.pending.pop_front().map(|(id, _)| id)
        } else {
            None
        };
        self.pending.push_back((self.last, now));
        (self.last, dropped)
    }
    pub fn finish(&mut self, id: u64, now: Instant) -> TraceEnd {
        match self.pending.iter().position(|(pending, _)| *pending == id) {
            Some(index) => {
                let (_, started) = self.pending.remove(index).expect("pending trace");
                TraceEnd::Elapsed(now.saturating_duration_since(started))
            }
            None if (1..=self.last).contains(&id) => TraceEnd::Stale,
            None => TraceEnd::Unknown,
        }
    }
}
pub struct FirstData {
    started: Instant,
    reported: bool,
}
impl FirstData {
    pub fn new(started: Instant) -> Self {
        Self {
            started,
            reported: false,
        }
    }
    /// Only the first report in the process carries an elapsed time.
    pub fn finish(&mut self, now: Instant) -> Option<Duration> {
        (!std::mem::replace(&mut self.reported, true))
            .then(|| now.saturating_duration_since(self.started))
    }
}
/// The steps of a start, in the order the supervisor finishes them. Each is logged with the time
/// since process start (the clock of `first-data-painted`), so a step's own cost is the
/// difference from the line before it. [다시 시도] and [다시 설정] run the steps again, and each
/// bridge reconnect logs `bridge` again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartPhase {
    /// Supervision began: Tauri, the tray and the main window exist.
    Supervise,
    /// The distro list from the registry and the saved choice.
    Distro,
    /// `wsl.exe` 1: PID 1 is systemd.
    Systemd,
    /// `wsl.exe` 2: the user manager answered.
    UserManager,
    /// The bundled Linux binary was read from the install folder.
    BundleRead,
    /// Its SHA-256 was computed on Windows.
    BundleHash,
    /// `wsl.exe` 3: the installed binary already had that SHA-256, so nothing was sent.
    InstallKept,
    /// `wsl.exe` 3: the binary was sent and published.
    InstallSent,
    /// `wsl.exe` 4: `setup --json`.
    Setup,
    /// `wsl.exe` 5: the startup doctor.
    Doctor,
    /// `wsl.exe` 6: the bridge answered.
    Bridge,
}
impl StartPhase {
    pub fn name(self) -> &'static str {
        match self {
            Self::Supervise => "supervise",
            Self::Distro => "distro",
            Self::Systemd => "systemd",
            Self::UserManager => "user-manager",
            Self::BundleRead => "bundle-read",
            Self::BundleHash => "bundle-hash",
            Self::InstallKept => "install-kept",
            Self::InstallSent => "install-sent",
            Self::Setup => "setup",
            Self::Doctor => "doctor",
            Self::Bridge => "bridge",
        }
    }
}
/// One log line per finished step: identifiers and numbers only.
pub fn start_phase_line(run: u32, phase: StartPhase, elapsed: Duration) -> String {
    format!(
        "start-phase run={run} phase={} elapsed_ms={}",
        phase.name(),
        elapsed.as_millis()
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_start_phase_line_carries_ids_and_numbers_only() {
        assert_eq!(
            start_phase_line(4242, StartPhase::InstallKept, Duration::from_millis(1830)),
            "start-phase run=4242 phase=install-kept elapsed_ms=1830"
        );
        let phases = [
            StartPhase::Supervise,
            StartPhase::Distro,
            StartPhase::Systemd,
            StartPhase::UserManager,
            StartPhase::BundleRead,
            StartPhase::BundleHash,
            StartPhase::InstallKept,
            StartPhase::InstallSent,
            StartPhase::Setup,
            StartPhase::Doctor,
            StartPhase::Bridge,
        ];
        let mut names = std::collections::HashSet::new();
        for phase in phases {
            let name = phase.name();
            assert!(
                name.bytes().all(|b| b.is_ascii_lowercase() || b == b'-'),
                "{name}"
            );
            assert!(names.insert(name), "{name}");
            // The acceptance harness reads `first-data-painted` lines by their own prefix.
            assert!(!start_phase_line(1, phase, Duration::ZERO).starts_with("first-data-painted"));
        }
    }
    #[test]
    fn the_start_logs_its_phases_in_the_order_it_runs_them() {
        // The call sites are Windows-only code: the MSVC check compiles them, and this pins their
        // order. Whitespace is dropped so that formatting cannot hide or fake a match.
        let flat = |source: &str| source.split_whitespace().collect::<String>();
        let ordered = |source: &str, marks: &[&str]| {
            let mut from = 0;
            for mark in marks {
                let at = source[from..]
                    .find(mark)
                    .unwrap_or_else(|| panic!("{mark} is missing or out of order"));
                from += at + mark.len();
            }
        };
        ordered(
            &flat(include_str!("../platform/supervisor.rs")),
            &[
                "start_phase(StartPhase::Supervise);",
                "wsl::distros()",
                "start_phase(StartPhase::Distro);",
                "wsl::systemd(&distro,cancel.clone()).await",
                "deploy::bundled_binary(app)?;",
                "start_phase(StartPhase::BundleRead);",
                "deploy::apply(&distro,INSTANCE,bytes,trigger,",
                "Some(Event::Ready)=>{",
                "start_phase(StartPhase::Bridge);",
            ],
        );
        ordered(
            &flat(include_str!("../platform/wsl.rs")),
            &[
                r#""cat","/proc/1/comm""#,
                "start_phase(StartPhase::Systemd);",
                r#""--property=Version","#,
                "start_phase(StartPhase::UserManager);",
            ],
        );
        ordered(
            &flat(include_str!("../platform/deploy.rs")),
            &[
                "letsha=digest(&bytes);",
                "start_phase(StartPhase::BundleHash);",
                "offer(cmd,bytes,Duration::from_secs(120),cancel).await?",
                "start_phase(transfer.phase());",
                ".args(setup_args(trigger));",
                "start_phase(StartPhase::Setup);",
                ".args(STARTUP_DOCTOR_ARGS);",
                "start_phase(StartPhase::Doctor);",
            ],
        );
    }
    #[test]
    fn palette_trace_is_acknowledged_once_and_unknown_ids_carry_no_elapsed_time() {
        let start = Instant::now();
        let mut traces = PaletteTraces::default();
        let (first, dropped) = traces.allocate(start);
        assert_eq!((first, dropped), (1, None));
        let (second, _) = traces.allocate(start + Duration::from_millis(5));
        assert_eq!(second, 2);
        assert_eq!(
            traces.finish(second, start + Duration::from_millis(45)),
            TraceEnd::Elapsed(Duration::from_millis(40))
        );
        assert_eq!(traces.finish(second, start), TraceEnd::Stale);
        assert_eq!(traces.finish(0, start), TraceEnd::Unknown);
        assert_eq!(traces.finish(99, start), TraceEnd::Unknown);
        assert_eq!(
            traces.finish(first, start + Duration::from_millis(7)),
            TraceEnd::Elapsed(Duration::from_millis(7))
        );
    }
    #[test]
    fn overflow_drops_and_expires_the_oldest_pending_trace() {
        let start = Instant::now();
        let mut traces = PaletteTraces::default();
        for expected in 1..=PENDING_LIMIT as u64 {
            assert_eq!(traces.allocate(start), (expected, None));
        }
        let (next, dropped) = traces.allocate(start);
        assert_eq!((next, dropped), (PENDING_LIMIT as u64 + 1, Some(1)));
        assert_eq!(traces.finish(1, start), TraceEnd::Stale);
        assert_eq!(
            traces.finish(2, start + Duration::from_millis(3)),
            TraceEnd::Elapsed(Duration::from_millis(3))
        );
        assert_eq!(traces.pending.len(), PENDING_LIMIT - 1);
    }
    #[test]
    fn first_data_paint_is_reported_once_per_process() {
        let start = Instant::now();
        let mut first = FirstData::new(start);
        assert_eq!(
            first.finish(start + Duration::from_millis(1200)),
            Some(Duration::from_millis(1200))
        );
        assert_eq!(first.finish(start + Duration::from_millis(1500)), None);
    }
}
