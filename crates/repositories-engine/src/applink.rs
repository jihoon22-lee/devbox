//! Cold-start argv와 hot single-instance relaunch 요청을 frontend가 listener를
//! 등록한 뒤 가져갈 때까지 한 번만 보관한다.

use devbox_applink::OpenRequest;
use std::sync::Mutex;

#[derive(Default)]
pub struct PendingOpen(Mutex<Option<OpenRequest>>);

impl PendingOpen {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&self, request: OpenRequest) {
        *self.0.lock().expect("PendingOpen mutex poisoned") = Some(request);
    }

    pub fn take(&self) -> Option<OpenRequest> {
        self.0.lock().expect("PendingOpen mutex poisoned").take()
    }
}

#[tauri::command]
pub fn take_pending_open(state: tauri::State<'_, PendingOpen>) -> Option<OpenRequest> {
    state.take()
}

/// Typed product adapter; the native host owns caller/session/owner admission.
pub(crate) async fn __component_take_pending_open(
    _component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use tauri::Manager;
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {}
    let _: Input = serde_json::from_value(args).map_err(|_| "component_args_invalid")?;
    let value = take_pending_open(
        _component_app
            .try_state()
            .ok_or("component_state_unavailable")?,
    );
    serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use devbox_applink::OpenTarget;

    fn path_request(path: &str) -> OpenRequest {
        OpenRequest {
            target: OpenTarget::Path {
                path: path.to_string(),
                line: None,
                column: None,
            },
            from: Some("workbench".to_string()),
        }
    }

    #[test]
    fn take_returns_a_request_once() {
        let pending = PendingOpen::new();
        pending.set(path_request("C:\\projects\\devbox"));

        assert_eq!(pending.take(), Some(path_request("C:\\projects\\devbox")));
        assert_eq!(pending.take(), None);
    }

    #[test]
    fn newest_request_replaces_an_unconsumed_request() {
        let pending = PendingOpen::new();
        pending.set(path_request("C:\\projects\\old"));
        pending.set(path_request("C:\\projects\\latest"));

        assert_eq!(pending.take(), Some(path_request("C:\\projects\\latest")));
    }
}
