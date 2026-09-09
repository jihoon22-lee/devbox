//! Reviewed main-window changes use the same writer and normal-bounds memory
//! as ordinary resize events. Native callers serialize review/apply on the UI
//! thread; geometry and persistent history are separate operations.
use super::*;

#[derive(Clone, Debug)]
pub struct MainWindowReview {
    pub before: WindowState,
    pub after: WindowState,
    monitors: Vec<MonitorInfo>,
}
impl MainWindowReview {
    /// Portable review construction for native adapters and synthetic fixtures.
    /// This does not mutate a window or save a state file.
    pub fn new(
        saved: &WindowState,
        current: WindowState,
        monitors: Vec<MonitorInfo>,
        config: RestoreConfig,
    ) -> Result<Self, &'static str> {
        saved.validate().map_err(|_| "window_state_invalid")?;
        current.validate().map_err(|_| "window_state_unavailable")?;
        if !monitors.iter().any(|monitor| {
            monitor.id == current.monitor_id
                && monitor.work_area == current.monitor_work_area
                && monitor.scale_factor == current.scale_factor
        }) {
            return Err("window_monitor_unavailable");
        }
        let restored = restore_window(Some(saved), &monitors, config).state;
        let monitor = monitors
            .iter()
            .find(|monitor| Some(&monitor.id) == restored.monitor_id.as_ref())
            .ok_or("window_monitor_unavailable")?;
        let after = WindowState::capture(restored.bounds, monitor, restored.maximized)
            .map_err(|_| "window_state_invalid")?;
        Ok(Self {
            before: current,
            after,
            monitors,
        })
    }
    pub fn revalidate(
        &self,
        current: &WindowState,
        monitors: &[MonitorInfo],
    ) -> Result<(), &'static str> {
        if current != &self.before || monitors != self.monitors {
            return Err("window_review_stale");
        }
        Ok(())
    }
}

/// Invoke on the main thread so move/DPI events cannot interleave the samples.
pub fn review_main_window<R: Runtime>(
    window: &WebviewWindow<R>,
    saved: &WindowState,
) -> Result<MainWindowReview, &'static str> {
    if window.label() != MAIN_WINDOW_LABEL {
        return Err("window_state_unavailable");
    }
    let path = state_path(Manager::app_handle(window)).ok_or("window_state_unavailable")?;
    let writer = writer_for(&path);
    let current = capture_state(window, &writer, None, None).ok_or("window_state_unavailable")?;
    MainWindowReview::new(
        saved,
        current,
        monitor_infos(window),
        restore_config(window),
    )
}

/// Apply only after a native owner preserves the before state and consumes its
/// expiring review token. A native failure can leave partial geometry; callers
/// retain the saved before state for a separate, explicit restoration review.
/// `check` supplies cancellation/deadline checks before each native mutation.
pub fn apply_main_window_review<R: Runtime>(
    window: &WebviewWindow<R>,
    review: &MainWindowReview,
    check: impl Fn() -> Result<(), &'static str>,
) -> Result<(), &'static str> {
    check()?;
    if window.label() != MAIN_WINDOW_LABEL {
        return Err("window_state_unavailable");
    }
    let path = state_path(Manager::app_handle(window)).ok_or("window_state_unavailable")?;
    let writer = writer_for(&path);
    let current = capture_state(window, &writer, None, None).ok_or("window_state_unavailable")?;
    review.revalidate(&current, &monitor_infos(window))?;
    let after = &review.after;
    let bounds = after.bounds;
    check()?;
    window.unmaximize().map_err(|_| "window_apply_failed")?;
    check()?;
    window
        .set_size(PhysicalSize::new(bounds.width, bounds.height))
        .map_err(|_| "window_apply_failed")?;
    check()?;
    window
        .set_position(PhysicalPosition::new(bounds.x, bounds.y))
        .map_err(|_| "window_apply_failed")?;
    let mut normal = after.clone();
    normal.maximized = false;
    writer.remember_normal(normal);
    if after.maximized {
        check()?;
        window.maximize().map_err(|_| "window_apply_failed")?;
    }
    // Schedule through the existing coalescing writer. Subsequent native events
    // may refine monitor/position facts; never introduce a competing file writer.
    writer.schedule(after.to_bytes().map_err(|_| "window_state_invalid")?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn monitor() -> MonitorInfo {
        MonitorInfo::new(
            MonitorId::new("current").unwrap(),
            WindowBounds::new(0, 0, 1920, 1040),
            1.0,
            true,
        )
        .unwrap()
    }
    #[test]
    fn review_maps_missing_monitor_and_rejects_changed_geometry_or_dpi() {
        let monitor = monitor();
        let current =
            WindowState::capture(WindowBounds::new(20, 30, 1000, 700), &monitor, false).unwrap();
        let saved = WindowState::new(
            MonitorId::new("old").unwrap(),
            WindowBounds::new(9000, 9000, 1000, 700),
            WindowBounds::new(8000, 8000, 1920, 1040),
            1.5,
            true,
        )
        .unwrap();
        let review = MainWindowReview::new(
            &saved,
            current.clone(),
            vec![monitor.clone()],
            RestoreConfig::default(),
        )
        .unwrap();
        assert_eq!(review.after.monitor_id, monitor.id);
        assert!(review.after.maximized);
        assert!(review.after.bounds.x < 1920 && review.after.bounds.y < 1040);
        assert!(review
            .revalidate(&current, std::slice::from_ref(&monitor))
            .is_ok());
        let mut moved = current.clone();
        moved.bounds.x += 1;
        assert_eq!(
            review.revalidate(&moved, std::slice::from_ref(&monitor)),
            Err("window_review_stale")
        );
        let mut changed = monitor;
        changed.scale_factor = 2.0;
        assert_eq!(
            review.revalidate(&current, &[changed]),
            Err("window_review_stale")
        );
        assert!(MainWindowReview::new(&saved, current, vec![], RestoreConfig::default()).is_err());
    }
}
