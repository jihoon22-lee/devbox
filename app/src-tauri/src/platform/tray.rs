use crate::core::{
    intents::{AppIntent, close_to_tray},
    settings,
};
use tauri::{
    AppHandle, Manager,
    menu::{MenuBuilder, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

pub struct Tray {
    summary: MenuItem<tauri::Wry>,
    palette: MenuItem<tauri::Wry>,
}
pub fn initialize(app: &AppHandle) -> tauri::Result<()> {
    let summary = MenuItem::with_id(
        app,
        "agents",
        "에이전트 0 · 입력 대기 0",
        true,
        None::<&str>,
    )?;
    let palette = MenuItem::with_id(app, "palette", "명령 팔레트", true, None::<&str>)?;
    let menu = MenuBuilder::new(app)
        .text("open", "devbox 열기")
        .item(&summary)
        .item(&palette)
        .separator()
        .text("quit", "종료")
        .build()?;
    let icon = app
        .default_window_icon()
        .ok_or_else(|| tauri::Error::Io(std::io::Error::other("앱 아이콘 없음")))?;
    TrayIconBuilder::with_id("devbox-main")
        .icon(icon.clone())
        .tooltip("devbox")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => super::window::focus_main(app),
            "agents" => {
                super::window::focus_main(app);
                super::notify::navigate_agents(app);
            }
            "palette" => {
                super::window::focus_main(app);
                super::intents::dispatch(app, AppIntent::Palette);
            }
            "quit" => {
                super::window::focus_main(app);
                super::intents::dispatch(app, AppIntent::Quit);
            }
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
                super::window::focus_main(tray.app_handle());
            }
        })
        .build(app)?;
    app.manage(Tray { summary, palette });
    Ok(())
}
pub fn set_shortcut_status(app: &AppHandle, status: &crate::core::shortcuts::ShortcutStatus) {
    if let Some(tray) = app.try_state::<Tray>() {
        let label = if status.palette_error.is_some() {
            "명령 팔레트 · 단축키 등록 안 됨"
        } else {
            "명령 팔레트"
        };
        if let Err(error) = tray.palette.set_text(label) {
            eprintln!("Cannot update tray shortcut status: {error}");
        }
    }
    if let Some(tray) = app.tray_by_id("devbox-main") {
        let mut tooltip = "devbox".to_string();
        if status.palette_error.is_some() {
            tooltip.push_str(" · 팔레트 단축키 등록 안 됨");
        }
        if status.bottom_terminal_error.is_some() {
            tooltip.push_str(" · 터미널 단축키 등록 안 됨");
        }
        if let Err(error) = tray.set_tooltip(Some(tooltip)) {
            eprintln!("Cannot update tray shortcut tooltip: {error}");
        }
    }
}
pub fn set_summary(app: &AppHandle, open: u32, attention: u32) {
    if let Some(tray) = app.try_state::<Tray>()
        && let Err(error) = tray
            .summary
            .set_text(format!("에이전트 {open} · 입력 대기 {attention}"))
    {
        eprintln!("Cannot update tray summary: {error}");
    }
}
pub fn available(app: &AppHandle, label: &str) -> bool {
    close_to_tray(
        label,
        app.try_state::<Tray>().is_some() && app.tray_by_id("devbox-main").is_some(),
    )
}
pub fn show_notice(app: &AppHandle) {
    // Development executables do not own a registered Windows toast identity.
    if cfg!(debug_assertions) {
        return;
    }
    let result = super::preferences::settings_path(app).and_then(|path| {
        settings::with_settings(&path, |settings| {
            if settings.tray_notice_shown {
                return Ok(());
            }
            let spec = devbox_win::toast::ToastSpec {
                title: "트레이에서 계속 실행됩니다".into(),
                body: "완전히 끄려면 트레이 메뉴의 종료를 누르세요".into(),
                launch_uri: format!("{}://open", crate::core::channel::CHANNEL.config().protocol),
                actions: vec![],
            };
            devbox_win::toast::show(&app.config().identifier, &spec, "tray-notice", "app")
                .map_err(std::io::Error::other)?;
            settings.tray_notice_shown = true;
            settings::save(&path, settings)
        })
        .map_err(|error| crate::core::connection::IpcError::new(error.to_string()))
    });
    if let Err(error) = result {
        eprintln!("Cannot show or persist tray notice: {error}");
    }
}
