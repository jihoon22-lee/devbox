//! Run Manager's bounded Launcher task receiver.
//!
//! Launcher sends only an opaque job/service id. The UI resolves that id
//! against the current Run Manager store before showing or executing anything;
//! no command, cwd, or environment value crosses the AppLink boundary.

use devbox_applink::{OpenRequest, OpenTarget};
use std::sync::Mutex;

const MAX_TASK_ID_BYTES: usize = 128;

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
            .expect("Run Manager PendingOpen mutex poisoned") = Some(request);
    }

    pub fn take(&self) -> Option<OpenRequest> {
        self.0
            .lock()
            .expect("Run Manager PendingOpen mutex poisoned")
            .take()
    }
}

#[tauri::command]
pub fn take_pending_open(state: tauri::State<'_, PendingOpen>) -> Option<OpenRequest> {
    state.take()
}

pub fn is_supported_request(request: &OpenRequest) -> bool {
    match &request.target {
        OpenTarget::Task { id } => {
            !id.is_empty()
                && id.len() <= MAX_TASK_ID_BYTES
                && id.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':')
                })
        }
        OpenTarget::Handoff { kind, id } => {
            kind == devbox_applink::TASK_CONTROL_HANDOFF_KIND
                && request.from.as_deref() == Some(devbox_applink::TASK_CONTROL_SOURCE_APP)
                && id.len() == 32
                && id
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(id: &str) -> OpenRequest {
        OpenRequest {
            target: OpenTarget::Task { id: id.into() },
            from: Some("devbox-launcher".into()),
        }
    }

    #[test]
    fn only_bounded_task_ids_are_accepted() {
        assert!(is_supported_request(&task("job-1")));
        assert!(!is_supported_request(&task("../secret")));
        assert!(!is_supported_request(&task(
            &"x".repeat(MAX_TASK_ID_BYTES + 1)
        )));
        assert!(!is_supported_request(&OpenRequest {
            target: OpenTarget::Query {
                text: "job-1".into(),
                filter: None,
            },
            from: None,
        }));
    }

    #[test]
    fn pending_request_is_one_shot() {
        let pending = PendingOpen::new();
        pending.set(task("job-1"));
        assert_eq!(pending.take(), Some(task("job-1")));
        assert_eq!(pending.take(), None);
    }
}
