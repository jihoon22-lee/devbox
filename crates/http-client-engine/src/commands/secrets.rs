//! secret 변수 봉인/해제 명령. 봉인 결과는 base64로 반환해 프론트가 저장한다.

use crate::platform::platform_sealer;
use base64::{engine::general_purpose::STANDARD as B64, Engine};

/// 값을 봉인해 base64 문자열로 반환한다.
#[tauri::command]
pub fn seal_secret(value: String) -> Result<String, String> {
    let sealer = platform_sealer();
    let blob = devbox_secrets::seal_v1(sealer.as_ref(), &value).map_err(|e| e.to_string())?;
    Ok(B64.encode(blob))
}

/// Typed product adapter; the caller owns component/session authorization.
pub(crate) async fn __component_seal_secret(
    _component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {
        value: String,
    }
    let Input { value } =
        serde_json::from_value(args).map_err(|_| "component_args_invalid".to_owned())?;
    let value = seal_secret(value)?;
    serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
}
