mod os;
mod runner;
use devbox_protocol::system::{DoctorReport, Status};
pub use devbox_tools::doctor::{Probe, checks};

pub struct OsProbe {
    pub env_file: std::path::PathBuf,
    pub hook_binary: std::path::PathBuf,
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
        assert!(
            !probe
                .which("fixture-tool", &temp.path().display().to_string())
                .expect("probe fixture")
        );
        assert!(!probe.hook_executable().expect("probe fixture"));
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
            assert!(
                !probe
                    .which("fixture-tool", &temp.path().display().to_string())
                    .expect("probe fixture")
            );
        }
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o700))
            .expect("set fixture permissions");
        assert!(
            probe
                .which("fixture-tool", &temp.path().display().to_string())
                .expect("probe fixture")
        );
        assert!(probe.hook_executable().expect("probe fixture"));
        assert!(
            !probe
                .which("fixture-tool", ":relative")
                .expect("probe fixture")
        );
    }

    #[test]
    fn env_path_valid_preserves_the_saved_environment_fixtures() {
        let temp = tempfile::tempdir().expect("create fixture directory");
        let probe = OsProbe {
            env_file: temp.path().join("env"),
            hook_binary: temp.path().join("unused-hook"),
        };
        assert!(!probe.env_path_valid().expect("probe fixture"));
        for text in [
            "LANG=\"C\"",
            "PATH=\"\"",
            "PATH=\"relative\"",
            "PATH=\"/bin\"trailing",
        ] {
            std::fs::write(&probe.env_file, text).expect("write owned environment fixture");
            assert!(!probe.env_path_valid().expect("probe fixture"));
        }
        std::fs::write(&probe.env_file, "PATH=\"/usr/bin\"\n")
            .expect("write valid owned environment fixture");
        assert!(probe.env_path_valid().expect("probe fixture"));
    }
}
