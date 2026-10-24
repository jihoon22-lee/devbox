use devbox_protocol::system::{DoctorReport, Status};
pub use devbox_tools::doctor::{Probe, checks};

pub struct OsProbe {
    pub env_file: std::path::PathBuf,
    pub hook_binary: std::path::PathBuf,
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
    fn daemon_path(&self) -> String {
        std::env::var("PATH").unwrap_or_default()
    }
    fn hook_executable(&self) -> bool {
        executable(&self.hook_binary)
    }
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
    fn env_path_valid(&self) -> bool {
        std::fs::read_to_string(&self.env_file)
            .ok()
            .and_then(|text| crate::setup::env_path(&text))
            .is_some()
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
        hook_binary: crate::paths()?.bin_dir.join("devbox"),
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

    #[test]
    fn which_requires_absolute_directory_and_executable_file() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().expect("create fixture directory");
        let tool = temp.path().join("fixture-tool");
        std::fs::write(&tool, "fixture").expect("write fixture file");
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o600))
            .expect("set fixture permissions");
        let probe = OsProbe {
            env_file: temp.path().join("env"),
            hook_binary: tool.clone(),
        };
        assert!(!probe.which("fixture-tool", &temp.path().display().to_string()));
        assert!(!probe.hook_executable());
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o001))
            .expect("set fixture permissions");
        // Root may execute a regular file with any execution bit set.
        if {
            use std::os::unix::fs::MetadataExt;
            std::fs::metadata("/proc/self")
                .expect("inspect fixture metadata")
                .uid()
        } != 0
        {
            assert!(!probe.which("fixture-tool", &temp.path().display().to_string()));
        }
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o700))
            .expect("set fixture permissions");
        assert!(probe.which("fixture-tool", &temp.path().display().to_string()));
        assert!(probe.hook_executable());
        assert!(!probe.which("fixture-tool", ":relative"));
    }

    #[test]
    fn env_path_valid_preserves_the_saved_environment_fixtures() {
        let temp = tempfile::tempdir().expect("create fixture directory");
        let probe = OsProbe {
            env_file: temp.path().join("env"),
            hook_binary: temp.path().join("unused-hook"),
        };
        assert!(!probe.env_path_valid());
        for text in [
            "LANG=\"C\"",
            "PATH=\"\"",
            "PATH=\"relative\"",
            "PATH=\"/bin\"trailing",
        ] {
            std::fs::write(&probe.env_file, text).expect("write owned environment fixture");
            assert!(!probe.env_path_valid());
        }
        std::fs::write(&probe.env_file, "PATH=\"/usr/bin\"\n")
            .expect("write valid owned environment fixture");
        assert!(probe.env_path_valid());
    }
}
