//! Claim-preview-apply lifecycle for incoming `toolbox-text/v1` handoffs.

use devbox_applink::{HandoffClaim, HandoffError, HandoffStore, ToolboxTextPayload};
use serde::Serialize;
use std::sync::{Mutex, MutexGuard};

const INVALID: &str = "텍스트 handoff를 사용할 수 없습니다";
const BUSY: &str = "다른 텍스트 handoff를 먼저 처리하세요";
const EXPIRED: &str = "텍스트 handoff 미리보기가 만료되었습니다. 다시 전달하세요";
const STORAGE: &str = "텍스트 handoff 저장소를 사용할 수 없습니다";

struct ClaimedToolboxText {
    claim: HandoffClaim,
    payload: ToolboxTextPayload,
}

pub struct PendingToolboxText {
    pending: Mutex<Option<ClaimedToolboxText>>,
    store: Option<HandoffStore>,
}

impl PendingToolboxText {
    fn has_claim(&self, id: &str) -> bool {
        self.slot()
            .as_ref()
            .is_some_and(|current| current.claim.envelope.id == id)
    }
    pub fn with_store(store: HandoffStore) -> Self {
        Self {
            store: Some(store),
            ..Self::new()
        }
    }
    fn store(&self) -> HandoffStore {
        self.store.clone().unwrap_or_else(store)
    }
    pub fn new() -> Self {
        Self {
            pending: Mutex::new(None),
            store: None,
        }
    }

    fn slot(&self) -> MutexGuard<'_, Option<ClaimedToolboxText>> {
        self.pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl Default for PendingToolboxText {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolboxTextPreview {
    pub handoff_id: String,
    pub producer_id: String,
    pub expires_at_ms: u64,
    pub text: String,
    pub redacted: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenewToolboxTextResult {
    pub lease_until_ms: u64,
}

#[tauri::command]
pub fn preview_toolbox_text(
    pending: tauri::State<'_, PendingToolboxText>,
    handoff_id: String,
) -> Result<ToolboxTextPreview, String> {
    if !valid_id(&handoff_id) {
        return Err(INVALID.to_string());
    }
    let mut slot = pending.slot();
    if slot.is_some() {
        return Err(BUSY.to_string());
    }
    let store = pending.store();
    let now = now_ms().ok_or_else(|| STORAGE.to_string())?;
    let claim = store
        .claim(
            &handoff_id,
            devbox_applink::TOOLBOX_TEXT_HANDOFF_KIND,
            devbox_applink::TOOLBOX_TEXT_TARGET_APP,
            now,
        )
        .map_err(map_error)?;
    let payload = match ToolboxTextPayload::from_claim(&claim) {
        Ok(payload) => payload,
        Err(_) => {
            let _ = store.restore(&claim, devbox_applink::TOOLBOX_TEXT_TARGET_APP, now);
            return Err(INVALID.to_string());
        }
    };
    let preview = ToolboxTextPreview {
        handoff_id: claim.envelope.id.clone(),
        producer_id: claim.envelope.source_app.clone(),
        expires_at_ms: claim.envelope.expires_at_ms,
        redacted: payload.text.contains("[REDACTED]"),
        text: payload.text.clone(),
    };
    *slot = Some(ClaimedToolboxText { claim, payload });
    Ok(preview)
}

#[tauri::command]
pub fn accept_toolbox_text(
    pending: tauri::State<'_, PendingToolboxText>,
    handoff_id: String,
) -> Result<String, String> {
    let mut slot = pending.slot();
    let current = exact_claim(&slot, &handoff_id)?;
    let text = current.payload.text.clone();
    match pending.store().ack(
        &current.claim,
        devbox_applink::TOOLBOX_TEXT_TARGET_APP,
        now_ms().ok_or_else(|| STORAGE.to_string())?,
    ) {
        Ok(()) => {
            slot.take();
            Ok(text)
        }
        Err(error) => {
            if terminal(&error) {
                slot.take();
            }
            Err(map_error(error))
        }
    }
}

#[tauri::command]
pub fn discard_toolbox_text(
    pending: tauri::State<'_, PendingToolboxText>,
    handoff_id: String,
) -> Result<(), String> {
    let mut slot = pending.slot();
    let current = exact_claim(&slot, &handoff_id)?;
    match pending.store().restore(
        &current.claim,
        devbox_applink::TOOLBOX_TEXT_TARGET_APP,
        now_ms().ok_or_else(|| STORAGE.to_string())?,
    ) {
        Ok(()) => {
            slot.take();
            Ok(())
        }
        Err(error) => {
            if terminal(&error) {
                slot.take();
            }
            Err(map_error(error))
        }
    }
}

#[tauri::command]
pub fn renew_toolbox_text(
    pending: tauri::State<'_, PendingToolboxText>,
    handoff_id: String,
) -> Result<RenewToolboxTextResult, String> {
    let mut slot = pending.slot();
    let current = exact_claim(&slot, &handoff_id)?;
    match pending.store().renew(
        &current.claim,
        devbox_applink::TOOLBOX_TEXT_TARGET_APP,
        now_ms().ok_or_else(|| STORAGE.to_string())?,
        devbox_applink::DEFAULT_CLAIM_LEASE_MS,
    ) {
        Ok(renewed) => {
            let lease_until_ms = renewed.lease_until_ms;
            slot.as_mut().expect("checked pending slot").claim = renewed;
            Ok(RenewToolboxTextResult { lease_until_ms })
        }
        Err(error) => {
            if terminal(&error) {
                slot.take();
            }
            Err(map_error(error))
        }
    }
}

fn exact_claim<'a>(
    slot: &'a Option<ClaimedToolboxText>,
    id: &str,
) -> Result<&'a ClaimedToolboxText, String> {
    if !valid_id(id) {
        return Err(INVALID.to_string());
    }
    let current = slot.as_ref().ok_or_else(|| INVALID.to_string())?;
    if current.claim.envelope.id != id {
        return Err(BUSY.to_string());
    }
    Ok(current)
}

fn store() -> HandoffStore {
    HandoffStore::new(devbox_applink::handoff_root_in(
        &devbox_integration::common_root(),
    ))
}

fn valid_id(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn now_ms() -> Option<u64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
        .filter(|value| *value > 0)
}

fn terminal(error: &HandoffError) -> bool {
    matches!(
        error,
        HandoffError::Expired
            | HandoffError::LeaseExpired
            | HandoffError::Missing
            | HandoffError::Corrupt
            | HandoffError::TokenMismatch
    )
}

fn map_error(error: HandoffError) -> String {
    match error {
        HandoffError::Expired | HandoffError::LeaseExpired | HandoffError::Missing => {
            EXPIRED.to_string()
        }
        HandoffError::AlreadyClaimed => BUSY.to_string(),
        HandoffError::UnsafeStorage | HandoffError::Storage | HandoffError::RandomUnavailable => {
            STORAGE.to_string()
        }
        HandoffError::InvalidRequest
        | HandoffError::InvalidPayload
        | HandoffError::TooLarge
        | HandoffError::WrongTarget
        | HandoffError::WrongKind
        | HandoffError::TokenMismatch
        | HandoffError::Corrupt => INVALID.to_string(),
    }
}

/// Typed product receiver; authorization remains with the native product router.
pub(crate) async fn __component_preview_toolbox_text(
    component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use tauri::Manager as _;
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {
        handoff_id: String,
    }
    let Input { handoff_id } =
        serde_json::from_value(args).map_err(|_| "component_args_invalid".to_owned())?;
    let result = preview_toolbox_text(component_app.state(), handoff_id.clone());
    if !component_app
        .state::<PendingToolboxText>()
        .has_claim(&handoff_id)
    {
        component_app
            .state::<crate::applink::PendingOpen>()
            .release(&handoff_id);
    }
    let value = result?;
    serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
}

/// Typed product receiver; authorization remains with the native product router.
pub(crate) async fn __component_accept_toolbox_text(
    component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use tauri::Manager as _;
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {
        handoff_id: String,
    }
    let Input { handoff_id } =
        serde_json::from_value(args).map_err(|_| "component_args_invalid".to_owned())?;
    let result = accept_toolbox_text(component_app.state(), handoff_id.clone());
    if !component_app
        .state::<PendingToolboxText>()
        .has_claim(&handoff_id)
    {
        component_app
            .state::<crate::applink::PendingOpen>()
            .release(&handoff_id);
    }
    let value = result?;
    serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
}

/// Typed product receiver; authorization remains with the native product router.
pub(crate) async fn __component_discard_toolbox_text(
    component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use tauri::Manager as _;
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {
        handoff_id: String,
    }
    let Input { handoff_id } =
        serde_json::from_value(args).map_err(|_| "component_args_invalid".to_owned())?;
    let result = discard_toolbox_text(component_app.state(), handoff_id.clone());
    if !component_app
        .state::<PendingToolboxText>()
        .has_claim(&handoff_id)
    {
        component_app
            .state::<crate::applink::PendingOpen>()
            .release(&handoff_id);
    }
    result?;
    serde_json::to_value(()).map_err(|_| "component_response_invalid".to_owned())
}

/// Typed product receiver; authorization remains with the native product router.
pub(crate) async fn __component_renew_toolbox_text(
    component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use tauri::Manager as _;
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {
        handoff_id: String,
    }
    let Input { handoff_id } =
        serde_json::from_value(args).map_err(|_| "component_args_invalid".to_owned())?;
    let result = renew_toolbox_text(component_app.state(), handoff_id.clone());
    if !component_app
        .state::<PendingToolboxText>()
        .has_claim(&handoff_id)
    {
        component_app
            .state::<crate::applink::PendingOpen>()
            .release(&handoff_id);
    }
    let value = result?;
    serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_errors_are_fixed_and_do_not_echo_input() {
        assert!(valid_id(&"a".repeat(32)));
        assert!(!valid_id("../secret"));
        assert_eq!(map_error(HandoffError::AlreadyClaimed), BUSY);
        assert_eq!(map_error(HandoffError::LeaseExpired), EXPIRED);
        assert_eq!(map_error(HandoffError::InvalidPayload), INVALID);
    }
}
