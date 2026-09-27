//! One installed Suite tray; menu interpretation is independent of native UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActivityState {
    Recording,
    Paused,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayAction {
    Open(&'static str),
    PauseActivity,
    ResumeActivity,
    Quit,
}
pub struct Item {
    pub id: &'static str,
    pub label: &'static str,
}
pub fn menu_items(state: ActivityState) -> Vec<Item> {
    vec![
        Item {
            id: "open-workspace",
            label: "Workspace 열기",
        },
        Item {
            id: "open-api-studio",
            label: "API Studio 열기",
        },
        Item {
            id: "open-knowledge",
            label: "Knowledge 열기",
        },
        Item {
            id: "open-control-center",
            label: "Control Center 열기",
        },
        if state == ActivityState::Recording {
            Item {
                id: "pause-activity",
                label: "활동 기록 일시중지",
            }
        } else {
            Item {
                id: "resume-activity",
                label: "활동 기록 다시 시작",
            }
        },
        Item {
            id: "quit",
            label: "백그라운드 작업 모두 멈추고 종료",
        },
    ]
}
pub fn action_for(id: &str) -> Option<TrayAction> {
    Some(match id {
        "open-workspace" => TrayAction::Open("workspace"),
        "open-api-studio" => TrayAction::Open("api-studio"),
        "open-knowledge" => TrayAction::Open("knowledge"),
        "open-control-center" => TrayAction::Open("control-center"),
        "pause-activity" => TrayAction::PauseActivity,
        "resume-activity" => TrayAction::ResumeActivity,
        "quit" => TrayAction::Quit,
        _ => return None,
    })
}
#[cfg(windows)]
fn menu(app: &tauri::AppHandle) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    use tauri::menu::{Menu, MenuItem};
    let tracking = activity_engine::component::tracking_status(app).unwrap_or(false);
    let menu = Menu::new(app)?;
    for item in menu_items(if tracking {
        ActivityState::Recording
    } else {
        ActivityState::Paused
    }) {
        menu.append(&MenuItem::with_id(
            app,
            item.id,
            item.label,
            true,
            None::<&str>,
        )?)?;
    }
    Ok(menu)
}
pub fn refresh(_app: &tauri::AppHandle) {
    #[cfg(windows)]
    if let Some(tray) = _app.tray_by_id("devbox-agent") {
        if let Ok(menu) = menu(_app) {
            let _ = tray.set_menu(Some(menu));
        }
    }
}
#[cfg(windows)]
pub fn initialize(
    app: &tauri::AppHandle,
    scope: std::sync::Arc<suite_runtime::platform::component_scope::CapturedScope>,
    routes: std::sync::Arc<crate::routes::Routes>,
    collectors: std::sync::Arc<crate::collectors::Collectors>,
) -> tauri::Result<()> {
    use std::os::windows::process::CommandExt;
    use tauri::tray::TrayIconBuilder;
    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| std::io::Error::other("agent_icon_unavailable"))?;
    TrayIconBuilder::with_id("devbox-agent").icon(icon).tooltip("Devbox 백그라운드 서비스")
        .menu(&menu(app)?).show_menu_on_left_click(true).on_menu_event(move |app, event| {
            if *routes.shutdown_receiver().borrow() { return; }
            let Some(action) = action_for(event.id.as_ref()) else { return; };
            if action == TrayAction::Quit { routes.shutdown(); return; }
            let app = app.clone(); let scope = scope.clone(); let collectors = collectors.clone(); let routes = routes.clone();
            tauri::async_runtime::spawn_blocking(move || {
                if *routes.shutdown_receiver().borrow() { return; }
                let result = match action {
                    TrayAction::Open(product) => (|| {
                        let (_, executable, _) = scope.member(product)?;
                        let mut child = std::process::Command::new(executable).creation_flags(0x08000000).spawn().map_err(|_| "product_launch_unavailable")?;
                        std::thread::spawn(move || { let _ = child.wait(); });
                        Ok(())
                    })(),
                    TrayAction::PauseActivity => collectors.set_tracking(false),
                    TrayAction::ResumeActivity => collectors.set_tracking(true),
                    TrayAction::Quit => unreachable!(),
                };
                refresh(&app);
                if result.is_err() {
                    use tauri_plugin_notification::NotificationExt;
                    let _ = app.notification().builder().title("Devbox").body("요청을 완료하지 못했습니다. 제품을 열어 저장소와 백그라운드 서비스 상태를 확인해 주세요.").show();
                }
            });
        }).build(app)?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tray_lists_exact_products_pause_and_complete_owner_shutdown() {
        let labels: Vec<_> = menu_items(ActivityState::Recording)
            .into_iter()
            .map(|item| item.label)
            .collect();
        assert_eq!(
            labels,
            [
                "Workspace 열기",
                "API Studio 열기",
                "Knowledge 열기",
                "Control Center 열기",
                "활동 기록 일시중지",
                "백그라운드 작업 모두 멈추고 종료"
            ]
        );
        assert_eq!(
            menu_items(ActivityState::Paused)[4].label,
            "활동 기록 다시 시작"
        );
        assert_eq!(
            action_for("open-knowledge"),
            Some(TrayAction::Open("knowledge"))
        );
        assert_eq!(action_for("quit"), Some(TrayAction::Quit));
        assert_eq!(action_for("open-arbitrary-product"), None);
    }
}
