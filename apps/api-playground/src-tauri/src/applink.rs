//! Cold-start and single-instance delivery for protocol-v2 AppLink requests.
//!
//! The renderer takes this slot only after registering its event listener. A
//! hot event is merely a wake-up signal; its payload is not trusted directly.

use devbox_applink::OpenRequest;
use std::sync::Mutex;

#[derive(Default)]
pub struct PendingOpen(Mutex<Option<OpenRequest>>);

impl PendingOpen {
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg_attr(not(feature = "standalone"), allow(dead_code))]
    pub fn set(&self, request: OpenRequest) {
        *self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(request);
    }

    pub fn take(&self) -> Option<OpenRequest> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
    }
}

#[tauri::command]
pub fn take_pending_open(state: tauri::State<'_, PendingOpen>) -> Option<OpenRequest> {
    state.take()
}

/// Typed product adapter; the caller owns component/session authorization.
pub(crate) async fn __component_take_pending_open(
    component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use tauri::Manager as _;
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {}
    let Input {} = serde_json::from_value(args).map_err(|_| "component_args_invalid".to_owned())?;
    let value = take_pending_open(component_app.state());
    serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use devbox_applink::OpenTarget;

    fn request() -> OpenRequest {
        OpenRequest {
            target: OpenTarget::Handoff {
                kind: "api-request/v1".into(),
                id: "0123456789abcdef0123456789abcdef".into(),
            },
            from: Some("webhook-lab".into()),
        }
    }

    #[test]
    fn take_is_one_shot_and_latest_request_wins() {
        let pending = PendingOpen::new();
        pending.set(request());
        assert_eq!(pending.take(), Some(request()));
        assert_eq!(pending.take(), None);
        pending.set(request());
        assert_eq!(pending.take(), Some(request()));
    }
}
