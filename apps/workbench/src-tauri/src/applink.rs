//! 콜드 스타트 argv 파싱 결과 또는 single-instance 포워딩으로 들어온 인바운드
//! 열기 요청을, 프론트가 마운트 시 가져갈 때까지 보관한다.
//!
//! pull 방식인 이유: setup 중 이벤트를 emit하면 프론트의 리스너 등록과 경합한다
//! (`docs/superpowers/specs/2026-08-17-app-interop-design.md` §3).

use devbox_applink::OpenRequest;
use std::sync::Mutex;

/// 프론트가 가져갈 때까지 보관하는 관리 상태. `take`는 값을 꺼내면서 비운다 —
/// 페이지 리로드가 같은 열기 동작을 다시 트리거하지 않도록 하는 근거다.
#[derive(Default)]
pub struct PendingOpen(Mutex<Option<OpenRequest>>);

impl PendingOpen {
    pub fn new() -> Self {
        Self::default()
    }

    /// 새 요청을 저장한다. 이전 값은 덮어쓴다.
    pub fn set(&self, request: OpenRequest) {
        *self.0.lock().expect("PendingOpen mutex poisoned") = Some(request);
    }

    /// 저장된 요청을 꺼내고 비운다.
    pub fn take(&self) -> Option<OpenRequest> {
        self.0.lock().expect("PendingOpen mutex poisoned").take()
    }
}

/// 프론트가 마운트 시 호출해 대기 중인 열기 요청을 가져간다.
#[tauri::command]
pub fn take_pending_open(state: tauri::State<'_, PendingOpen>) -> Option<OpenRequest> {
    state.take()
}

#[cfg(test)]
mod tests {
    use super::*;
    use devbox_applink::OpenTarget;

    fn sample() -> OpenRequest {
        OpenRequest {
            target: OpenTarget::Path {
                path: "/tmp/repo".to_string(),
                line: None,
                column: None,
            },
            from: Some("repo-manager".to_string()),
        }
    }

    #[test]
    fn take_returns_none_when_nothing_set() {
        let pending = PendingOpen::new();
        assert_eq!(pending.take(), None);
    }

    #[test]
    fn take_returns_set_value_then_clears() {
        let pending = PendingOpen::new();
        pending.set(sample());
        assert_eq!(pending.take(), Some(sample()));
        // 한 번 꺼내면 비어야 한다 — 페이지 리로드가 재트리거하지 않는 근거.
        assert_eq!(pending.take(), None);
    }

    #[test]
    fn set_overwrites_previous_value() {
        let pending = PendingOpen::new();
        pending.set(sample());
        let second = OpenRequest {
            target: OpenTarget::Profile {
                id: "prof-1".to_string(),
            },
            from: None,
        };
        pending.set(second.clone());
        assert_eq!(pending.take(), Some(second));
    }
}
