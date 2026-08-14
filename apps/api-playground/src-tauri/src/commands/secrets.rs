//! secret 변수 봉인/해제 명령. 봉인 결과는 base64로 반환해 프론트가 저장한다.

use crate::platform::platform_sealer;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use zeroize::Zeroizing;

/// 값을 봉인해 base64 문자열로 반환한다.
#[tauri::command]
pub fn seal_secret(value: String) -> Result<String, String> {
    let sealer = platform_sealer();
    let blob = devbox_secrets::seal_v1(sealer.as_ref(), &value).map_err(|e| e.to_string())?;
    Ok(B64.encode(blob))
}

/// base64 봉인 blob을 해제한다. 오류 메시지에 평문을 노출하지 않는다.
#[tauri::command]
pub fn unseal_secret(blob_b64: String) -> Result<String, String> {
    let blob = B64
        .decode(blob_b64)
        .map_err(|_| "봉인 형식이 올바르지 않습니다".to_string())?;
    let sealer = platform_sealer();
    let plaintext: Zeroizing<String> = devbox_secrets::unseal_v1(sealer.as_ref(), &blob)
        .map_err(|_| "secret을 해제할 수 없습니다".to_string())?;
    Ok(plaintext.to_string())
}
