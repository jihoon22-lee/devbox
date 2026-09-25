use devbox_applink::{
    handoff_root_in, CreateHandoff, HandoffStore, OpenRequest, ToolboxTextPayload,
    TOOLBOX_TEXT_HANDOFF_KIND, TOOLBOX_TEXT_TARGET_APP,
};
use serde::Serialize;
use zeroize::Zeroizing;

const SOURCE_APP: &str = "api-playground";
const INVALID_SELECTION: &str = "Developer Toolbox로 보낼 선택 영역이 유효하지 않습니다";
const TARGET_UNAVAILABLE: &str =
    "Developer Toolbox를 사용할 수 없습니다. 클립보드로 자동 전환하지 않습니다";
const DELIVERY_FAILED: &str =
    "Developer Toolbox로 선택 영역을 전달하지 못했습니다. 클립보드로 자동 전환하지 않습니다";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolboxDispatch {
    pub handoff_id: String,
    pub redacted: bool,
}

/// Send only the explicit masked response selection.  Raw response headers,
/// cookies, and binary vault bytes are not reachable through this command.
#[tauri::command]
// The standalone AppLink entry point is not registered by the product adapter.
#[allow(dead_code)]
pub fn send_selection_to_toolbox(text: String) -> Result<ToolboxDispatch, String> {
    let _ = (text,);
    Err(TARGET_UNAVAILABLE.into())
}

fn now_ms() -> Option<u64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
        .filter(|value| *value > 0)
}
