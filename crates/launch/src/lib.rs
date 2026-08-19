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
//!
//! `crates/applink`에 의존한다 — 발신 앱이 argv를 문자열 리터럴로 손수 조립하면
//! 수신측(`crates/applink::parse_argv`)과 포맷이 조용히 어긋날 수 있으므로,
//! [`launch_open`]이 `devbox_applink::build_argv`를 거쳐 그 어긋남을 구조적으로
//! 막는다. 수신 앱 13개는 이 크레이트에 의존하지 않는다 — 계약만 필요하지 설치
//! 경로 해석은 필요 없기 때문이다
//! (`docs/superpowers/specs/2026-08-17-app-interop-design.md` §1.1, §7 #3).

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
    let mut best: Option<(Vec<u32>, PathBuf)> = None;
    for entry in std::fs::read_dir(versions).ok()?.flatten() {
        let exe = entry.path().join(format!("{app_id}.exe"));
        if !exe.exists() {
            continue;
        }
        let key = version_sort_key(&entry.file_name().to_string_lossy());
        let better = best.as_ref().is_none_or(|(best_key, _)| key > *best_key);
        if better {
            best = Some((key, exe));
        }
    }
    best.map(|(_, exe)| exe)
}

/// 버전 디렉터리 이름("0.9.0", "0.10.0")을 숫자 세그먼트로 분해한다. 이전 구현은 exe
/// 경로 문자열을 그대로 비교해 "0.10.0" < "0.9.0"으로 잘못 판정했다 (사전순으로는 '1' <
/// '9'). 파싱 실패한 세그먼트는 0으로 취급해 이름이 예상과 다르더라도 패닉하지 않는다.
fn version_sort_key(name: &str) -> Vec<u32> {
    name.split('.').map(|s| s.parse().unwrap_or(0)).collect()
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

/// `OpenRequest`를 `devbox_applink::build_argv`로 argv에 인코딩한 뒤 실행한다.
///
/// 발신 앱(repo-manager·workbench 등)이 argv를 직접 문자열 리터럴로 조립하던 것을
/// 대체한다 — 계약(`crates/applink`)과 실행(`crates/launch`)이 갈라져 있으면
/// 발신측과 수신측 argv 포맷이 조용히 어긋날 수 있으므로, 한쪽 진실 원본에서 뽑아
/// 쓰도록 강제한다
/// (`docs/superpowers/specs/2026-08-17-app-interop-design.md` §7 #3).
///
/// argv 없이 실행하고 싶은 호출자(요청이 없는 경우)는 그대로 [`launch`]를 쓴다.
pub fn launch_open(app_id: &str, req: &devbox_applink::OpenRequest) -> Result<u32, String> {
    let argv = open_argv(req);
    let args: Vec<&str> = argv.iter().map(String::as_str).collect();
    launch(app_id, &args)
}

/// [`launch_open`]에서 프로세스 spawn 없이 argv 구성만 떼어낸 부분. 테스트가 실제
/// 프로세스를 띄우지 않고도 각 호출부가 만드는 argv를 단언할 수 있도록 공개한다.
pub fn open_argv(req: &devbox_applink::OpenRequest) -> Vec<String> {
    devbox_applink::build_argv(req)
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

    /// 회귀 테스트: 경로 문자열을 그대로 비교하면 "0.10.0" < "0.9.0"으로 잘못 판정된다
    /// (사전순 비교에서 '1' < '9'). semver 세그먼트 비교로 고쳐야 한다.
    #[test]
    fn latest_version_exe_compares_numerically_not_lexically() {
        let tmp = std::env::temp_dir().join(format!("launch-test-numeric-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("versions/0.9.0")).unwrap();
        std::fs::create_dir_all(tmp.join("versions/0.10.0")).unwrap();
        std::fs::write(tmp.join("versions/0.9.0/test-app.exe"), b"").unwrap();
        std::fs::write(tmp.join("versions/0.10.0/test-app.exe"), b"").unwrap();

        let got = latest_version_exe(&tmp, "test-app").unwrap();
        assert!(
            got.to_string_lossy().contains("0.10.0"),
            "0.10.0이 0.9.0보다 최신이어야 하는데 {got:?}가 선택됨"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn version_sort_key_orders_numerically() {
        assert!(version_sort_key("0.10.0") > version_sort_key("0.9.0"));
        assert!(version_sort_key("1.0.0") > version_sort_key("0.99.99"));
        assert_eq!(version_sort_key("0.3.1"), vec![0, 3, 1]);
    }

    #[test]
    fn version_sort_key_does_not_panic_on_malformed_input() {
        assert_eq!(version_sort_key("not-a-version"), vec![0]);
        assert_eq!(version_sort_key(""), vec![0]);
        assert_eq!(version_sort_key("1.x.0"), vec![1, 0, 0]);
    }

    // ---- open_argv: 세 호출부의 요청 모양이 만드는 argv (실제 spawn 없이 검증) ----

    /// repo-manager `open_in` → `OpenTarget::Path`.
    #[test]
    fn open_argv_repo_manager_path_request() {
        let req = devbox_applink::OpenRequest {
            target: devbox_applink::OpenTarget::Path {
                path: "/repos/foo".to_string(),
                line: None,
                column: None,
            },
            from: Some("repo-manager".to_string()),
        };
        assert_eq!(
            open_argv(&req),
            vec!["--path", "/repos/foo", "--from", "repo-manager"]
        );
    }

    /// workbench → wsl-desktop → `OpenTarget::Profile`.
    #[test]
    fn open_argv_workbench_wsl_desktop_profile_request() {
        let req = devbox_applink::OpenRequest {
            target: devbox_applink::OpenTarget::Profile {
                id: "prof-1".to_string(),
            },
            from: Some("workbench".to_string()),
        };
        assert_eq!(
            open_argv(&req),
            vec!["--profile", "prof-1", "--from", "workbench"]
        );
    }

    /// workbench → code-pad → `OpenTarget::Workspace`.
    #[test]
    fn open_argv_workbench_code_pad_workspace_request() {
        let req = devbox_applink::OpenRequest {
            target: devbox_applink::OpenTarget::Workspace {
                path: "C:\\ws\\proj".to_string(),
            },
            from: Some("workbench".to_string()),
        };
        assert_eq!(
            open_argv(&req),
            vec!["--workspace", "C:\\ws\\proj", "--from", "workbench"]
        );
    }

    #[test]
    fn current_exe_prefers_current_json() {
        let tmp = std::env::temp_dir().join(format!("launch-current-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let exe = tmp.join("versions/0.2.0/test-app.exe");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, b"").unwrap();
        let json = serde_json::json!({
            "exePath": exe.to_string_lossy(),
            "version": "0.2.0",
        });
        std::fs::write(tmp.join("current.json"), json.to_string()).unwrap();

        assert_eq!(current_exe(&tmp).unwrap(), exe);

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
