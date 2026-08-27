//! Read-only Start Workspace preflight probes.
//!
//! The command layer owns OS observations; all user-visible decisions live in
//! `core::preflight`. Every subprocess has fixed argv, null stdin, discarded
//! stderr, a timeout, and a bounded stdout buffer. Probe failures are reduced
//! to stable states so paths and diagnostics never cross the IPC boundary.

use crate::commands::workspace::{active_service_ids, load_store};
use crate::core::health::{distro_is_running, has_distro};
use crate::core::preflight::{
    assess_distro, assess_ports, assess_required_apps, assess_service_dependencies_with_running,
    assess_working_directories, DirectoryProbe, PortProbe, ServiceSnapshotProbe,
    WorkspacePreflight,
};
use crate::core::profile::{validate_profile_id, ProjectProfile};
use devbox_filesystem::parse_safe_project_path;
use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};
use tokio::time::{timeout_at, Instant as TokioInstant};

const PREFLIGHT_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_WSL_LIST_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandProbe {
    Success,
    Failed,
    Unavailable,
}

async fn terminate(child: &mut Child) {
    let _ = child.kill().await;
    let _ = child.wait().await;
}

/// Read one bounded stdout stream, then wait for the fixed command. stderr is
/// never captured because it may contain a path, username, or credential.
async fn run_bounded_stdout(mut command: Command) -> Result<(CommandProbe, Vec<u8>), ()> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command.spawn().map_err(|_| ())?;
    let Some(stdout) = child.stdout.take() else {
        terminate(&mut child).await;
        return Err(());
    };

    let mut bytes = Vec::with_capacity(MAX_WSL_LIST_BYTES.min(4096));
    let deadline = TokioInstant::now() + PREFLIGHT_TIMEOUT;
    let read = timeout_at(
        deadline,
        stdout
            .take((MAX_WSL_LIST_BYTES.saturating_add(1)) as u64)
            .read_to_end(&mut bytes),
    )
    .await;
    match read {
        Ok(Ok(_)) if bytes.len() <= MAX_WSL_LIST_BYTES => {}
        _ => {
            terminate(&mut child).await;
            return Ok((CommandProbe::Unavailable, Vec::new()));
        }
    }

    let status = match timeout_at(deadline, child.wait()).await {
        Ok(Ok(status)) => status,
        _ => {
            terminate(&mut child).await;
            return Ok((CommandProbe::Unavailable, Vec::new()));
        }
    };
    Ok((
        if status.success() {
            CommandProbe::Success
        } else {
            CommandProbe::Failed
        },
        bytes,
    ))
}

async fn run_fixed_command(argv: &[String]) -> CommandProbe {
    let Some((program, args)) = argv.split_first() else {
        return CommandProbe::Unavailable;
    };
    let mut command = Command::new(program);
    command.args(args);
    #[cfg(target_os = "windows")]
    command.creation_flags(0x0800_0000);
    let Ok(mut child) = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return CommandProbe::Unavailable;
    };
    match tokio::time::timeout(PREFLIGHT_TIMEOUT, child.wait()).await {
        Ok(Ok(status)) if status.success() => CommandProbe::Success,
        Ok(Ok(_)) => CommandProbe::Failed,
        _ => {
            terminate(&mut child).await;
            CommandProbe::Unavailable
        }
    }
}

async fn wsl_distro_probe(distro: &str) -> (DirectoryProbe, bool) {
    if devbox_wsl::distro::validate_distro_name(distro).is_err() {
        return (DirectoryProbe::Unsafe, false);
    }
    let mut command = Command::new("wsl.exe");
    command.args(["-l", "-v"]);
    #[cfg(target_os = "windows")]
    command.creation_flags(0x0800_0000);
    let Ok((probe, bytes)) = run_bounded_stdout(command).await else {
        return (DirectoryProbe::Unavailable, false);
    };
    if probe != CommandProbe::Success {
        return (DirectoryProbe::Unavailable, false);
    }
    let output = devbox_wsl::output::decode_output(&bytes);
    if !has_distro(distro, &output) {
        return (DirectoryProbe::Missing, false);
    }
    // Do not invoke `wsl.exe -d ... --cd ...` for a stopped distro: that
    // command would start the distro as a side effect of a read-only review.
    let running = distro_is_running(distro, &output).unwrap_or(false);
    (DirectoryProbe::Available, running)
}

async fn wsl_directory_probe(distro: &str, path: &str) -> DirectoryProbe {
    if devbox_wsl::distro::validate_distro_name(distro).is_err()
        || parse_safe_project_path(path).is_none()
    {
        return DirectoryProbe::Unsafe;
    }
    let Ok(argv) = devbox_wsl::argv::build_exec_argv(distro, Some(path), "/usr/bin/true") else {
        return DirectoryProbe::Unsafe;
    };
    match run_fixed_command(&argv).await {
        CommandProbe::Success => DirectoryProbe::Available,
        CommandProbe::Failed => DirectoryProbe::Missing,
        CommandProbe::Unavailable => DirectoryProbe::Unavailable,
    }
}

fn path_has_link_component(path: &Path) -> Result<bool, ()> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component);
        let metadata = match std::fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            // A missing parent is already covered by the target metadata
            // check. Other metadata failures must not be treated as a safe
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
    if parse_safe_project_path(path).is_none() {
        return DirectoryProbe::Unsafe;
    }
    let path = Path::new(path);
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
    match path_has_link_component(path) {
        Ok(true) => return DirectoryProbe::Unsafe,
        Ok(false) => {}
        Err(()) => return DirectoryProbe::Unavailable,
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

fn port_probe(port: u16) -> PortProbe {
    let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    match TcpListener::bind(address) {
        Ok(listener) => {
            drop(listener);
            PortProbe::Free
        }
        Err(_) => match TcpStream::connect_timeout(&address, Duration::from_millis(150)) {
            Ok(_) => PortProbe::Existing,
            Err(_) => PortProbe::Conflict,
        },
    }
}

fn installed_required_app_ids() -> Vec<String> {
    let mut ids = HashSet::new();
    for capability in ["path", "workspace"] {
        for target in devbox_launch::installed_targets(capability) {
            ids.insert(target.id);
        }
    }
    ids.into_iter().collect()
}

fn service_snapshot_probe(profile: &ProjectProfile) -> (ServiceSnapshotProbe, HashSet<String>) {
    if profile.run_manager_service_ids.is_empty() {
        return (ServiceSnapshotProbe::AllRunning, HashSet::new());
    }
    match devbox_integration::read_snapshot("run-manager", 1) {
        Ok(None) => (ServiceSnapshotProbe::Missing, HashSet::new()),
        Err(_) => (ServiceSnapshotProbe::Unavailable, HashSet::new()),
        Ok(Some(snapshot)) => {
            let Ok(active) = active_service_ids(&snapshot.data) else {
                return (ServiceSnapshotProbe::Unavailable, HashSet::new());
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
    }
}

pub(crate) async fn preflight_profile(profile: &ProjectProfile) -> WorkspacePreflight {
    let installed = installed_required_app_ids();
    let installed_refs = installed.iter().map(String::as_str).collect::<Vec<_>>();
    let required_apps = assess_required_apps(&installed_refs);

    let (distro_item, wsl_directory) = match profile.wsl.as_ref() {
        Some(wsl) => {
            let (distro_probe, distro_running) = wsl_distro_probe(&wsl.distro).await;
            let directory_probe = match (distro_probe, distro_running) {
                (DirectoryProbe::Available, true) => {
                    wsl_directory_probe(&wsl.distro, &wsl.path).await
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

    let port_probes = profile
        .expected_ports
        .iter()
        .copied()
        .map(port_probe)
        .collect::<Vec<_>>();
    let ports = assess_ports(&port_probes);
    let (service_probe, active_services) = service_snapshot_probe(profile);
    let service_running = profile
        .run_manager_service_ids
        .iter()
        .map(|id| active_services.contains(id))
        .collect::<Vec<_>>();
    let services = assess_service_dependencies_with_running(&service_running, service_probe);

    WorkspacePreflight::new(
        profile.id.clone(),
        vec![required_apps, distro_item, directories, ports, services],
    )
}

/// Read-only preflight command. It never starts an app, service, WSL distro,
/// or project process.
#[tauri::command]
pub async fn workspace_preflight(
    app: tauri::AppHandle,
    profile_id: String,
) -> Result<WorkspacePreflight, String> {
    validate_profile_id(&profile_id)?;
    let profile = load_store(&app)?
        .profiles
        .into_iter()
        .find(|profile| profile.id == profile_id)
        .ok_or_else(|| "프로필을 찾을 수 없습니다".to_string())?;
    Ok(preflight_profile(&profile).await)
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
        ] {
            assert_eq!(windows_directory_probe(path), DirectoryProbe::Unsafe);
        }
    }

    #[cfg(unix)]
    #[test]
    fn windows_probe_rejects_a_symlink_component() {
        let root =
            std::env::temp_dir().join(format!("workbench-preflight-link-{}", std::process::id()));
        let real = root.join("real");
        let link = root.join("link");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&real).unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert_eq!(
            windows_directory_probe(link.to_str().unwrap()),
            DirectoryProbe::Unsafe
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn port_probe_reports_a_free_ephemeral_port() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let occupied = listener.local_addr().unwrap().port();
        assert_eq!(port_probe(occupied), PortProbe::Existing);
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
        let probe = run_fixed_command(&[
            "devbox-command-that-does-not-exist".into(),
            "--secret=value".into(),
        ])
        .await;
        assert_eq!(probe, CommandProbe::Unavailable);
    }
}
