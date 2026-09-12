//! Explicit, marker-owned Bash/Zsh integration for OSC 7 cwd reporting.

use crate::core::multiplexer::PRINTENV_EXECUTABLES;
use crate::core::shell_integration::{
    canonical_block, content_revision, inspect_content, plan_content, ShellIntegrationAction,
    ShellIntegrationStatus, ShellKind, MAX_RC_FILE_BYTES,
};
use serde::Serialize;
use std::process::Stdio;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::State;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::sync::Mutex;

const SAFE_ERROR: &str = "WSL 셸 연동을 안전하게 처리하지 못했습니다.";
const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_ENVIRONMENT_BYTES: usize = 8 * 1024;

#[derive(Default)]
pub struct ShellIntegrationState {
    mutation: Mutex<()>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellIntegrationInfo {
    shell: ShellKind,
    rc_file: String,
    status: ShellIntegrationStatus,
    revision: String,
    block_preview: String,
    default_shell: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellIntegrationReport {
    distro: String,
    shells: Vec<ShellIntegrationInfo>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellIntegrationMutation {
    changed: bool,
    backup_file: Option<String>,
    integration: ShellIntegrationInfo,
}

struct ExecOutcome {
    success: bool,
    stdout: Vec<u8>,
}

struct RcSnapshot {
    exists: bool,
    blocked: bool,
    content: String,
}

fn validate_distro(distro: &str) -> Result<String, String> {
    let distro = distro.trim();
    if distro.len() > 128 || devbox_wsl::distro::validate_distro_name(distro).is_err() {
        return Err(SAFE_ERROR.into());
    }
    Ok(distro.to_owned())
}

fn normalize_home(output: &[u8]) -> Result<String, String> {
    if output.len() > MAX_ENVIRONMENT_BYTES {
        return Err(SAFE_ERROR.into());
    }
    let value = std::str::from_utf8(output)
        .map_err(|_| SAFE_ERROR)?
        .trim_end_matches(['\r', '\n']);
    if value.is_empty()
        || value.len() > 4_096
        || !value.starts_with('/')
        || value.chars().any(char::is_control)
        || value.split('/').any(|part| matches!(part, "." | ".."))
    {
        return Err(SAFE_ERROR.into());
    }
    Ok(value.trim_end_matches('/').to_owned())
}

fn normalize_shell(output: &[u8]) -> Option<&str> {
    let value = std::str::from_utf8(output)
        .ok()?
        .trim_end_matches(['\r', '\n']);
    if value.len() > 4_096 || value.chars().any(char::is_control) {
        return None;
    }
    value
        .rsplit('/')
        .next()
        .filter(|name| matches!(*name, "bash" | "zsh"))
}

fn direct_argv(distro: &str, program: &str, args: &[&str]) -> Result<Vec<String>, String> {
    let mut argv = devbox_wsl::argv::build_direct_exec_argv(distro, None, program)
        .map_err(|_| SAFE_ERROR.to_owned())?;
    argv.extend(args.iter().map(|value| (*value).to_owned()));
    Ok(argv)
}

struct Execution<'a> {
    lease: Option<&'a dyn crate::component::TerminalLaunchLease>,
}

async fn run_exact(
    execution: &Execution<'_>,
    mut argv: Vec<String>,
    stdin: Option<&[u8]>,
    max_stdout: usize,
) -> Result<ExecOutcome, String> {
    if let Some(lease) = execution.lease {
        argv = lease.bind_argv(argv)?;
    }
    let (program, args) = argv.split_first().ok_or_else(|| SAFE_ERROR.to_owned())?;
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(if max_stdout == 0 {
            Stdio::null()
        } else {
            Stdio::piped()
        })
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(target_os = "windows")]
    command.creation_flags(0x0800_0000);
    let mut child = command.spawn().map_err(|_| SAFE_ERROR.to_owned())?;
    let operation = async {
        if let Some(input) = stdin {
            let mut writer = child.stdin.take().ok_or_else(|| SAFE_ERROR.to_owned())?;
            writer
                .write_all(input)
                .await
                .map_err(|_| SAFE_ERROR.to_owned())?;
            writer.shutdown().await.map_err(|_| SAFE_ERROR.to_owned())?;
        }
        let mut stdout = Vec::new();
        if max_stdout > 0 {
            let reader = child.stdout.take().ok_or_else(|| SAFE_ERROR.to_owned())?;
            let mut bounded = reader.take((max_stdout + 1) as u64);
            bounded
                .read_to_end(&mut stdout)
                .await
                .map_err(|_| SAFE_ERROR.to_owned())?;
            if stdout.len() > max_stdout {
                return Err(SAFE_ERROR.to_owned());
            }
        }
        let status = child.wait().await.map_err(|_| SAFE_ERROR.to_owned())?;
        Ok(ExecOutcome {
            success: status.success(),
            stdout,
        })
    };
    match tokio::time::timeout(COMMAND_TIMEOUT, operation).await {
        Ok(result) => result,
        Err(_) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            Err(SAFE_ERROR.into())
        }
    }
}

async fn user_environment(
    execution: &Execution<'_>,
    distro: &str,
) -> Result<(String, Option<String>), String> {
    for printenv in PRINTENV_EXECUTABLES {
        let home = run_exact(
            execution,
            direct_argv(distro, printenv, &["HOME"])?,
            None,
            MAX_ENVIRONMENT_BYTES,
        )
        .await?;
        if !home.success {
            continue;
        }
        let home = normalize_home(&home.stdout)?;
        let shell = run_exact(
            execution,
            direct_argv(distro, printenv, &["SHELL"])?,
            None,
            MAX_ENVIRONMENT_BYTES,
        )
        .await
        .ok()
        .filter(|outcome| outcome.success)
        .and_then(|outcome| normalize_shell(&outcome.stdout).map(str::to_owned));
        return Ok((home, shell));
    }
    Err(SAFE_ERROR.into())
}

async fn test_path(
    execution: &Execution<'_>,
    distro: &str,
    flag: &str,
    path: &str,
) -> Result<bool, String> {
    Ok(run_exact(
        execution,
        direct_argv(distro, "/bin/test", &[flag, path])?,
        None,
        0,
    )
    .await?
    .success)
}

async fn read_rc(
    execution: &Execution<'_>,
    distro: &str,
    path: &str,
) -> Result<RcSnapshot, String> {
    if test_path(execution, distro, "-L", path).await? {
        return Ok(RcSnapshot {
            exists: true,
            blocked: true,
            content: String::new(),
        });
    }
    let exists = test_path(execution, distro, "-e", path).await?;
    if !exists {
        return Ok(RcSnapshot {
            exists: false,
            blocked: false,
            content: String::new(),
        });
    }
    if !test_path(execution, distro, "-f", path).await? {
        return Ok(RcSnapshot {
            exists: true,
            blocked: true,
            content: String::new(),
        });
    }
    let output = run_exact(
        execution,
        direct_argv(distro, "/bin/cat", &["--", path])?,
        None,
        MAX_RC_FILE_BYTES,
    )
    .await?;
    if !output.success {
        return Err(SAFE_ERROR.into());
    }
    let content = String::from_utf8(output.stdout).map_err(|_| SAFE_ERROR.to_owned())?;
    Ok(RcSnapshot {
        exists: true,
        blocked: false,
        content,
    })
}

fn integration_info(
    shell: ShellKind,
    snapshot: &RcSnapshot,
    default_shell: Option<&str>,
) -> ShellIntegrationInfo {
    ShellIntegrationInfo {
        shell,
        rc_file: format!("~/{}", shell.rc_file()),
        status: if snapshot.blocked {
            ShellIntegrationStatus::Blocked
        } else {
            inspect_content(&snapshot.content, shell)
        },
        revision: if snapshot.blocked {
            String::new()
        } else {
            content_revision(snapshot.exists, &snapshot.content)
        },
        block_preview: canonical_block(shell).to_owned(),
        default_shell: default_shell == Some(shell.executable_name()),
    }
}

async fn inspect_one(
    execution: &Execution<'_>,
    distro: &str,
    home: &str,
    shell: ShellKind,
    default_shell: Option<&str>,
) -> Result<ShellIntegrationInfo, String> {
    let path = format!("{home}/{}", shell.rc_file());
    let snapshot = read_rc(execution, distro, &path).await?;
    Ok(integration_info(shell, &snapshot, default_shell))
}

#[tauri::command]
pub async fn inspect_shell_integration(distro: String) -> Result<ShellIntegrationReport, String> {
    inspect_bound(&Execution { lease: None }, distro).await
}
async fn inspect_bound(
    execution: &Execution<'_>,
    distro: String,
) -> Result<ShellIntegrationReport, String> {
    let distro = validate_distro(&distro)?;
    let (home, default_shell) = user_environment(execution, &distro).await?;
    let bash = inspect_one(
        execution,
        &distro,
        &home,
        ShellKind::Bash,
        default_shell.as_deref(),
    )
    .await?;
    let zsh = inspect_one(
        execution,
        &distro,
        &home,
        ShellKind::Zsh,
        default_shell.as_deref(),
    )
    .await?;
    Ok(ShellIntegrationReport {
        distro,
        shells: vec![bash, zsh],
    })
}

async fn run_mutation_command(
    execution: &Execution<'_>,
    distro: &str,
    program: &str,
    args: &[&str],
) -> Result<(), String> {
    let outcome = run_exact(execution, direct_argv(distro, program, args)?, None, 0).await?;
    if outcome.success {
        Ok(())
    } else {
        Err(SAFE_ERROR.into())
    }
}

async fn write_temp(
    execution: &Execution<'_>,
    distro: &str,
    path: &str,
    content: &[u8],
) -> Result<(), String> {
    for program in ["/usr/bin/tee", "/bin/tee"] {
        let outcome = run_exact(
            execution,
            direct_argv(distro, program, &["--", path])?,
            Some(content),
            0,
        )
        .await?;
        if outcome.success {
            return Ok(());
        }
    }
    Err(SAFE_ERROR.into())
}

#[tauri::command]
pub async fn update_shell_integration(
    state: State<'_, ShellIntegrationState>,
    distro: String,
    shell: ShellKind,
    action: ShellIntegrationAction,
    expected_revision: String,
) -> Result<ShellIntegrationMutation, String> {
    update_bound(
        &Execution { lease: None },
        &state,
        distro,
        shell,
        action,
        expected_revision,
    )
    .await
}
async fn update_bound(
    execution: &Execution<'_>,
    state: &ShellIntegrationState,
    distro: String,
    shell: ShellKind,
    action: ShellIntegrationAction,
    expected_revision: String,
) -> Result<ShellIntegrationMutation, String> {
    let _guard = state.mutation.lock().await;
    let distro = validate_distro(&distro)?;
    let (home, default_shell) = user_environment(execution, &distro).await?;
    let rc_path = format!("{home}/{}", shell.rc_file());
    let snapshot = read_rc(execution, &distro, &rc_path).await?;
    if snapshot.blocked
        || expected_revision.len() > 64
        || expected_revision != content_revision(snapshot.exists, &snapshot.content)
    {
        return Err("셸 설정이 미리보기 이후 변경되었거나 자동 수정할 수 없는 파일입니다.".into());
    }
    let plan = plan_content(&snapshot.content, shell, action)?;
    let Some(next) = plan.next else {
        return Ok(ShellIntegrationMutation {
            changed: false,
            backup_file: None,
            integration: integration_info(shell, &snapshot, default_shell.as_deref()),
        });
    };

    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let temp_path = format!("{rc_path}.devbox-tmp-{nonce}");
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| SAFE_ERROR.to_owned())?
        .as_secs();
    let backup_path = format!("{rc_path}.devbox-backup-{timestamp}-{}", &nonce[..8]);
    let backup_file = if snapshot.exists {
        run_mutation_command(
            execution,
            &distro,
            "/bin/cp",
            &["-p", "--", &rc_path, &backup_path],
        )
        .await?;
        Some(format!(
            "~/{}.devbox-backup-{timestamp}-{}",
            shell.rc_file(),
            &nonce[..8]
        ))
    } else {
        None
    };

    let write_result = async {
        write_temp(execution, &distro, &temp_path, next.as_bytes()).await?;
        if snapshot.exists {
            run_mutation_command(
                execution,
                &distro,
                "/bin/chmod",
                &[&format!("--reference={rc_path}"), "--", &temp_path],
            )
            .await?;
        } else {
            run_mutation_command(
                execution,
                &distro,
                "/bin/chmod",
                &["0644", "--", &temp_path],
            )
            .await?;
        }

        // Refuse to replace a file edited externally after the user accepted the preview.
        let latest = read_rc(execution, &distro, &rc_path).await?;
        if latest.blocked
            || content_revision(latest.exists, &latest.content)
                != content_revision(snapshot.exists, &snapshot.content)
        {
            return Err("셸 설정이 적용 중 변경되어 덮어쓰지 않았습니다.".into());
        }
        run_mutation_command(execution, &distro, "/bin/mv", &["--", &temp_path, &rc_path]).await
    }
    .await;

    if let Err(error) = write_result {
        let _ =
            run_mutation_command(execution, &distro, "/bin/rm", &["-f", "--", &temp_path]).await;
        return Err(error);
    }

    let updated = RcSnapshot {
        exists: true,
        blocked: false,
        content: next,
    };
    Ok(ShellIntegrationMutation {
        changed: true,
        backup_file,
        integration: integration_info(shell, &updated, default_shell.as_deref()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn replaced_native_target_rejects_before_any_shell_command() {
        struct Revoked;
        impl crate::component::TerminalLaunchLease for Revoked {
            fn revalidate(&self) -> Result<(), String> {
                Err("fixture_target_replaced".into())
            }
            fn bind_argv(&self, _argv: Vec<String>) -> Result<Vec<String>, String> {
                self.revalidate()?;
                unreachable!()
            }
            fn retire(&self) -> Result<(), String> {
                Ok(())
            }
        }
        let result = inspect_bound(
            &Execution {
                lease: Some(&Revoked),
            },
            "SyntheticDistro".into(),
        )
        .await;
        assert!(matches!(result, Err(error) if error == "fixture_target_replaced"));
    }

    #[test]
    fn home_and_shell_output_are_bounded_and_normalized() {
        assert_eq!(
            normalize_home(b"/home/dev user\n").unwrap(),
            "/home/dev user"
        );
        assert!(normalize_home(b"relative/home\n").is_err());
        assert!(normalize_home(b"/home/../root\n").is_err());
        assert!(normalize_home(b"/home/user\0secret\n").is_err());
        assert_eq!(normalize_shell(b"/usr/bin/bash\n"), Some("bash"));
        assert_eq!(normalize_shell(b"/bin/fish\n"), None);
    }

    #[test]
    fn direct_argv_keeps_distro_and_rc_path_as_exact_arguments() {
        let argv = direct_argv(
            "Ubuntu 24.04",
            "/bin/cat",
            &["--", "/home/dev user/.bashrc"],
        )
        .unwrap();
        assert_eq!(
            argv,
            vec![
                "wsl.exe",
                "-d",
                "Ubuntu 24.04",
                "--exec",
                "/bin/cat",
                "--",
                "/home/dev user/.bashrc",
            ]
        );
        assert!(direct_argv("--help", "/bin/cat", &[]).is_err());
    }

    #[test]
    fn blocked_snapshots_never_expose_a_revision() {
        let snapshot = RcSnapshot {
            exists: true,
            blocked: true,
            content: String::new(),
        };
        let info = integration_info(ShellKind::Bash, &snapshot, Some("bash"));
        assert_eq!(info.status, ShellIntegrationStatus::Blocked);
        assert!(info.revision.is_empty());
        assert!(info.default_shell);
    }
}

/// Strict component adapter; caller/window/session admission belongs to Workspace.
#[cfg(feature = "desktop")]
pub(crate) async fn __component_inspect_shell_integration(
    app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {
        distro: String,
    }
    if !args.is_object() {
        return Err("terminal_args_invalid".into());
    }
    let input: Input = serde_json::from_value(args).map_err(|_| "terminal_args_invalid")?;
    let _ = app;
    let value = inspect_shell_integration(input.distro).await?;
    serde_json::to_value(value).map_err(|_| "terminal_response_invalid".into())
}

/// Strict component adapter; caller/window/session admission belongs to Workspace.
#[cfg(feature = "desktop")]
pub(crate) async fn __component_update_shell_integration(
    app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use tauri::Manager;
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {
        distro: String,
        shell: ShellKind,
        action: ShellIntegrationAction,
        expected_revision: String,
    }
    if !args.is_object() {
        return Err("terminal_args_invalid".into());
    }
    let input: Input = serde_json::from_value(args).map_err(|_| "terminal_args_invalid")?;
    let _ = app;
    let value = update_shell_integration(
        app.try_state().ok_or("terminal_state_unavailable")?,
        input.distro,
        input.shell,
        input.action,
        input.expected_revision,
    )
    .await?;
    serde_json::to_value(value).map_err(|_| "terminal_response_invalid".into())
}

/// The product supplies a retained native running-distro/executable binding.
/// Existing marker, preview revision and backup rules remain the mutation owner.
#[cfg(feature = "desktop")]
pub async fn dispatch_owned(
    app: &tauri::AppHandle,
    method: &str,
    args: serde_json::Value,
    lease: &dyn crate::component::TerminalLaunchLease,
) -> Result<serde_json::Value, String> {
    use tauri::Manager;
    let execution = Execution { lease: Some(lease) };
    let value = if method == "inspect_shell_integration" {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Input {
            distro: String,
        }
        let input: Input = serde_json::from_value(args).map_err(|_| "terminal_args_invalid")?;
        serde_json::to_value(inspect_bound(&execution, input.distro).await?)
    } else if method == "update_shell_integration" {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Input {
            distro: String,
            shell: ShellKind,
            action: ShellIntegrationAction,
            expected_revision: String,
        }
        let input: Input = serde_json::from_value(args).map_err(|_| "terminal_args_invalid")?;
        serde_json::to_value(
            update_bound(
                &execution,
                app.try_state::<ShellIntegrationState>()
                    .ok_or("terminal_state_unavailable")?
                    .inner(),
                input.distro,
                input.shell,
                input.action,
                input.expected_revision,
            )
            .await?,
        )
    } else {
        return Err("terminal_method_invalid".into());
    };
    value.map_err(|_| "terminal_response_invalid".into())
}
