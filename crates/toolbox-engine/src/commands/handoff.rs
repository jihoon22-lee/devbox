use crate::core::handoff::{
    build_api_request_payload, API_REQUEST_HANDOFF_KIND, CONSUMER_APP_ID, HANDOFF_INPUT_ERROR,
    PRODUCER_APP_ID,
};
use devbox_applink::{handoff_root_in, CreateHandoff, HandoffError, HandoffStore, OpenRequest};
use serde::Serialize;
use zeroize::Zeroizing;

pub const API_TARGET_UNAVAILABLE_ERROR: &str =
    "API Playground를 사용할 수 없습니다. 설치 또는 업데이트 후 다시 시도하세요. 클립보드로 자동 전환하지 않습니다";
pub const HANDOFF_CREATE_ERROR: &str =
    "API Playground handoff를 만들지 못했습니다. 클립보드로 자동 전환하지 않습니다";
pub const API_LAUNCH_ERROR: &str =
    "API Playground를 실행하지 못했습니다. 전달 데이터는 폐기했습니다. 클립보드로 자동 전환하지 않습니다";
pub const KNOWLEDGE_TARGET_UNAVAILABLE_ERROR: &str =
    "Knowledge를 사용할 수 없습니다. 설치 또는 업데이트 후 다시 시도하세요. 클립보드로 자동 전환하지 않습니다";
pub const KNOWLEDGE_HANDOFF_ERROR: &str =
    "Knowledge draft를 만들거나 전달하지 못했습니다. 클립보드로 자동 전환하지 않습니다";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiHandoffDispatch {
    pub handoff_id: String,
    pub producer_id: String,
    pub consumer_id: String,
    pub created_at_ms: u64,
    pub expires_at_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeDraftDispatch {
    pub handoff_id: String,
    pub redacted: bool,
}

/// Publish the current visible output as a one-time API Playground request.
///
/// The payload is bounded and validated by the shared store before it is
/// written. Launch failure revokes an envelope that is still pending; there
/// is no clipboard or alternate channel.
#[tauri::command]
// The standalone AppLink entry point is not registered by the product adapter.
#[allow(dead_code)]
pub fn create_api_request_handoff(output: String) -> Result<ApiHandoffDispatch, String> {
    let _ = (output,);
    Err(API_TARGET_UNAVAILABLE_ERROR.into())
}

/// Publish the current visible transform result as a strict Knowledge draft.
/// The consumer still previews and explicitly saves it; this command never
/// writes a note or falls back to clipboard transport.
#[tauri::command]
// The standalone AppLink entry point is not registered by the product adapter.
#[allow(dead_code)]
pub fn create_knowledge_draft_handoff(output: String) -> Result<KnowledgeDraftDispatch, String> {
    let _ = (output,);
    Err(KNOWLEDGE_TARGET_UNAVAILABLE_ERROR.into())
}

fn map_handoff_create_error(error: HandoffError) -> String {
    match error {
        HandoffError::InvalidPayload | HandoffError::InvalidRequest | HandoffError::TooLarge => {
            HANDOFF_INPUT_ERROR.to_string()
        }
        HandoffError::UnsafeStorage | HandoffError::Storage | HandoffError::RandomUnavailable => {
            HANDOFF_CREATE_ERROR.to_string()
        }
        HandoffError::Missing
        | HandoffError::AlreadyClaimed
        | HandoffError::WrongTarget
        | HandoffError::WrongKind
        | HandoffError::Expired
        | HandoffError::LeaseExpired
        | HandoffError::TokenMismatch
        | HandoffError::Corrupt => HANDOFF_CREATE_ERROR.to_string(),
    }
}

fn handoff_now_ms() -> Option<u64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
        .filter(|now| *now > 0)
}
