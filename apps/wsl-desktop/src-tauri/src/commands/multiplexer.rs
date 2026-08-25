//! Read-only multiplexer detection and exact-session probes.

use crate::core::multiplexer::{
    build_probe_argv, build_session_probe_argv, normalize_version, session_name,
    zellij_session_is_running,
};
use crate::core::workspace::MultiplexerKind;
use serde::Serialize;
use std::process::Output;
use std::time::Duration;
use tokio::process::Command;

const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MultiplexerAvailability {
    pub kind: MultiplexerKind,
    pub available: bool,
    pub version: Option<String>,
}

async fn run_argv(argv: Vec<String>) -> Option<Output> {
    let (program, args) = argv.split_first()?;
    let mut command = Command::new(program);
    command.args(args).kill_on_drop(true);
    tokio::time::timeout(PROBE_TIMEOUT, command.output())
        .await
        .ok()?
        .ok()
}

async fn detect_one(distro: &str, kind: MultiplexerKind) -> MultiplexerAvailability {
    let output = match build_probe_argv(distro, kind) {
        Ok(argv) => run_argv(argv).await,
        Err(_) => None,
    };
    let version = output
        .as_ref()
        .filter(|result| result.status.success())
        .and_then(|result| normalize_version(&result.stdout));
    MultiplexerAvailability {
        kind,
        available: version.is_some(),
        version,
    }
}

pub async fn kind_is_available(distro: &str, kind: MultiplexerKind) -> bool {
    kind == MultiplexerKind::Native || detect_one(distro, kind).await.available
}

#[tauri::command]
pub async fn detect_multiplexers(distro: String) -> Vec<MultiplexerAvailability> {
    vec![
        MultiplexerAvailability {
            kind: MultiplexerKind::Native,
            available: true,
            version: None,
        },
        detect_one(&distro, MultiplexerKind::Tmux).await,
        detect_one(&distro, MultiplexerKind::Zellij).await,
    ]
}

pub async fn session_is_running(distro: &str, pane_key: &str, kind: MultiplexerKind) -> bool {
    let argv = match build_session_probe_argv(distro, pane_key, kind) {
        Ok(argv) => argv,
        Err(_) => return false,
    };
    let Some(output) = run_argv(argv).await else {
        return false;
    };
    match kind {
        MultiplexerKind::Native => false,
        MultiplexerKind::Tmux => output.status.success(),
        MultiplexerKind::Zellij => {
            let expected = match session_name(distro, pane_key) {
                Ok(name) => name,
                Err(_) => return false,
            };
            std::str::from_utf8(&output.stdout)
                .ok()
                .is_some_and(|stdout| zellij_session_is_running(stdout, &expected))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn optional_mux_absence_is_a_safe_unavailable_result() {
        // An invalid distro is rejected before process creation and must never become an error
        // that blocks the native workspace path.
        let result = detect_one("bad\ndistro", MultiplexerKind::Tmux).await;
        assert_eq!(
            result,
            MultiplexerAvailability {
                kind: MultiplexerKind::Tmux,
                available: false,
                version: None,
            }
        );
    }
}
