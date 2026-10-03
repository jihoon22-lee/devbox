//! Every UI close retains the native unsaved-note review. Agent-owned work survives.
use std::sync::Mutex;
use tauri::{Emitter, Manager};
#[derive(Default)]
struct QuitReview {
    pending: Option<String>,
    approved: bool,
}
impl QuitReview {
    fn request(&mut self) -> String {
        self.pending
            .get_or_insert_with(|| uuid::Uuid::new_v4().to_string())
            .clone()
    }
    fn decide(&mut self, id: &str, quit: bool) -> Result<(), String> {
        if self.pending.as_deref() != Some(id) || self.approved {
            return Err("quit_review_stale".into());
        }
        self.pending = None;
        self.approved = quit;
        Ok(())
    }
}
#[derive(Default)]
struct Lifecycle {
    quit: Mutex<QuitReview>,
}
fn request_quit(app: &tauri::AppHandle) {
    let state = app.state::<Lifecycle>();
    let Ok(mut review) = state.quit.lock() else {
        return;
    };
    let id = review.request();
    drop(review);
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
        let _ = window.emit("knowledge://quit-request", id);
    }
}
pub fn quit_dispatch_typed(
    app: &tauri::AppHandle,
    call: crate::ipc::commands::QuitCall,
) -> Result<serde_json::Value, String> {
    let state = app.state::<Lifecycle>();
    let mut review = state.quit.lock().map_err(|_| "quit_unavailable")?;
    match call {
        crate::ipc::commands::QuitCall::LifecycleStatus {} => Err("quit_unavailable".into()),
        crate::ipc::commands::QuitCall::PendingQuit {} => Ok(serde_json::json!(review.pending)),
        crate::ipc::commands::QuitCall::DecideQuit { id, quit } => {
            review.decide(&id, quit)?;
            drop(review);
            if quit {
                app.exit(0);
            }
            Ok(serde_json::Value::Null)
        }
    }
}
pub fn initialize(app: &tauri::AppHandle) {
    app.manage(Lifecycle::default());
}
pub fn on_event(app: &tauri::AppHandle, event: &tauri::RunEvent) {
    match event {
        tauri::RunEvent::WindowEvent {
            label,
            event: tauri::WindowEvent::CloseRequested { api, .. },
            ..
        } if label == "main" => {
            api.prevent_close();
            request_quit(app);
        }
        tauri::RunEvent::ExitRequested { api, .. } => {
            if !app
                .state::<Lifecycle>()
                .quit
                .lock()
                .is_ok_and(|review| review.approved)
            {
                api.prevent_exit();
                request_quit(app);
                return;
            }
            if matches!(crate::collector_owner::installed(app), Ok(false)) {
                let _ = activity_engine::component::shutdown(app);
            }
        }
        _ => {}
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quit_requires_current_review_and_cancel_keeps_collection_running() {
        let mut review = QuitReview::default();
        let id = review.request();
        assert_eq!(review.request(), id);
        assert!(review.decide("stale", true).is_err());
        review.decide(&id, false).unwrap();
        assert!(!review.approved);
        let next = review.request();
        assert_ne!(next, id);
        assert!(review.decide(&id, true).is_err());
        review.decide(&next, true).unwrap();
        assert!(review.approved);
        assert!(review.decide(&next, true).is_err());
    }
}

#[derive(serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct CollectorStatus {
    pub owner: CollectorOwner,
    pub tracking: Option<bool>,
    pub consent: Option<bool>,
}
#[derive(serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum CollectorOwner {
    InstalledAgent,
    PortableLocal,
    Unknown,
}
pub async fn collector_status(app: &tauri::AppHandle) -> Result<serde_json::Value, String> {
    let owner = match crate::collector_owner::installed(app) {
        Ok(true) => CollectorOwner::InstalledAgent,
        Ok(false) => CollectorOwner::PortableLocal,
        Err(_) => CollectorOwner::Unknown,
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "quit_unavailable")?
        .as_millis() as u64;
    let status = crate::collector_owner::call(
        app,
        "knowledge.activity",
        "collection_status",
        serde_json::json!({}),
        now.saturating_add(5000),
    )
    .await
    .ok();
    let tracking = status
        .as_ref()
        .and_then(|value| value.get("tracking"))
        .and_then(serde_json::Value::as_bool);
    let consent = status
        .as_ref()
        .and_then(|value| value.get("consent"))
        .and_then(serde_json::Value::as_bool);
    serde_json::to_value(CollectorStatus {
        owner,
        tracking,
        consent,
    })
    .map_err(|_| "quit_unavailable".into())
}
