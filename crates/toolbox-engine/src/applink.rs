//! Cold/hot AppLink buffering for the bounded `toolbox-text/v1` receiver.

use devbox_applink::{OpenRequest, OpenTarget, TOOLBOX_TEXT_HANDOFF_KIND};
use std::sync::Mutex;

#[derive(Default)]
struct PendingSlot {
    request: Option<OpenRequest>,
    // Retained after take until native preview apply/restore, so another send
    // cannot disappear behind an already-open renderer preview.
    product_delivery: Option<(String, std::time::Instant)>,
}
#[derive(Default)]
pub struct PendingOpen(Mutex<PendingSlot>);
impl PendingOpen {
    pub fn new() -> Self {
        Self::default()
    }
    #[cfg_attr(not(feature = "standalone"), allow(dead_code))]
    pub fn set(&self, request: OpenRequest) {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .request = Some(request);
    }
    pub fn try_set(&self, request: OpenRequest) -> Result<(), String> {
        let devbox_applink::OpenTarget::Handoff { id, .. } = &request.target else {
            return Err("component_delivery_invalid".into());
        };
        let mut slot = self
            .0
            .lock()
            .map_err(|_| "component_delivery_unavailable")?;
        if slot.product_delivery.as_ref().is_some_and(|(_, started)| {
            started.elapsed().as_millis() >= u128::from(devbox_applink::DEFAULT_HANDOFF_TTL_MS)
        }) {
            slot.product_delivery = None;
            slot.request = None;
        }
        if slot.request.is_some() || slot.product_delivery.is_some() {
            return Err("component_delivery_busy".into());
        }
        slot.product_delivery = Some((id.clone(), std::time::Instant::now()));
        slot.request = Some(request);
        Ok(())
    }
    pub fn release(&self, id: &str) {
        let mut slot = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if slot
            .product_delivery
            .as_ref()
            .is_some_and(|(current, _)| current == id)
        {
            slot.product_delivery = None;
            slot.request = None;
        }
    }
    pub fn take(&self) -> Option<OpenRequest> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .request
            .take()
    }
}

#[tauri::command]
pub fn take_pending_open(state: tauri::State<'_, PendingOpen>) -> Option<OpenRequest> {
    state.take()
}

#[cfg_attr(not(feature = "standalone"), allow(dead_code))]
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

/// Typed product adapter; the caller owns component/session authorization.
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
    #[test]
    fn product_delivery_never_overwrites_an_unread_request() {
        let pending = PendingOpen::new();
        let request = OpenRequest {
            target: devbox_applink::OpenTarget::Handoff {
                kind: "api-request/v1".into(),
                id: "a".repeat(32),
            },
            from: Some("webhook-lab".into()),
        };
        pending.try_set(request.clone()).unwrap();
        assert!(pending.try_set(request.clone()).is_err());
        assert_eq!(pending.take(), Some(request.clone()));
        assert!(
            pending.try_set(request.clone()).is_err(),
            "a taken delivery still owns its preview slot"
        );
        pending.release("wrong-id");
        assert!(pending.try_set(request.clone()).is_err());
        pending.release(&"a".repeat(32));
        pending.try_set(request).unwrap();
    }

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
