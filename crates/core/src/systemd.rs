//! User-manager operations and the unit-file escaping shared with setup.
use crate::{CoreError, Paths};
use serde::Deserialize;
use std::{path::Path, process::Output, time::Duration};

#[derive(Clone, Debug, Default)]
pub struct Systemd;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnitState {
    pub load_state: String,
    pub active_state: String,
    pub sub_state: String,
    pub result: String,
    pub n_restarts: u32,
    pub main_pid: Option<u32>,
    pub invocation_id: Option<String>,
}
impl Systemd {
    pub async fn run_transient(&self, args: &[String]) -> Result<(), CoreError> {
        let args: Vec<_> = args.iter().map(String::as_str).collect();
        command("systemd-run", &args).await.map(|_| ())
    }
    pub async fn stop(&self, unit: &str) -> Result<(), CoreError> {
        command("systemctl", &["--user", "stop", "--", unit])
            .await
            .map(|_| ())
    }
    pub async fn reset_failed(&self, unit: &str) -> Result<(), CoreError> {
        command("systemctl", &["--user", "reset-failed", "--", unit])
            .await
            .map(|_| ())
    }
    pub async fn show(&self, unit: &str) -> Result<UnitState, CoreError> {
        let output = output(
            "systemctl",
            &[
                "--user",
                "show",
                "--property=LoadState,ActiveState,SubState,Result,NRestarts,MainPID,InvocationID",
                "--",
                unit,
            ],
        )
        .await?;
        let state = parse_state(&output.stdout)?;
        if output.status.success() || state.load_state == "not-found" {
            Ok(state)
        } else {
            Err(CoreError::CommandFailed {
                program: "systemctl".into(),
            })
        }
    }
    pub async fn list_units(&self, pattern: &str) -> Result<Vec<String>, CoreError> {
        #[derive(Deserialize)]
        struct Unit {
            unit: String,
        }
        let output = command(
            "systemctl",
            &[
                "--user",
                "list-units",
                "--all",
                "--output=json",
                "--",
                pattern,
            ],
        )
        .await?;
        let units: Vec<Unit> =
            serde_json::from_slice(&output.stdout).map_err(|_| CoreError::SystemdResponse)?;
        Ok(units.into_iter().map(|unit| unit.unit).collect())
    }
}
fn parse_state(bytes: &[u8]) -> Result<UnitState, CoreError> {
    let text = std::str::from_utf8(bytes).map_err(|_| CoreError::SystemdResponse)?;
    let fields: std::collections::BTreeMap<_, _> = text
        .lines()
        .filter_map(|line| line.split_once('='))
        .collect();
    let field = |key| fields.get(key).copied().ok_or(CoreError::SystemdResponse);
    let load_state = field("LoadState")?.to_owned();
    let absent = load_state == "not-found";
    let number = |key| -> Result<u32, CoreError> {
        match fields.get(key) {
            Some(value) => value.parse().map_err(|_| CoreError::SystemdResponse),
            None if absent => Ok(0),
            None => Err(CoreError::SystemdResponse),
        }
    };
    let main_pid = number("MainPID")?;
    Ok(UnitState {
        active_state: field("ActiveState")?.to_owned(),
        sub_state: field("SubState")?.to_owned(),
        result: fields.get("Result").copied().unwrap_or_default().to_owned(),
        n_restarts: number("NRestarts")?,
        main_pid: (main_pid != 0).then_some(main_pid),
        invocation_id: fields
            .get("InvocationID")
            .filter(|id| !id.is_empty())
            .map(|id| (*id).to_owned()),
        load_state,
    })
}
fn invocation(program: &str) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(program);
    if matches!(program, "systemctl" | "systemd-run") {
        // The manager belongs to the login session, even for an instance whose
        // socket and unit Environment= use a private runtime directory.
        use crate::Env;
        command.env(
            "XDG_RUNTIME_DIR",
            format!("/run/user/{}", crate::OsEnv.uid()),
        );
    }
    command
}
async fn output(program: &str, args: &[&str]) -> Result<Output, CoreError> {
    let mut command = invocation(program);
    command.args(args).kill_on_drop(true);
    tokio::time::timeout(Duration::from_secs(15), command.output())
        .await
        .map_err(|_| CoreError::CommandTimedOut {
            program: program.into(),
        })?
        .map_err(|_| CoreError::CommandUnavailable {
            program: program.into(),
        })
}
/// Deliberately omit arguments/output from errors: callers can pass environment values.
pub async fn command(program: &str, args: &[&str]) -> Result<Output, CoreError> {
    let output = output(program, args).await?;
    if output.status.success() {
        Ok(output)
    } else {
        Err(CoreError::CommandFailed {
            program: program.into(),
        })
    }
}
/// One systemd unit-file word. Specifier expansion is escaped independently of shell quoting.
pub fn unit_quote(path: &Path) -> Result<String, CoreError> {
    Ok(quote_value(checked_path(path)?))
}
pub fn quote_value(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('%', "%%")
    )
}
/// Transient D-Bus values are already resolved. systemd escapes specifiers
/// when serializing them to its transient unit file, so '%' stays literal here.
pub fn transient_quote(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}
pub fn transient_path(path: &Path) -> Result<String, CoreError> {
    Ok(checked_path(path)?.to_owned())
}
/// EnvironmentFile, ListenStream and WorkingDirectory consume raw rvalues.
pub fn raw_unit_path(path: &Path) -> Result<String, CoreError> {
    Ok(checked_path(path)?.replace('%', "%%"))
}
fn checked_path(path: &Path) -> Result<&str, CoreError> {
    let value = path
        .to_str()
        .ok_or(CoreError::UnitPath("unit path is not UTF-8"))?;
    if value.chars().any(char::is_control) {
        return Err(CoreError::UnitPath("unit path contains control characters"));
    }
    Ok(value)
}
/// Reconstruct the exact HOME/XDG roots used to resolve this instance's Paths.
pub fn path_environment(paths: &Paths) -> [(&'static str, &Path); 5] {
    [
        ("HOME", paths.home.as_path()),
        ("XDG_CONFIG_HOME", paths.xdg_config_home.as_path()),
        (
            "XDG_DATA_HOME",
            paths
                .data_dir
                .parent()
                .and_then(Path::parent)
                .expect("resolved data ancestor"),
        ),
        (
            "XDG_STATE_HOME",
            paths
                .state_dir
                .parent()
                .and_then(Path::parent)
                .expect("resolved state ancestor"),
        ),
        (
            "XDG_RUNTIME_DIR",
            paths.socket.parent().expect("resolved runtime parent"),
        ),
    ]
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn manager_commands_use_login_runtime_without_rewriting_instance_paths() {
        use crate::Env;
        let expected = format!("/run/user/{}", crate::OsEnv.uid());
        for program in ["systemctl", "systemd-run"] {
            let command = invocation(program);
            let configured: Vec<_> = command.as_std().get_envs().collect();
            assert_eq!(
                configured,
                [(
                    std::ffi::OsStr::new("XDG_RUNTIME_DIR"),
                    Some(std::ffi::OsStr::new(&expected))
                )]
            );
        }
        assert_eq!(invocation("bash").as_std().get_envs().count(), 0);
    }
    #[test]
    fn show_retains_restart_and_collected_unit_state() {
        let state = parse_state(b"LoadState=loaded\nActiveState=activating\nSubState=auto-restart\nResult=exit-code\nNRestarts=2\nMainPID=0\nInvocationID=abc\n").expect("state");
        assert_eq!((state.n_restarts, state.main_pid), (2, None));
        assert_eq!(state.sub_state, "auto-restart");
        assert_eq!(state.invocation_id.as_deref(), Some("abc"));
        let collected = parse_state(b"LoadState=not-found\nActiveState=inactive\nSubState=dead\n")
            .expect("collected");
        assert_eq!(collected.load_state, "not-found");
        assert!(parse_state(b"LoadState=loaded\nActiveState=active\nSubState=running\nNRestarts=oops\nMainPID=4\n").is_err());
    }
    #[test]
    fn unit_paths_preserve_whitespace_quotes_and_literal_specifiers() {
        let path = Path::new("/a b/%h/\"quote\"/\\file");
        assert_eq!(
            raw_unit_path(path).expect("raw"),
            "/a b/%%h/\"quote\"/\\file"
        );
        assert_eq!(
            unit_quote(path).expect("word"),
            "\"/a b/%%h/\\\"quote\\\"/\\\\file\""
        );
        assert!(raw_unit_path(Path::new("/a\nb")).is_err());
    }
}
