//! Tauri command boundary for Launcher. Commands rebuild the bounded index for
//! every read/action, so a result cannot outlive a changed catalog or snapshot.

use crate::core::launcher::{self, Index, SearchResponse};
use crate::core::preferences::{Preferences, PREFERENCES_FILE};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::Manager;

pub(crate) const CATALOG_JSON: &str = include_str!("../../../catalog.json");

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SearchRequest {
    pub query: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LaunchResultRequest {
    pub result_id: String,
    pub expected_revision: String,
    #[serde(default)]
    pub allow_stale: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TextPreviewRequest {
    pub action_id: String,
    pub expected_revision: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FavoriteRequest {
    pub result_id: String,
    pub expected_revision: String,
    pub favorite: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchResponse {
    pub status: String,
    pub app_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextPreview {
    pub action_id: String,
    pub kind: String,
    pub max_bytes: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TextHandoffRequest {
    pub action_id: String,
    pub expected_revision: String,
    pub text: String,
}

fn index() -> Result<Index, String> {
    Index::build(CATALOG_JSON, &devbox_integration::integration_root())
}

fn preferences_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn preferences_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_local_data_dir()
        .map(|directory| directory.join(PREFERENCES_FILE))
        .map_err(|_| "Launcher 즐겨찾기 설정 경로를 확인할 수 없습니다".to_string())
}

fn load_preferences(app: &tauri::AppHandle) -> Result<Preferences, String> {
    Preferences::load(&preferences_path(app)?)
}

fn update_preferences(
    app: &tauri::AppHandle,
    update: impl FnOnce(&mut Preferences) -> Result<(), String>,
) -> Result<(), String> {
    let _guard = preferences_lock()
        .lock()
        .map_err(|_| "Launcher 즐겨찾기 설정을 갱신할 수 없습니다".to_string())?;
    let path = preferences_path(app)?;
    let mut preferences = Preferences::load(&path)?;
    update(&mut preferences)?;
    let parent = path
        .parent()
        .ok_or_else(|| "Launcher 즐겨찾기 설정 경로를 확인할 수 없습니다".to_string())?;
    std::fs::create_dir_all(parent)
        .map_err(|_| "Launcher 즐겨찾기 설정을 갱신할 수 없습니다".to_string())?;
    preferences.save(&path)
}

/// Best-effort cleanup for a handoff that could not be delivered to its
/// target. `HandoffStore` intentionally exposes no general delete operation;
/// keep this narrow fallback local to the producer and remove only a
/// generated, regular pending file. In particular, never follow a link or
/// accept an arbitrary renderer-provided path.
fn discard_pending_handoff(store: &devbox_applink::HandoffStore, id: &str) {
    if id.len() != 32 || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return;
    }
    let pending = store.root().join("pending").join(format!("{id}.json"));
    let Ok(metadata) = std::fs::symlink_metadata(&pending) else {
        return;
    };
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        return;
    }
    let _ = std::fs::remove_file(pending);
}

#[tauri::command]
pub fn search(app: tauri::AppHandle, request: SearchRequest) -> Result<SearchResponse, String> {
    // Corrupt preferences must never make catalog discovery unusable. Reads
    // degrade to neutral ordering, while every write still loads strictly and
    // therefore cannot overwrite the malformed file.
    let preferences = preferences_lock()
        .lock()
        .ok()
        .and_then(|_guard| load_preferences(&app).ok())
        .unwrap_or_default();
    index()?.search_with_preferences(&request.query, &preferences)
}

/// Re-resolve an opaque result id and launch only after the current catalog,
/// accepted payload kind, and install metadata have been checked again.
#[tauri::command]
pub fn launch_result(
    app: tauri::AppHandle,
    request: LaunchResultRequest,
) -> Result<LaunchResponse, String> {
    let index = index()?;
    let action = index.resolve_checked(
        &request.result_id,
        &request.expected_revision,
        request.allow_stale,
    )?;
    let (app_id, target) = (action.app_id, action.target);
    if devbox_launch::resolve_installed(&app_id).is_none() {
        if app_id == "devbox-manager" {
            return Err("대상 앱이 설치되어 있지 않습니다".into());
        }
        let request = devbox_applink::OpenRequest {
            target: devbox_applink::OpenTarget::Install {
                app_id: app_id.clone(),
            },
            from: Some("devbox-launcher".into()),
        };
        devbox_launch::launch_open("devbox-manager", &request)
            .map_err(|_| "Devbox Manager 설치 화면을 열 수 없습니다".to_string())?;
        return Ok(LaunchResponse {
            status: "installRequired".into(),
            app_id,
        });
    }

    let open_request = match target {
        launcher::Target::Task { id: _ } if request.result_id.starts_with("catalog/app/") => None,
        launcher::Target::Path { path } => Some(devbox_applink::OpenRequest {
            target: devbox_applink::OpenTarget::Path {
                path,
                line: None,
                column: None,
            },
            from: Some("devbox-launcher".into()),
        }),
        launcher::Target::Profile { id } => Some(devbox_applink::OpenRequest {
            target: devbox_applink::OpenTarget::Profile { id },
            from: Some("devbox-launcher".into()),
        }),
        launcher::Target::Workspace { path } => Some(devbox_applink::OpenRequest {
            target: devbox_applink::OpenTarget::Workspace { path },
            from: Some("devbox-launcher".into()),
        }),
        launcher::Target::Query { text, filter } => Some(devbox_applink::OpenRequest {
            target: devbox_applink::OpenTarget::Query { text, filter },
            from: Some("devbox-launcher".into()),
        }),
        launcher::Target::Task { id } => Some(devbox_applink::OpenRequest {
            target: devbox_applink::OpenTarget::Task { id },
            from: Some("devbox-launcher".into()),
        }),
        launcher::Target::ClipboardPreview => None,
    };
    if let Some(open_request) = open_request {
        devbox_launch::launch_open(&app_id, &open_request)
            .map_err(|_| "대상 앱을 실행할 수 없습니다".to_string())?;
    } else {
        devbox_launch::launch(&app_id, &[])
            .map_err(|_| "대상 앱을 실행할 수 없습니다".to_string())?;
    }
    let launched_result_id = request.result_id;
    let _ = update_preferences(&app, |preferences| {
        preferences.record_recent(&launched_result_id)
    });
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
    Ok(LaunchResponse {
        status: "launched".into(),
        app_id,
    })
}

#[tauri::command]
pub fn preview_text_action(request: TextPreviewRequest) -> Result<TextPreview, String> {
    let index = index()?;
    let (_target_app, kind) =
        index.resolve_text_action(&request.action_id, &request.expected_revision)?;
    Ok(TextPreview {
        action_id: request.action_id,
        kind,
        max_bytes: launcher::MAX_HANDOFF_TEXT_BYTES,
    })
}

/// The text is accepted only after the user has explicitly previewed and
/// confirmed it. It is placed in the protocol's short-lived one-time handoff,
/// never in Launcher settings/history or logs.
#[tauri::command]
pub fn perform_text_action(
    app: tauri::AppHandle,
    request: TextHandoffRequest,
) -> Result<LaunchResponse, String> {
    let index = index()?;
    let (target_app, kind) =
        index.resolve_text_action(&request.action_id, &request.expected_revision)?;
    if kind == "clipboard-preview/v1" {
        return Err("Clipboard 미리보기는 전달할 수 없습니다".into());
    }
    let text = launcher::validate_text_handoff(&kind, &request.text)?;
    if devbox_launch::resolve_installed(&target_app).is_none() {
        let install = devbox_applink::OpenRequest {
            target: devbox_applink::OpenTarget::Install {
                app_id: target_app.clone(),
            },
            from: Some("devbox-launcher".into()),
        };
        devbox_launch::launch_open("devbox-manager", &install)
            .map_err(|_| "Devbox Manager 설치 화면을 열 수 없습니다".to_string())?;
        return Ok(LaunchResponse {
            status: "installRequired".into(),
            app_id: target_app,
        });
    }
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "현재 시각을 확인할 수 없습니다")?
        .as_millis() as u64;
    let store = devbox_applink::HandoffStore::new(devbox_applink::handoff_root_in(
        &devbox_integration::common_root(),
    ));
    let descriptor = store
        .create(
            devbox_applink::CreateHandoff {
                kind: text.kind,
                source_app: "devbox-launcher".into(),
                target_app: Some(target_app.clone()),
                payload: serde_json::to_value(devbox_applink::ToolboxTextPayload {
                    text: text.text,
                })
                .map_err(|_| "텍스트 handoff를 안전하게 만들 수 없습니다")?,
            },
            now_ms,
        )
        .map_err(|_| "텍스트 handoff를 안전하게 만들 수 없습니다".to_string())?;
    let handoff_id = descriptor.id.clone();
    let open = devbox_applink::OpenRequest {
        target: devbox_applink::OpenTarget::from(descriptor),
        from: Some("devbox-launcher".into()),
    };
    if devbox_launch::launch_open(&target_app, &open).is_err() {
        discard_pending_handoff(&store, &handoff_id);
        return Err("대상 앱을 실행할 수 없습니다".into());
    }
    let launched_result_id = request.action_id;
    let _ = update_preferences(&app, |preferences| {
        preferences.record_recent(&launched_result_id)
    });
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
    Ok(LaunchResponse {
        status: "launched".into(),
        app_id: target_app,
    })
}

#[tauri::command]
pub fn set_favorite(app: tauri::AppHandle, request: FavoriteRequest) -> Result<(), String> {
    index()?.validate_selection(&request.result_id, &request.expected_revision)?;
    update_preferences(&app, |preferences| {
        preferences.set_favorite(&request.result_id, request.favorite)
    })
}

#[tauri::command]
pub fn clear_recents(app: tauri::AppHandle) -> Result<(), String> {
    update_preferences(&app, |preferences| {
        preferences.clear_recents();
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_read_only_and_bounded() {
        assert!(CATALOG_JSON.len() < 128 * 1024);
    }

    #[test]
    fn pending_handoff_cleanup_removes_only_a_generated_regular_file() {
        let root =
            std::env::temp_dir().join(format!("launcher-handoff-cleanup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("pending")).unwrap();
        let store = devbox_applink::HandoffStore::new(&root);
        let id = "a".repeat(32);
        let pending = root.join("pending").join(format!("{id}.json"));
        std::fs::write(&pending, b"payload").unwrap();

        discard_pending_handoff(&store, &id);

        assert!(!pending.exists());
        discard_pending_handoff(&store, "../outside");
        let _ = std::fs::remove_dir_all(root);
    }
}
