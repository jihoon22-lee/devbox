use serde::Serialize;

const TARGET_UNAVAILABLE: &str =
    "Developer Toolbox를 사용할 수 없습니다. 클립보드로 자동 전환하지 않습니다";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolboxDispatch {
    pub handoff_id: String,
    pub redacted: bool,
}

/// Send only the explicit masked response selection.  Raw response headers,
/// cookies, and binary vault bytes are not reachable through this command.
// The standalone AppLink entry point is not registered by the product adapter.
#[allow(dead_code)]
pub fn send_selection_to_toolbox(text: String) -> Result<ToolboxDispatch, String> {
    let _ = (text,);
    Err(TARGET_UNAVAILABLE.into())
}
