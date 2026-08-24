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
            },
            from: None,
        };
        pending.set(latest.clone());

        assert_eq!(pending.take(), Some(latest));
    }
}
