//! Tauri/native adapter for the portable [`window_state`] contract.
//!
//! The state crate intentionally has no Tauri or filesystem dependency.  This
//! small bridge owns the platform monitor snapshot and the app-local atomic
//! file, so all ordinary app windows use the same bounded restore behavior.
//! Only the `main` window is handled; dialogs, launchers, and other transient
//! windows are never persisted.

use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use filesystem::atomic_write;
use tauri::{
    AppHandle, Manager, Monitor, PhysicalPosition, PhysicalSize, Runtime, WebviewWindow, Window,
    WindowEvent,
};
use window_state::{
    decode_state, restore_from_bytes, restore_window, MonitorId, MonitorInfo, RestoreConfig,
    WindowBounds, WindowSize, WindowState, DEFAULT_WINDOW_HEIGHT, DEFAULT_WINDOW_WIDTH,
    MAX_SCALE_FACTOR, MAX_STATE_BYTES, MIN_SCALE_FACTOR,
};

/// Stable file name inside each app's Tauri `app_local_data_dir`.
pub const STATE_FILE_NAME: &str = "window-state-v1.json";
const MAIN_WINDOW_LABEL: &str = "main";
const WRITE_DEBOUNCE: Duration = Duration::from_millis(150);

/// A single latest-write worker is shared by each app-local state path.  Move
/// and resize notifications can arrive once per frame; keeping one bounded
/// pending document prevents synchronous fsyncs from blocking the Tauri event
/// loop while still allowing close/exit paths to flush synchronously.
enum WriteCommand {
    Wake,
    Flush(Sender<()>),
}

struct WriterMemory {
    /// The last known normal (not maximized) geometry.  A maximized window's
    /// native rectangle is the monitor work area, not the user's normal size,
    /// so it must not overwrite this snapshot.
    last_normal: Option<WindowState>,
}

struct StateWriter {
    sender: SyncSender<WriteCommand>,
    pending: Arc<Mutex<Option<Vec<u8>>>>,
    memory: Mutex<WriterMemory>,
}

impl StateWriter {
    fn new(path: PathBuf) -> Self {
        let (sender, receiver) = mpsc::sync_channel(1);
        let pending = Arc::new(Mutex::new(None));
        let pending_for_worker = Arc::clone(&pending);
        thread::spawn(move || writer_loop(path, receiver, pending_for_worker));
        Self {
            sender,
            pending,
            memory: Mutex::new(WriterMemory { last_normal: None }),
        }
    }

    fn remember_normal(&self, state: WindowState) {
        let mut memory = self
            .memory
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        memory.last_normal = Some(state);
    }

    fn last_normal(&self) -> Option<WindowState> {
        self.memory
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .last_normal
            .clone()
    }

    fn schedule(&self, bytes: Vec<u8>) {
        let mut pending = self
            .pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *pending = Some(bytes);
        drop(pending);
        // The channel carries only a wake-up signal.  The actual document is
        // kept in one bounded latest-value slot, so a resize storm can never
        // grow an unbounded queue or block the Tauri event loop.
        let _ = self.sender.try_send(WriteCommand::Wake);
    }

    fn flush(&self) {
        let (done_sender, done_receiver) = mpsc::channel();
        if self.sender.send(WriteCommand::Flush(done_sender)).is_ok() {
            // The worker only writes a bounded document with the existing
            // atomic helper.  Waiting here is intentional for close/exit: a
            // tray hide may return immediately, but an actual exit must not
            // race the final preference write.
            let _ = done_receiver.recv();
        }
    }
}

static WRITERS: OnceLock<Mutex<HashMap<PathBuf, Arc<StateWriter>>>> = OnceLock::new();

fn writer_for(path: &Path) -> Arc<StateWriter> {
    let registry = WRITERS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut registry = registry
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    registry
        .entry(path.to_path_buf())
        .or_insert_with(|| Arc::new(StateWriter::new(path.to_path_buf())))
        .clone()
}

fn writer_loop(
    path: PathBuf,
    receiver: Receiver<WriteCommand>,
    shared_pending: Arc<Mutex<Option<Vec<u8>>>>,
) {
    let mut pending: Option<Vec<u8>> = None;
    let mut deadline: Option<Instant> = None;

    loop {
        let command = match deadline {
            Some(wait_until) => {
                match receiver.recv_timeout(wait_until.saturating_duration_since(Instant::now())) {
                    Ok(command) => command,
                    Err(RecvTimeoutError::Timeout) => {
                        write_pending(&path, &mut pending);
                        let has_fresh_document = take_shared_pending(&shared_pending, &mut pending);
                        if has_fresh_document && pending.is_some() {
                            deadline = Some(Instant::now() + WRITE_DEBOUNCE);
                            continue;
                        }
                        // A failed write remains pending, but it is retried
                        // only after the next geometry event or explicit
                        // flush. Retrying every debounce interval would turn
                        // a persistent permission error into background disk
                        // and CPU churn for the rest of the app lifetime.
                        deadline = None;
                        continue;
                    }
                    Err(RecvTimeoutError::Disconnected) => {
                        take_shared_pending(&shared_pending, &mut pending);
                        write_pending(&path, &mut pending);
                        return;
                    }
                }
            }
            None => match receiver.recv() {
                Ok(command) => command,
                Err(_) => {
                    take_shared_pending(&shared_pending, &mut pending);
                    write_pending(&path, &mut pending);
                    return;
                }
            },
        };

        match command {
            WriteCommand::Wake => {
                take_shared_pending(&shared_pending, &mut pending);
                if pending.is_some() {
                    deadline = Some(Instant::now() + WRITE_DEBOUNCE);
                }
            }
            WriteCommand::Flush(done) => {
                take_shared_pending(&shared_pending, &mut pending);
                write_pending(&path, &mut pending);
                deadline = None;
                let _ = done.send(());
            }
        }
    }
}

fn take_shared_pending(
    shared_pending: &Mutex<Option<Vec<u8>>>,
    pending: &mut Option<Vec<u8>>,
) -> bool {
    let mut shared_pending = shared_pending
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(bytes) = shared_pending.take() {
        *pending = Some(bytes);
        true
    } else {
        false
    }
}

fn write_pending(path: &Path, pending: &mut Option<Vec<u8>>) {
    let Some(bytes) = pending.take() else {
        return;
    };
    let Some(parent) = path.parent() else {
        *pending = Some(bytes);
        return;
    };
    if std::fs::create_dir_all(parent).is_err() || atomic_write(path, &bytes).is_err() {
        // Keep the latest bounded document available for the next wake/flush
        // instead of silently losing the user's last geometry on a transient
        // permission, lock, or filesystem failure.  The writer loop's
        // debounce prevents this from becoming a hot retry loop.
        *pending = Some(bytes);
    }
}

/// Restore the ordinary main window.  Missing, corrupt, oversized, or
/// unsupported state is deliberately non-fatal: Tauri keeps the configured
/// initial geometry and the pure contract supplies a safe fallback.
pub fn restore_main_window<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) else {
        return;
    };

    restore_window_state(&window, app);
}

fn restore_window_state<R: Runtime, W: NativeWindow<R>>(window: &W, app: &AppHandle<R>) {
    let monitors = monitor_infos(window);
    let config = restore_config(window);
    let path = state_path(app);
    let bytes = path.as_deref().and_then(read_bounded);
    let result = restore_from_bytes(bytes.as_deref(), &monitors, config);

    // Remember a safe normal rectangle before applying native geometry.  This
    // lets a later maximize event persist `maximized: true` without replacing
    // the user's normal bounds with the maximized monitor rectangle.
    if let Some(path) = path {
        let writer = writer_for(&path);
        if let Some(monitor) = result
            .state
            .monitor_id
            .as_ref()
            .and_then(|id| monitors.iter().find(|monitor| &monitor.id == id))
        {
            if let Ok(normal) = WindowState::capture(result.state.bounds, monitor, false) {
                writer.remember_normal(normal);
            }
        } else if let Some(saved) = bytes.as_deref().and_then(|bytes| decode_state(bytes).ok()) {
            // If monitor enumeration is temporarily unavailable, retain the
            // validated persisted geometry for a future maximize/unmaximize
            // event instead of losing it.
            let mut normal = saved;
            normal.maximized = false;
            writer.remember_normal(normal);
        }
    }

    // Always unmaximize before setting the normal geometry.  Tauri/tao's
    // set_size also clears maximized state, but doing it explicitly makes the
    // order deterministic across runtimes.  Position is skipped when monitor
    // enumeration failed so a transient native failure cannot move the window
    // to an arbitrary (possibly invisible) desktop origin.
    let bounds = result.state.bounds;
    let _ = window.unmaximize();
    let _ = window.set_size(PhysicalSize::new(bounds.width, bounds.height));
    if result.state.monitor_id.is_some() {
        let _ = window.set_position(PhysicalPosition::new(bounds.x, bounds.y));
    }
    if result.state.maximized {
        let _ = window.maximize();
    }
}

/// Persist the current main-window geometry immediately.  This is used by
/// close-to-tray and orderly-exit paths where the caller needs a completed
/// write before hiding or terminating the application.
pub fn save_main_window<R: Runtime>(window: &Window<R>) {
    save_window_state(window, true, None, None);
}

/// Persist a webview-backed main window immediately from an explicit app-exit
/// path (for example, a tray Quit action). Tauri's global `on_window_event`
/// callback receives `Window`, so this companion keeps programmatic exits
/// covered too.
pub fn save_main_webview_window<R: Runtime>(window: &WebviewWindow<R>) {
    save_window_state(window, true, None, None);
}

fn save_window_state<R: Runtime, W: NativeWindow<R>>(
    window: &W,
    flush: bool,
    size_override: Option<PhysicalSize<u32>>,
    scale_override: Option<f64>,
) {
    if window.label() != MAIN_WINDOW_LABEL {
        return;
    }

    let Some(path) = state_path(window.app_handle()) else {
        return;
    };
    let writer = writer_for(&path);
    let Some(state) = capture_state(window, &writer, size_override, scale_override) else {
        return;
    };
    let Ok(bytes) = state.to_bytes() else {
        return;
    };
    writer.schedule(bytes);
    if flush {
        writer.flush();
    }
}

/// Attach this to a builder's global window-event callback. The label check
/// makes the callback safe even when an app creates a transient dialog.
pub fn handle_window_event<R: Runtime>(window: &Window<R>, event: &WindowEvent) {
    if window.label() != MAIN_WINDOW_LABEL {
        return;
    }
    match event {
        WindowEvent::Moved(_) | WindowEvent::Resized(_) => {
            save_window_state(window, false, None, None);
        }
        WindowEvent::ScaleFactorChanged {
            scale_factor,
            new_inner_size,
            ..
        } => {
            // The event carries the post-DPI physical client size.  Using it
            // avoids sampling the old size before tao applies the change;
            // any following Resized event is coalesced into the same write.
            save_window_state(window, false, Some(*new_inner_size), Some(*scale_factor));
        }
        WindowEvent::CloseRequested { .. } => {
            // Close-to-tray handlers run after this callback.  Flush first so
            // their hide/prevent-close path cannot discard the latest bounds.
            save_main_window(window);
        }
        _ => {}
    }
}

fn capture_state<R: Runtime, W: NativeWindow<R>>(
    window: &W,
    writer: &StateWriter,
    size_override: Option<PhysicalSize<u32>>,
    scale_override: Option<f64>,
) -> Option<WindowState> {
    let position = window.outer_position().ok()?;
    let size = size_override.or_else(|| window.inner_size().ok())?;
    let maximized = window.is_maximized().ok()?;
    let monitor = window.current_monitor().ok().flatten()?;
    let monitor = monitor_info_with_scale(&monitor, scale_override)?;

    let state = if maximized {
        let fallback = WindowBounds::new(position.x, position.y, size.width, size.height);
        maximized_state(writer.last_normal().as_ref(), &monitor, fallback)?
    } else {
        WindowState::capture(
            WindowBounds::new(position.x, position.y, size.width, size.height),
            &monitor,
            false,
        )
        .ok()?
    };

    if !state.maximized {
        writer.remember_normal(state.clone());
    }
    Some(state)
}

fn maximized_state(
    normal: Option<&WindowState>,
    monitor: &MonitorInfo,
    fallback: WindowBounds,
) -> Option<WindowState> {
    // A maximized window reports the monitor work area, not the user's normal
    // rectangle.  If the window was maximized and then moved to a monitor
    // with another DPI, carry that normal rectangle through the same
    // monitor/scale transform used during startup restoration.  Otherwise the
    // next unmaximize would retain stale physical-pixel dimensions.
    let bounds = normal
        .map(|state| {
            restore_window(
                Some(state),
                std::slice::from_ref(monitor),
                RestoreConfig::default(),
            )
            .state
            .bounds
        })
        .unwrap_or(fallback);
    WindowState::new(
        monitor.id.clone(),
        bounds,
        monitor.work_area,
        monitor.scale_factor,
        true,
    )
    .ok()
}

fn restore_config<R: Runtime, W: NativeWindow<R>>(window: &W) -> RestoreConfig {
    let default_size = window
        .inner_size()
        .ok()
        .map(|size| WindowSize::new(size.width, size.height))
        .unwrap_or_else(|| WindowSize::new(DEFAULT_WINDOW_WIDTH, DEFAULT_WINDOW_HEIGHT));
    RestoreConfig {
        default_size,
        ..RestoreConfig::default()
    }
}

fn state_path<R: Runtime>(app: &AppHandle<R>) -> Option<PathBuf> {
    app.path()
        .app_local_data_dir()
        .ok()
        .map(|directory| directory.join(STATE_FILE_NAME))
}

/// Read at most `MAX_STATE_BYTES + 1` bytes.  The extra byte lets the pure
/// decoder classify an oversized document as corruption without allocating
/// according to an attacker-controlled file length.
fn read_bounded(path: &Path) -> Option<Vec<u8>> {
    let file = File::open(path).ok()?;
    let length = file.metadata().ok()?.len();
    let limit = (MAX_STATE_BYTES as u64).saturating_add(1);
    if length > limit {
        return Some(vec![0; limit as usize]);
    }
    let mut bytes = Vec::with_capacity(length as usize);
    // A concurrent replacement/growth is still bounded by `take`.
    file.take(limit).read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

fn monitor_infos<R: Runtime, W: NativeWindow<R>>(window: &W) -> Vec<MonitorInfo> {
    let primary_id = window
        .primary_monitor()
        .ok()
        .flatten()
        .and_then(|monitor| monitor_id(&monitor));
    window
        .available_monitors()
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|monitor| {
            let id = monitor_id(&monitor)?;
            let primary = primary_id.as_ref().is_some_and(|primary| primary == &id);
            monitor_info_with_id(&monitor, id, primary)
        })
        .collect()
}

trait NativeWindow<R: Runtime> {
    fn label(&self) -> &str;
    fn app_handle(&self) -> &AppHandle<R>;
    fn outer_position(&self) -> tauri::Result<PhysicalPosition<i32>>;
    fn inner_size(&self) -> tauri::Result<PhysicalSize<u32>>;
    fn is_maximized(&self) -> tauri::Result<bool>;
    fn current_monitor(&self) -> tauri::Result<Option<Monitor>>;
    fn primary_monitor(&self) -> tauri::Result<Option<Monitor>>;
    fn available_monitors(&self) -> tauri::Result<Vec<Monitor>>;
    fn set_size(&self, size: PhysicalSize<u32>) -> tauri::Result<()>;
    fn set_position(&self, position: PhysicalPosition<i32>) -> tauri::Result<()>;
    fn unmaximize(&self) -> tauri::Result<()>;
    fn maximize(&self) -> tauri::Result<()>;
}

impl<R: Runtime> NativeWindow<R> for Window<R> {
    fn label(&self) -> &str {
        self.label()
    }

    fn app_handle(&self) -> &AppHandle<R> {
        Manager::app_handle(self)
    }

    fn outer_position(&self) -> tauri::Result<PhysicalPosition<i32>> {
        self.outer_position()
    }

    fn inner_size(&self) -> tauri::Result<PhysicalSize<u32>> {
        self.inner_size()
    }

    fn is_maximized(&self) -> tauri::Result<bool> {
        self.is_maximized()
    }

    fn current_monitor(&self) -> tauri::Result<Option<Monitor>> {
        self.current_monitor()
    }

    fn primary_monitor(&self) -> tauri::Result<Option<Monitor>> {
        self.primary_monitor()
    }

    fn available_monitors(&self) -> tauri::Result<Vec<Monitor>> {
        self.available_monitors()
    }

    fn set_size(&self, size: PhysicalSize<u32>) -> tauri::Result<()> {
        self.set_size(size)
    }

    fn set_position(&self, position: PhysicalPosition<i32>) -> tauri::Result<()> {
        self.set_position(position)
    }

    fn unmaximize(&self) -> tauri::Result<()> {
        self.unmaximize()
    }

    fn maximize(&self) -> tauri::Result<()> {
        self.maximize()
    }
}

impl<R: Runtime> NativeWindow<R> for WebviewWindow<R> {
    fn label(&self) -> &str {
        self.label()
    }

    fn app_handle(&self) -> &AppHandle<R> {
        Manager::app_handle(self)
    }

    fn outer_position(&self) -> tauri::Result<PhysicalPosition<i32>> {
        self.outer_position()
    }

    fn inner_size(&self) -> tauri::Result<PhysicalSize<u32>> {
        self.inner_size()
    }

    fn is_maximized(&self) -> tauri::Result<bool> {
        self.is_maximized()
    }

    fn current_monitor(&self) -> tauri::Result<Option<Monitor>> {
        self.current_monitor()
    }

    fn primary_monitor(&self) -> tauri::Result<Option<Monitor>> {
        self.primary_monitor()
    }

    fn available_monitors(&self) -> tauri::Result<Vec<Monitor>> {
        self.available_monitors()
    }

    fn set_size(&self, size: PhysicalSize<u32>) -> tauri::Result<()> {
        self.set_size(size)
    }

    fn set_position(&self, position: PhysicalPosition<i32>) -> tauri::Result<()> {
        self.set_position(position)
    }

    fn unmaximize(&self) -> tauri::Result<()> {
        self.unmaximize()
    }

    fn maximize(&self) -> tauri::Result<()> {
        self.maximize()
    }
}

fn monitor_info_with_scale(monitor: &Monitor, scale_override: Option<f64>) -> Option<MonitorInfo> {
    let id = monitor_id(monitor)?;
    let work_area = monitor.work_area();
    let scale_factor = scale_override
        .filter(|scale| scale.is_finite() && (MIN_SCALE_FACTOR..=MAX_SCALE_FACTOR).contains(scale))
        .unwrap_or_else(|| monitor.scale_factor());
    MonitorInfo::new(
        id,
        WindowBounds::new(
            work_area.position.x,
            work_area.position.y,
            work_area.size.width,
            work_area.size.height,
        ),
        scale_factor,
        false,
    )
    .ok()
}

fn monitor_info_with_id(monitor: &Monitor, id: MonitorId, primary: bool) -> Option<MonitorInfo> {
    let work_area = monitor.work_area();
    MonitorInfo::new(
        id,
        WindowBounds::new(
            work_area.position.x,
            work_area.position.y,
            work_area.size.width,
            work_area.size.height,
        ),
        monitor.scale_factor(),
        primary,
    )
    .ok()
}

fn monitor_id(monitor: &Monitor) -> Option<MonitorId> {
    if let Some(name) = monitor.name() {
        if let Ok(id) = MonitorId::new(name.clone()) {
            return Some(id);
        }
    }
    let position = monitor.position();
    let size = monitor.size();
    MonitorId::new(format!(
        "geometry:{}:{}:{}:{}",
        position.x, position.y, size.width, size.height
    ))
    .ok()
}

#[cfg(test)]
mod tests {
    use super::{maximized_state, read_bounded, write_pending, writer_loop, WriteCommand};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::thread;
    use window_state::{MonitorId, MonitorInfo, WindowBounds, WindowState, MAX_STATE_BYTES};

    static NEXT_TEST_FILE: AtomicUsize = AtomicUsize::new(0);

    fn test_path() -> std::path::PathBuf {
        let id = NEXT_TEST_FILE.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "devbox-window-state-tauri-{}-{id}.json",
            std::process::id()
        ))
    }

    fn monitor() -> MonitorInfo {
        MonitorInfo::new(
            MonitorId::new("DISPLAY1").unwrap(),
            WindowBounds::new(0, 0, 1_920, 1_040),
            1.0,
            true,
        )
        .unwrap()
    }

    #[test]
    fn bounded_reader_caps_oversized_documents_without_allocating_the_file_size() {
        let path = test_path();
        std::fs::write(&path, vec![b'x'; MAX_STATE_BYTES + 1]).unwrap();

        let bytes = read_bounded(&path).expect("test state should be readable");

        assert_eq!(bytes.len(), MAX_STATE_BYTES + 1);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn bounded_reader_preserves_small_documents() {
        let path = test_path();
        let expected = br#"{"schemaVersion":1}"#;
        std::fs::write(&path, expected).unwrap();

        let bytes = read_bounded(&path).expect("test state should be readable");

        assert_eq!(bytes, expected);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn failed_atomic_write_keeps_the_latest_bounded_document_for_retry() {
        let parent = test_path();
        std::fs::write(&parent, b"not a directory").unwrap();
        let target = parent.join("window-state-v1.json");
        let expected = b"latest".to_vec();
        let mut pending = Some(expected.clone());

        write_pending(&target, &mut pending);

        assert_eq!(pending, Some(expected));
        let _ = std::fs::remove_file(parent);
    }

    #[test]
    fn writer_coalesces_pending_events_and_flushes_the_latest_document() {
        let path = test_path();
        let (sender, receiver) = mpsc::sync_channel(1);
        let shared_pending = std::sync::Arc::new(std::sync::Mutex::new(None));
        let shared_pending_for_worker = std::sync::Arc::clone(&shared_pending);
        let worker_path = path.clone();
        thread::spawn(move || writer_loop(worker_path, receiver, shared_pending_for_worker));

        *shared_pending.lock().unwrap() = Some(b"old".to_vec());
        let _ = sender.try_send(WriteCommand::Wake);
        *shared_pending.lock().unwrap() = Some(b"latest".to_vec());
        let _ = sender.try_send(WriteCommand::Wake);
        let (done_sender, done_receiver) = mpsc::channel();
        sender.send(WriteCommand::Flush(done_sender)).unwrap();
        done_receiver.recv().unwrap();

        assert_eq!(std::fs::read(&path).unwrap(), b"latest");
        drop(sender);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn maximized_state_keeps_the_last_normal_bounds() {
        let normal = WindowState::new(
            MonitorId::new("DISPLAY1").unwrap(),
            WindowBounds::new(100, 140, 1_200, 800),
            WindowBounds::new(0, 0, 1_920, 1_040),
            1.0,
            false,
        )
        .unwrap();

        let maximized =
            maximized_state(Some(&normal), &monitor(), WindowBounds::new(0, 0, 1, 1)).unwrap();

        assert!(maximized.maximized);
        assert_eq!(maximized.bounds, normal.bounds);
        assert_eq!(maximized.monitor_id, normal.monitor_id);
    }

    #[test]
    fn maximized_state_transforms_normal_bounds_after_monitor_dpi_change() {
        let normal = WindowState::new(
            MonitorId::new("DISPLAY1").unwrap(),
            WindowBounds::new(100, 140, 1_200, 800),
            WindowBounds::new(0, 0, 1_920, 1_040),
            1.0,
            false,
        )
        .unwrap();
        let current = MonitorInfo::new(
            MonitorId::new("DISPLAY2").unwrap(),
            WindowBounds::new(0, 0, 1_280, 720),
            2.0,
            false,
        )
        .unwrap();

        let maximized =
            maximized_state(Some(&normal), &current, WindowBounds::new(0, 0, 1, 1)).unwrap();

        assert!(maximized.maximized);
        assert_eq!(maximized.monitor_id, current.id);
        assert_eq!(maximized.scale_factor, 2.0);
        assert_eq!(maximized.bounds, WindowBounds::new(200, 280, 2_400, 1_600));
    }
}
