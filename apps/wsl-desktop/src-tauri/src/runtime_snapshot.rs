//! WSL Desktop의 `wsl-desktop/runtime/v1` snapshot producer.
//!
//! 수집 대상은 이미 실행 중인 WSL distro뿐이다. 각 distro에 대해 고정된 argv로
//! read-only Docker 목록을 순차 조회하고, 완성된 하나의 envelope만 공용 integration
//! writer에 넘긴다. 수집·파싱·상한·privacy 검증 중 하나라도 실패하면 이 모듈은
//! `summary.json`을 쓰지 않으므로 직전 last-good 파일이 그대로 남는다.

use crate::commands::terminal::SessionState;
use crate::core::runtime_snapshot as model;
use devbox_integration::{Envelope, SnapshotView, SnapshotViews};
use std::collections::BTreeMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::{Child, Command};
use tokio::time::{sleep, timeout, Duration};

const PRODUCER_ID: &str = "wsl-desktop";
const RUNTIME_VIEW_KIND: &str = "runtime";
const SNAPSHOT_INTERVAL: Duration = Duration::from_secs(60);
const SNAPSHOT_DEBOUNCE: Duration = Duration::from_millis(250);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_STDERR_BYTES: usize = 64 * 1024;
const DOCKER_PS_FORMAT: &str = "{{.ID}}\t{{.Names}}\t{{.State}}\t{{.Ports}}";
const COLLECTION_ERROR: &str = "WSL runtime 상태를 수집하지 못했습니다";

/// 한 producer 파일을 쓰는 worker를 하나만 유지하기 위한 상태.
///
/// `pending`은 terminal/dashboard event를 debounce하고, `running`은 여러 trigger가
/// 동시에 worker를 만들지 않게 한다. `write_lock`은 테스트나 향후 수동 refresh가
/// worker와 겹쳐도 마지막 단계의 atomic replace가 한 번에 수행되도록 한다.
pub struct SnapshotCoordinator {
    pending: AtomicBool,
    running: AtomicBool,
    write_lock: Mutex<()>,
}

impl SnapshotCoordinator {
    pub fn new() -> Self {
        Self {
            pending: AtomicBool::new(false),
            running: AtomicBool::new(false),
            write_lock: Mutex::new(()),
        }
    }
}

impl Default for SnapshotCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

/// 앱 시작 이후 60초 주기 writer를 시작한다. 초기 발행도 debounce worker를 통해
/// 수행하므로 setup 경계에서 WSL process를 동기 실행하지 않는다.
pub fn spawn_snapshot_writer(state: Arc<SessionState>) {
    tauri::async_runtime::spawn(async move {
        request_snapshot_write(Arc::clone(&state));
        loop {
            sleep(SNAPSHOT_INTERVAL).await;
            request_snapshot_write(Arc::clone(&state));
        }
    });
}

/// 성공한 dashboard refresh 또는 terminal lifecycle 변화가 snapshot을 갱신하도록
/// 요청한다. 여러 이벤트는 하나의 debounce worker로 합쳐진다.
pub fn request_snapshot_write(state: Arc<SessionState>) {
    let coordinator = Arc::clone(&state.snapshot_coordinator);
    coordinator.pending.store(true, Ordering::Release);
    if coordinator
        .running
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        tauri::async_runtime::spawn(run_pending_writes(state, coordinator));
    }
}

async fn run_pending_writes(state: Arc<SessionState>, coordinator: Arc<SnapshotCoordinator>) {
    loop {
        while coordinator.pending.swap(false, Ordering::AcqRel) {
            sleep(SNAPSHOT_DEBOUNCE).await;
            if let Err(error) = write_snapshot(Arc::clone(&state), Arc::clone(&coordinator)).await {
                // error는 고정된 사용자 안전 메시지만 반환한다. command argv, path,
                // stdout/stderr와 Docker 원문은 로그에도 남기지 않는다.
                eprintln!("wsl-desktop runtime snapshot 실패: {error}");
            }
        }

        coordinator.running.store(false, Ordering::Release);
        // running을 내리는 순간 새 event가 들어온 경우 request 쪽에서 이미 running=true를
        // 보고 worker를 만들지 못했을 수 있다. 이 worker가 그 pending을 이어받는다.
        if coordinator.pending.load(Ordering::Acquire)
            && coordinator
                .running
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
        {
            continue;
        }
        break;
    }
}

async fn write_snapshot(
    state: Arc<SessionState>,
    coordinator: Arc<SnapshotCoordinator>,
) -> Result<(), String> {
    let entries = collect_entries(state).await?;
    let envelope = build_envelope(entries)?;
    write_envelope(&coordinator, &envelope)
}

fn write_envelope(coordinator: &SnapshotCoordinator, envelope: &Envelope) -> Result<(), String> {
    // No await follows this guard. It is therefore safe to use the small synchronous mutex
    // without holding a non-Send guard across the async collection.
    let _guard = coordinator
        .write_lock
        .lock()
        .map_err(|_| COLLECTION_ERROR.to_owned())?;
    let directory = devbox_integration::snapshot_dir(PRODUCER_ID, model::SNAPSHOT_SCHEMA_VERSION);
    devbox_integration::write_atomic(envelope, &directory)
}

async fn collect_entries(
    state: Arc<SessionState>,
) -> Result<Vec<model::RuntimeDistroEntry>, String> {
    let running_output = run_fixed_command(&build_running_distros_argv())
        .await
        .map_err(|_| COLLECTION_ERROR)?;
    if !running_output.success {
        return Err(COLLECTION_ERROR.into());
    }
    let running_distros =
        model::parse_running_distros(&devbox_wsl::output::decode_output(&running_output.stdout))
            .map_err(|_| COLLECTION_ERROR)?;
    let terminal_counts = state
        .terminal_counts_by_distro()
        .map_err(|_| COLLECTION_ERROR)?;

    let mut docker_results = BTreeMap::new();
    for distro in &running_distros {
        let argv = build_docker_argv(distro).map_err(|_| COLLECTION_ERROR)?;
        let result = match run_fixed_command(&argv).await {
            Ok(output) if output.success => model::DockerResult {
                availability: model::DockerAvailability::Available,
                containers: model::parse_docker_snapshot(&devbox_wsl::output::decode_output(
                    &output.stdout,
                ))
                .map_err(|_| COLLECTION_ERROR)?,
            },
            Ok(output) if output.exit_code == Some(127) && output.stdout.is_empty() => {
                model::DockerResult {
                    availability: model::DockerAvailability::Missing,
                    containers: Vec::new(),
                }
            }
            Ok(output) if output.stdout.is_empty() => model::DockerResult {
                availability: model::DockerAvailability::Error,
                containers: Vec::new(),
            },
            // A non-zero command with stdout is a partial result, not a trustworthy error
            // status. Preserve last-good rather than publishing a mixed collection.
            Ok(_output) => return Err(COLLECTION_ERROR.into()),
            // A timeout, output overflow, I/O failure or inability to spawn wsl.exe is a
            // partial collection, not a valid empty/error result. Preserve last-good.
            Err(_) => return Err(COLLECTION_ERROR.into()),
        };
        docker_results.insert(distro.clone(), result);
    }

    model::build_entries(&running_distros, &terminal_counts, &docker_results)
        .map_err(|_| COLLECTION_ERROR.into())
}

/// Build the exact argv used to enumerate only already-running distros.
fn build_running_distros_argv() -> Vec<String> {
    ["wsl.exe", "--list", "--running", "--quiet"]
        .into_iter()
        .map(str::to_owned)
        .collect()
}

/// Build the exact read-only Docker query. The distro is one validated argv element; no
/// shell, `bash -lc`, environment expansion or user-provided command is involved.
fn build_docker_argv(distro: &str) -> Result<Vec<String>, String> {
    if distro.is_empty() || distro.len() > model::MAX_DISTRO_NAME_BYTES {
        return Err(COLLECTION_ERROR.to_owned());
    }
    let mut argv = devbox_wsl::argv::build_exec_argv(distro, None, "docker")
        .map_err(|_| COLLECTION_ERROR.to_owned())?;
    argv.extend(
        ["ps", "-a", "--no-trunc", "--format", DOCKER_PS_FORMAT]
            .into_iter()
            .map(str::to_owned),
    );
    Ok(argv)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandFailure {
    NotFound,
    Io,
    TimedOut,
    OutputTooLarge,
}

#[derive(Debug)]
struct CommandOutput {
    stdout: Vec<u8>,
    success: bool,
    exit_code: Option<i32>,
}

/// Run one fixed command with bounded stdout/stderr and a total timeout. stderr is drained
/// only to prevent pipe backpressure; it is never decoded, returned or logged.
async fn run_fixed_command(argv: &[String]) -> Result<CommandOutput, CommandFailure> {
    run_fixed_command_with_timeout(argv, COMMAND_TIMEOUT).await
}

async fn run_fixed_command_with_timeout(
    argv: &[String],
    command_timeout: Duration,
) -> Result<CommandOutput, CommandFailure> {
    let Some(program) = argv.first() else {
        return Err(CommandFailure::Io);
    };
    let mut command = Command::new(program);
    command
        .args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(target_os = "windows")]
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(CommandFailure::NotFound)
        }
        Err(_) => return Err(CommandFailure::Io),
    };
    let Some(stdout) = child.stdout.take() else {
        terminate_child(&mut child).await;
        return Err(CommandFailure::Io);
    };
    let Some(stderr) = child.stderr.take() else {
        terminate_child(&mut child).await;
        return Err(CommandFailure::Io);
    };

    let result = timeout(command_timeout, async {
        let (stdout, _) = tokio::try_join!(
            read_bounded(stdout, model::MAX_DOCKER_OUTPUT_BYTES),
            drain_bounded(stderr, MAX_STDERR_BYTES),
        )?;
        let status = child.wait().await.map_err(|_| CommandFailure::Io)?;
        Ok::<_, CommandFailure>(CommandOutput {
            stdout,
            success: status.success(),
            exit_code: status.code(),
        })
    })
    .await;

    match result {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(error)) => {
            terminate_child(&mut child).await;
            Err(error)
        }
        Err(_) => {
            terminate_child(&mut child).await;
            Err(CommandFailure::TimedOut)
        }
    }
}

async fn read_bounded<R: AsyncRead + Unpin>(
    mut reader: R,
    max_bytes: usize,
) -> Result<Vec<u8>, CommandFailure> {
    let mut output = Vec::with_capacity(max_bytes.min(64 * 1024));
    let mut buffer = [0u8; 8 * 1024];
    loop {
        let count = reader
            .read(&mut buffer)
            .await
            .map_err(|_| CommandFailure::Io)?;
        if count == 0 {
            return Ok(output);
        }
        if output.len().saturating_add(count) > max_bytes {
            return Err(CommandFailure::OutputTooLarge);
        }
        output.extend_from_slice(&buffer[..count]);
    }
}

async fn drain_bounded<R: AsyncRead + Unpin>(
    mut reader: R,
    max_bytes: usize,
) -> Result<(), CommandFailure> {
    let mut total = 0usize;
    let mut buffer = [0u8; 8 * 1024];
    loop {
        let count = reader
            .read(&mut buffer)
            .await
            .map_err(|_| CommandFailure::Io)?;
        if count == 0 {
            return Ok(());
        }
        total = total.saturating_add(count);
        if total > max_bytes {
            return Err(CommandFailure::OutputTooLarge);
        }
    }
}

async fn terminate_child(child: &mut Child) {
    let _ = child.kill().await;
    let _ = child.wait().await;
}

/// Build a complete envelope from already validated entries. Keeping this function free of
/// process/file I/O makes deterministic fixture tests independent from WSL and Docker.
pub fn build_envelope(entries: Vec<model::RuntimeDistroEntry>) -> Result<Envelope, String> {
    model::validate_entries(&entries).map_err(|_| COLLECTION_ERROR.to_owned())?;
    let mut entries = entries;
    entries.sort_by(|left, right| left.id.cmp(&right.id));
    for entry in &mut entries {
        entry.containers.sort_by(|left, right| {
            left.id
                .cmp(&right.id)
                .then_with(|| left.name.cmp(&right.name))
        });
        for container in &mut entry.containers {
            container.port_mappings.sort_by(|left, right| {
                left.published
                    .cmp(&right.published)
                    .then_with(|| left.target.cmp(&right.target))
                    .then_with(|| left.protocol.cmp(&right.protocol))
            });
        }
    }
    let entries = entries
        .into_iter()
        .map(|entry| serde_json::to_value(entry).map_err(|_| COLLECTION_ERROR.to_owned()))
        .collect::<Result<Vec<_>, _>>()?;
    let views = SnapshotViews::from([(
        RUNTIME_VIEW_KIND.to_owned(),
        SnapshotView {
            schema_version: model::RUNTIME_VIEW_SCHEMA_VERSION,
            freshness_ms: 0,
            entries,
        },
    )]);
    Ok(Envelope::with_views(
        PRODUCER_ID,
        env!("CARGO_PKG_VERSION"),
        views,
    ))
}

#[cfg(test)]
fn write_snapshot_in(
    root: &std::path::Path,
    entries: Vec<model::RuntimeDistroEntry>,
) -> Result<(), String> {
    let envelope = build_envelope(entries)?;
    let directory =
        devbox_integration::snapshot_dir_in(root, PRODUCER_ID, model::SNAPSHOT_SCHEMA_VERSION);
    devbox_integration::write_atomic(&envelope, &directory)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::runtime_snapshot::{
        build_entries, parse_docker_snapshot, DockerAvailability, DockerResult,
    };
    use std::path::PathBuf;

    fn test_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "devbox-wsl-runtime-{label}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ))
    }

    fn entries() -> Vec<model::RuntimeDistroEntry> {
        let distros = vec!["Ubuntu".to_owned()];
        let counts = BTreeMap::from([("Ubuntu".to_owned(), 2usize)]);
        let containers =
            parse_docker_snapshot("abc123\tapi\trunning\t0.0.0.0:8080->80/tcp, :::8080->80/tcp\n")
                .unwrap();
        let docker = BTreeMap::from([(
            "Ubuntu".to_owned(),
            DockerResult {
                availability: DockerAvailability::Available,
                containers,
            },
        )]);
        build_entries(&distros, &counts, &docker).unwrap()
    }

    #[test]
    fn exact_argv_never_contains_shell_or_mutating_docker_actions() {
        assert_eq!(
            build_running_distros_argv(),
            vec!["wsl.exe", "--list", "--running", "--quiet"]
        );
        assert_eq!(
            build_docker_argv("Ubuntu 24.04").unwrap(),
            vec![
                "wsl.exe",
                "-d",
                "Ubuntu 24.04",
                "--",
                "docker",
                "ps",
                "-a",
                "--no-trunc",
                "--format",
                DOCKER_PS_FORMAT,
            ]
        );
        let argv = build_docker_argv("a;b");
        assert!(argv.is_err());
        let argv = build_docker_argv(&"x".repeat(model::MAX_DISTRO_NAME_BYTES + 1));
        assert!(argv.is_err());
    }

    #[test]
    fn running_distro_fixture_accepts_wsl_utf16_output() {
        let mut bytes = vec![0xFF, 0xFE];
        for character in "* Ubuntu 24.04\r\n".encode_utf16() {
            bytes.extend_from_slice(&character.to_le_bytes());
        }
        let output = devbox_wsl::output::decode_output(&bytes);
        assert_eq!(
            model::parse_running_distros(&output).unwrap(),
            vec!["Ubuntu 24.04"]
        );
    }

    #[test]
    fn complete_envelope_has_only_runtime_view_and_public_structured_values() {
        let envelope = build_envelope(entries()).unwrap();
        assert_eq!(envelope.producer, PRODUCER_ID);
        assert_eq!(envelope.schema_version, 1);
        let views = envelope.views().unwrap();
        assert_eq!(views.keys().collect::<Vec<_>>(), vec![RUNTIME_VIEW_KIND]);
        let json = serde_json::to_string(&envelope).unwrap();
        assert!(json.contains("terminalCount"));
        assert!(json.contains("dockerAvailability"));
        assert!(json.contains("portMappings"));
        assert!(json.contains("published"));
        assert!(json.contains("target"));
        assert!(!json.contains("0.0.0.0"));
        assert!(!json.contains("image"));
        assert!(!json.contains("status"));
        assert!(!json.contains("command"));
    }

    #[test]
    fn envelope_serialization_canonicalizes_entry_and_mapping_order() {
        let mut entries = entries();
        let mut second = entries[0].clone();
        second.id = "Debian".into();
        second.name = "Debian".into();
        second.containers[0].id = "def456".into();
        second.containers[0].name = "worker".into();
        second.containers[0].port_mappings = vec![
            model::PortMapping {
                published: 9000,
                target: 90,
                protocol: "udp".into(),
            },
            model::PortMapping {
                published: 8080,
                target: 80,
                protocol: "tcp".into(),
            },
        ];
        entries.push(second);
        entries.reverse();
        entries[0].containers.reverse();

        let envelope = build_envelope(entries).unwrap();
        let runtime = envelope.views().unwrap().remove(RUNTIME_VIEW_KIND).unwrap();
        assert_eq!(runtime.entries[0]["id"], "Debian");
        assert_eq!(runtime.entries[1]["id"], "Ubuntu");
        assert_eq!(
            runtime.entries[0]["containers"][0]["portMappings"][0]["published"],
            8080
        );
        assert_eq!(
            runtime.entries[0]["containers"][0]["portMappings"][1]["published"],
            9000
        );
    }

    #[test]
    fn atomic_write_replaces_complete_snapshot_and_leaves_no_temp_files() {
        let root = test_root("atomic");
        write_snapshot_in(&root, entries()).unwrap();
        let first = devbox_integration::read_snapshot_in(&root, PRODUCER_ID, 1)
            .unwrap()
            .unwrap();
        assert_eq!(first.views().unwrap()[RUNTIME_VIEW_KIND].entries.len(), 1);

        let mut replacement = entries();
        replacement[0].terminal_count = 0;
        replacement[0].containers.clear();
        write_snapshot_in(&root, replacement).unwrap();
        let second = devbox_integration::read_snapshot_in(&root, PRODUCER_ID, 1)
            .unwrap()
            .unwrap();
        let entry = &second.views().unwrap()[RUNTIME_VIEW_KIND].entries[0];
        assert_eq!(entry["terminalCount"], 0);
        assert!(entry["containers"].as_array().unwrap().is_empty());
        let files = std::fs::read_dir(devbox_integration::snapshot_dir_in(&root, PRODUCER_ID, 1))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        assert_eq!(files, vec!["summary.json"]);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn malformed_collection_preserves_last_good_file() {
        let root = test_root("last-good");
        write_snapshot_in(&root, entries()).unwrap();
        let path = devbox_integration::snapshot_path_in(&root, PRODUCER_ID, 1);
        let before = std::fs::read(&path).unwrap();

        let malformed = parse_docker_snapshot("abc123\tapi\trunning\n");
        assert!(malformed.is_err());
        // The producer builds no envelope and does not call write_atomic on parse failure.
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn shared_writer_rejects_credential_shaped_identity_without_creating_a_snapshot() {
        let root = test_root("credential-identity");
        let mut unsafe_entries = entries();
        let credential_like_name = "ghp_12345678901234567890";
        unsafe_entries[0].id = credential_like_name.into();
        unsafe_entries[0].name = credential_like_name.into();

        let error = write_snapshot_in(&root, unsafe_entries).unwrap_err();
        assert!(!error.contains(credential_like_name));
        assert!(!devbox_integration::snapshot_path_in(&root, PRODUCER_ID, 1).exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn missing_error_and_available_empty_docker_are_distinct() {
        let distros = vec!["Ubuntu".to_owned()];
        let counts = BTreeMap::new();
        let missing = BTreeMap::from([(
            "Ubuntu".to_owned(),
            DockerResult {
                availability: DockerAvailability::Missing,
                containers: Vec::new(),
            },
        )]);
        let available_empty = BTreeMap::from([(
            "Ubuntu".to_owned(),
            DockerResult {
                availability: DockerAvailability::Available,
                containers: Vec::new(),
            },
        )]);
        let command_error = BTreeMap::from([(
            "Ubuntu".to_owned(),
            DockerResult {
                availability: DockerAvailability::Error,
                containers: Vec::new(),
            },
        )]);
        let missing_entry = build_entries(&distros, &counts, &missing)
            .unwrap()
            .remove(0);
        let empty_entry = build_entries(&distros, &counts, &available_empty)
            .unwrap()
            .remove(0);
        let error_entry = build_entries(&distros, &counts, &command_error)
            .unwrap()
            .remove(0);
        assert_eq!(
            missing_entry.docker_availability,
            DockerAvailability::Missing
        );
        assert_eq!(
            empty_entry.docker_availability,
            DockerAvailability::Available
        );
        assert_eq!(error_entry.docker_availability, DockerAvailability::Error);
        assert!(empty_entry.containers.is_empty());
        assert!(error_entry.containers.is_empty());
    }

    #[tokio::test]
    async fn bounded_reader_rejects_output_without_retaining_it() {
        use tokio::io::AsyncWriteExt;

        let (mut writer, reader) = tokio::io::duplex(64);
        writer.write_all(b"too-large").await.unwrap();
        drop(writer);
        assert_eq!(
            read_bounded(reader, 3).await.unwrap_err(),
            CommandFailure::OutputTooLarge
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn timeout_child_fixture() {
        // A normal workspace test run reaches this fixture without `--exact` and returns.
        // The async timeout test below launches this same test binary with an exact filter,
        // records the child PID, and then deliberately waits long enough to be terminated.
        if !std::env::args().any(|argument| argument == "--exact") {
            return;
        }
        let status = std::fs::read_to_string("/proc/self/status").unwrap();
        let parent_pid = status
            .lines()
            .find_map(|line| line.strip_prefix("PPid:"))
            .and_then(|value| value.trim().parse::<u32>().ok())
            .unwrap();
        let marker =
            std::env::temp_dir().join(format!("devbox-wsl-runtime-timeout-child-{parent_pid}.pid"));
        std::fs::write(marker, std::process::id().to_string()).unwrap();
        std::thread::sleep(std::time::Duration::from_secs(30));
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn timeout_kills_and_reaps_the_direct_child_fixture() {
        let marker = std::env::temp_dir().join(format!(
            "devbox-wsl-runtime-timeout-child-{}.pid",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&marker);
        let executable = std::env::current_exe().unwrap();
        let argv = vec![
            executable.to_string_lossy().into_owned(),
            "--exact".into(),
            "runtime_snapshot::tests::timeout_child_fixture".into(),
            "--nocapture".into(),
            "--test-threads=1".into(),
        ];

        assert_eq!(
            run_fixed_command_with_timeout(&argv, Duration::from_secs(3))
                .await
                .unwrap_err(),
            CommandFailure::TimedOut
        );
        let child_pid = std::fs::read_to_string(&marker)
            .expect("child fixture must start before the timeout")
            .trim()
            .parse::<u32>()
            .unwrap();
        assert!(
            !std::path::Path::new(&format!("/proc/{child_pid}")).exists(),
            "the timed-out direct child must be reaped before returning"
        );
        let _ = std::fs::remove_file(marker);
    }

    #[test]
    fn concurrent_complete_producer_writes_leave_one_valid_snapshot() {
        let root = test_root("concurrent");
        let mut workers = Vec::new();
        for worker in 0..4u16 {
            let root = root.clone();
            workers.push(std::thread::spawn(move || {
                for sequence in 0..8u16 {
                    let mut snapshot = entries();
                    snapshot[0].terminal_count = (worker + sequence) % 3;
                    write_snapshot_in(&root, snapshot).unwrap();
                }
            }));
        }
        for worker in workers {
            worker.join().unwrap();
        }

        let final_snapshot = devbox_integration::read_snapshot_in(&root, PRODUCER_ID, 1)
            .unwrap()
            .unwrap();
        let runtime = final_snapshot
            .views()
            .unwrap()
            .remove(RUNTIME_VIEW_KIND)
            .unwrap();
        assert_eq!(runtime.entries.len(), 1);
        assert!(runtime.entries[0]["terminalCount"].as_u64().unwrap() < 3);
        let files = std::fs::read_dir(devbox_integration::snapshot_dir_in(&root, PRODUCER_ID, 1))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        assert_eq!(files, vec!["summary.json"]);
        let _ = std::fs::remove_dir_all(root);
    }
}
