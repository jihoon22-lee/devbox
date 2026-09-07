//! Cold-start argv와 single-instance relaunch가 전달한 열기 요청을 frontend가
//! listener를 등록한 뒤 가져갈 때까지 보관한다.
//!
//! `take`가 값을 비우므로 hot-instance event payload와 pending pull이 같은 요청을
//! 중복 적용하지 않는다.

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

    fn path_request(path: &str) -> OpenRequest {
        OpenRequest {
            target: OpenTarget::Path {
                path: path.to_string(),
                line: None,
                column: None,
            },
            from: Some("devbox-launcher".to_string()),
        }
    }

    #[test]
    fn take_is_one_shot() {
        let pending = PendingOpen::new();
        pending.set(path_request("C:/Knowledge/Notes/one.md"));

        assert_eq!(
            pending.take(),
            Some(path_request("C:/Knowledge/Notes/one.md"))
        );
        assert_eq!(pending.take(), None);
    }

    #[test]
    fn newest_request_replaces_an_unconsumed_request() {
        let pending = PendingOpen::new();
        pending.set(path_request("C:/Knowledge/Notes/old.md"));
        let latest = OpenRequest {
            target: OpenTarget::Query {
                text: "latest".to_string(),
                filter: None,
            },
            from: None,
        };
        pending.set(latest.clone());

        assert_eq!(pending.take(), Some(latest));
    }
}
