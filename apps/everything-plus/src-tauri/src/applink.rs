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
