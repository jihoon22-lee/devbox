//! Foreground-window activity collector (01 §6.1). A dedicated thread owns two
//! WinEvent hooks (foreground change, foreground title change) and a 5 s input
//! timer. It reports raw observations to a sink; consent, pause, privacy rules
//! and numbering belong to the caller. `Tracker` is the platform-independent
//! decision logic, tested on Linux.
use devbox_activity::model::event::{Observation, ObservationKind};

/// Input timer period.
pub const TICK_MS: u32 = 5_000;
/// An input heartbeat once the last input advanced this far.
pub const HEARTBEAT_MS: i64 = 30_000;
/// Ticks further apart than this mean the system slept (or the thread stalled).
pub const GAP_MS: i64 = 2 * TICK_MS as i64;

/// `(app, title)` of the foreground window.
pub type Window = (String, Option<String>);

#[derive(Debug, Default)]
pub(crate) struct Tracker {
    current: Option<Window>,
    idle: bool,
    heartbeat_input_ms: i64,
    /// `(monotonic ms, last input)` of the previous tick.
    previous_tick: Option<(i64, i64)>,
}

fn observation(
    kind: ObservationKind,
    window: Option<&Window>,
    at_ms: i64,
    last_input_ms: i64,
) -> Observation {
    Observation {
        seq: 0,
        at_ms,
        kind,
        app: window.map(|w| w.0.clone()),
        title: window.and_then(|w| w.1.clone()),
        last_input_ms,
    }
}

impl Tracker {
    /// The foreground window or its title changed. The same app and title in a
    /// row is not reported; while idle it is only remembered.
    pub(crate) fn foreground(
        &mut self,
        window: Window,
        at_ms: i64,
        last_input_ms: i64,
    ) -> Option<Observation> {
        if self.current.as_ref() == Some(&window) {
            return None;
        }
        self.current = Some(window);
        if self.idle {
            return None;
        }
        self.heartbeat_input_ms = last_input_ms;
        Some(observation(
            ObservationKind::Foreground,
            self.current.as_ref(),
            at_ms,
            last_input_ms,
        ))
    }

    /// The input timer: idle once past the threshold, the current window again
    /// when input resumes, otherwise a heartbeat every 30 s of input progress.
    /// A gap over `GAP_MS` on the monotonic clock (`monotonic_ms`, counting
    /// sleep) is a suspend: idle at the last input seen before it, so waking
    /// input never stretches the session across the sleep (v0.9.0's 10 s rule).
    pub(crate) fn tick(
        &mut self,
        at_ms: i64,
        monotonic_ms: i64,
        since_input_ms: i64,
        threshold_ms: i64,
        window: impl FnOnce() -> Option<Window>,
    ) -> Option<Observation> {
        let last_input_ms = at_ms.saturating_sub(since_input_ms.max(0));
        let previous = self.previous_tick.replace((monotonic_ms, last_input_ms));
        if let Some((tick_ms, before_ms)) = previous
            && monotonic_ms.saturating_sub(tick_ms) > GAP_MS
            && !self.idle
        {
            self.idle = true;
            return Some(observation(ObservationKind::Idle, None, at_ms, before_ms));
        }
        if since_input_ms >= threshold_ms {
            if self.idle {
                return None;
            }
            self.idle = true;
            return Some(observation(
                ObservationKind::Idle,
                None,
                at_ms,
                last_input_ms,
            ));
        }
        if self.idle {
            self.idle = false;
            if let Some(now) = window() {
                self.current = Some(now);
            }
            self.heartbeat_input_ms = last_input_ms;
            return self
                .current
                .as_ref()
                .map(|w| observation(ObservationKind::Foreground, Some(w), at_ms, last_input_ms));
        }
        if last_input_ms - self.heartbeat_input_ms < HEARTBEAT_MS {
            return None;
        }
        self.heartbeat_input_ms = last_input_ms;
        Some(observation(
            ObservationKind::Input,
            self.current.as_ref(),
            at_ms,
            last_input_ms,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(app: &str, title: &str) -> Window {
        (app.into(), Some(title.into()))
    }
    fn kind(o: &Option<Observation>) -> Option<ObservationKind> {
        o.as_ref().map(|o| o.kind)
    }

    #[test]
    fn tracker_emits_idle_once_heartbeats_every_30s_and_skips_duplicate_foregrounds() {
        let mut t = Tracker::default();
        let first = t
            .foreground(win("code.exe", "a"), 1_000, 1_000)
            .expect("first window");
        assert_eq!(
            (first.kind, first.app.as_deref(), first.title.as_deref()),
            (ObservationKind::Foreground, Some("code.exe"), Some("a"))
        );
        assert!(
            t.foreground(win("code.exe", "a"), 2_000, 2_000).is_none(),
            "same app and title"
        );
        assert!(
            t.foreground(win("code.exe", "b"), 3_000, 3_000).is_some(),
            "title change"
        );
        let current = || Some(win("code.exe", "b"));

        // Input advanced 29 s since the last report: nothing; 30 s: a heartbeat.
        assert!(t.tick(40_000, 40_000, 8_000, 300_000, current).is_none());
        let beat = t.tick(45_000, 45_000, 12_000, 300_000, current);
        assert_eq!(kind(&beat), Some(ObservationKind::Input));
        let beat = beat.expect("heartbeat");
        assert_eq!(
            (beat.last_input_ms, beat.title.as_deref()),
            (33_000, Some("b"))
        );
        assert!(
            t.tick(50_000, 50_000, 17_000, 300_000, current).is_none(),
            "no new input"
        );

        // Idle once, carrying the last input time.
        let idle = t
            .tick(400_000, 400_000, 367_000, 300_000, current)
            .expect("idle");
        assert_eq!(
            (idle.kind, idle.last_input_ms),
            (ObservationKind::Idle, 33_000)
        );
        assert!(
            t.tick(405_000, 405_000, 372_000, 300_000, current)
                .is_none(),
            "idle once"
        );
        assert!(
            t.foreground(win("other.exe", "c"), 406_000, 33_000)
                .is_none(),
            "remembered while idle"
        );

        // Input resumes: the current window again even if unchanged.
        let back = t
            .tick(410_000, 410_000, 1_000, 300_000, current)
            .expect("resume");
        assert_eq!(
            (back.kind, back.title.as_deref(), back.last_input_ms),
            (ObservationKind::Foreground, Some("b"), 409_000)
        );
        assert!(
            t.foreground(win("code.exe", "b"), 411_000, 409_000)
                .is_none()
        );
        assert!(
            t.tick(415_000, 415_000, 6_000, 300_000, current).is_none(),
            "resume counts as the last report"
        );
    }

    #[test]
    fn a_suspend_gap_goes_idle_at_the_last_input_before_it() {
        let mut t = Tracker::default();
        let current = || Some(win("code.exe", "a"));
        t.foreground(win("code.exe", "a"), 1_000, 1_000);
        assert!(t.tick(5_000, 5_000, 1_000, 300_000, current).is_none());
        assert!(
            t.tick(10_000, 10_000, 2_000, 300_000, current).is_none(),
            "10 s is not a gap"
        );
        // Asleep for an hour; the waking key press is 1 s old.
        let idle = t
            .tick(3_610_000, 3_610_000, 1_000, 300_000, current)
            .expect("gap");
        assert_eq!(
            (idle.kind, idle.last_input_ms),
            (ObservationKind::Idle, 8_000)
        );
        let back = t
            .tick(3_615_000, 3_615_000, 2_000, 300_000, current)
            .expect("resume");
        assert_eq!(
            (back.kind, back.last_input_ms),
            (ObservationKind::Foreground, 3_613_000)
        );
        // A gap while already idle adds no second idle; input resumes as usual.
        assert!(
            t.tick(3_620_000, 3_620_000, 400_000, 300_000, current)
                .is_some_and(|o| o.kind == ObservationKind::Idle)
        );
        assert!(
            t.tick(9_000_000, 9_000_000, 1_000, 300_000, current)
                .is_some_and(|o| o.kind == ObservationKind::Foreground)
        );
        // A wall-clock jump is not a gap; only the monotonic clock decides.
        assert!(
            t.tick(20_000_000, 9_005_000, 1_000, 300_000, current)
                .is_none_or(|o| o.kind != ObservationKind::Idle)
        );
    }
}

#[cfg(windows)]
pub use native::{CollectorHandle, start};

#[cfg(windows)]
mod native {
    use super::{TICK_MS, Tracker, Window};
    use devbox_activity::model::event::Observation;
    use devbox_activity::model::idle::duration_from_input_ticks;
    use std::cell::RefCell;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicI64, Ordering};
    use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, WPARAM};
    use windows::Win32::System::SystemInformation::GetTickCount64;
    use windows::Win32::System::Threading::{
        GetCurrentThreadId, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
        QueryFullProcessImageNameW,
    };
    use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
    use windows::Win32::UI::WindowsAndMessaging::{
        CHILDID_SELF, DispatchMessageW, EVENT_OBJECT_NAMECHANGE, EVENT_SYSTEM_FOREGROUND,
        GetForegroundWindow, GetMessageW, GetWindowThreadProcessId, InternalGetWindowText,
        KillTimer, MSG, OBJID_WINDOW, PM_NOREMOVE, PeekMessageW, PostThreadMessageW, SetTimer,
        WINEVENT_OUTOFCONTEXT, WM_QUIT, WM_TIMER,
    };
    use windows::core::PWSTR;

    type Sink = Box<dyn Fn(Observation) + Send>;

    struct Thread {
        tracker: Tracker,
        sink: Sink,
        threshold_ms: Arc<AtomicI64>,
    }
    thread_local! {
        // WinEvent callbacks carry no context; out-of-context hooks are called
        // on the thread that set them, so its state lives here.
        static STATE: RefCell<Option<Thread>> = const { RefCell::new(None) };
    }

    /// Owns the hook thread; `stop` (or drop) unhooks and joins it.
    pub struct CollectorHandle {
        thread_id: u32,
        join: Option<std::thread::JoinHandle<()>>,
    }
    impl CollectorHandle {
        pub fn stop(mut self) {
            self.shutdown();
        }
        fn shutdown(&mut self) {
            if let Some(join) = self.join.take() {
                // SAFETY: posts a message with no pointers to the hook thread's
                // queue, which exists before its id is published.
                let _ =
                    unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0)) };
                let _ = join.join();
            }
        }
    }
    impl Drop for CollectorHandle {
        fn drop(&mut self) {
            self.shutdown();
        }
    }

    fn now_ms() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    }

    /// Milliseconds since the last user input (low 32-bit tick domain).
    fn since_input_ms() -> i64 {
        let mut info = LASTINPUTINFO {
            cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
            dwTime: 0,
        };
        // SAFETY: `info` is a live, correctly sized LASTINPUTINFO.
        if !unsafe { GetLastInputInfo(&mut info) }.as_bool() {
            return 0;
        }
        // SAFETY: no arguments.
        duration_from_input_ticks(unsafe { GetTickCount64() }, info.dwTime)
    }

    fn app_name(hwnd: HWND) -> String {
        let mut pid = 0u32;
        // SAFETY: reads the window's owner into a live u32; a stale handle returns 0.
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        if pid == 0 {
            return "unknown".into();
        }
        // SAFETY: no pointers; the returned handle is closed below.
        let Ok(process) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) })
        else {
            return "unknown".into();
        };
        let mut buffer = [0u16; 1024];
        let mut len = buffer.len() as u32;
        // SAFETY: `buffer` holds `len` u16s and outlives the call; `process` is open.
        let named = unsafe {
            QueryFullProcessImageNameW(
                process,
                PROCESS_NAME_WIN32,
                PWSTR(buffer.as_mut_ptr()),
                &mut len,
            )
        };
        // SAFETY: this handle was opened above and is closed once.
        let _ = unsafe { CloseHandle(process) };
        if named.is_err() {
            return "unknown".into();
        }
        let path = String::from_utf16_lossy(&buffer[..len as usize]);
        match path.rsplit(['\\', '/']).next() {
            Some(name) if !name.is_empty() => name.to_string(),
            _ => "unknown".into(),
        }
    }

    /// The stored caption. Unlike `GetWindowTextW` this never sends
    /// `WM_GETTEXT`, so the hook thread cannot wait on devbox's own UI thread
    /// while that thread joins it (`stop`).
    fn title(hwnd: HWND) -> Option<String> {
        let mut buffer = [0u16; 4097];
        // SAFETY: the slice is live and its length bounds the copy; a stale handle yields 0.
        let copied = unsafe { InternalGetWindowText(hwnd, &mut buffer) }.max(0) as usize;
        let title = String::from_utf16_lossy(&buffer[..copied.min(buffer.len() - 1)])
            .trim()
            .to_string();
        (!title.is_empty()).then_some(title)
    }

    fn foreground() -> Option<(HWND, Window)> {
        // SAFETY: no arguments; may return a null handle.
        let hwnd = unsafe { GetForegroundWindow() };
        (!hwnd.is_invalid()).then(|| (hwnd, (app_name(hwnd), title(hwnd))))
    }

    fn with_state(f: impl FnOnce(&mut Thread)) {
        STATE.with(|state| {
            if let Ok(mut state) = state.try_borrow_mut()
                && let Some(state) = state.as_mut()
            {
                f(state);
            }
        });
    }

    fn report_foreground() {
        let Some((_, window)) = foreground() else {
            return;
        };
        let at = now_ms();
        let last_input = at.saturating_sub(since_input_ms());
        with_state(|s| {
            if let Some(o) = s.tracker.foreground(window, at, last_input) {
                (s.sink)(o);
            }
        });
    }

    unsafe extern "system" fn on_event(
        _hook: HWINEVENTHOOK,
        event: u32,
        hwnd: HWND,
        object: i32,
        child: i32,
        _thread: u32,
        _time: u32,
    ) {
        if event == EVENT_OBJECT_NAMECHANGE {
            // Only the foreground window's own title, not its controls or other windows.
            // SAFETY: no arguments.
            let foreground = unsafe { GetForegroundWindow() };
            if hwnd != foreground || object != OBJID_WINDOW.0 || child != CHILDID_SELF as i32 {
                return;
            }
        }
        report_foreground();
    }

    fn tick() {
        let at = now_ms();
        let since = since_input_ms();
        with_state(|s| {
            let threshold = s.threshold_ms.load(Ordering::Relaxed);
            // SAFETY: no arguments; counts time spent asleep, unlike `Instant`.
            let monotonic = unsafe { GetTickCount64() } as i64;
            if let Some(o) = s.tracker.tick(at, monotonic, since, threshold, || {
                foreground().map(|(_, w)| w)
            }) {
                (s.sink)(o);
            }
        });
    }

    fn run(
        sink: Sink,
        threshold_ms: Arc<AtomicI64>,
        ready: std::sync::mpsc::Sender<std::io::Result<u32>>,
    ) {
        let mut msg = MSG::default();
        // SAFETY: `msg` is live; this creates the thread's queue so the id
        // published below can receive WM_QUIT immediately.
        let _ = unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE) };
        STATE.with(|state| {
            *state.borrow_mut() = Some(Thread {
                tracker: Tracker::default(),
                sink,
                threshold_ms,
            })
        });
        // SAFETY: `on_event` is a 'static callback; out-of-context hooks are
        // delivered to this thread while it pumps messages below.
        let hooks = unsafe {
            [
                SetWinEventHook(
                    EVENT_SYSTEM_FOREGROUND,
                    EVENT_SYSTEM_FOREGROUND,
                    None,
                    Some(on_event),
                    0,
                    0,
                    WINEVENT_OUTOFCONTEXT,
                ),
                SetWinEventHook(
                    EVENT_OBJECT_NAMECHANGE,
                    EVENT_OBJECT_NAMECHANGE,
                    None,
                    Some(on_event),
                    0,
                    0,
                    WINEVENT_OUTOFCONTEXT,
                ),
            ]
        };
        // SAFETY: a thread timer without a window or callback posts WM_TIMER here.
        let timer = unsafe { SetTimer(None, 0, TICK_MS, None) };
        if hooks.iter().any(|h| h.is_invalid()) || timer == 0 {
            let _ = ready.send(Err(std::io::Error::other("activity hook setup failed")));
        } else {
            // SAFETY: no arguments.
            let _ = ready.send(Ok(unsafe { GetCurrentThreadId() }));
            report_foreground();
            // SAFETY: `msg` is live for each call; 0 is WM_QUIT and -1 an error.
            while unsafe { GetMessageW(&mut msg, None, 0, 0) }.0 > 0 {
                if msg.message == WM_TIMER && msg.hwnd.is_invalid() {
                    tick();
                } else {
                    // SAFETY: dispatches a message this thread just received.
                    unsafe { DispatchMessageW(&msg) };
                }
            }
        }
        // SAFETY: only handles created above on this thread are released.
        unsafe {
            if timer != 0 {
                let _ = KillTimer(None, timer);
            }
            for hook in hooks.into_iter().filter(|h| !h.is_invalid()) {
                let _ = UnhookWinEvent(hook);
            }
        }
        STATE.with(|state| state.borrow_mut().take());
    }

    /// Starts the hook thread. It reports the current window at once, then
    /// changes, idle and heartbeats. `threshold_ms` is read at every tick.
    pub fn start(
        threshold_ms: Arc<AtomicI64>,
        sink: impl Fn(Observation) + Send + 'static,
    ) -> std::io::Result<CollectorHandle> {
        let (ready, started) = std::sync::mpsc::channel();
        let sink: Sink = Box::new(sink);
        let join = std::thread::Builder::new()
            .name("devbox-activity".into())
            .spawn(move || run(sink, threshold_ms, ready))?;
        match started.recv() {
            Ok(Ok(thread_id)) => Ok(CollectorHandle {
                thread_id,
                join: Some(join),
            }),
            Ok(Err(error)) => {
                let _ = join.join();
                Err(error)
            }
            Err(_) => {
                let _ = join.join();
                Err(std::io::Error::other("activity hook thread ended"))
            }
        }
    }
}
