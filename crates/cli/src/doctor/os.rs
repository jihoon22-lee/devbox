use super::{OsProbe, runner};
use devbox_tools::doctor::{
    AgentAuth, AgentTool, DockerProbe, Probe, ProbeIssue, Probed, RustVersions,
};
use std::path::{Path, PathBuf};
const GH_AUTH_ARGS: &[&str] = &["auth", "status", "--no-refresh"];
// Fixed read-only PowerShell, no profiles and no caller-provided script fragments.
const WSL_CONFIG: &str = r#"$ErrorActionPreference='Stop'; [Console]::OutputEncoding=[System.Text.UTF8Encoding]::new($false); $p=Join-Path $env:USERPROFILE '.wslconfig'; if(-not (Test-Path -LiteralPath $p -PathType Leaf)){exit 3}; $r=[System.IO.File]::OpenText($p); try { $b=[char[]]::new(65537); $n=$r.ReadBlock($b,0,$b.Length); if($n -gt 65536){exit 4}; [Console]::Write([string]::new($b,0,$n)) } finally { $r.Dispose() }"#;
fn config_root(agent: AgentTool) -> Probed<PathBuf> {
    let home = crate::paths().map_err(|_| ProbeIssue::Unavailable)?.home;
    let (key, default) = match agent {
        AgentTool::Claude => ("CLAUDE_CONFIG_DIR", ".claude"),
        AgentTool::Codex => ("CODEX_HOME", ".codex"),
    };
    config_root_from(&home, std::env::var_os(key).map(PathBuf::from), default)
}
fn config_root_from(home: &Path, override_path: Option<PathBuf>, default: &str) -> Probed<PathBuf> {
    match override_path {
        Some(path) if path.is_absolute() => Ok(path),
        Some(_) => Err(ProbeIssue::InvalidData),
        None => Ok(home.join(default)),
    }
}
impl Probe for OsProbe {
    fn daemon_path(&self) -> String {
        std::env::var("PATH").unwrap_or_default()
    }
    fn hook_executable(&self) -> Probed<bool> {
        runner::run(runner::is_executable(self.hook_binary.clone()))
    }
    fn which(&self, tool: &str, path: &str) -> Probed<bool> {
        runner::run(runner::which(tool.into(), path.into()))
    }
    fn pid1(&self) -> Probed<String> {
        runner::run(runner::read_text("/proc/1/comm".into()))
    }
    fn user_manager_available(&self) -> Probed<bool> {
        runner::run(async {
            Ok(runner::command(
                "systemctl",
                &["--user", "show", "--property=Version", "--value"],
                false,
            )
            .await?
            .success)
        })
    }
    fn linger(&self) -> Probed<String> {
        let user = std::env::var("USER").unwrap_or_default();
        runner::run(async {
            let output =
                runner::command("loginctl", &["show-user", &user, "-p", "Linger"], true).await?;
            if output.success {
                Ok(output.text)
            } else {
                Err(ProbeIssue::Unavailable)
            }
        })
    }
    fn tmux_version(&self) -> Probed<Option<String>> {
        runner::run(runner::version("tmux", &["-V"]))
    }
    fn inotify_instances(&self) -> Probed<String> {
        runner::run(runner::read_text(
            "/proc/sys/fs/inotify/max_user_instances".into(),
        ))
    }
    fn env_path_valid(&self) -> Probed<bool> {
        runner::run(async {
            match runner::read_text(self.env_file.clone()).await {
                Ok(text) => Ok(crate::setup::env_path(&text).is_some()),
                Err(ProbeIssue::Missing) => Ok(false),
                Err(issue) => Err(issue),
            }
        })
    }
    fn wsl_config(&self) -> Probed<Option<String>> {
        runner::run(async {
            let output = runner::command(
                "powershell.exe",
                &["-NoProfile", "-NonInteractive", "-Command", WSL_CONFIG],
                true,
            )
            .await?;
            if output.success {
                Ok(Some(output.text))
            } else if output.code == Some(3) {
                Ok(None)
            } else {
                Err(ProbeIssue::Unavailable)
            }
        })
    }
    fn rust_versions(&self) -> Probed<RustVersions> {
        runner::run(async {
            Ok(RustVersions {
                cargo: runner::version("cargo", &["--version"]).await?,
                rustc: runner::version("rustc", &["--version"]).await?,
            })
        })
    }
    fn docker(&self) -> Probed<DockerProbe> {
        runner::run(async {
            let version = runner::version("docker", &["--version"]).await?;
            let responsive = if version.is_some() {
                runner::command("docker", &["info", "--format", "{{.ServerVersion}}"], false)
                    .await?
                    .success
            } else {
                false
            };
            Ok(DockerProbe {
                version,
                responsive,
            })
        })
    }
    fn gh_authenticated(&self) -> Probed<bool> {
        runner::run(gh_auth_status("gh"))
    }
    fn agent_auth(&self, agent: AgentTool) -> Probed<AgentAuth> {
        let root = config_root(agent)?;
        let file = root.join(match agent {
            AgentTool::Claude => ".credentials.json",
            AgentTool::Codex => "auth.json",
        });
        runner::run(async {
            Ok(AgentAuth {
                version: runner::version(agent.binary(), &["--version"]).await?,
                file_present: runner::file_exists(file).await?,
            })
        })
    }
    fn agent_hook(&self, agent: AgentTool) -> Probed<bool> {
        let file = config_root(agent)?.join(match agent {
            AgentTool::Claude => "settings.json",
            AgentTool::Codex => "config.toml",
        });
        runner::run(async {
            match runner::read_text(file).await {
                Ok(text) => devbox_tools::doctor::hook_present(agent, &text),
                Err(ProbeIssue::Missing) => Ok(false),
                Err(issue) => Err(issue),
            }
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn doctor_auth_probe_never_refreshes_credentials_and_honors_absolute_overrides() {
        assert_eq!(GH_AUTH_ARGS, ["auth", "status", "--no-refresh"]);
        assert_eq!(
            config_root_from(Path::new("/owned/home"), None, ".codex"),
            Ok(PathBuf::from("/owned/home/.codex"))
        );
        assert_eq!(
            config_root_from(
                Path::new("/owned/home"),
                Some("/owned/custom".into()),
                ".claude"
            ),
            Ok(PathBuf::from("/owned/custom"))
        );
        assert_eq!(
            config_root_from(Path::new("/owned/home"), Some("relative".into()), ".codex"),
            Err(ProbeIssue::InvalidData)
        );
    }
}

async fn gh_auth_status(program: &str) -> Probed<bool> {
    // Help is local/read-only. Old versions must never fall back to an auth
    // invocation that could refresh stored credentials.
    let help = runner::command(program, &["auth", "status", "--help"], true).await?;
    if !help.success {
        return Err(ProbeIssue::Unavailable);
    }
    let supported = help.text.lines().any(|line| {
        line.trim_start().starts_with('-')
            && line
                .split_whitespace()
                .take(2)
                .any(|word| word.trim_end_matches(',') == "--no-refresh")
    });
    if !supported {
        return Err(ProbeIssue::UnsupportedCapability);
    }
    // Both commands run inside the caller's single two-second deadline.
    match runner::command(program, GH_AUTH_ARGS, false).await?.code {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(ProbeIssue::Unavailable),
    }
}
#[cfg(test)]
#[path = "gh_tests.rs"]
mod gh_tests;
