//! Native project `.env` preview returns bounded metadata and masked values.
use crate::commands::workspace::RunRegistry;
use crate::core::environment::{
    parse_environment, preview, EnvironmentError, ParsedEnvironment, ProjectEnvironmentPreview,
    MAX_ENV_FILE_BYTES,
};
use crate::core::operation::{wait_for_change, OperationBudget, OperationError, OperationToken};
use crate::core::profile::WslProfile;
use devbox_filesystem::{parse_safe_project_path, ProjectPathKind};
use serde::Deserialize;
use std::fs::Metadata;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;
use zeroize::Zeroizing;

const ENVIRONMENT_READ_ERROR: &str = "환경 파일을 안전하게 읽을 수 없습니다";
const MAX_WSL_DISTRO_CHARS: usize = 128;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectEnvironmentPreviewRequest {
    #[serde(default)]
    pub windows_path: Option<String>,
    #[serde(default)]
    pub wsl: Option<WslProfile>,
    pub source: String,
    #[serde(default)]
    pub request_id: Option<String>,
}

/// Preview a user-selected source.  Paths are accepted only as project root
/// plus a `.env` filename; no absolute source path is returned.
pub async fn preview_project_environment(
    registry: tauri::State<'_, std::sync::Arc<RunRegistry>>,
    request: ProjectEnvironmentPreviewRequest,
) -> Result<ProjectEnvironmentPreview, String> {
    let request_id = request
        .request_id
        .as_deref()
        .filter(|value| {
            !value.is_empty() && value.len() <= 128 && !value.chars().any(char::is_control)
        })
        .ok_or_else(|| EnvironmentError::InvalidSource.to_string())?;
    let operation = &registry.preview_operation;
    let budget = OperationBudget::from_now(Duration::from_secs(5));
    operation.cancel_active().map_err(str::to_string)?;
    let pending = operation
        .prepare(request_id.to_string())
        .map_err(str::to_string)?;
    let token = pending.token();
    operation.wait_until_idle(token.clone(), budget).await?;
    budget.check(&token).map_err(OperationError::message)?;
    let claim = pending.claim().map_err(str::to_string)?;
    let token = claim.token();
    let worker_guard = claim.worker_guard().map_err(str::to_string)?;
    let worker_token = token.clone();
    let worker = tokio::task::spawn_blocking(move || {
        let _worker_guard = worker_guard;
        read_request_source_with_control(&request, &worker_token, budget)
            .map(|parsed| preview(&parsed))
            .map_err(|error| error.to_string())
    });
    tokio::pin!(worker);
    let result = tokio::select! {
        result = &mut worker => result.map_err(|_| ENVIRONMENT_READ_ERROR.to_string())?,
        control = wait_for_change(token.clone(), budget) => {
            token.cancel();
            let _ = worker.await;
            Err(control.message().to_string())
        }
    }?;
    budget.check(&token).map_err(OperationError::message)?;
    Ok(result)
}

/// Cancel the currently active preview request. The request id is generated
/// and retained by the frontend API wrapper; native work observes the same
/// sticky bit as its file reader.
pub fn cancel_project_environment(
    registry: tauri::State<'_, std::sync::Arc<RunRegistry>>,
    request_id: String,
) -> Result<bool, String> {
    if request_id.is_empty() || request_id.len() > 128 || request_id.chars().any(char::is_control) {
        return Err(EnvironmentError::InvalidSource.to_string());
    }
    registry
        .preview_operation
        .cancel(&request_id)
        .map_err(str::to_string)
}

#[cfg(test)]
fn read_request_source(
    request: &ProjectEnvironmentPreviewRequest,
) -> Result<ParsedEnvironment, EnvironmentError> {
    let token = OperationToken::new();
    let budget = OperationBudget::from_now(Duration::from_secs(5));
    read_request_source_with_control(request, &token, budget)
}

fn read_request_source_with_control(
    request: &ProjectEnvironmentPreviewRequest,
    token: &OperationToken,
    budget: OperationBudget,
) -> Result<ParsedEnvironment, EnvironmentError> {
    budget.check(token).map_err(environment_operation_error)?;
    let root = project_root(request.windows_path.as_deref(), request.wsl.as_ref())
        .map_err(|_| EnvironmentError::InvalidSource)?;
    read_source_file_with_root_control(
        &root.path,
        &request.source,
        token,
        budget,
        Some(root.identity),
    )
}

struct ProjectRoot {
    path: PathBuf,
    identity: crate::platform::FileIdentity,
}

fn project_root(
    windows_path: Option<&str>,
    wsl: Option<&WslProfile>,
) -> Result<ProjectRoot, EnvironmentError> {
    #[cfg_attr(not(windows), allow(unused_variables))]
    let windows = windows_path
        .filter(|path| !path.trim().is_empty())
        .map(|path| parse_safe_project_path(path).ok_or(EnvironmentError::InvalidSource))
        .transpose()?
        .filter(|safe| safe.kind() != ProjectPathKind::Posix);
    let wsl = wsl
        .map(|wsl| {
            if wsl.distro.is_empty()
                || wsl.distro != wsl.distro.trim()
                || wsl.distro.chars().count() > MAX_WSL_DISTRO_CHARS
                || devbox_wsl::distro::validate_distro_name(&wsl.distro).is_err()
            {
                return Err(EnvironmentError::InvalidSource);
            }
            parse_safe_project_path(&wsl.path)
                .filter(|safe| safe.kind() == ProjectPathKind::Posix)
                .map(|_| wsl)
                .ok_or(EnvironmentError::InvalidSource)
        })
        .transpose()?;
    #[cfg(windows)]
    let candidate = match windows {
        Some(safe) if safe.kind() != ProjectPathKind::Posix => PathBuf::from(safe.as_str()),
        _ => match wsl {
            Some(wsl) => {
                let windows_path = devbox_wsl::path::wsl_to_windows(&wsl.distro, &wsl.path)
                    .map_err(|_| EnvironmentError::InvalidSource)?;
                let safe = parse_safe_project_path(&windows_path)
                    .filter(|safe| safe.kind() != ProjectPathKind::Posix)
                    .ok_or(EnvironmentError::InvalidSource)?;
                PathBuf::from(safe.as_str())
            }
            None => {
                // A Windows packaged build must never reinterpret a POSIX
                // string supplied in `windows_path` as a path on the current
                // drive. WSL-only profiles take the explicit conversion
                // branch above instead.
                let _ = windows_path;
                return Err(EnvironmentError::InvalidSource);
            }
        },
    };
    #[cfg(not(windows))]
    let candidate = match wsl {
        Some(wsl) => {
            // Native POSIX builds must use the validated WSL path when one is
            // present. A Windows drive/UNC string is never treated as a
            // relative path such as `C:/...` on the host filesystem.
            PathBuf::from(wsl.path.trim())
        }
        None => {
            // This branch is useful for native POSIX fixtures and profiles
            // that contain only a POSIX project path.
            let path = windows_path.ok_or(EnvironmentError::InvalidSource)?;
            let safe = parse_safe_project_path(path)
                .filter(|safe| safe.kind() == ProjectPathKind::Posix)
                .ok_or(EnvironmentError::InvalidSource)?;
            PathBuf::from(safe.as_str())
        }
    };
    let raw = candidate.as_path();
    reject_links_in_existing_path(raw)?;
    let raw_metadata =
        std::fs::symlink_metadata(raw).map_err(|_| EnvironmentError::InvalidSource)?;
    if is_link_metadata(&raw_metadata) || !raw_metadata.file_type().is_dir() {
        return Err(EnvironmentError::InvalidSource);
    }
    let raw_identity =
        crate::platform::path_identity(raw, true).map_err(|_| EnvironmentError::InvalidSource)?;
    let canonical = raw
        .canonicalize()
        .map_err(|_| EnvironmentError::InvalidSource)?;
    // The root itself is not opened as a regular file, so compare its
    // identity around canonicalization as the directory-level TOCTOU guard.
    // A junction/symlink replacement between the first link walk and
    // `canonicalize` must not become the authority for the source read.
    reject_links_in_existing_path(raw)?;
    let after_raw_metadata =
        std::fs::symlink_metadata(raw).map_err(|_| EnvironmentError::InvalidSource)?;
    let after_raw_identity =
        crate::platform::path_identity(raw, true).map_err(|_| EnvironmentError::InvalidSource)?;
    if is_link_metadata(&after_raw_metadata) || raw_identity != after_raw_identity {
        return Err(EnvironmentError::InvalidSource);
    }
    let metadata = std::fs::metadata(&canonical).map_err(|_| EnvironmentError::InvalidSource)?;
    let canonical_identity = crate::platform::path_identity(&canonical, true)
        .map_err(|_| EnvironmentError::InvalidSource)?;
    if !metadata.is_dir() || raw_identity != canonical_identity {
        return Err(EnvironmentError::InvalidSource);
    }
    reject_links_in_existing_path(&canonical)?;
    Ok(ProjectRoot {
        path: canonical,
        identity: raw_identity,
    })
}

#[cfg(test)]
fn read_source_file(root: &Path, source: &str) -> Result<ParsedEnvironment, EnvironmentError> {
    let token = OperationToken::new();
    let budget = OperationBudget::from_now(Duration::from_secs(5));
    // Production callers always pass `ProjectRoot::path`, which has already
    // been canonicalized. Keep the test-only convenience wrapper on the same
    // contract: Windows canonicalization commonly adds the `\\?\` prefix, so
    // comparing a canonical source against the raw temp-dir spelling would
    // otherwise fail the containment check even though both name the same
    // directory.
    let canonical_root = root
        .canonicalize()
        .map_err(|_| EnvironmentError::InvalidSource)?;
    read_source_file_with_control(&canonical_root, source, &token, budget)
}

#[cfg(test)]
fn read_source_file_with_control(
    root: &Path,
    source: &str,
    token: &OperationToken,
    budget: OperationBudget,
) -> Result<ParsedEnvironment, EnvironmentError> {
    read_source_file_with_root_control(root, source, token, budget, None)
}

fn read_source_file_with_root_control(
    root: &Path,
    source: &str,
    token: &OperationToken,
    budget: OperationBudget,
    expected_root: Option<crate::platform::FileIdentity>,
) -> Result<ParsedEnvironment, EnvironmentError> {
    budget.check(token).map_err(environment_operation_error)?;
    crate::core::environment::validate_source_name(source)?;
    let path = root.join(source);
    let root_metadata =
        std::fs::symlink_metadata(root).map_err(|_| EnvironmentError::InvalidSource)?;
    if is_link_metadata(&root_metadata) || !root_metadata.file_type().is_dir() {
        return Err(EnvironmentError::InvalidSource);
    }
    let root_identity =
        crate::platform::path_identity(root, true).map_err(|_| EnvironmentError::InvalidSource)?;
    if expected_root.is_some_and(|expected_identity| expected_identity != root_identity) {
        return Err(EnvironmentError::InvalidSource);
    }
    reject_links_in_existing_path(&path)?;
    let canonical_source = path
        .canonicalize()
        .map_err(|_| EnvironmentError::InvalidSource)?;
    if !canonical_source.starts_with(root) {
        return Err(EnvironmentError::InvalidSource);
    }
    reject_links_in_existing_path(&canonical_source)?;
    let metadata = std::fs::symlink_metadata(&canonical_source)
        .map_err(|_| EnvironmentError::InvalidSource)?;
    if is_link_metadata(&metadata) || !metadata.file_type().is_file() {
        return Err(EnvironmentError::InvalidSource);
    }
    if metadata.len() > MAX_ENV_FILE_BYTES as u64 {
        return Err(EnvironmentError::FileTooLarge);
    }
    // Bound the actual read as well as the metadata preflight. The file can
    // grow between those two operations; `read_to_end` must never allocate an
    // attacker-controlled amount before the parser gets a chance to reject it.
    // The bounded file buffer still contains every parsed value until the
    // metadata/preview has been produced. Zeroize the whole backing buffer on
    // drop as well as the per-entry value holders returned by the parser. A
    // fixed upper-bound capacity also prevents a file-growth race from
    // reallocating an intermediate, non-zeroized Vec backing allocation.
    budget.check(token).map_err(environment_operation_error)?;
    let source_identity = crate::platform::path_identity(&canonical_source, false)
        .map_err(|_| EnvironmentError::InvalidSource)?;
    let (file, opened_identity) =
        crate::platform::open_readonly_with_identity(&canonical_source, false)
            .map_err(|_| EnvironmentError::InvalidSource)?;
    let opened_metadata = file
        .metadata()
        .map_err(|_| EnvironmentError::InvalidSource)?;
    if is_link_metadata(&opened_metadata) || source_identity != opened_identity {
        return Err(EnvironmentError::InvalidSource);
    }
    let mut reader = file.take((MAX_ENV_FILE_BYTES + 1) as u64);
    let mut bytes = Zeroizing::new(Vec::with_capacity(MAX_ENV_FILE_BYTES + 1));
    let mut chunk = [0_u8; 8 * 1024];
    loop {
        budget.check(token).map_err(environment_operation_error)?;
        let count = reader
            .read(&mut chunk)
            .map_err(|_| EnvironmentError::InvalidSource)?;
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..count]);
        if bytes.len() > MAX_ENV_FILE_BYTES {
            return Err(EnvironmentError::FileTooLarge);
        }
    }
    if bytes.len() > MAX_ENV_FILE_BYTES {
        return Err(EnvironmentError::FileTooLarge);
    }
    let parsed = parse_environment(source, &bytes)?;
    budget.check(token).map_err(environment_operation_error)?;
    let after_root_metadata =
        std::fs::symlink_metadata(root).map_err(|_| EnvironmentError::InvalidSource)?;
    let after_root_identity =
        crate::platform::path_identity(root, true).map_err(|_| EnvironmentError::InvalidSource)?;
    if root_identity != after_root_identity
        || is_link_metadata(&after_root_metadata)
        || expected_root.is_some_and(|expected_identity| expected_identity != after_root_identity)
    {
        return Err(EnvironmentError::InvalidSource);
    }
    reject_links_in_existing_path(&path)?;
    let after_read_source = path
        .canonicalize()
        .map_err(|_| EnvironmentError::InvalidSource)?;
    if after_read_source != canonical_source || !after_read_source.starts_with(root) {
        return Err(EnvironmentError::InvalidSource);
    }
    let after_source_metadata = std::fs::symlink_metadata(&canonical_source)
        .map_err(|_| EnvironmentError::InvalidSource)?;
    let after_source_identity = crate::platform::path_identity(&canonical_source, false)
        .map_err(|_| EnvironmentError::InvalidSource)?;
    if is_link_metadata(&after_source_metadata) || source_identity != after_source_identity {
        return Err(EnvironmentError::InvalidSource);
    }
    Ok(parsed)
}

fn environment_operation_error(error: OperationError) -> EnvironmentError {
    match error {
        OperationError::Cancelled => EnvironmentError::Cancelled,
        OperationError::TimedOut => EnvironmentError::TimedOut,
    }
}

fn reject_links_in_existing_path(path: &Path) -> Result<(), EnvironmentError> {
    // Keep Windows drive prefixes and UNC roots intact while checking every
    // existing component. Component-wise PathBuf construction can otherwise
    // reinterpret `C:\\...` as the relative drive path `C:`.
    for ancestor in path.ancestors() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        let metadata =
            std::fs::symlink_metadata(ancestor).map_err(|_| EnvironmentError::InvalidSource)?;
        if is_link_metadata(&metadata) {
            return Err(EnvironmentError::InvalidSource);
        }
    }
    Ok(())
}

fn is_link_metadata(metadata: &Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::environment::preview;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_ROOT: AtomicUsize = AtomicUsize::new(0);

    fn fixture_root(label: &str) -> PathBuf {
        let id = NEXT_ROOT.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "workbench-project-environment-{label}-{}-{id}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn preview_has_only_metadata_mask_and_revision() {
        let root = fixture_root("preview");
        std::fs::write(root.join(".env"), b"TOKEN=top-secret\nNAME=devbox\n").unwrap();
        let parsed = read_source_file(&root, ".env").unwrap();
        let value = serde_json::to_string(&preview(&parsed)).unwrap();
        assert!(!value.contains("top-secret"));
        assert!(value.contains("secretReference"));
        assert!(value.contains("maskedValue"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn source_must_be_project_env_file_and_links_are_rejected() {
        let root = fixture_root("source");
        std::fs::write(root.join(".env.local"), b"NAME=ok").unwrap();
        assert!(read_source_file(&root, ".env.local").is_ok());
        for source in ["/tmp/.env", "../.env", ".env/child", "config.json"] {
            assert!(
                read_source_file(&root, source).is_err(),
                "accepted {source}"
            );
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join(".env.local"), root.join(".env.link")).unwrap();
            assert!(read_source_file(&root, ".env.link").is_err());
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn preview_rejects_overlong_or_untrimmed_wsl_distro_at_the_ipc_boundary() {
        for distro in ["d".repeat(MAX_WSL_DISTRO_CHARS + 1), " Ubuntu ".into()] {
            let request = ProjectEnvironmentPreviewRequest {
                windows_path: None,
                wsl: Some(WslProfile {
                    distro,
                    path: "/tmp/project".into(),
                }),
                source: ".env".into(),
                request_id: None,
            };
            assert!(matches!(
                read_request_source(&request),
                Err(EnvironmentError::InvalidSource)
            ));
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_rejects_posix_windows_path_without_wsl_context() {
        let request = ProjectEnvironmentPreviewRequest {
            windows_path: Some("/tmp/project".into()),
            wsl: None,
            source: ".env".into(),
            request_id: None,
        };
        assert!(matches!(
            read_request_source(&request),
            Err(EnvironmentError::InvalidSource)
        ));
    }

    #[cfg(not(windows))]
    #[test]
    fn native_build_rejects_windows_path_as_a_relative_host_path() {
        let request = ProjectEnvironmentPreviewRequest {
            windows_path: Some("C:\\tmp\\project".into()),
            wsl: None,
            source: ".env".into(),
            request_id: None,
        };
        assert!(matches!(
            read_request_source(&request),
            Err(EnvironmentError::InvalidSource)
        ));
    }
}
