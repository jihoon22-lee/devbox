//! Cold/hot AppLink handoff request buffering for Log Lens.

use devbox_applink::{OpenRequest, OpenTarget};
use std::sync::Mutex;

const LOG_SOURCE_KIND: &str = "log-source/v1";
const WEBHOOK_LOG_KIND: &str = "webhook-log/v1";

#[derive(Default)]
pub struct PendingOpen(Mutex<Option<OpenRequest>>);

impl PendingOpen {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&self, request: OpenRequest) {
        *self.0.lock().expect("Log Lens PendingOpen mutex poisoned") = Some(request);
    }

    pub fn take(&self) -> Option<OpenRequest> {
        self.0
            .lock()
            .expect("Log Lens PendingOpen mutex poisoned")
            .take()
    }
}

#[tauri::command]
pub fn take_pending_open(state: tauri::State<'_, PendingOpen>) -> Option<OpenRequest> {
    state.take()
}

pub fn is_log_source_request(request: &OpenRequest) -> bool {
    let OpenTarget::Handoff { kind, id } = &request.target else {
        return false;
    };
    (kind == LOG_SOURCE_KIND || kind == WEBHOOK_LOG_KIND)
        && id.len() == 32
        && id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Typed product adapter; native admission precedes this existing command.
#[cfg(feature = "desktop")]
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

    fn request(kind: &str, id: &str) -> OpenRequest {
        OpenRequest {
            target: OpenTarget::Handoff {
                kind: kind.into(),
                id: id.into(),
            },
            from: Some("run-manager".into()),
        }
    }

    #[test]
    fn only_expected_kind_and_opaque_id_are_routed() {
        assert!(is_log_source_request(&request(
            LOG_SOURCE_KIND,
            &"a".repeat(32)
        )));
        assert!(is_log_source_request(&request(
            WEBHOOK_LOG_KIND,
            &"b".repeat(32)
        )));
        assert!(!is_log_source_request(&request(
            "api-request/v1",
            &"a".repeat(32)
        )));
        assert!(!is_log_source_request(&request(
            LOG_SOURCE_KIND,
            "../source"
        )));
    }

    #[test]
    fn pending_request_is_one_shot() {
        let pending = PendingOpen::new();
        let value = request(LOG_SOURCE_KIND, &"a".repeat(32));
        pending.set(value.clone());
        assert_eq!(pending.take(), Some(value));
        assert_eq!(pending.take(), None);
    }
}
