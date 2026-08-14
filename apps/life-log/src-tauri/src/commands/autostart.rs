//! 시작프로그램 등록 (자동 시작) 상태 확인과 토글.
//!
//! [설계] 등록 방식은 **레지스트리 Run 키**(HKCU\Software\Microsoft\Windows\CurrentVersion\Run)를
//! 쓴다. run-manager는 시작 폴더 .lnk(Windows Shell COM)를 쓰지만, Windows 전용 코드는
//! `crates/`에 둘 수 없어(CONVENTIONS §4) 둘을 공유할 크레이트가 마련되지 않았다.
//! 트레이 앱이 조용히 시작되는 용도에는 Run 키가 더 간결하고 충분하다.
//! 세 번째 소비자가 생기면 CONVENTIONS 예외 또는 순수/플랫폼 분리를 검토한다.

use serde::Serialize;

#[cfg(target_os = "windows")]
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
#[cfg(target_os = "windows")]
const VALUE_NAME: &str = "LifeLog";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutostartStatus {
    pub supported: bool,
    pub enabled: bool,
    /// 등록된 명령줄 (있으면)
    pub command: Option<String>,
}

/// 현재 자동 시작 등록 상태.
#[tauri::command]
pub fn autostart_status() -> AutostartStatus {
    #[cfg(target_os = "windows")]
    {
        match read_value() {
            Ok(Some(command)) => AutostartStatus {
                supported: true,
                enabled: true,
                command: Some(command),
            },
            Ok(None) => AutostartStatus {
                supported: true,
                enabled: false,
                command: None,
            },
            Err(_) => AutostartStatus {
                supported: true,
                enabled: false,
                command: None,
            },
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        AutostartStatus {
            supported: false,
            enabled: false,
            command: None,
        }
    }
}

/// 자동 시작 등록/해제를 되돌릴 수 있게 토글한다.
#[tauri::command]
pub fn set_autostart(enabled: bool) -> Result<AutostartStatus, String> {
    #[cfg(target_os = "windows")]
    {
        if enabled {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            let command = format!("\"{}\"", exe.display());
            set_value(&command)?;
        } else {
            delete_value()?;
        }
        Ok(autostart_status())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = enabled;
        Ok(AutostartStatus {
            supported: false,
            enabled: false,
            command: None,
        })
    }
}

#[cfg(target_os = "windows")]
fn open_run_key() -> Result<winreg::RegKey, String> {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE};
    let hkcu = winreg::RegKey::predef(HKEY_CURRENT_USER);
    hkcu.open_subkey_with_flags(RUN_KEY, KEY_READ | KEY_SET_VALUE)
        .map_err(|e| format!("Run 키 열기 실패: {e}"))
}

#[cfg(target_os = "windows")]
fn read_value() -> Result<Option<String>, String> {
    use winreg::enums::KEY_READ;
    let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
    let key = hkcu
        .open_subkey_with_flags(RUN_KEY, KEY_READ)
        .map_err(|e| e.to_string())?;
    match key.get_value::<String, _>(VALUE_NAME) {
        Ok(command) => Ok(Some(command)),
        Err(_) => Ok(None),
    }
}

#[cfg(target_os = "windows")]
fn set_value(command: &str) -> Result<(), String> {
    open_run_key()?
        .set_value(VALUE_NAME, &command)
        .map_err(|e| e.to_string())
}

#[cfg(target_os = "windows")]
fn delete_value() -> Result<(), String> {
    open_run_key()?
        .delete_value(VALUE_NAME)
        .map_err(|e| e.to_string())
}
