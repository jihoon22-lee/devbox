mod agent_cli;
mod environment;
mod os;
mod runner;
use devbox_protocol::system::{DoctorReport, Status};
pub use devbox_tools::doctor::{Probe, Scope, checks, checks_for};

pub struct OsProbe {
    pub env_file: std::path::PathBuf,
    pub hook_binary: std::path::PathBuf,
    /// Set over the process environment for tool lookups and the commands this check starts.
    /// Empty for the daemon and the app's start-up check, whose own environment is the answer.
    vars: Vec<(String, String)>,
    /// This check's look at the agent CLIs, made once, on first use.
    agents: std::sync::OnceLock<
        devbox_tools::doctor::Probed<Vec<Option<devbox_tools::doctor::AgentCommand>>>,
    >,
}
impl OsProbe {
    /// A probe for one check: each check looks at the agent CLIs afresh.
    pub fn new(env_file: std::path::PathBuf, hook_binary: std::path::PathBuf) -> Self {
        Self {
            env_file,
            hook_binary,
            vars: Vec::new(),
            agents: std::sync::OnceLock::new(),
        }
    }
    /// A hand-run check looks in the environment selected for it.
    fn looking_in(mut self, lookup: environment::Lookup) -> Self {
        if let environment::Lookup::Instance(vars) = lookup {
            self.vars = vars;
        }
        self
    }
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
/// Both startup scopes use the caller's environment. A full, hand-run check uses the saved one.
pub async fn run(json: bool, scope: Scope) -> anyhow::Result<()> {
    let paths = crate::paths()?;
    let env_file = paths.instance_config_dir.join("env");
    let hook_binary = paths.bin_dir.join("devbox");
    let (used, checks) = tokio::task::spawn_blocking(move || {
        let lookup = environment::select(&env_file, scope != Scope::Full);
        let used = environment::Used::of(&lookup, &env_file);
        let probe = OsProbe::new(env_file, hook_binary).looking_in(lookup);
        (used, checks_for(&probe, scope))
    })
    .await?;
    let report = DoctorReport { checks };
    if json {
        let output = environment::Output {
            environment: &used,
            report: &report,
        };
        println!("{}", serde_json::to_string(&output)?);
    } else {
        println!("{}", used.line());
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
mod scope_tests {
    /// Startup scopes leave the agent CLI rows out.
    /// 환경 점검, 에이전트 settings and support bundles read these two routes, which keep them.
    #[test]
    fn the_rpc_doctor_and_the_support_bundle_keep_the_agent_rows() {
        for source in [
            include_str!("daemon/routes.rs"),
            include_str!("daemon/tools_routes.rs"),
        ] {
            let flat = source.split_whitespace().collect::<String>();
            assert!(flat.contains("crate::doctor::checks(&probe)"));
            assert!(!flat.contains("checks_for"));
            assert!(!flat.contains("Scope::Startup"));
        }
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
        let probe = OsProbe::new(temp.path().join("env"), tool.clone());
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
        let probe = OsProbe::new(temp.path().join("env"), temp.path().join("unused-hook"));
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
