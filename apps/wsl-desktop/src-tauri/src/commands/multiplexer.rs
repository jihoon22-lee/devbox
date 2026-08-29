//! Read-only multiplexer resolution and exact-session probes.
//!
//! WSL's direct exec environment is intentionally different from an interactive shell. The
//! resolver reads only the distro user's HOME and PATH with a fixed `printenv` executable, then
//! probes bounded absolute candidates. Shell rc files, aliases and arbitrary environment output
//! are never evaluated or returned to the renderer.

use crate::core::multiplexer::{
    build_environment_probe_argv, build_session_probe_argv, build_version_probe_argv,
    executable_candidates, normalize_version, parse_user_environment, session_name,
    zellij_session_is_running, MultiplexerSource, PRINTENV_EXECUTABLES,
};
use crate::core::workspace::MultiplexerKind;
use serde::Serialize;
use std::future::Future;
use std::pin::Pin;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

const PROBE_TIMEOUT: Duration = Duration::from_secs(3);
const PROBE_KILL_WAIT: Duration = Duration::from_millis(500);
const MAX_PROBE_STDOUT_BYTES: usize = 4 * 1024;
const COMMAND_NOT_FOUND_EXIT_CODE: i32 = 127;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MultiplexerStatus {
    Available,
    Missing,
    Error,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MultiplexerAvailability {
    pub kind: MultiplexerKind,
    pub status: MultiplexerStatus,
    pub version: Option<String>,
    pub source: Option<MultiplexerSource>,
}

/// Backend-only launch token. The absolute executable path is deliberately absent from the
/// serializable availability DTO and is re-resolved for every start request.
pub(crate) struct ResolvedMultiplexer {
    kind: MultiplexerKind,
    executable: String,
}

impl ResolvedMultiplexer {
    pub(crate) fn kind(&self) -> MultiplexerKind {
        self.kind
    }

    pub(crate) fn executable(&self) -> &str {
        &self.executable
    }
}

#[derive(Clone)]
enum RunOutcome {
    Completed {
        exit_code: Option<i32>,
        stdout: Vec<u8>,
    },
    TimedOut,
    Failed,
}

trait ProbeRunner: Sync {
    fn run(&self, argv: Vec<String>) -> Pin<Box<dyn Future<Output = RunOutcome> + Send + '_>>;
}

struct SystemProbeRunner;

impl ProbeRunner for SystemProbeRunner {
    fn run(&self, argv: Vec<String>) -> Pin<Box<dyn Future<Output = RunOutcome> + Send + '_>> {
        Box::pin(run_argv(argv))
    }
}

async fn run_argv(argv: Vec<String>) -> RunOutcome {
    let Some((program, args)) = argv.split_first() else {
        return RunOutcome::Failed;
    };
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let Ok(mut child) = command.spawn() else {
        return RunOutcome::Failed;
    };
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill().await;
        return RunOutcome::Failed;
    };
    let mut bounded_stdout = stdout.take((MAX_PROBE_STDOUT_BYTES + 1) as u64);
    let mut output = Vec::new();
    let collected = tokio::time::timeout(PROBE_TIMEOUT, async {
        let (read_result, wait_result) =
            tokio::join!(bounded_stdout.read_to_end(&mut output), child.wait(),);
        (read_result, wait_result)
    })
    .await;
    match collected {
        Ok((Ok(_), Ok(status))) if output.len() <= MAX_PROBE_STDOUT_BYTES => {
            RunOutcome::Completed {
                exit_code: status.code(),
                stdout: output,
            }
        }
        Ok(_) => RunOutcome::Failed,
        Err(_) => {
            let _ = child.kill().await;
            let _ = tokio::time::timeout(PROBE_KILL_WAIT, child.wait()).await;
            RunOutcome::TimedOut
        }
    }
}

fn availability(
    kind: MultiplexerKind,
    status: MultiplexerStatus,
    version: Option<String>,
    source: Option<MultiplexerSource>,
) -> MultiplexerAvailability {
    MultiplexerAvailability {
        kind,
        status,
        version,
        source,
    }
}

async fn resolve_with_runner<R: ProbeRunner>(
    distro: &str,
    kind: MultiplexerKind,
    runner: &R,
) -> (MultiplexerAvailability, Option<ResolvedMultiplexer>) {
    if kind == MultiplexerKind::Native {
        return (
            availability(kind, MultiplexerStatus::Available, None, None),
            None,
        );
    }

    let mut environment = None;
    for printenv in PRINTENV_EXECUTABLES {
        let argv = match build_environment_probe_argv(distro, printenv) {
            Ok(argv) => argv,
            Err(_) => {
                return (
                    availability(kind, MultiplexerStatus::Error, None, None),
                    None,
                )
            }
        };
        match runner.run(argv).await {
            RunOutcome::Completed {
                exit_code: Some(0),
                stdout,
            } => match parse_user_environment(&stdout) {
                Ok(value) => {
                    environment = Some(value);
                    break;
                }
                Err(_) => {
                    return (
                        availability(kind, MultiplexerStatus::Error, None, None),
                        None,
                    )
                }
            },
            RunOutcome::Completed {
                exit_code: Some(COMMAND_NOT_FOUND_EXIT_CODE),
                ..
            } => continue,
            RunOutcome::Completed { .. } | RunOutcome::TimedOut | RunOutcome::Failed => {
                return (
                    availability(kind, MultiplexerStatus::Error, None, None),
                    None,
                )
            }
        }
    }

    let candidates = match executable_candidates(kind, environment.as_ref()) {
        Ok(candidates) => candidates,
        Err(_) => {
            return (
                availability(kind, MultiplexerStatus::Error, None, None),
                None,
            )
        }
    };
    let mut saw_broken_candidate = false;
    for candidate in candidates {
        let argv = match build_version_probe_argv(distro, kind, &candidate.path) {
            Ok(argv) => argv,
            Err(_) => {
                return (
                    availability(kind, MultiplexerStatus::Error, None, None),
                    None,
                )
            }
        };
        match runner.run(argv).await {
            RunOutcome::Completed {
                exit_code: Some(0),
                stdout,
            } => {
                let Some(version) = normalize_version(kind, &stdout) else {
                    return (
                        availability(kind, MultiplexerStatus::Error, None, None),
                        None,
                    );
                };
                return (
                    availability(
                        kind,
                        MultiplexerStatus::Available,
                        Some(version),
                        Some(candidate.source),
                    ),
                    Some(ResolvedMultiplexer {
                        kind,
                        executable: candidate.path,
                    }),
                );
            }
            RunOutcome::Completed {
                exit_code: Some(COMMAND_NOT_FOUND_EXIT_CODE),
                ..
            } => {}
            RunOutcome::Completed { .. } => saw_broken_candidate = true,
            RunOutcome::TimedOut | RunOutcome::Failed => {
                return (
                    availability(kind, MultiplexerStatus::Error, None, None),
                    None,
                )
            }
        }
    }

    let status = if saw_broken_candidate {
        MultiplexerStatus::Error
    } else {
        MultiplexerStatus::Missing
    };
    (availability(kind, status, None, None), None)
}

async fn detect_one(distro: &str, kind: MultiplexerKind) -> MultiplexerAvailability {
    resolve_with_runner(distro, kind, &SystemProbeRunner)
        .await
        .0
}

/// Resolve again immediately before launch. A stale renderer result therefore cannot select a
/// replaced or removed executable, and every probe/session/launch argv uses this exact path.
pub(crate) async fn resolve_for_launch(
    distro: &str,
    kind: MultiplexerKind,
) -> Option<ResolvedMultiplexer> {
    resolve_with_runner(distro, kind, &SystemProbeRunner)
        .await
        .1
}

#[tauri::command]
pub async fn detect_multiplexers(distro: String) -> Vec<MultiplexerAvailability> {
    vec![
        availability(
            MultiplexerKind::Native,
            MultiplexerStatus::Available,
            None,
            None,
        ),
        detect_one(&distro, MultiplexerKind::Tmux).await,
        detect_one(&distro, MultiplexerKind::Zellij).await,
    ]
}

pub(crate) async fn session_is_running(
    distro: &str,
    pane_key: &str,
    resolved: &ResolvedMultiplexer,
) -> bool {
    let argv =
        match build_session_probe_argv(distro, pane_key, resolved.kind(), resolved.executable()) {
            Ok(argv) => argv,
            Err(_) => return false,
        };
    let RunOutcome::Completed { exit_code, stdout } = run_argv(argv).await else {
        return false;
    };
    match resolved.kind() {
        MultiplexerKind::Native => false,
        MultiplexerKind::Tmux => exit_code == Some(0),
        MultiplexerKind::Zellij => {
            let expected = match session_name(distro, pane_key) {
                Ok(name) => name,
                Err(_) => return false,
            };
            exit_code == Some(0)
                && std::str::from_utf8(&stdout)
                    .ok()
                    .is_some_and(|value| zellij_session_is_running(value, &expected))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    struct FakeRunner {
        outcomes: Mutex<VecDeque<RunOutcome>>,
        calls: Mutex<Vec<Vec<String>>>,
    }

    impl FakeRunner {
        fn new(outcomes: Vec<RunOutcome>) -> Self {
            Self {
                outcomes: Mutex::new(outcomes.into()),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn calls(&self) -> Vec<Vec<String>> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl ProbeRunner for FakeRunner {
        fn run(&self, argv: Vec<String>) -> Pin<Box<dyn Future<Output = RunOutcome> + Send + '_>> {
            self.calls.lock().unwrap().push(argv);
            let outcome = self
                .outcomes
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(RunOutcome::Failed);
            Box::pin(async move { outcome })
        }
    }

    fn completed(exit_code: i32, stdout: &[u8]) -> RunOutcome {
        RunOutcome::Completed {
            exit_code: Some(exit_code),
            stdout: stdout.to_vec(),
        }
    }

    #[tokio::test]
    async fn resolves_user_local_zellij_without_a_login_shell() {
        let runner = FakeRunner::new(vec![
            completed(0, b"/home/jihoon\n/usr/bin\n"),
            completed(COMMAND_NOT_FOUND_EXIT_CODE, b""),
            completed(0, b"zellij 0.41.2\n"),
        ]);
        let (public, resolved) =
            resolve_with_runner("Ubuntu", MultiplexerKind::Zellij, &runner).await;

        assert_eq!(
            public,
            MultiplexerAvailability {
                kind: MultiplexerKind::Zellij,
                status: MultiplexerStatus::Available,
                version: Some("zellij 0.41.2".into()),
                source: Some(MultiplexerSource::UserLocal),
            }
        );
        assert_eq!(
            resolved.unwrap().executable(),
            "/home/jihoon/.local/bin/zellij"
        );
        let calls = runner.calls();
        assert_eq!(calls[0][4], "/usr/bin/printenv");
        assert_eq!(calls[1][4], "/usr/bin/zellij");
        assert_eq!(calls[2][4], "/home/jihoon/.local/bin/zellij");
        assert!(calls
            .iter()
            .flatten()
            .all(|arg| arg != "bash" && arg != "-lc"));
    }

    #[tokio::test]
    async fn returns_missing_only_when_all_bounded_candidates_are_absent() {
        let runner = FakeRunner::new(vec![
            completed(0, b"/home/dev\n/usr/bin\n"),
            completed(COMMAND_NOT_FOUND_EXIT_CODE, b""),
            completed(COMMAND_NOT_FOUND_EXIT_CODE, b""),
            completed(COMMAND_NOT_FOUND_EXIT_CODE, b""),
            completed(COMMAND_NOT_FOUND_EXIT_CODE, b""),
            completed(COMMAND_NOT_FOUND_EXIT_CODE, b""),
        ]);
        let (public, resolved) =
            resolve_with_runner("Ubuntu", MultiplexerKind::Tmux, &runner).await;
        assert_eq!(public.status, MultiplexerStatus::Missing);
        assert_eq!(public.version, None);
        assert_eq!(public.source, None);
        assert!(resolved.is_none());
    }

    #[tokio::test]
    async fn distinguishes_probe_errors_from_missing_tools() {
        let runner = FakeRunner::new(vec![
            completed(0, b"/home/dev\n/usr/bin\n"),
            RunOutcome::TimedOut,
        ]);
        let (public, resolved) =
            resolve_with_runner("Ubuntu", MultiplexerKind::Tmux, &runner).await;
        assert_eq!(public.status, MultiplexerStatus::Error);
        assert!(resolved.is_none());
    }

    #[tokio::test]
    async fn invalid_distro_is_a_safe_error_without_process_creation() {
        let runner = FakeRunner::new(Vec::new());
        let (public, resolved) =
            resolve_with_runner("bad\ndistro", MultiplexerKind::Tmux, &runner).await;
        assert_eq!(public.status, MultiplexerStatus::Error);
        assert!(resolved.is_none());
        assert!(runner.calls().is_empty());
    }

    #[tokio::test]
    async fn environment_query_failure_does_not_expose_output_or_claim_missing() {
        let runner = FakeRunner::new(vec![completed(1, b"C:\\private\\path\n")]);
        let (public, resolved) =
            resolve_with_runner("Ubuntu", MultiplexerKind::Zellij, &runner).await;
        assert_eq!(public.status, MultiplexerStatus::Error);
        assert_eq!(public.version, None);
        assert_eq!(public.source, None);
        assert!(resolved.is_none());
    }
}
