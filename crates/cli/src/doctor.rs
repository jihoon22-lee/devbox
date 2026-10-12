#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn versions_are_validated() {
        for v in ["tmux 3.2", "tmux 3.3a", "tmux next-3.6"] {
            assert!(tmux_at_least(v, (3, 2)));
        }
        for v in ["tmux 3", "garbage 9.9", "tmux 2.9", "tmux 3.x"] {
            assert!(!tmux_at_least(v, (3, 2)));
        }
    }
}

use devbox_protocol::system::{Check, DoctorReport, Status};
pub trait Probe {
    fn pid1(&self) -> String;
    fn user_manager_available(&self) -> bool;
    fn linger(&self) -> String;
    fn tmux_version(&self) -> Option<String>;
    fn inotify_instances(&self) -> String;
    fn env_file(&self) -> Option<String>;
    fn which(&self, tool: &str, path: &str) -> bool;
}
pub fn tmux_at_least(v: &str, want: (u32, u32)) -> bool {
    let Some(v) = v.strip_prefix("tmux ") else {
        return false;
    };
    let v = v.strip_prefix("next-").unwrap_or(v);
    let Some((major, rest)) = v.split_once('.') else {
        return false;
    };
    let len = rest.bytes().take_while(u8::is_ascii_digit).count();
    let (minor, suffix) = rest.split_at(len);
    if !suffix.bytes().all(|b| b.is_ascii_alphabetic()) {
        return false;
    }
    match (major.parse::<u32>(), minor.parse::<u32>()) {
        (Ok(a), Ok(b)) => (a, b) >= want,
        _ => false,
    }
}
fn check(id: &str, status: Status, message: impl Into<String>, fix: Option<&str>) -> Check {
    Check {
        id: id.into(),
        status,
        message: message.into(),
        fix: fix.map(Into::into),
    }
}
pub fn checks(p: &dyn Probe) -> Vec<Check> {
    let mut out = vec![
        if p.pid1().trim() == "systemd" && p.user_manager_available() {
            check("systemd", Status::Ok, "systemd가 켜져 있습니다", None)
        } else {
            check(
                "systemd",
                Status::Fail,
                "WSL에서 systemd가 꺼져 있습니다",
                Some("/etc/wsl.conf에 [boot] systemd=true를 넣고 wsl --shutdown"),
            )
        },
    ];
    out.push(if p.linger().lines().any(|l| l.trim() == "Linger=yes") {
        check(
            "linger",
            Status::Ok,
            "linger가 켜져 있습니다(사용자 관리자 유지용 보험이며 WSL 종료를 막지는 않습니다)",
            None,
        )
    } else {
        check(
            "linger",
            Status::Warn,
            "linger가 꺼져 있습니다(권장, WSL 종료 방지 기능은 아닙니다)",
            Some("loginctl enable-linger <user>"),
        )
    });
    out.push(match p.tmux_version() {
        Some(v) if tmux_at_least(&v, (3, 2)) => check(
            "tmux",
            Status::Ok,
            "tmux 3.2 이상을 사용할 수 있습니다",
            None,
        ),
        _ => check(
            "tmux",
            Status::Fail,
            "tmux 3.2 이상이 필요합니다",
            Some("sudo apt install tmux 또는 최신 tmux 설치"),
        ),
    });
    let inotify = p.inotify_instances().trim().parse::<u32>().unwrap_or(0);
    out.push(check(
        "inotify",
        if inotify >= 512 {
            Status::Ok
        } else {
            Status::Warn
        },
        format!("max_user_instances={inotify}"),
        (inotify < 512).then_some("fs.inotify.max_user_instances를 1024로 설정"),
    ));
    match p.env_file().and_then(|text| crate::setup::env_path(&text)) {
        None => out.push(check(
            "env",
            Status::Fail,
            "사용자 환경 파일에 유효한 PATH가 없습니다",
            Some("devbox setup"),
        )),
        Some(path) => {
            out.push(check(
                "env",
                Status::Ok,
                "사용자 환경을 잡아 두었습니다",
                None,
            ));
            for tool in ["git", "claude", "codex", "node", "pnpm"] {
                let found = p.which(tool, &path);
                out.push(check(
                    &format!("tool.{tool}"),
                    if found {
                        Status::Ok
                    } else if tool == "git" {
                        Status::Fail
                    } else {
                        Status::Warn
                    },
                    if found {
                        format!("{tool} 사용 가능")
                    } else {
                        format!("{tool}를 찾지 못했습니다")
                    },
                    (!found).then_some("설치한 뒤 환경 다시 잡기"),
                ));
            }
        }
    }
    out
}
pub struct OsProbe {
    pub env_file: std::path::PathBuf,
}
fn run_cmd(cmd: &str, args: &[&str]) -> Option<String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .ok()?;
    let out = runtime.block_on(crate::setup::command(cmd, args)).ok()?;
    String::from_utf8(out.stdout).ok().map(|s| s.trim().into())
}
#[allow(unsafe_code)]
fn executable(path: &std::path::Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    if !std::fs::metadata(path).is_ok_and(|m| m.is_file()) {
        return false;
    }
    let Ok(path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: CString owns a valid NUL-terminated pathname for the duration of
    // this read-only syscall. AT_EACCESS checks effective credentials and ACLs.
    unsafe { libc::faccessat(libc::AT_FDCWD, path.as_ptr(), libc::X_OK, libc::AT_EACCESS) == 0 }
}
impl Probe for OsProbe {
    fn pid1(&self) -> String {
        std::fs::read_to_string("/proc/1/comm").unwrap_or_default()
    }
    fn user_manager_available(&self) -> bool {
        // `show` queries the manager rather than the health of unrelated units.
        run_cmd(
            "systemctl",
            &["--user", "show", "--property=Version", "--value"],
        )
        .is_some()
    }
    fn linger(&self) -> String {
        let user = std::env::var("USER").unwrap_or_default();
        run_cmd("loginctl", &["show-user", &user, "-p", "Linger"]).unwrap_or_default()
    }
    fn tmux_version(&self) -> Option<String> {
        run_cmd("tmux", &["-V"])
    }
    fn inotify_instances(&self) -> String {
        std::fs::read_to_string("/proc/sys/fs/inotify/max_user_instances").unwrap_or_default()
    }
    fn env_file(&self) -> Option<String> {
        std::fs::read_to_string(&self.env_file).ok()
    }
    fn which(&self, tool: &str, path: &str) -> bool {
        path.split(':')
            .filter(|d| !d.is_empty() && std::path::Path::new(d).is_absolute())
            .any(|d| executable(&std::path::Path::new(d).join(tool)))
    }
}
pub async fn run(json: bool) -> anyhow::Result<()> {
    let probe = OsProbe {
        env_file: crate::paths()?.instance_config_dir.join("env"),
    };
    let report = DoctorReport {
        checks: tokio::task::spawn_blocking(move || checks(&probe)).await?,
    };
    if json {
        println!("{}", serde_json::to_string(&report)?);
    } else {
        for c in &report.checks {
            println!(
                "{:?} {} {}{}",
                c.status,
                c.id,
                c.message,
                c.fix
                    .as_ref()
                    .map(|f| format!(" → {f}"))
                    .unwrap_or_default()
            );
        }
    }
    if report.checks.iter().any(|c| c.status == Status::Fail) {
        anyhow::bail!("doctor found failed checks")
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod probe_tests {
    use super::*;
    struct Fake {
        available: bool,
        env: Option<&'static str>,
    }
    impl Probe for Fake {
        fn pid1(&self) -> String {
            "systemd".into()
        }
        fn user_manager_available(&self) -> bool {
            self.available
        }
        fn linger(&self) -> String {
            "Linger=no".into()
        }
        fn tmux_version(&self) -> Option<String> {
            Some("tmux 3.3a".into())
        }
        fn inotify_instances(&self) -> String {
            "1024".into()
        }
        fn env_file(&self) -> Option<String> {
            self.env.map(Into::into)
        }
        fn which(&self, _tool: &str, _path: &str) -> bool {
            true
        }
    }
    #[test]
    fn unavailable_manager_fails_and_responsive_manager_allows_degraded_units() {
        for available in [true, false] {
            let checks = checks(&Fake {
                available,
                env: Some("PATH=\"/usr/bin\"\n"),
            });
            assert_eq!(
                checks[0].status,
                if available { Status::Ok } else { Status::Fail }
            );
            assert_eq!(
                checks.iter().find(|c| c.id == "linger").unwrap().status,
                Status::Warn
            );
        }
    }
    #[test]
    fn env_missing_empty_relative_or_malformed_fails_without_tool_claims() {
        for env in [
            None,
            Some("LANG=\"C\""),
            Some("PATH=\"\""),
            Some("PATH=\"relative\""),
            Some("PATH=\"/bin\"trailing"),
        ] {
            let checks = checks(&Fake {
                available: true,
                env,
            });
            assert_eq!(
                checks.iter().find(|c| c.id == "env").unwrap().status,
                Status::Fail
            );
            assert!(!checks.iter().any(|c| c.id.starts_with("tool.")));
        }
    }
    #[test]
    fn which_requires_absolute_directory_and_executable_file() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let tool = temp.path().join("fixture-tool");
        std::fs::write(&tool, "fixture").unwrap();
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o600)).unwrap();
        let probe = OsProbe {
            env_file: temp.path().join("env"),
        };
        assert!(!probe.which("fixture-tool", &temp.path().display().to_string()));
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o001)).unwrap();
        // Root may execute a regular file with any execution bit set.
        if {
            use std::os::unix::fs::MetadataExt;
            std::fs::metadata("/proc/self").unwrap().uid()
        } != 0
        {
            assert!(!probe.which("fixture-tool", &temp.path().display().to_string()));
        }
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(probe.which("fixture-tool", &temp.path().display().to_string()));
        assert!(!probe.which("fixture-tool", ":relative"));
    }
}
