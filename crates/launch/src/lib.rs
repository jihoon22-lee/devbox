//! 설치된 devbox 앱 실행.
//!
//! Devbox Manager의 설치 layout 규약을 단일 원본으로 둔다.
//!
//! ```text
//! %LOCALAPPDATA%\com.devbox.devboxmanager\
//! └─ apps/<app-id>/
//!    ├─ versions/<version>/<app-id>.exe
//!    └─ current.json   { "exePath": "...", "version": "...", ... }
//! ```
//!
//! 추출 근거: repo-manager(`open_in`)와 workbench(Start Workspace)가 같은 "설치된
//! 앱 실행" 규칙을 필요로 한다. 이전에는 `Command::new("<ProductName>.exe")`로 잘못된
//! 이름을 하드코딩해 portable 설치(`<app-id>.exe`)를 실행하지 못했다.
//!
//! 제약: 순수 경로/프로세스 로직만 담는다. Manager의 install/update/rollback 정책은
//! Manager 앱이 소유한다.

use std::path::{Path, PathBuf};

const MANAGER_ID: &str = "com.devbox.devboxmanager";

/// Manager 설치 base. Windows 전용 규약이므로 비-Windows에서는 `None`이다.
pub fn manager_base() -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")?;
    Some(PathBuf::from(base).join(MANAGER_ID))
}

/// 설치된 앱의 현재 exe 경로를 찾는다.
///
/// 1. `current.json`의 `exePath`를 우선 사용 (Manager가 기록한 실제 경로).
/// 2. 없으면 `versions/<최신>/<app-id>.exe`로 폴백한다.
pub fn resolve_installed(app_id: &str) -> Option<PathBuf> {
    let base = manager_base()?;
    let app_dir = base.join("apps").join(app_id);

    if let Some(exe) = current_exe(&app_dir) {
        if exe.exists() {
            return Some(exe);
        }
    }
    latest_version_exe(&app_dir, app_id)
}

fn current_exe(app_dir: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(app_dir.join("current.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let p = v.get("exePath")?.as_str()?;
    Some(PathBuf::from(p))
}

fn latest_version_exe(app_dir: &Path, app_id: &str) -> Option<PathBuf> {
    let versions = app_dir.join("versions");
    let mut best: Option<PathBuf> = None;
    for entry in std::fs::read_dir(versions).ok()?.flatten() {
        let exe = entry.path().join(format!("{app_id}.exe"));
        if !exe.exists() {
            continue;
        }
        let better = best
            .as_ref()
            .map_or(true, |b| exe.to_string_lossy() > b.to_string_lossy());
        if better {
            best = Some(exe);
        }
    }
    best
}

/// 설치된 앱을 실행하고 자식 pid를 반환한다.
pub fn launch(app_id: &str, args: &[&str]) -> Result<u32, String> {
    let exe = resolve_installed(app_id)
        .ok_or_else(|| format!("{app_id} 설치 없음 — Devbox Manager에서 먼저 설치하세요"))?;
    std::process::Command::new(&exe)
        .args(args)
        .spawn()
        .map(|c| c.id())
        .map_err(|e| format!("{} 실행 실패: {e}", exe.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latest_version_exe_prefers_higher_version() {
        // 실제 파일 생성 없이는 존재 검사 때문에 빈 결과가 나온다. 경로 계산만
        // 검증할 수 있도록 임시 디렉터리를 쓴다.
        let tmp = std::env::temp_dir().join(format!("launch-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("versions/0.2.0")).unwrap();
        std::fs::create_dir_all(tmp.join("versions/0.3.0")).unwrap();
        std::fs::write(tmp.join("versions/0.3.0/test-app.exe"), b"").unwrap();
        std::fs::write(tmp.join("versions/0.2.0/test-app.exe"), b"").unwrap();

        let got = latest_version_exe(&tmp, "test-app").unwrap();
        assert!(got.to_string_lossy().contains("0.3.0"));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn current_exe_prefers_current_json() {
        let tmp = std::env::temp_dir().join(format!("launch-current-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let exe = tmp.join("versions/0.2.0/test-app.exe");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, b"").unwrap();
        std::fs::write(
            tmp.join("current.json"),
            format!(r#"{{"exePath": "{}", "version": "0.2.0"}}"#, exe.display()),
        )
        .unwrap();

        assert_eq!(current_exe(&tmp).unwrap(), exe);

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
