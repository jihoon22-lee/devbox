//! Cold/hot AppLink handoff request buffering for Log Lens.

use devbox_applink::{OpenRequest, OpenTarget};
use std::sync::Mutex;

const EXPECTED_KIND: &str = "log-source/v1";

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
    kind == EXPECTED_KIND
        && id.len() == 32
        && id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
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
            EXPECTED_KIND,
            &"a".repeat(32)
        )));
        assert!(!is_log_source_request(&request(
            "api-request/v1",
            &"a".repeat(32)
        )));
        assert!(!is_log_source_request(&request(EXPECTED_KIND, "../source")));
    }

    #[test]
    fn pending_request_is_one_shot() {
        let pending = PendingOpen::new();
        let value = request(EXPECTED_KIND, &"a".repeat(32));
        pending.set(value.clone());
        assert_eq!(pending.take(), Some(value));
        assert_eq!(pending.take(), None);
    }
}
