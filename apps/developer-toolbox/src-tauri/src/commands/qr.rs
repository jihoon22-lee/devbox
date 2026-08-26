use crate::core::qr::{generate, GenerateQrRequest, QrResult};

/// Generate a QR symbol and bounded SVG/PNG exports.
#[tauri::command]
pub fn generate_qr(request: GenerateQrRequest) -> Result<QrResult, String> {
    generate(request)
}
