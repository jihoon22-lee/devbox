//! Cold/hot AppLink buffering for the bounded `toolbox-text/v1` receiver.

use devbox_applink::{OpenRequest, OpenTarget, TOOLBOX_TEXT_HANDOFF_KIND};
use std::sync::Mutex;

#[derive(Default)]
pub struct PendingOpen(Mutex<Option<OpenRequest>>);

impl PendingOpen {
    pub fn new() -> Self {
        Self::default()
    }

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

pub fn is_toolbox_text_request(request: &OpenRequest) -> bool {
    let OpenTarget::Handoff { kind, id } = &request.target else {
        return false;
    };
    kind == TOOLBOX_TEXT_HANDOFF_KIND
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
                kind: kind.to_string(),
                id: id.to_string(),
            },
            from: Some("api-playground".to_string()),
        }
    }

    #[test]
    fn routes_only_the_exact_kind_and_opaque_id() {
        assert!(is_toolbox_text_request(&request(
            TOOLBOX_TEXT_HANDOFF_KIND,
            &"a".repeat(32)
        )));
        assert!(!is_toolbox_text_request(&request(
            "knowledge-draft/v1",
            &"a".repeat(32)
        )));
        assert!(!is_toolbox_text_request(&request(
            TOOLBOX_TEXT_HANDOFF_KIND,
            "../payload"
        )));
    }

    #[test]
    fn pending_open_is_one_shot() {
        let pending = PendingOpen::new();
        let value = request(TOOLBOX_TEXT_HANDOFF_KIND, &"b".repeat(32));
        pending.set(value.clone());
        assert_eq!(pending.take(), Some(value));
        assert_eq!(pending.take(), None);
    }
}
