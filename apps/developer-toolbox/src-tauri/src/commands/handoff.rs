use crate::core::handoff::{
    build_api_request_payload, API_REQUEST_HANDOFF_KIND, CONSUMER_APP_ID, HANDOFF_INPUT_ERROR,
    PRODUCER_APP_ID,
};
use devbox_applink::{handoff_root_in, CreateHandoff, HandoffError, HandoffStore, OpenRequest};
use serde::Serialize;

pub const API_TARGET_UNAVAILABLE_ERROR: &str =
    "API Playground를 사용할 수 없습니다. 설치 또는 업데이트 후 다시 시도하세요. 클립보드로 자동 전환하지 않습니다";
pub const HANDOFF_CREATE_ERROR: &str =
    "API Playground handoff를 만들지 못했습니다. 클립보드로 자동 전환하지 않습니다";
pub const API_LAUNCH_ERROR: &str =
    "API Playground를 실행하지 못했습니다. 전달 데이터는 폐기했습니다. 클립보드로 자동 전환하지 않습니다";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiHandoffDispatch {
    pub handoff_id: String,
    pub producer_id: String,
    pub consumer_id: String,
    pub created_at_ms: u64,
    pub expires_at_ms: u64,
}

/// Publish the current visible output as a one-time API Playground request.
///
/// The payload is bounded and validated by the shared store before it is
/// written. Launch failure revokes an envelope that is still pending; there
/// is no clipboard or alternate channel.
#[tauri::command]
pub fn create_api_request_handoff(output: String) -> Result<ApiHandoffDispatch, String> {
    let output = zeroize::Zeroizing::new(output);
    let payload =
        build_api_request_payload(output.as_str()).map_err(|_| HANDOFF_INPUT_ERROR.to_string())?;
    let target_available =
        devbox_launch::installed_targets(&format!("handoff:{API_REQUEST_HANDOFF_KIND}"))
            .into_iter()
            .any(|target| target.id == CONSUMER_APP_ID);
    if !target_available {
        return Err(API_TARGET_UNAVAILABLE_ERROR.to_string());
    }

    let created_at_ms = handoff_now_ms().ok_or_else(|| HANDOFF_CREATE_ERROR.to_string())?;
    let expires_at_ms = created_at_ms
        .checked_add(devbox_applink::DEFAULT_HANDOFF_TTL_MS)
        .ok_or_else(|| HANDOFF_CREATE_ERROR.to_string())?;
    let store = HandoffStore::new(handoff_root_in(&devbox_integration::common_root()));
    let descriptor = store
        .create(
            CreateHandoff {
                kind: API_REQUEST_HANDOFF_KIND.to_string(),
                source_app: PRODUCER_APP_ID.to_string(),
                target_app: Some(CONSUMER_APP_ID.to_string()),
                payload: serde_json::to_value(payload)
                    .map_err(|_| HANDOFF_CREATE_ERROR.to_string())?,
            },
            created_at_ms,
        )
        .map_err(map_handoff_create_error)?;
    let request = OpenRequest {
        target: descriptor.clone().into(),
        from: Some(PRODUCER_APP_ID.to_string()),
    };
    if devbox_launch::launch_open(CONSUMER_APP_ID, &request).is_err() {
        let _ = store.revoke_pending(&descriptor, PRODUCER_APP_ID);
        return Err(API_LAUNCH_ERROR.to_string());
    }

    Ok(ApiHandoffDispatch {
        handoff_id: descriptor.id,
        producer_id: PRODUCER_APP_ID.to_string(),
        consumer_id: CONSUMER_APP_ID.to_string(),
        created_at_ms,
        expires_at_ms,
    })
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
