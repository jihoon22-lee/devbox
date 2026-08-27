//! Native project `.env` reader and execution-time injection boundary.
//!
//! The only input returned by the preview command is metadata plus masked
//! values.  A profile stores the same metadata and an opaque file revision.
//! At Start Workspace time the file is read again, the revision and metadata
//! are compared, and only then are short-lived values resolved for child
//! processes.  This keeps a stale preview, a renamed file, and a changed
//! secret from silently becoming an execution authority.

use crate::commands::workspace::RunRegistry;
use crate::core::environment::{
    parse_environment, preview, EnvironmentError, ParsedEnvironment, ProjectEnvironmentPreview,
    MAX_ENV_FILE_BYTES,
};
use crate::core::operation::{
    wait_for_change, OperationBudget, OperationClaim, OperationError, OperationToken,
};
use crate::core::profile::{ProjectProfile, WslProfile};
use crate::platform::resolve_secret_for_execution;
use devbox_filesystem::{parse_safe_project_path, ProjectPathKind};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fmt;
use std::fs::Metadata;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;
use zeroize::Zeroizing;

const ENVIRONMENT_READ_ERROR: &str = "환경 파일을 안전하게 읽을 수 없습니다";
const ENVIRONMENT_STALE_ERROR: &str = "환경 파일이 변경되어 다시 확인해야 합니다";
const ENVIRONMENT_SECRET_ERROR: &str = "환경 secret을 안전하게 준비할 수 없습니다";
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

/// Values in this type live only until the child process has been spawned.
/// It has no Serialize implementation and its Debug output is redacted.
pub struct EnvironmentInjection {
    values: BTreeMap<String, Zeroizing<String>>,
}

impl fmt::Debug for EnvironmentInjection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EnvironmentInjection")
            .field("variable_count", &self.values.len())
            .field("values", &"<redacted>")
            .finish()
    }
}

impl EnvironmentInjection {
    /// Borrow the values for `Command::envs`; no clone or serialization is
    /// performed.  The returned references cannot outlive this owner.
    pub fn pairs(&self) -> Vec<(&str, &str)> {
        self.values
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
            .collect()
    }
}

/// Preview a user-selected source.  Paths are accepted only as project root
/// plus a `.env` filename; no absolute source path is returned.
#[tauri::command]
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
#[tauri::command]
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
pub fn resolve_profile_environment(
    profile: &ProjectProfile,
) -> Result<Option<EnvironmentInjection>, String> {
    resolve_profile_environment_with_control(
        profile,
        OperationToken::new(),
        OperationBudget::from_now(Duration::from_secs(5)),
    )
}

/// Async command-layer wrapper for the blocking file/secret preparation.
/// Keeping the resolver off the Tokio runtime thread lets Start Workspace
/// observe cancellation while a filesystem read or platform sealer is in
/// progress, then join the worker before discarding its result.
pub async fn resolve_profile_environment_async_with_control(
    profile: ProjectProfile,
    token: OperationToken,
    budget: OperationBudget,
    claim: &OperationClaim,
) -> Result<Option<EnvironmentInjection>, String> {
    budget.check(&token).map_err(OperationError::message)?;
    let worker_guard = claim.worker_guard().map_err(str::to_string)?;
    let worker_token = token.clone();
    let worker = tokio::task::spawn_blocking(move || {
        let _worker_guard = worker_guard;
        resolve_profile_environment_with_control(&profile, worker_token, budget)
    });
    tokio::pin!(worker);
    let result = tokio::select! {
        result = &mut worker => result
            .map_err(|_| ENVIRONMENT_READ_ERROR.to_string())?,
        control = wait_for_change(token.clone(), budget) => {
            token.cancel();
            let _ = worker.await;
            Err(control.message().to_string())
        }
    }?;
    budget.check(&token).map_err(OperationError::message)?;
    Ok(result)
}

/// Resolve the immutable, short-lived child overlay while observing the
/// Start Workspace operation's cancellation/deadline. A caller must keep the
/// returned holder alive only until its spawn boundary.
pub fn resolve_profile_environment_with_control(
    profile: &ProjectProfile,
    token: OperationToken,
    budget: OperationBudget,
) -> Result<Option<EnvironmentInjection>, String> {
    budget.check(&token).map_err(OperationError::message)?;
    let Some(config) = profile.environment.as_ref() else {
        return Ok(None);
    };
    if !config.enabled {
        // Disabled configuration is intentionally not read.  This permits a
        // user to retain metadata for a temporarily unavailable `.env` while
        // ensuring no secret or path is touched during Start Workspace.
        return Ok(None);
    }
    let parsed =
        read_profile_source_with_control(profile, &token, budget).map_err(|error| match error {
            EnvironmentError::Cancelled => OperationError::Cancelled.message().to_string(),
            EnvironmentError::TimedOut => OperationError::TimedOut.message().to_string(),
            _ => ENVIRONMENT_READ_ERROR.to_string(),
        })?;
    budget.check(&token).map_err(OperationError::message)?;
    if parsed.revision() != config.revision || parsed.metadata() != config.variables {
        return Err(ENVIRONMENT_STALE_ERROR.to_string());
    }
    if parsed.has_conflicts() {
        return Err("환경 파일에 해결되지 않은 충돌이 있습니다".to_string());
    }
    let injection = build_injection(&parsed).map(Some)?;
    budget.check(&token).map_err(OperationError::message)?;
    Ok(injection)
}

fn build_injection(parsed: &ParsedEnvironment) -> Result<EnvironmentInjection, String> {
    if parsed.has_conflicts() {
        return Err("환경 파일에 해결되지 않은 충돌이 있습니다".to_string());
    }
    let mut values = BTreeMap::new();
    for entry in parsed.entries() {
        let value = if entry.metadata.secret_reference.is_some() {
            resolve_secret_for_execution(entry.value.as_str())
                .map_err(|_| ENVIRONMENT_SECRET_ERROR.to_string())?
        } else {
            Zeroizing::new(entry.value.to_string())
        };
        values.insert(entry.metadata.name.clone(), value);
    }
    Ok(EnvironmentInjection { values })
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
        Some(&root.metadata),
    )
}

fn read_profile_source_with_control(
    profile: &ProjectProfile,
    token: &OperationToken,
    budget: OperationBudget,
) -> Result<ParsedEnvironment, EnvironmentError> {
    budget.check(token).map_err(environment_operation_error)?;
    let config = profile
        .environment
        .as_ref()
        .ok_or(EnvironmentError::InvalidMetadata)?;
    let root = project_root(profile.windows_path.as_deref(), profile.wsl.as_ref())
        .map_err(|_| EnvironmentError::InvalidSource)?;
    read_source_file_with_root_control(
        &root.path,
        &config.source,
        token,
        budget,
        Some(&root.metadata),
    )
}

struct ProjectRoot {
    path: PathBuf,
    metadata: Metadata,
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
    if is_link_metadata(&after_raw_metadata)
        || !same_file_identity(&raw_metadata, &after_raw_metadata)
    {
        return Err(EnvironmentError::InvalidSource);
    }
    let metadata = std::fs::metadata(&canonical).map_err(|_| EnvironmentError::InvalidSource)?;
    if !metadata.is_dir() || !same_file_identity(&raw_metadata, &metadata) {
        return Err(EnvironmentError::InvalidSource);
    }
    reject_links_in_existing_path(&canonical)?;
    Ok(ProjectRoot {
        path: canonical,
        metadata,
    })
}

#[cfg(test)]
fn read_source_file(root: &Path, source: &str) -> Result<ParsedEnvironment, EnvironmentError> {
    let token = OperationToken::new();
    let budget = OperationBudget::from_now(Duration::from_secs(5));
    read_source_file_with_control(root, source, &token, budget)
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
    expected_root_metadata: Option<&Metadata>,
) -> Result<ParsedEnvironment, EnvironmentError> {
    budget.check(token).map_err(environment_operation_error)?;
    crate::core::environment::validate_source_name(source)?;
    let path = root.join(source);
    let root_metadata =
        std::fs::symlink_metadata(root).map_err(|_| EnvironmentError::InvalidSource)?;
    if is_link_metadata(&root_metadata) || !root_metadata.file_type().is_dir() {
        return Err(EnvironmentError::InvalidSource);
    }
    if expected_root_metadata.is_some_and(|expected| !same_file_identity(expected, &root_metadata))
    {
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
    let file = open_source_file(&canonical_source).map_err(|_| EnvironmentError::InvalidSource)?;
    let opened_metadata = file
        .metadata()
        .map_err(|_| EnvironmentError::InvalidSource)?;
    if is_link_metadata(&opened_metadata) || !same_file_identity(&metadata, &opened_metadata) {
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
    if !same_file_identity(&root_metadata, &after_root_metadata)
        || is_link_metadata(&after_root_metadata)
        || expected_root_metadata
            .is_some_and(|expected| !same_file_identity(expected, &after_root_metadata))
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
    if is_link_metadata(&after_source_metadata)
        || !same_file_identity(&metadata, &after_source_metadata)
    {
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

fn same_file_identity(left: &Metadata, right: &Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        left.dev() == right.dev() && left.ino() == right.ino()
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        left.volume_serial_number() == right.volume_serial_number()
            && left.file_index() == right.file_index()
    }
    #[cfg(not(any(unix, windows)))]
    {
        left.len() == right.len()
            && left.modified().ok() == right.modified().ok()
            && left.file_type() == right.file_type()
    }
}

fn open_source_file(path: &Path) -> std::io::Result<std::fs::File> {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // O_NOFOLLOW makes the final component a handle-level check in
        // addition to the symlink_metadata/canonical path checks above. The
        // parent components are still rechecked before and after the read.
        std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(0x20000) // O_NOFOLLOW
            .open(path)
    }
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        return std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(0x100) // O_NOFOLLOW
            .open(path);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        return std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path);
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    std::fs::File::open(path)
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
    use crate::core::environment::{preview, ProjectEnvironmentConfig};
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

    #[test]
    fn stale_metadata_prevents_injection() {
        let root = fixture_root("stale");
        std::fs::write(root.join(".env"), b"NAME=before").unwrap();
        let parsed = read_source_file(&root, ".env").unwrap();
        let mut profile = ProjectProfile::new("project");
        profile.windows_path = Some(root.to_string_lossy().into_owned());
        profile.environment = Some(ProjectEnvironmentConfig {
            enabled: true,
            source: ".env".into(),
            revision: parsed.revision().into(),
            variables: parsed.metadata(),
        });
        std::fs::write(root.join(".env"), b"NAME=after").unwrap();
        assert_eq!(
            resolve_profile_environment(&profile).unwrap_err(),
            ENVIRONMENT_STALE_ERROR
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn disabled_environment_does_not_read_missing_source() {
        let mut profile = ProjectProfile::new("project");
        profile.windows_path = Some("C:\\does-not-exist".into());
        profile.environment = Some(ProjectEnvironmentConfig {
            enabled: false,
            source: ".env".into(),
            revision: "0".repeat(64),
            variables: Vec::new(),
        });
        assert!(resolve_profile_environment(&profile).unwrap().is_none());
    }

    #[test]
    fn empty_source_is_a_valid_noop_overlay() {
        let root = fixture_root("empty");
        std::fs::write(root.join(".env"), b"# intentionally empty\n").unwrap();
        let parsed = read_source_file(&root, ".env").unwrap();
        assert!(parsed.entries().is_empty());
        assert!(!parsed.has_conflicts());
        let injection = build_injection(&parsed).unwrap();
        assert!(injection.pairs().is_empty());
        let _ = std::fs::remove_dir_all(root);
    }
}
