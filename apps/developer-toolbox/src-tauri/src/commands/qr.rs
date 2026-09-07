use crate::core::qr::{generate, GenerateQrRequest, QrResult};

/// Generate a QR symbol and bounded SVG/PNG exports.
#[tauri::command]
pub fn generate_qr(request: GenerateQrRequest) -> Result<QrResult, String> {
    generate(request)
}

/// Typed product adapter; the caller owns component/session authorization.
pub(crate) async fn __component_generate_qr(
    _component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {
        request: GenerateQrRequest,
    }
    let Input { request } =
        serde_json::from_value(args).map_err(|_| "component_args_invalid".to_owned())?;
    let value = generate_qr(request)?;
    serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
}
