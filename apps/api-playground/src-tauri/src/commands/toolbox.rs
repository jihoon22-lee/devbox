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
pub fn send_selection_to_toolbox(text: String) -> Result<ToolboxDispatch, String> {
    let text = Zeroizing::new(text);
    let (payload, redacted) = ToolboxTextPayload::from_selected_text(SOURCE_APP, text.as_str())
        .map_err(|_| INVALID_SELECTION.to_string())?;
    if !devbox_launch::installed_targets(&format!("handoff:{TOOLBOX_TEXT_HANDOFF_KIND}"))
        .into_iter()
        .any(|target| target.id == TOOLBOX_TEXT_TARGET_APP)
    {
        return Err(TARGET_UNAVAILABLE.to_string());
    }

    let now = now_ms().ok_or_else(|| DELIVERY_FAILED.to_string())?;
    let store = HandoffStore::new(handoff_root_in(&devbox_integration::common_root()));
    let descriptor = store
        .create(
            CreateHandoff {
                kind: TOOLBOX_TEXT_HANDOFF_KIND.to_string(),
                source_app: SOURCE_APP.to_string(),
                target_app: Some(TOOLBOX_TEXT_TARGET_APP.to_string()),
                payload: serde_json::to_value(payload).map_err(|_| DELIVERY_FAILED.to_string())?,
            },
            now,
        )
        .map_err(|_| DELIVERY_FAILED.to_string())?;
    let request = OpenRequest {
        target: descriptor.clone().into(),
        from: Some(SOURCE_APP.to_string()),
    };
    if devbox_launch::launch_open(TOOLBOX_TEXT_TARGET_APP, &request).is_err() {
        let _ = store.revoke_pending(&descriptor, SOURCE_APP);
        return Err(DELIVERY_FAILED.to_string());
    }

    Ok(ToolboxDispatch {
        handoff_id: descriptor.id,
        redacted,
    })
}

fn now_ms() -> Option<u64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
        .filter(|value| *value > 0)
}
