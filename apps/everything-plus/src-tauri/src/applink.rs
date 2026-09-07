//! Cold-start argv와 hot single-instance relaunch 요청을 frontend가 listener를
//! 등록한 뒤 가져갈 때까지 보관한다.

use devbox_applink::OpenRequest;
use std::sync::Mutex;

#[derive(Default)]
pub struct PendingOpen(Mutex<Option<OpenRequest>>);

impl PendingOpen {
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(feature = "standalone")]
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

/// Typed product adapter; the caller enforces native owner/session authorization.
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

    fn query_request(text: &str) -> OpenRequest {
        OpenRequest {
            target: OpenTarget::Query {
                text: text.to_string(),
                filter: None,
            },
            from: Some("devbox-launcher".to_string()),
        }
    }

    #[test]
    fn take_returns_a_request_once() {
        let pending = PendingOpen::new();
        pending.set(query_request("Cargo.toml"));

        assert_eq!(pending.take(), Some(query_request("Cargo.toml")));
        assert_eq!(pending.take(), None);
    }

    #[test]
    fn newest_request_replaces_an_unconsumed_request() {
        let pending = PendingOpen::new();
        pending.set(query_request("old"));
        pending.set(query_request("latest"));

        assert_eq!(pending.take(), Some(query_request("latest")));
    }
}
