use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::{plugin::TauriPlugin, AppHandle, Manager, Runtime, State, WebviewWindow};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

const MAIN_WINDOW_LABEL: &str = "main";
const TRAY_ID: &str = "quick-summon-tray";

/// Renderer input stays on a fixed list so a compromised webview cannot register
/// arbitrary system-wide key combinations.
const ALLOWED_SHORTCUTS: &[&str] = &[
    "Ctrl+Alt+Space",
    "Ctrl+Shift+Space",
    "Alt+Shift+Space",
    "Ctrl+Alt+F12",
];

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuickSummonConfig {
    shortcut_enabled: bool,
    shortcut: String,
    keep_in_tray: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum QuickSummonIssue {
    InvalidShortcut,
    ShortcutBackendUnavailable,
    ShortcutUnavailable,
    ShortcutRollbackFailed,
    TrayUnavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CloseBehavior {
    Exit,
    HideToTray,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickSummonStatus {
    shortcut_registered: bool,
    active_shortcut: Option<String>,
    tray_enabled: bool,
    close_behavior: CloseBehavior,
    issues: Vec<QuickSummonIssue>,
}

#[derive(Clone)]
struct ActiveShortcut {
    label: String,
    shortcut: Shortcut,
}

#[derive(Default)]
struct RuntimeConfig {
    active_shortcut: Option<ActiveShortcut>,
    shortcut_backend_available: bool,
    tray_enabled: bool,
    issues: Vec<QuickSummonIssue>,
}

#[derive(Default)]
pub struct QuickSummonState {
    /// Native register/unregister operations are transactional and must never
    /// interleave when renderer invokes arrive close together.
    operation: Mutex<()>,
    runtime: Mutex<RuntimeConfig>,
}

impl QuickSummonState {
    fn runtime(&self) -> std::sync::MutexGuard<'_, RuntimeConfig> {
        self.runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn active_shortcut(&self) -> Option<ActiveShortcut> {
        self.runtime().active_shortcut.clone()
    }

    fn set_active_shortcut(&self, active: Option<ActiveShortcut>) {
        self.runtime().active_shortcut = active;
    }

    fn shortcut_matches(&self, shortcut: &Shortcut) -> bool {
        self.runtime()
            .active_shortcut
            .as_ref()
            .is_some_and(|active| active.shortcut == *shortcut)
    }

    pub fn close_to_tray(&self) -> bool {
        self.runtime().tray_enabled
    }

    pub fn set_shortcut_backend_available(&self, available: bool) {
        self.runtime().shortcut_backend_available = available;
    }

    fn status(&self) -> QuickSummonStatus {
        let runtime = self.runtime();
        QuickSummonStatus {
            shortcut_registered: runtime.active_shortcut.is_some(),
            active_shortcut: runtime
                .active_shortcut
                .as_ref()
                .map(|active| active.label.clone()),
            tray_enabled: runtime.tray_enabled,
            close_behavior: if runtime.tray_enabled {
                CloseBehavior::HideToTray
            } else {
                CloseBehavior::Exit
            },
            issues: runtime.issues.clone(),
        }
    }
}

pub fn plugin<R: Runtime>() -> TauriPlugin<R> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, shortcut, event| {
            if event.state() != ShortcutState::Pressed {
                return;
            }
            let Some(state) = app.try_state::<QuickSummonState>() else {
                return;
            };
            if state.shortcut_matches(shortcut) {
                toggle_main_window(app);
            }
        })
        .build()
}

#[tauri::command]
pub fn configure_quick_summon<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, QuickSummonState>,
    config: QuickSummonConfig,
) -> QuickSummonStatus {
    let _operation = state
        .operation
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut issues = Vec::new();

    configure_shortcut(&app, &state, &config, &mut issues);
    configure_tray(&app, &state, config.keep_in_tray, &mut issues);
    state.runtime().issues = issues;
    state.status()
}

fn allowed_shortcut(label: &str) -> Option<Shortcut> {
    if !ALLOWED_SHORTCUTS.contains(&label) {
        return None;
    }
    label.parse().ok()
}

fn configure_shortcut<R: Runtime>(
    app: &AppHandle<R>,
    state: &QuickSummonState,
    config: &QuickSummonConfig,
    issues: &mut Vec<QuickSummonIssue>,
) {
    let requested = if config.shortcut_enabled {
        match allowed_shortcut(&config.shortcut) {
            Some(shortcut) => Some(ActiveShortcut {
                label: config.shortcut.clone(),
                shortcut,
            }),
            None => {
                issues.push(QuickSummonIssue::InvalidShortcut);
                return;
            }
        }
    } else {
        None
    };

    if requested.is_some() && !state.runtime().shortcut_backend_available {
        issues.push(QuickSummonIssue::ShortcutBackendUnavailable);
        return;
    }

    let previous = state.active_shortcut();
    if previous.as_ref().map(|active| active.shortcut)
        == requested.as_ref().map(|active| active.shortcut)
    {
        return;
    }

    if let Some(active) = previous.as_ref() {
        if let Err(error) = app.global_shortcut().unregister(active.shortcut) {
            eprintln!("quick summon: failed to unregister shortcut: {error}");
            issues.push(QuickSummonIssue::ShortcutUnavailable);
            return;
        }
        state.set_active_shortcut(None);
    }

    let Some(requested) = requested else {
        return;
    };
    if let Err(error) = app.global_shortcut().register(requested.shortcut) {
        eprintln!("quick summon: requested shortcut is unavailable: {error}");
        issues.push(QuickSummonIssue::ShortcutUnavailable);
        if let Some(previous) = previous {
            match app.global_shortcut().register(previous.shortcut) {
                Ok(()) => state.set_active_shortcut(Some(previous)),
                Err(rollback_error) => {
                    eprintln!("quick summon: failed to restore prior shortcut: {rollback_error}");
                    issues.push(QuickSummonIssue::ShortcutRollbackFailed);
                }
            }
        }
        return;
    }
    state.set_active_shortcut(Some(requested));
}

fn configure_tray<R: Runtime>(
    app: &AppHandle<R>,
    state: &QuickSummonState,
    enabled: bool,
    issues: &mut Vec<QuickSummonIssue>,
) {
    if !enabled {
        drop(app.remove_tray_by_id(TRAY_ID));
        state.runtime().tray_enabled = false;
        return;
    }

    if app.tray_by_id(TRAY_ID).is_some() {
        state.runtime().tray_enabled = true;
        return;
    }

    match build_tray(app) {
        Ok(()) => state.runtime().tray_enabled = true,
        Err(error) => {
            eprintln!("quick summon: failed to create tray: {error}");
            state.runtime().tray_enabled = false;
            issues.push(QuickSummonIssue::TrayUnavailable);
        }
    }
}

fn build_tray<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    let show = MenuItem::with_id(
        app,
        "quick-summon-show",
        "WSL Desktop 열기",
        true,
        None::<&str>,
    )
    .map_err(|error| error.to_string())?;
    let hide = MenuItem::with_id(app, "quick-summon-hide", "창 숨기기", true, None::<&str>)
        .map_err(|error| error.to_string())?;
    let quit = MenuItem::with_id(app, "quick-summon-quit", "완전히 종료", true, None::<&str>)
        .map_err(|error| error.to_string())?;
    let menu = Menu::with_items(app, &[&show, &hide, &quit]).map_err(|error| error.to_string())?;
    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| "default app icon is missing".to_owned())?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .tooltip("WSL Desktop")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "quick-summon-show" => reveal_main_window(app),
            "quick-summon-hide" => hide_main_window(app),
            "quick-summon-quit" => exit_app(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                reveal_main_window(tray.app_handle());
            }
        })
        .build(app)
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ToggleAction {
    Hide,
    Reveal,
}

fn toggle_action(visible: bool, focused: bool, minimized: bool) -> ToggleAction {
    if visible && focused && !minimized {
        ToggleAction::Hide
    } else {
        ToggleAction::Reveal
    }
}

fn toggle_main_window<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) else {
        return;
    };
    let action = toggle_action(
        window.is_visible().unwrap_or(false),
        window.is_focused().unwrap_or(false),
        window.is_minimized().unwrap_or(false),
    );
    match action {
        ToggleAction::Hide => {
            let _ = window.hide();
        }
        ToggleAction::Reveal => reveal_window(&window),
    }
}

pub fn reveal_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        reveal_window(&window);
    }
}

fn reveal_window<R: Runtime>(window: &WebviewWindow<R>) {
    let _ = window.show();
    let _ = window.unminimize();
    let _ = window.set_focus();
}

fn hide_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        let _ = window.hide();
    }
}

fn exit_app<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        devbox_window_state_tauri::save_main_webview_window(&window);
    }
    app.exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_fixed_shortcut_presets_cross_the_native_boundary() {
        for shortcut in ALLOWED_SHORTCUTS {
            assert!(allowed_shortcut(shortcut).is_some(), "{shortcut}");
        }
        assert!(allowed_shortcut("Alt+F4").is_none());
        assert!(allowed_shortcut("Ctrl+Alt+Space+Q").is_none());
    }

    #[test]
    fn focused_visible_window_hides_and_every_other_state_reveals() {
        assert_eq!(toggle_action(true, true, false), ToggleAction::Hide);
        assert_eq!(toggle_action(true, false, false), ToggleAction::Reveal);
        assert_eq!(toggle_action(true, true, true), ToggleAction::Reveal);
        assert_eq!(toggle_action(false, false, false), ToggleAction::Reveal);
    }

    #[test]
    fn status_makes_close_behavior_explicit() {
        let state = QuickSummonState::default();
        assert_eq!(state.status().close_behavior, CloseBehavior::Exit);
        state.runtime().tray_enabled = true;
        assert_eq!(state.status().close_behavior, CloseBehavior::HideToTray);
    }

    #[test]
    fn status_uses_the_renderer_contract_shape() {
        let state = QuickSummonState::default();
        state.runtime().issues = vec![QuickSummonIssue::ShortcutBackendUnavailable];
        let value = serde_json::to_value(state.status()).expect("serialize status");

        assert_eq!(value["shortcutRegistered"], false);
        assert_eq!(value["activeShortcut"], serde_json::Value::Null);
        assert_eq!(value["closeBehavior"], "exit");
        assert_eq!(value["issues"][0], "shortcutBackendUnavailable");
    }
}
