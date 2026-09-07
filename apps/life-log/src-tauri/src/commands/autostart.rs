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
#[cfg(all(target_os = "windows", feature = "standalone"))]
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
#[cfg(feature = "standalone")]
#[tauri::command]
pub fn autostart_status() -> AutostartStatus {
    #[cfg(target_os = "windows")]
    {
        match read_value(VALUE_NAME) {
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
#[cfg(feature = "standalone")]
#[tauri::command]
pub fn set_autostart(enabled: bool) -> Result<AutostartStatus, String> {
    #[cfg(target_os = "windows")]
    {
        if enabled {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            let command = format!("\"{}\"", exe.display());
            set_value(VALUE_NAME, &command)?;
        } else {
            delete_value(VALUE_NAME)?;
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

#[cfg(all(target_os = "windows", feature = "standalone"))]
fn read_value(name: &str) -> Result<Option<String>, String> {
    use winreg::enums::KEY_READ;
    let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
    let key = hkcu
        .open_subkey_with_flags(RUN_KEY, KEY_READ)
        .map_err(|e| e.to_string())?;
    match key.get_value::<String, _>(name) {
        Ok(command) => Ok(Some(command)),
        Err(_) => Ok(None),
    }
}

#[cfg(target_os = "windows")]
fn set_value(name: &str, command: &str) -> Result<(), String> {
    open_run_key()?
        .set_value(name, &command)
        .map_err(|e| e.to_string())
}

#[cfg(target_os = "windows")]
fn delete_value(name: &str) -> Result<(), String> {
    open_run_key()?
        .delete_value(name)
        .map_err(|e| e.to_string())
}

#[cfg(any(target_os = "windows", test))]
fn product_value_name(identifier: &str) -> Result<String, String> {
    let installation = identifier
        .strip_prefix("com.devbox.v08.knowledge.i")
        .filter(|value| {
            value.len() == 64
                && value
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
        .ok_or("autostart_owner_invalid")?;
    Ok(format!("DevboxKnowledge-{installation}"))
}

#[cfg(target_os = "windows")]
fn product_owner(app: &tauri::AppHandle) -> Result<(String, String), String> {
    let name = product_value_name(&app.config().identifier)?;
    let executable = std::env::current_exe().map_err(|_| "autostart_unavailable")?;
    Ok((name, format!("\"{}\"", executable.display())))
}

#[cfg(target_os = "windows")]
fn owned_value(name: &str, expected: &str) -> Result<Option<String>, String> {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};
    let key =
        match winreg::RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(RUN_KEY, KEY_READ) {
            Ok(key) => key,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err("autostart_unavailable".into()),
        };
    match key.get_value::<String, _>(name) {
        Ok(value) if value == expected => Ok(Some(value)),
        Ok(_) => Err("autostart_owner_conflict".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("autostart_unavailable".into()),
    }
}

fn product_status(_app: &tauri::AppHandle) -> Result<AutostartStatus, String> {
    #[cfg(target_os = "windows")]
    {
        let (name, expected) = product_owner(_app)?;
        let command = owned_value(&name, &expected)?;
        Ok(AutostartStatus {
            supported: true,
            enabled: command.is_some(),
            command,
        })
    }
    #[cfg(not(target_os = "windows"))]
    Ok(AutostartStatus {
        supported: false,
        enabled: false,
        command: None,
    })
}

fn set_product_autostart(app: &tauri::AppHandle, enabled: bool) -> Result<AutostartStatus, String> {
    #[cfg(target_os = "windows")]
    {
        let (name, expected) = product_owner(app)?;
        let existing = owned_value(&name, &expected)?;
        if enabled {
            set_value(&name, &expected)?;
        } else if existing.is_some() {
            delete_value(&name)?;
        }
    }
    #[cfg(not(target_os = "windows"))]
    let _ = enabled;
    product_status(app)
}

/// Typed product adapter; the caller enforces native owner/session authorization.
pub(crate) async fn __component_autostart_status(
    component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {}
    let Input {} = serde_json::from_value(args).map_err(|_| "component_args_invalid".to_owned())?;
    let value = product_status(component_app)?;
    serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
}

/// Typed product adapter; the caller enforces native owner/session authorization.
pub(crate) async fn __component_set_autostart(
    component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {
        enabled: bool,
    }
    let Input { enabled } =
        serde_json::from_value(args).map_err(|_| "component_args_invalid".to_owned())?;
    let value = set_product_autostart(component_app, enabled)?;
    serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
}

#[cfg(test)]
mod tests {
    #[test]
    fn product_autostart_names_cannot_target_legacy_or_another_product() {
        let a = super::product_value_name(&format!("com.devbox.v08.knowledge.i{}", "a".repeat(64)))
            .unwrap();
        let b = super::product_value_name(&format!("com.devbox.v08.knowledge.i{}", "b".repeat(64)))
            .unwrap();
        assert_ne!(a, b);
        assert_ne!(a, "LifeLog");
        for invalid in ["com.devbox.lifelog", "com.devbox.v08.knowledge", "com.devbox.v08.knowledge.i../LifeLog", "com.devbox.v08.api-studio.iaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"] {
            assert!(super::product_value_name(invalid).is_err());
        }
    }
}
