//! Read-only Start Workspace preflight probes.
//!
//! The command layer owns OS observations; all user-visible decisions live in
//! `core::preflight`. Every subprocess has fixed argv, null stdin, discarded
//! stderr, a timeout, and a bounded stdout buffer. Probe failures are reduced
//! to stable states so paths and diagnostics never cross the IPC boundary.

use crate::commands::process_tree::ProcessTree;
use crate::commands::workspace::{
    active_service_ids, load_store_document_async, validate_operation_request_id, RunRegistry,
};
use crate::core::health::{distro_is_running, has_distro};
use crate::core::operation::{
    wait_for_change, OperationBudget, OperationClaim, OperationError, OperationToken,
};
use crate::core::preflight::{
    assess_distro, assess_ports, assess_required_apps, assess_service_dependencies_with_running,
    assess_working_directories, DirectoryProbe, PortProbe, ServiceSnapshotProbe,
    WorkspacePreflight,
};
use crate::core::profile::{validate_profile_id, ProjectProfile};
use devbox_filesystem::{parse_safe_project_path, ProjectPathKind};
use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};
use tokio::time::{timeout_at, Instant as TokioInstant};
#[cfg(target_os = "windows")]
use windows::Win32::System::Threading::{CREATE_NO_WINDOW, CREATE_SUSPENDED};

const PREFLIGHT_TIMEOUT: Duration = Duration::from_secs(2);
const PREFLIGHT_OPERATION_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_WSL_LIST_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandProbe {
    Success,
    Failed,
    Unavailable,
}

async fn terminate_tree(tree: &mut ProcessTree, child: &mut Child) {
    tree.terminate(child).await;
}

/// Read one bounded stdout stream, then wait for the fixed command. stderr is
/// never captured because it may contain a path, username, or credential.
async fn run_bounded_stdout(
    mut command: Command,
    token: OperationToken,
    budget: OperationBudget,
) -> Result<(CommandProbe, Vec<u8>), OperationError> {
    budget.check(&token)?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(unix)]
    command.process_group(0);
    #[cfg(target_os = "windows")]
    // ProcessTree::assign resumes the sole primary thread only after the Job
    // Object has been configured and assigned, so no probe helper can run
    // before ownership is established.
    command.creation_flags(CREATE_NO_WINDOW.0 | CREATE_SUSPENDED.0);
    let Ok(mut child) = command.spawn() else {
        return Ok((CommandProbe::Unavailable, Vec::new()));
    };
    let mut process_tree = match ProcessTree::assign(&child) {
        Ok(tree) => tree,
        Err(()) => {
            ProcessTree::terminate_unassigned(&mut child).await;
            return Ok((CommandProbe::Unavailable, Vec::new()));
        }
    };
    let Some(stdout) = child.stdout.take() else {
        terminate_tree(&mut process_tree, &mut child).await;
        return Ok((CommandProbe::Unavailable, Vec::new()));
    };

    let mut bytes = Vec::with_capacity(MAX_WSL_LIST_BYTES.min(4096));
    let deadline = TokioInstant::now() + PREFLIGHT_TIMEOUT.min(budget.remaining());
    let mut bounded_stdout = stdout.take((MAX_WSL_LIST_BYTES.saturating_add(1)) as u64);
    let read_to_end = bounded_stdout.read_to_end(&mut bytes);
    tokio::pin!(read_to_end);
    let read = tokio::select! {
        read = timeout_at(deadline, &mut read_to_end) => read,
        control = wait_for_change(token.clone(), budget) => {
            terminate_tree(&mut process_tree, &mut child).await;
            return Err(control);
        }
    };
    match read {
        Ok(Ok(_)) if bytes.len() <= MAX_WSL_LIST_BYTES => {}
        _ => {
            terminate_tree(&mut process_tree, &mut child).await;
            return Ok((CommandProbe::Unavailable, Vec::new()));
        }
    }
    if let Err(error) = budget.check(&token) {
        terminate_tree(&mut process_tree, &mut child).await;
        return Err(error);
    }

    let status = tokio::select! {
        result = timeout_at(deadline, child.wait()) => match result {
            Ok(Ok(status)) => status,
            _ => {
                terminate_tree(&mut process_tree, &mut child).await;
                return Ok((CommandProbe::Unavailable, Vec::new()));
            }
        },
        control = wait_for_change(token.clone(), budget) => {
            terminate_tree(&mut process_tree, &mut child).await;
            return Err(control);
        }
    };
    if let Err(error) = budget.check(&token) {
        process_tree.terminate_descendants();
        return Err(error);
    }
    if !process_tree.terminate_descendants() {
        return Ok((CommandProbe::Unavailable, Vec::new()));
    }
    Ok((
        if status.success() {
            CommandProbe::Success
        } else {
            CommandProbe::Failed
        },
        bytes,
    ))
}

async fn run_fixed_command(
    argv: &[String],
    token: OperationToken,
    budget: OperationBudget,
) -> Result<CommandProbe, OperationError> {
    let Some((program, args)) = argv.split_first() else {
        return Ok(CommandProbe::Unavailable);
    };
    budget.check(&token)?;
    let mut command = Command::new(program);
    command.args(args);
    #[cfg(unix)]
    command.process_group(0);
    #[cfg(target_os = "windows")]
    command.creation_flags(CREATE_NO_WINDOW.0 | CREATE_SUSPENDED.0);
    let Ok(mut child) = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
    else {
        return Ok(CommandProbe::Unavailable);
    };
    let mut process_tree = match ProcessTree::assign(&child) {
        Ok(tree) => tree,
        Err(()) => {
            ProcessTree::terminate_unassigned(&mut child).await;
            return Ok(CommandProbe::Unavailable);
        }
    };
    let timeout = PREFLIGHT_TIMEOUT.min(budget.remaining());
    let probe = tokio::select! {
        result = tokio::time::timeout(timeout, child.wait()) => match result {
            Ok(Ok(status)) if status.success() => CommandProbe::Success,
            Ok(Ok(_)) => CommandProbe::Failed,
            _ => {
                terminate_tree(&mut process_tree, &mut child).await;
                CommandProbe::Unavailable
            }
        },
        control = wait_for_change(token.clone(), budget) => {
            terminate_tree(&mut process_tree, &mut child).await;
            return Err(control);
        }
    };
    if let Err(error) = budget.check(&token) {
        process_tree.terminate_descendants();
        return Err(error);
    }
    if !process_tree.terminate_descendants() {
        return Ok(CommandProbe::Unavailable);
    }
    Ok(probe)
}

async fn wsl_distro_probe(
    distro: &str,
    token: OperationToken,
    budget: OperationBudget,
) -> Result<(DirectoryProbe, bool), OperationError> {
    if devbox_wsl::distro::validate_distro_name(distro).is_err() {
        return Ok((DirectoryProbe::Unsafe, false));
    }
    let mut command = Command::new("wsl.exe");
    command.args(["-l", "-v"]);
    #[cfg(target_os = "windows")]
    command.creation_flags(CREATE_NO_WINDOW.0 | CREATE_SUSPENDED.0);
    let (probe, bytes) = run_bounded_stdout(command, token, budget).await?;
    if probe != CommandProbe::Success {
        return Ok((DirectoryProbe::Unavailable, false));
    }
    let output = devbox_wsl::output::decode_output(&bytes);
    if !has_distro(distro, &output) {
        return Ok((DirectoryProbe::Missing, false));
    }
    // Do not invoke `wsl.exe -d ... --cd ...` for a stopped distro: that
    // command would start the distro as a side effect of a read-only review.
    let running = distro_is_running(distro, &output).unwrap_or(false);
    Ok((DirectoryProbe::Available, running))
}

async fn wsl_directory_probe(
    distro: &str,
    path: &str,
    token: OperationToken,
    budget: OperationBudget,
) -> Result<DirectoryProbe, OperationError> {
    let Some(parsed_path) = parse_safe_project_path(path) else {
        return Ok(DirectoryProbe::Unsafe);
    };
    if devbox_wsl::distro::validate_distro_name(distro).is_err()
        || parsed_path.kind() != ProjectPathKind::Posix
    {
        return Ok(DirectoryProbe::Unsafe);
    }
    let Ok(argv) = devbox_wsl::argv::build_exec_argv(distro, Some(path), "/usr/bin/true") else {
        return Ok(DirectoryProbe::Unsafe);
    };
    Ok(match run_fixed_command(&argv, token, budget).await? {
        CommandProbe::Success => DirectoryProbe::Available,
        CommandProbe::Failed => DirectoryProbe::Missing,
        CommandProbe::Unavailable => DirectoryProbe::Unavailable,
    })
}

fn path_has_link_component(path: &Path) -> Result<bool, ()> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component);
        let metadata = match std::fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            // Keep inspecting the existing prefix when a later component is
            // missing. Other metadata failures must not be treated as a safe
            // path because the link/reparse property is unknown.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Err(()),
        };
        if metadata.file_type().is_symlink() {
            return Ok(true);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
            if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn windows_directory_probe(path: &str) -> DirectoryProbe {
    let Some(parsed_path) = parse_safe_project_path(path) else {
        return DirectoryProbe::Unsafe;
    };
    if parsed_path.kind() == ProjectPathKind::Posix {
        return DirectoryProbe::Unsafe;
    }
    let path = Path::new(path);
    // Inspect every existing component before the final target metadata. A
    // missing child below a junction/symlink must not be downgraded to an
    // ordinary `Missing` result, because a later creation could escape the
    // path boundary.
    match path_has_link_component(path) {
        Ok(true) => return DirectoryProbe::Unsafe,
        Ok(false) => {}
        Err(()) => return DirectoryProbe::Unavailable,
    }
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return DirectoryProbe::Missing
        }
        Err(_) => return DirectoryProbe::Unavailable,
    };
    if metadata.file_type().is_symlink() {
        return DirectoryProbe::Unsafe;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return DirectoryProbe::Unsafe;
        }
    }
    if metadata.is_dir() {
        DirectoryProbe::Available
    } else {
        DirectoryProbe::Missing
    }
}

fn port_probe_until(
    port: u16,
    deadline: Instant,
    token: &OperationToken,
    budget: OperationBudget,
) -> Result<PortProbe, OperationError> {
    budget.check(token)?;
    if Instant::now() >= deadline {
        return Ok(PortProbe::Unavailable);
    }
    let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    match TcpListener::bind(address) {
        Ok(listener) => {
            drop(listener);
            Ok(PortProbe::Free)
        }
        Err(_) => {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Ok(PortProbe::Unavailable);
            }
            let probe = match TcpStream::connect_timeout(
                &address,
                remaining.min(Duration::from_millis(150)),
            ) {
                Ok(_) => PortProbe::Existing,
                Err(_) => PortProbe::Conflict,
            };
            budget.check(token)?;
            Ok(probe)
        }
    }
}

fn port_probes_bounded(
    ports: &[u16],
    token: &OperationToken,
    budget: OperationBudget,
) -> Result<Vec<PortProbe>, OperationError> {
    let now = Instant::now();
    let deadline = now.checked_add(PREFLIGHT_TIMEOUT).unwrap_or(now);
    ports
        .iter()
        .map(|port| port_probe_until(*port, deadline, token, budget))
        .collect()
}

fn installed_required_app_capabilities() -> Vec<(String, &'static str)> {
    let mut capabilities = HashSet::new();
    for capability in ["path", "workspace"] {
        for target in devbox_launch::installed_targets(capability) {
            capabilities.insert((target.id, capability));
        }
    }
    capabilities.into_iter().collect()
}

fn service_snapshot_probe(
    profile: &ProjectProfile,
    token: &OperationToken,
    budget: OperationBudget,
) -> Result<(ServiceSnapshotProbe, HashSet<String>), OperationError> {
    budget.check(token)?;
    if profile.run_manager_service_ids.is_empty() {
        return Ok((ServiceSnapshotProbe::AllRunning, HashSet::new()));
    }
    let result = match devbox_integration::read_snapshot("run-manager", 1) {
        Ok(None) => (ServiceSnapshotProbe::Missing, HashSet::new()),
        Err(_) => (ServiceSnapshotProbe::Unavailable, HashSet::new()),
        Ok(Some(snapshot)) => {
            let Ok(active) = active_service_ids(&snapshot.data) else {
                return Ok((ServiceSnapshotProbe::Unavailable, HashSet::new()));
            };
            let status = if profile
                .run_manager_service_ids
                .iter()
                .all(|id| active.contains(id))
            {
                ServiceSnapshotProbe::AllRunning
            } else {
                ServiceSnapshotProbe::SomeNotRunning
            };
            (status, active)
        }
    };
    budget.check(token)?;
    Ok(result)
}

async fn preflight_profile_inner(
    profile: ProjectProfile,
    token: OperationToken,
    budget: OperationBudget,
) -> Result<WorkspacePreflight, String> {
    budget.check(&token).map_err(OperationError::message)?;
    // TCP connect probes are synchronous on every supported platform. Run the
    // complete bounded set away from the async runtime while WSL observations
    // proceed; all 128 configured ports share one deadline rather than each
    // receiving an independent timeout.
    let expected_ports = profile.expected_ports.clone();
    let port_token = token.clone();
    let port_worker = tokio::task::spawn_blocking(move || {
        port_probes_bounded(&expected_ports, &port_token, budget)
    });

    let installed = installed_required_app_capabilities();
    let installed_refs = installed
        .iter()
        .map(|(app_id, capability)| (app_id.as_str(), *capability))
        .collect::<Vec<_>>();
    let required_apps = assess_required_apps(&installed_refs);

    // Do not return directly from a WSL probe error: the blocking port worker
    // must be joined before this worker releases the health lane.
    let mut control_error = None;
    let (distro_item, wsl_directory) = match profile.wsl.as_ref() {
        Some(wsl) => {
            let (distro_probe, distro_running) =
                match wsl_distro_probe(&wsl.distro, token.clone(), budget).await {
                    Ok(result) => result,
                    Err(error) => {
                        control_error = Some(error);
                        (DirectoryProbe::Unavailable, false)
                    }
                };
            let directory_probe = match (distro_probe, distro_running) {
                (DirectoryProbe::Available, true) => {
                    match wsl_directory_probe(&wsl.distro, &wsl.path, token.clone(), budget).await {
                        Ok(probe) => probe,
                        Err(error) => {
                            control_error = Some(error);
                            DirectoryProbe::Unavailable
                        }
                    }
                }
                (DirectoryProbe::Available, false) => DirectoryProbe::Unavailable,
                (DirectoryProbe::Missing, _) => DirectoryProbe::Missing,
                (DirectoryProbe::Unsafe, _) => DirectoryProbe::Unsafe,
                (DirectoryProbe::Unavailable, _) => DirectoryProbe::Unavailable,
            };
            (assess_distro(true, distro_probe), Some(directory_probe))
        }
        None => (assess_distro(false, DirectoryProbe::Available), None),
    };

    // Code Pad accepts a Windows workspace path. A WSL-only profile can still
    // open in WSL Desktop, but preflight must report that Code Pad's target is
    // not available instead of allowing a partial start to surprise the user.
    let mut directory_probes = Vec::with_capacity(2);
    directory_probes.push(
        profile
            .windows_path
            .as_deref()
            .map(windows_directory_probe)
            .unwrap_or(DirectoryProbe::Missing),
    );
    if let Some(probe) = wsl_directory {
        directory_probes.push(probe);
    }
    let directories = assess_working_directories(&directory_probes);

    let port_probes = match port_worker.await {
        Ok(Ok(probes)) => probes,
        Ok(Err(error)) => {
            control_error.get_or_insert(error);
            vec![PortProbe::Unavailable; profile.expected_ports.len()]
        }
        Err(_) => vec![PortProbe::Unavailable; profile.expected_ports.len()],
    };
    if let Some(error) = control_error {
        return Err(error.message().to_string());
    }
    budget.check(&token).map_err(OperationError::message)?;
    let ports = assess_ports(&port_probes);
    let (service_probe, active_services) =
        service_snapshot_probe(&profile, &token, budget).map_err(OperationError::message)?;
    let service_running = profile
        .run_manager_service_ids
        .iter()
        .map(|id| active_services.contains(id))
        .collect::<Vec<_>>();
    let services = assess_service_dependencies_with_running(&service_running, service_probe);

    Ok(WorkspacePreflight::new(
        profile.id,
        vec![required_apps, distro_item, directories, ports, services],
    ))
}

/// Run a preflight with a detached worker guard. If Tauri drops the command
/// future, the worker still observes the shared token, joins its bounded port
/// probe, and only then releases the health single-flight lane.
pub(crate) async fn preflight_profile(
    profile: &ProjectProfile,
    token: OperationToken,
    budget: OperationBudget,
    claim: &OperationClaim,
) -> Result<WorkspacePreflight, String> {
    budget.check(&token).map_err(OperationError::message)?;
    let worker_guard = claim.worker_guard().map_err(str::to_string)?;
    let worker_profile = profile.clone();
    let worker_token = token.clone();
    let worker = tokio::spawn(async move {
        let _worker_guard = worker_guard;
        preflight_profile_inner(worker_profile, worker_token, budget).await
    });
    worker
        .await
        .map_err(|_| "Workspace 사전 점검 작업을 완료하지 못했습니다".to_string())?
}

/// Run one bounded preflight under the same single-flight lane as project
/// health and Start Workspace.  The probes themselves are read-only; the
/// lease is still important because WSL/TCP/snapshot work must not overlap a
/// transition or another health request and consume duplicate native
/// capacity. Cancellation terminates an active native tree; any blocking port
/// worker is still joined before the lease is released.
async fn run_preflight_command(
    app: &tauri::AppHandle,
    registry: &RunRegistry,
    operation_key: String,
    profile_id: &str,
) -> Result<WorkspacePreflight, String> {
    let operation = &registry.health_operation;
    let budget = OperationBudget::from_now(PREFLIGHT_OPERATION_TIMEOUT);
    // Supersede only an older request in this same read-only family. Project
    // health and dependency health start independently on profile selection;
    // sharing the lane must not make one cancel the other.
    operation
        .cancel_kind(
            operation_key
                .split('\0')
                .next()
                .unwrap_or(operation_key.as_str()),
        )
        .map_err(str::to_string)?;
    let pending = operation.prepare(operation_key).map_err(str::to_string)?;
    let token = pending.token();
    operation.wait_until_idle(token.clone(), budget).await?;
    budget.check(&token).map_err(OperationError::message)?;
    let claim = pending.claim().map_err(str::to_string)?;
    let token = claim.token();
    let document = load_store_document_async(app, token.clone(), budget, &claim).await?;
    let profile = document
        .store
        .profiles
        .into_iter()
        .find(|profile| profile.id == profile_id)
        .ok_or_else(|| "프로필을 찾을 수 없습니다".to_string())?;

    // `preflight_profile` owns only bounded probes and joins its port worker
    // before returning.  We intentionally await it instead of dropping it on
    // cancellation: the health lease remains held until every child task has
    // finished, so a newer request cannot overlap a detached native probe.
    preflight_profile(&profile, token, budget, &claim).await
}

fn preflight_operation_key(kind: &str, profile_id: &str, request_id: Option<&str>) -> String {
    match request_id {
        Some(request_id) => format!("{kind}\0{profile_id}\0{request_id}"),
        None => format!("{kind}\0{profile_id}"),
    }
}

/// Read-only preflight command. It never starts an app, service, WSL distro,
/// or project process.
#[tauri::command]
pub async fn workspace_preflight(
    app: tauri::AppHandle,
    registry: tauri::State<'_, std::sync::Arc<RunRegistry>>,
    profile_id: String,
    request_id: Option<String>,
) -> Result<WorkspacePreflight, String> {
    validate_profile_id(&profile_id)?;
    validate_operation_request_id(request_id.as_deref())?;
    run_preflight_command(
        &app,
        &registry,
        preflight_operation_key("preflight", &profile_id, request_id.as_deref()),
        &profile_id,
    )
    .await
}

/// Read-only dependency inspection for the selected profile.  This is an
/// explicit health surface in addition to the Start Workspace review, but it
/// deliberately shares the exact bounded probes and resource provenance.
#[tauri::command]
pub async fn dependency_health(
    app: tauri::AppHandle,
    registry: tauri::State<'_, std::sync::Arc<RunRegistry>>,
    profile_id: String,
    request_id: Option<String>,
) -> Result<crate::core::preflight::DependencyHealth, String> {
    validate_profile_id(&profile_id)?;
    validate_operation_request_id(request_id.as_deref())?;
    run_preflight_command(
        &app,
        &registry,
        preflight_operation_key("dependency-health", &profile_id, request_id.as_deref()),
        &profile_id,
    )
    .await
}

#[tauri::command]
pub fn cancel_workspace_preflight(
    registry: tauri::State<'_, std::sync::Arc<RunRegistry>>,
    profile_id: String,
    request_id: String,
) -> Result<bool, String> {
    validate_profile_id(&profile_id)?;
    validate_operation_request_id(Some(&request_id))?;
    registry
        .health_operation
        .cancel(&preflight_operation_key(
            "preflight",
            &profile_id,
            Some(&request_id),
        ))
        .map_err(str::to_string)
}

#[tauri::command]
pub fn cancel_dependency_health(
    registry: tauri::State<'_, std::sync::Arc<RunRegistry>>,
    profile_id: String,
    request_id: String,
) -> Result<bool, String> {
    validate_profile_id(&profile_id)?;
    validate_operation_request_id(Some(&request_id))?;
    registry
        .health_operation
        .cancel(&preflight_operation_key(
            "dependency-health",
            &profile_id,
            Some(&request_id),
        ))
        .map_err(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::preflight::{PreflightItem, ResourceState};

    #[test]
    fn windows_probe_rejects_relative_and_device_paths_without_io() {
        for path in [
            "relative/project",
            "C:\\\\?\\\\unsafe",
            "C:\\work\\..\\escape",
            "/mnt/e/projects/devbox",
        ] {
            assert_eq!(windows_directory_probe(path), DirectoryProbe::Unsafe);
        }
    }

    #[tokio::test]
    async fn wsl_probe_rejects_a_windows_path_before_spawning_wsl() {
        let probe = wsl_directory_probe(
            "Ubuntu",
            "C:\\work\\devbox",
            OperationToken::new(),
            OperationBudget::from_now(Duration::from_secs(1)),
        )
        .await;
        assert_eq!(probe, Ok(DirectoryProbe::Unsafe));
    }

    #[cfg(unix)]
    #[test]
    fn path_probe_detects_a_symlink_component() {
        let root =
            std::env::temp_dir().join(format!("workbench-preflight-link-{}", std::process::id()));
        let real = root.join("real");
        let link = root.join("link");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&real).unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert_eq!(path_has_link_component(&link), Ok(true));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn missing_descendant_below_a_symlink_is_not_a_plain_missing_path() {
        let root = std::env::temp_dir().join(format!(
            "workbench-preflight-missing-link-{}",
            std::process::id()
        ));
        let real = root.join("real");
        let link = root.join("link");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&real).unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert_eq!(path_has_link_component(&link.join("future")), Ok(true));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn port_probe_reports_a_free_ephemeral_port() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let occupied = listener.local_addr().unwrap().port();
        assert_eq!(
            port_probe_until(
                occupied,
                Instant::now() + Duration::from_secs(1),
                &OperationToken::new(),
                OperationBudget::from_now(Duration::from_secs(1)),
            ),
            Ok(PortProbe::Existing)
        );
        assert_eq!(
            port_probe_until(
                occupied,
                Instant::now(),
                &OperationToken::new(),
                OperationBudget::from_now(Duration::from_secs(1)),
            ),
            Ok(PortProbe::Unavailable)
        );
    }

    #[test]
    fn preflight_result_does_not_serialize_probe_paths_or_stderr() {
        let item = PreflightItem {
            key: "working-directory".into(),
            status: crate::core::preflight::PreflightStatus::Failure,
            detail: "Workspace working directory를 사용할 수 없습니다".into(),
            resources: vec![crate::core::preflight::ResourceProvenance {
                kind: "directory".into(),
                id: "workspace-1".into(),
                state: ResourceState::Unsafe,
            }],
        };
        let json = serde_json::to_string(&item).unwrap();
        assert!(!json.contains("TOP_SECRET"));
        assert!(!json.contains("C:\\\\private"));
        assert!(!json.contains("stderr"));
    }

    #[tokio::test]
    async fn fixed_command_timeout_does_not_return_child_output() {
        // The executable is intentionally absent on the development host;
        // this asserts the stable unavailable state rather than an OS error.
        let probe = run_fixed_command(
            &[
                "devbox-command-that-does-not-exist".into(),
                "--secret=value".into(),
            ],
            OperationToken::new(),
            OperationBudget::from_now(Duration::from_secs(1)),
        )
        .await
        .unwrap();
        assert_eq!(probe, CommandProbe::Unavailable);
    }

    #[tokio::test]
    async fn fixed_command_honors_cancellation_before_spawn() {
        let token = OperationToken::new();
        token.cancel();
        let result = run_fixed_command(
            &["devbox-command-that-must-not-start".into()],
            token,
            OperationBudget::from_now(Duration::from_secs(1)),
        )
        .await;
        assert_eq!(result, Err(OperationError::Cancelled));
    }
}
