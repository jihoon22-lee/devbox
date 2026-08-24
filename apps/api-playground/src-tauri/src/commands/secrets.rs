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
