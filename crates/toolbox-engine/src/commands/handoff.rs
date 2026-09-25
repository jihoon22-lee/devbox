use serde::Serialize;

pub const API_TARGET_UNAVAILABLE_ERROR: &str =
    "API Playground를 사용할 수 없습니다. 설치 또는 업데이트 후 다시 시도하세요. 클립보드로 자동 전환하지 않습니다";
pub const KNOWLEDGE_TARGET_UNAVAILABLE_ERROR: &str =
    "Knowledge를 사용할 수 없습니다. 설치 또는 업데이트 후 다시 시도하세요. 클립보드로 자동 전환하지 않습니다";

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
// The standalone AppLink entry point is not registered by the product adapter.
#[allow(dead_code)]
pub fn create_api_request_handoff(output: String) -> Result<ApiHandoffDispatch, String> {
    let _ = (output,);
    Err(API_TARGET_UNAVAILABLE_ERROR.into())
}

/// Publish the current visible transform result as a strict Knowledge draft.
/// The consumer still previews and explicitly saves it; this command never
/// writes a note or falls back to clipboard transport.
// The standalone AppLink entry point is not registered by the product adapter.
#[allow(dead_code)]
pub fn create_knowledge_draft_handoff(output: String) -> Result<KnowledgeDraftDispatch, String> {
    let _ = (output,);
    Err(KNOWLEDGE_TARGET_UNAVAILABLE_ERROR.into())
}
