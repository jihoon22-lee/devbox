//! Manager's small inbound AppLink receiver. Launcher only sends an opaque
//! app id for the install screen; Manager never accepts an executable path or
//! installer URL from argv.

use devbox_applink::{OpenRequest, OpenTarget};
use std::sync::Mutex;

const MAX_APP_ID_BYTES: usize = 64;
const BUILD_CATALOG: &str = include_str!("../../../catalog.json");

#[derive(Default)]
pub struct PendingOpen(Mutex<Option<OpenRequest>>);

impl PendingOpen {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn set(&self, request: OpenRequest) {
        *self.0.lock().expect("Manager PendingOpen mutex poisoned") = Some(request);
    }
    pub fn take(&self) -> Option<OpenRequest> {
        self.0
            .lock()
            .expect("Manager PendingOpen mutex poisoned")
            .take()
    }
}

#[tauri::command]
pub fn take_pending_open(state: tauri::State<'_, PendingOpen>) -> Option<OpenRequest> {
    state.take()
}

pub fn is_install_request(request: &OpenRequest) -> bool {
    let OpenTarget::Install { app_id } = &request.target else {
        return false;
    };
    let shape_is_valid = !app_id.is_empty()
        && app_id.len() <= MAX_APP_ID_BYTES
        && app_id.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        });
    if !shape_is_valid {
        return false;
    }
    devbox_catalog::parse_catalog(BUILD_CATALOG)
        .ok()
        .is_some_and(|catalog| {
            catalog
                .apps
                .iter()
                .any(|app| app.id == *app_id && app.release && app.manager_visible)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_install_target_is_accepted_by_manager_route() {
        assert!(is_install_request(&OpenRequest {
            target: OpenTarget::Install {
                app_id: "workbench".into()
            },
            from: None
        }));
        assert!(!is_install_request(&OpenRequest {
            target: OpenTarget::Install {
                app_id: "../secret".into()
            },
            from: None
        }));
        assert!(!is_install_request(&OpenRequest {
            target: OpenTarget::Install {
                app_id: "unknown-app".into()
            },
            from: None
        }));
        assert!(!is_install_request(&OpenRequest {
            target: OpenTarget::Install {
                app_id: "x".repeat(MAX_APP_ID_BYTES + 1)
            },
            from: None
        }));
        assert!(!is_install_request(&OpenRequest {
            target: OpenTarget::Path {
                path: "/tmp/x".into(),
                line: None,
                column: None
            },
            from: None
        }));
    }
}
