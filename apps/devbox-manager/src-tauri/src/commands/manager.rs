use crate::core::asset::{
    select_asset, validate_artifact_coordinates, validate_manifest_artifacts,
    validate_version_component,
};
use crate::core::batch::{
    is_install_or_upgrade, validate_batch_requests, BatchInstallRequest, BatchInstallResult,
};
use crate::core::catalog::CatalogApp;
use crate::core::custom_root::{
    self, ActiveInstallLocation, CustomRootError, InstallRootPreviewStatus,
};
use crate::core::download::{partial_path, validate_digest, validate_size};
use crate::core::managed_install::{
    prepare_installer_destination, prepare_portable_destination, resolve_portable_install,
    validate_download_target, ManagedPortableInstall,
};
use crate::core::manifest::{parse_manifest, ReleaseManifest};
use crate::core::removal::{
    inspect_portable_removal, remove_portable_tree, RemovalError, RemovalPlan,
};
use crate::core::url_policy::is_allowed;
use futures_util::StreamExt;
use reqwest::header::USER_AGENT;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use tauri::Manager;
use tauri_plugin_opener::OpenerExt;

const REPO: &str = "https://api.github.com/repos/jihoon22-lee/devbox";
const DOWNLOAD_ROOT: &str = "https://github.com/jihoon22-lee/devbox/releases/download";
const MAX_PARTIAL_CLEANUP_APPS: usize = 256;
const MAX_PARTIAL_CLEANUP_VERSIONS: usize = 256;

static REMOVAL_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

/// 빌드 시 임베드된 카탈로그. Manager 자신의 버전이 아는 앱 목록이 명확해지고
/// 오프라인에서도 목록이 보인다. 새 앱은 Manager 업데이트로 반영된다.
pub(crate) const CATALOG_JSON: &str = include_str!("../../../../catalog.json");

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InstalledApp {
    pub app: String,
    pub version: String,
    /// "portable" | "installer"
    pub mode: String,
    pub exe_path: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InstalledAppView {
    pub app: String,
    pub version: String,
    pub mode: String,
}

impl From<&InstalledApp> for InstalledAppView {
    fn from(value: &InstalledApp) -> Self {
        Self {
            app: value.app.clone(),
            version: value.version.clone(),
            mode: value.mode.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CurrentView {
    pub version: String,
    pub installed_at: i64,
    pub previous_version: Option<String>,
}

impl From<&crate::core::layout::Current> for CurrentView {
    fn from(value: &crate::core::layout::Current) -> Self {
        Self {
            version: value.version.clone(),
            installed_at: value.installed_at,
            previous_version: value.previous_version.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InstallPathView {
    pub app_id: String,
    pub mode: String,
    pub executable: Option<String>,
    pub install_root: Option<String>,
    pub source_manifest: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstallRootRequest {
    pub path: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstallRootApplyRequest {
    pub path: String,
    pub expected_registry_revision: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InstallRootPreviewView {
    pub status: String,
    pub can_apply: bool,
    pub registry_revision: u64,
    pub catalog_revision: u64,
    pub candidate_path: String,
    pub root_id: String,
    pub free_space_bytes: Option<u64>,
    pub required_free_space_bytes: u64,
    pub active_install_count: usize,
    pub candidate_entry_count: usize,
    pub migration: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InstallRootApplyView {
    pub status: String,
    pub registry_revision: u64,
    pub root_id: String,
    pub candidate_path: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoveAppRequest {
    pub app_id: String,
    pub expected_registry_revision: u64,
    pub expected_catalog_revision: u64,
    pub expected_root_id: String,
    pub expected_manifest_digest: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RemovePreviewView {
    pub app_id: String,
    pub mode: String,
    pub version: String,
    pub state: String,
    pub can_remove: bool,
    pub registry_revision: u64,
    pub catalog_revision: u64,
    pub root_id: String,
    pub manifest_digest: String,
    pub target_path: Option<String>,
    pub owned_entry_count: usize,
    pub owned_bytes: u64,
    pub preserves_user_data: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RemoveResultView {
    pub status: String,
    pub message: String,
    pub removed_entry_count: usize,
    pub remaining_entry_count: usize,
    pub preserves_user_data: bool,
}

#[derive(Debug, Clone)]
struct RegistrySnapshot {
    location: ActiveInstallLocation,
    records: Vec<custom_root::InstallRecord>,
    bytes: Vec<u8>,
    digest: String,
}

pub(crate) fn data_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = data_dir_path(app)?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

pub(crate) fn data_dir_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path().app_local_data_dir().map_err(|e| e.to_string())
}

fn registry_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(active_install_location(app)?.manifest)
}

fn active_install_location(app: &tauri::AppHandle) -> Result<ActiveInstallLocation, String> {
    // Resolving the active root is read-only. In particular, preview must not
    // create the legacy root as a side effect of asking where it is.
    let default_root = data_dir_path(app)?;
    let locator_path = devbox_launch::install_root_registry_path()
        .ok_or_else(|| "설치 root 출처를 확인할 수 없습니다.".to_string())?;
    let location = custom_root::resolve_active_location(&locator_path, &default_root)
        .map_err(|_| "설치 root 상태를 안전하게 확인할 수 없습니다.".to_string())?;
    if !location.from_legacy_fallback && location.catalog_revision != selected_catalog_revision()? {
        return Err("설치 root 상태를 안전하게 확인할 수 없습니다.".to_string());
    }
    Ok(location)
}

fn install_root_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(active_install_location(app)?.root)
}

fn read_registry(app: &tauri::AppHandle) -> Result<Vec<InstalledApp>, String> {
    Ok(read_registry_snapshot(app)?
        .records
        .into_iter()
        .map(|record| InstalledApp {
            app: record.app,
            version: record.version,
            mode: record.mode,
            exe_path: record.exe_path,
        })
        .collect())
}

fn read_registry_snapshot(app: &tauri::AppHandle) -> Result<RegistrySnapshot, String> {
    let location = active_install_location(app)?;
    if location.from_legacy_fallback {
        match std::fs::symlink_metadata(&location.manifest) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(RegistrySnapshot {
                    location,
                    records: Vec::new(),
                    bytes: b"[]".to_vec(),
                    digest: manifest_digest(b"[]"),
                })
            }
            Err(_) => return Err("설치 상태를 안전하게 읽을 수 없습니다.".to_string()),
            Ok(_) => {}
        }
    }
    let snapshot = custom_root::read_install_manifest_snapshot(&location.manifest)
        .map_err(|_| "설치 상태를 안전하게 읽을 수 없습니다.".to_string())?;
    let known_apps = catalog()?
        .into_iter()
        .filter(|entry| entry.manager_visible && !entry.self_managed)
        .map(|entry| entry.id)
        .collect::<HashSet<_>>();
    if !registry_apps_are_known(&snapshot.records, &known_apps) {
        return Err("설치 상태를 안전하게 읽을 수 없습니다.".to_string());
    }
    Ok(RegistrySnapshot {
        location,
        records: snapshot.records,
        bytes: snapshot.bytes,
        digest: snapshot.digest,
    })
}

/// Read a registry for the removal recovery path. Normal lifecycle reads
/// require every portable executable to still exist; a removal may have
/// already deleted that exact executable before the process was interrupted.
/// The core recovery parser permits only that missing final layout slot and
/// keeps every schema/path/link check intact.
fn read_registry_snapshot_for_removal(app: &tauri::AppHandle) -> Result<RegistrySnapshot, String> {
    let location = active_install_location(app)?;
    if location.from_legacy_fallback {
        match std::fs::symlink_metadata(&location.manifest) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(RegistrySnapshot {
                    location,
                    records: Vec::new(),
                    bytes: b"[]".to_vec(),
                    digest: manifest_digest(b"[]"),
                })
            }
            Err(_) => return Err("설치 상태를 안전하게 읽을 수 없습니다.".to_string()),
            Ok(_) => {}
        }
    }
    let snapshot = custom_root::read_install_manifest_snapshot_for_removal(&location.manifest)
        .map_err(|_| "설치 상태를 안전하게 읽을 수 없습니다.".to_string())?;
    let known_apps = catalog()?
        .into_iter()
        .filter(|entry| entry.manager_visible && !entry.self_managed)
        .map(|entry| entry.id)
        .collect::<HashSet<_>>();
    if !registry_apps_are_known(&snapshot.records, &known_apps) {
        return Err("설치 상태를 안전하게 읽을 수 없습니다.".to_string());
    }
    Ok(RegistrySnapshot {
        location,
        records: snapshot.records,
        bytes: snapshot.bytes,
        digest: snapshot.digest,
    })
}

fn registry_apps_are_known(
    records: &[custom_root::InstallRecord],
    known_apps: &HashSet<String>,
) -> bool {
    records
        .iter()
        .all(|record| known_apps.contains(&record.app))
}

fn manifest_digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn encode_registry(reg: &[InstalledApp]) -> Result<Vec<u8>, String> {
    let json = serde_json::to_vec_pretty(reg)
        .map_err(|_| "설치 상태를 직렬화할 수 없습니다.".to_string())?;
    custom_root::parse_install_manifest(&json)
        .map_err(|_| "설치 상태를 안전하게 기록할 수 없습니다.".to_string())?;
    Ok(json)
}

fn write_manifest_bytes(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    write_manifest_bytes_with_validation(path, bytes, false)
}

fn write_manifest_bytes_for_removal(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    write_manifest_bytes_with_validation(path, bytes, true)
}

fn write_manifest_bytes_with_validation(
    path: &std::path::Path,
    bytes: &[u8],
    allow_missing_executable: bool,
) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "설치 상태를 안전하게 기록할 수 없습니다.".to_string())?;
    let records = custom_root::parse_install_manifest(bytes)
        .map_err(|_| "설치 상태를 안전하게 기록할 수 없습니다.".to_string())?;
    if allow_missing_executable {
        custom_root::validate_install_manifest_at_root_for_removal(parent, &records)
    } else {
        custom_root::validate_install_manifest_at_root(parent, &records)
    }
    .map_err(|_| "설치 상태를 안전하게 기록할 수 없습니다.".to_string())?;
    let metadata = std::fs::symlink_metadata(parent)
        .map_err(|_| "설치 상태를 안전하게 기록할 수 없습니다.".to_string())?;
    if metadata_is_link_or_reparse(&metadata) || !metadata.is_dir() {
        return Err("설치 상태를 안전하게 기록할 수 없습니다.".to_string());
    }
    devbox_filesystem::atomic_write(path, bytes)
        .map_err(|_| "설치 상태를 안전하게 기록할 수 없습니다.".to_string())
}

fn write_registry(app: &tauri::AppHandle, reg: &[InstalledApp]) -> Result<(), String> {
    let path = registry_path(app)?;
    let json = encode_registry(reg)?;
    write_manifest_bytes(&path, &json)
}

fn same_location(left: &ActiveInstallLocation, right: &ActiveInstallLocation) -> bool {
    left.root_id == right.root_id
        && left.registry_revision == right.registry_revision
        && left.catalog_revision == right.catalog_revision
        && same_path_identity(&left.root, &right.root)
        && same_path_identity(&left.manifest, &right.manifest)
}

fn same_path_identity(left: &std::path::Path, right: &std::path::Path) -> bool {
    #[cfg(windows)]
    {
        normalize_windows_identity(left) == normalize_windows_identity(right)
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

#[cfg(windows)]
fn normalize_windows_identity(path: &std::path::Path) -> String {
    let mut value = path.to_string_lossy().replace('/', "\\");
    if let Some(rest) = value.strip_prefix(r"\\?\UNC\") {
        value = format!(r"\\{rest}");
    } else if let Some(rest) = value.strip_prefix(r"\\?\") {
        value = rest.to_string();
    }
    while value.len() > 3 && value.ends_with('\\') {
        value.pop();
    }
    value.to_ascii_lowercase()
}

fn write_registry_if_current(
    app: &tauri::AppHandle,
    expected: &RegistrySnapshot,
    next: &[InstalledApp],
) -> Result<String, String> {
    let current_location = active_install_location(app)?;
    if !same_location(&current_location, &expected.location) {
        return Err("설치 상태가 바뀌었습니다. 최신 제거 미리 보기를 다시 확인하세요.".to_string());
    }
    let current =
        read_registry_snapshot(app).or_else(|_| read_registry_snapshot_for_removal(app))?;
    if current.digest != expected.digest {
        return Err("설치 상태가 바뀌었습니다. 최신 제거 미리 보기를 다시 확인하세요.".to_string());
    }
    let json = encode_registry(next)?;
    write_manifest_bytes(&current.location.manifest, &json)?;
    Ok(manifest_digest(&json))
}

/// Restore only when the manifest still contains the exact bytes written by
/// this removal attempt.  A competing writer is preserved and reported as a
/// recovery failure rather than being overwritten.
fn restore_manifest_if_current(
    app: &tauri::AppHandle,
    expected_location: &ActiveInstallLocation,
    expected_digest: &str,
    original_bytes: &[u8],
) -> bool {
    let Ok(location) = active_install_location(app) else {
        return false;
    };
    if !same_location(&location, expected_location) {
        return false;
    }
    let Ok(current) =
        read_registry_snapshot(app).or_else(|_| read_registry_snapshot_for_removal(app))
    else {
        return false;
    };
    if current.digest != expected_digest {
        return false;
    }
    write_manifest_bytes_for_removal(&location.manifest, original_bytes).is_ok()
}

fn metadata_common_root() -> Result<PathBuf, String> {
    devbox_launch::runtime_catalog_path()
        .and_then(|path| path.parent().map(PathBuf::from))
        .ok_or_else(|| "runtime metadata root is unavailable".to_string())
}

fn metadata_locator_path() -> Result<PathBuf, String> {
    devbox_launch::install_root_registry_path()
        .ok_or_else(|| "install-root locator is unavailable".to_string())
}

fn selected_catalog_revision() -> Result<u64, String> {
    let runtime =
        devbox_launch::runtime_catalog_path().and_then(|path| std::fs::read_to_string(path).ok());
    devbox_catalog::select_catalog(CATALOG_JSON, runtime.as_deref())
        .map_err(|_| "앱 카탈로그를 확인할 수 없습니다.".to_string())?
        .catalog
        .catalog_revision
        .filter(|revision| *revision > 0)
        .ok_or_else(|| "앱 카탈로그 revision을 확인할 수 없습니다.".to_string())
}

fn custom_root_error(error: CustomRootError) -> String {
    match error {
        CustomRootError::InvalidPath | CustomRootError::PathTooLong => {
            "설치 root 경로가 올바르지 않습니다.".to_string()
        }
        CustomRootError::MissingDirectory => {
            "선택한 설치 root 디렉터리를 찾을 수 없습니다.".to_string()
        }
        CustomRootError::UnsafePath | CustomRootError::ProtectedPath => {
            "안전하지 않은 설치 root는 사용할 수 없습니다.".to_string()
        }
        CustomRootError::LocatorInvalid => "현재 설치 root 상태를 확인할 수 없습니다.".to_string(),
        CustomRootError::ActiveStateInvalid
        | CustomRootError::ManifestTooLarge
        | CustomRootError::ManifestInvalid
        | CustomRootError::ManifestTooManyEntries => {
            "현재 설치 상태가 올바르지 않아 변경할 수 없습니다.".to_string()
        }
        CustomRootError::CandidateConflict => "선택한 설치 root가 비어 있지 않습니다.".to_string(),
        CustomRootError::PermissionDenied => "선택한 설치 root에 쓸 권한이 없습니다.".to_string(),
        CustomRootError::ExistingInstall => {
            "기존 설치가 있어 자동 이동하지 않습니다. 별도 migration을 먼저 진행하세요.".to_string()
        }
        CustomRootError::FreeSpaceUnavailable => {
            "설치 root의 여유 공간을 확인할 수 없습니다.".to_string()
        }
        CustomRootError::InsufficientFreeSpace => "설치 root의 여유 공간이 부족합니다.".to_string(),
        CustomRootError::RevisionMismatch => {
            "설치 root 상태가 바뀌었습니다. 최신 preview를 다시 확인하세요.".to_string()
        }
        CustomRootError::RevisionOverflow => "설치 root revision을 증가할 수 없습니다.".to_string(),
        CustomRootError::InvalidCatalogRevision => {
            "앱 카탈로그 revision을 확인할 수 없습니다.".to_string()
        }
        CustomRootError::Storage | CustomRootError::Serialization => {
            "설치 root 상태를 안전하게 저장할 수 없습니다.".to_string()
        }
        CustomRootError::RollbackFailed => {
            "설치 root 변경 실패 후 안전하게 복구하지 못했습니다.".to_string()
        }
        CustomRootError::NonUtf8Path => "설치 root 경로를 안전하게 표시할 수 없습니다.".to_string(),
    }
}

fn install_root_preview_view(
    preview: custom_root::InstallRootPreview,
) -> Result<InstallRootPreviewView, String> {
    let candidate_path = preview
        .candidate_root
        .to_str()
        .ok_or_else(|| custom_root_error(CustomRootError::NonUtf8Path))?
        .to_string();
    Ok(InstallRootPreviewView {
        status: preview.status.as_code().to_string(),
        can_apply: preview.status.can_apply(),
        registry_revision: preview.registry_revision,
        catalog_revision: preview.catalog_revision,
        candidate_path,
        root_id: preview.candidate_root_id,
        free_space_bytes: preview.free_space_bytes,
        required_free_space_bytes: preview.required_free_space_bytes,
        active_install_count: preview.active_install_count,
        candidate_entry_count: preview.candidate_entry_count,
        migration: if preview.status == InstallRootPreviewStatus::ExistingInstall {
            "blocked-existing-install".to_string()
        } else {
            "no-automatic-migration".to_string()
        },
    })
}

/// Read-only custom install-root preflight. The selected directory is
/// canonicalized and checked again by `apply_install_root`; this command does
/// not create, move, delete, or rewrite any file.
#[tauri::command]
pub fn preview_install_root(
    app: tauri::AppHandle,
    request: InstallRootRequest,
) -> Result<InstallRootPreviewView, String> {
    let default_root =
        data_dir_path(&app).map_err(|_| "현재 설치 root를 확인할 수 없습니다.".to_string())?;
    let locator_path = metadata_locator_path()?;
    let common_root = metadata_common_root()?;
    let catalog_revision = selected_catalog_revision()?;
    let preview = custom_root::preview_custom_root(
        &locator_path,
        &default_root,
        Some(&common_root),
        &request.path,
        catalog_revision,
        None,
    )
    .map_err(custom_root_error)?;
    install_root_preview_view(preview)
}

/// Apply a confirmed custom install root. Only an empty candidate is allowed;
/// existing installations are never moved or deleted in this issue.
#[tauri::command]
pub fn apply_install_root(
    app: tauri::AppHandle,
    request: InstallRootApplyRequest,
) -> Result<InstallRootApplyView, String> {
    let default_root =
        data_dir_path(&app).map_err(|_| "현재 설치 root를 확인할 수 없습니다.".to_string())?;
    let locator_path = metadata_locator_path()?;
    let common_root = metadata_common_root()?;
    let catalog_revision = selected_catalog_revision()?;
    let applied = custom_root::apply_custom_root(
        &locator_path,
        &default_root,
        Some(&common_root),
        &request.path,
        request.expected_registry_revision,
        catalog_revision,
        now_ms().max(1) as u64,
    )
    .map_err(custom_root_error)?;
    let candidate_path = applied
        .root
        .to_str()
        .ok_or_else(|| custom_root_error(CustomRootError::NonUtf8Path))?
        .to_string();
    Ok(InstallRootApplyView {
        status: if applied.status == InstallRootPreviewStatus::AlreadyActive {
            "already-active".to_string()
        } else {
            "applied".to_string()
        },
        registry_revision: applied.registry_revision,
        root_id: applied.root_id,
        candidate_path,
    })
}

/// Publish the embedded catalog and the default Manager install-root locator.
/// Errors are deliberately safe and contain no raw filesystem path.
pub(crate) fn sync_runtime_metadata(app: &tauri::AppHandle) -> Result<(), String> {
    let manager_root =
        data_dir(app).map_err(|_| "runtime metadata root is unavailable".to_string())?;
    let catalog_path = devbox_launch::runtime_catalog_path()
        .ok_or_else(|| "runtime metadata root is unavailable".to_string())?;
    let common_root = catalog_path
        .parent()
        .ok_or_else(|| "runtime metadata root is unavailable".to_string())?;
    crate::core::runtime_metadata::sync_runtime_metadata(
        &manager_root,
        common_root,
        CATALOG_JSON,
        now_ms().max(1) as u64,
    )
    .map(|_| ())
    .map_err(|error| error.to_string())
}

/// 앱 시작 시 남아 있는 중단된 `.partial` 파일을 정리한다.
pub fn cleanup_partials(app: &tauri::AppHandle) {
    let Ok(base) = install_root_dir(app) else {
        return;
    };
    let Ok(known_apps) = catalog().map(|entries| {
        entries
            .into_iter()
            .filter(|entry| entry.manager_visible && !entry.self_managed)
            .map(|entry| entry.id)
            .collect::<HashSet<_>>()
    }) else {
        return;
    };
    cleanup_managed_partials(&base, &known_apps);
}

fn cleanup_managed_partials(base: &std::path::Path, known_apps: &HashSet<String>) {
    let Some(targets) = managed_partial_targets(base, known_apps) else {
        return;
    };
    for target in targets {
        let _ = std::fs::remove_file(target);
    }
}

/// Return only the exact partial slots that Manager derives for a known app
/// and strict version. The complete scan finishes before deletion so an
/// unsafe link, malformed component, or oversized tree leaves every file
/// untouched instead of partially cleaning an untrusted custom root.
fn managed_partial_targets(
    base: &std::path::Path,
    known_apps: &HashSet<String>,
) -> Option<Vec<PathBuf>> {
    let apps_root = base.join("apps");
    let Ok(metadata) = std::fs::symlink_metadata(&apps_root) else {
        return Some(Vec::new());
    };
    if metadata_is_link_or_reparse(&metadata) || !metadata.is_dir() {
        return None;
    }
    let Ok(entries) = std::fs::read_dir(&apps_root) else {
        return None;
    };
    let mut app_count = 0_usize;
    let mut targets = Vec::new();
    for entry in entries {
        let entry = entry.ok()?;
        app_count = app_count.checked_add(1)?;
        if app_count > MAX_PARTIAL_CLEANUP_APPS {
            return None;
        }
        let app_id = entry.file_name().into_string().ok()?;
        if !known_apps.contains(&app_id) {
            continue;
        }
        let app_dir = entry.path();
        let Ok(metadata) = std::fs::symlink_metadata(&app_dir) else {
            return None;
        };
        if metadata_is_link_or_reparse(&metadata) || !metadata.is_dir() {
            return None;
        }
        let versions_dir = app_dir.join("versions");
        let metadata = match std::fs::symlink_metadata(&versions_dir) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return None,
        };
        if metadata_is_link_or_reparse(&metadata) || !metadata.is_dir() {
            return None;
        }
        let Ok(versions) = std::fs::read_dir(&versions_dir) else {
            return None;
        };
        let mut version_count = 0_usize;
        for version in versions {
            let version = version.ok()?;
            version_count = version_count.checked_add(1)?;
            if version_count > MAX_PARTIAL_CLEANUP_VERSIONS {
                return None;
            }
            let version_name = version.file_name().into_string().ok()?;
            if validate_version_component(&version_name).is_err() {
                continue;
            }
            let version_dir = version.path();
            let Ok(metadata) = std::fs::symlink_metadata(&version_dir) else {
                return None;
            };
            if metadata_is_link_or_reparse(&metadata) || !metadata.is_dir() {
                return None;
            }
            let partial = version_dir.join(format!("{app_id}.exe.partial"));
            let metadata = match std::fs::symlink_metadata(&partial) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(_) => return None,
            };
            if metadata_is_link_or_reparse(&metadata) || !metadata.is_file() {
                return None;
            }
            targets.push(partial);
        }
    }
    Some(targets)
}

#[cfg(windows)]
fn metadata_is_link_or_reparse(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
    metadata.file_type().is_symlink()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn metadata_is_link_or_reparse(metadata: &std::fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

/// 번들에 포함된 카탈로그를 반환한다.
#[tauri::command]
pub fn catalog() -> Result<Vec<CatalogApp>, String> {
    let runtime =
        devbox_launch::runtime_catalog_path().and_then(|path| std::fs::read_to_string(path).ok());
    let selected = devbox_catalog::select_catalog(CATALOG_JSON, runtime.as_deref())
        .map_err(|_| "앱 카탈로그를 확인할 수 없습니다.".to_string())?;
    Ok(selected.catalog.apps)
}

/// Return only catalog identity needed by the local-quality view. Runtime
/// source paths and selector diagnostics remain inside the native layer.
pub(crate) fn local_quality_catalog_observation(
) -> Result<crate::core::local_quality::CatalogObservation, String> {
    let runtime =
        devbox_launch::runtime_catalog_path().and_then(|path| std::fs::read_to_string(path).ok());
    let selected = devbox_catalog::select_catalog(CATALOG_JSON, runtime.as_deref())
        .map_err(|_| "앱 카탈로그를 확인할 수 없습니다.".to_string())?;
    let revision = selected
        .catalog
        .catalog_revision
        .filter(|revision| *revision > 0)
        .ok_or_else(|| "앱 카탈로그 revision을 확인할 수 없습니다.".to_string())?;
    Ok(crate::core::local_quality::CatalogObservation {
        revision,
        managed_app_ids: selected
            .catalog
            .apps
            .into_iter()
            .filter(|entry| entry.manager_visible && !entry.self_managed)
            .map(|entry| entry.id)
            .collect(),
    })
}

/// Return a validated registry projection without copying executable, root,
/// manifest, or locator paths into the local-quality DTO.
pub(crate) fn local_quality_registry_observation(
    app: &tauri::AppHandle,
) -> Result<crate::core::local_quality::RegistryObservation, String> {
    let snapshot = read_registry_snapshot(app)?;
    Ok(crate::core::local_quality::RegistryObservation {
        revision: snapshot.location.registry_revision,
        records: snapshot
            .records
            .into_iter()
            .map(
                |record| crate::core::local_quality::RegistryRecordObservation {
                    app_id: record.app,
                    version: record.version,
                    mode: record.mode,
                },
            )
            .collect(),
    })
}

fn ensure_catalog_target(app_id: &str) -> Result<CatalogApp, String> {
    catalog()
        .map_err(|_| "앱 카탈로그를 확인할 수 없습니다.".to_string())?
        .into_iter()
        .find(|entry| entry.id == app_id && entry.manager_visible && !entry.self_managed)
        .ok_or_else(|| "관리 가능한 카탈로그 앱이 아닙니다.".to_string())
}

fn portable_registry_entry(app: &tauri::AppHandle, app_id: &str) -> Result<InstalledApp, String> {
    ensure_catalog_target(app_id)?;
    let installed = read_registry(app)?
        .into_iter()
        .find(|entry| entry.app == app_id)
        .ok_or_else(|| "설치된 앱이 없습니다. 먼저 설치하세요.".to_string())?;
    if installed.mode != "portable" {
        return Err("설치 패키지 방식 앱은 이 작업을 지원하지 않습니다.".to_string());
    }
    validate_version_component(&installed.version)
        .map_err(|_| "설치된 앱의 버전 정보가 올바르지 않습니다.".to_string())?;
    Ok(installed)
}

fn managed_portable(
    app: &tauri::AppHandle,
    app_id: &str,
) -> Result<ManagedPortableInstall, String> {
    let installed = portable_registry_entry(app, app_id)?;
    let root =
        install_root_dir(app).map_err(|_| "Manager 설치 root를 확인할 수 없습니다.".to_string())?;
    let resolved = resolve_portable_install(&root, app_id, &installed.version, &installed.exe_path)
        .map_err(|_| "검증된 휴대용 앱 경로를 확인할 수 없습니다.".to_string())?;
    Ok(resolved)
}

/// 최신 릴리스의 `release-manifest.json`을 받아 파싱한다.
/// Manager는 GitHub asset 이름을 추측하지 않고 이 manifest만 신뢰한다.
#[tauri::command]
pub async fn available() -> Result<ReleaseManifest, String> {
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{REPO}/releases/latest"))
        .header(USER_AGENT, "devbox-manager")
        .send()
        .await
        .map_err(|_| "릴리스 정보를 조회할 수 없습니다.".to_string())?;
    if !resp.status().is_success() {
        return Err("릴리스 정보를 제공받지 못했습니다.".to_string());
    }
    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|_| "릴리스 응답을 읽을 수 없습니다.".to_string())?;
    let manifest_url = json["assets"]
        .as_array()
        .and_then(|arr| {
            arr.iter()
                .find(|a| a["name"].as_str() == Some("release-manifest.json"))
        })
        .and_then(|a| a["browser_download_url"].as_str().map(|s| s.to_string()))
        .ok_or("release-manifest.json asset이 없다 (PR 9 이후의 릴리스가 필요하다)")?;

    if !is_allowed(&manifest_url) {
        return Err("허용되지 않은 manifest URL".into());
    }
    let mresp = client
        .get(&manifest_url)
        .header(USER_AGENT, "devbox-manager")
        .send()
        .await
        .map_err(|_| "release manifest를 다운로드할 수 없습니다.".to_string())?;
    if !mresp.status().is_success() {
        return Err("release manifest를 제공받지 못했습니다.".to_string());
    }
    let text = mresp
        .text()
        .await
        .map_err(|_| "release manifest 응답을 읽을 수 없습니다.".to_string())?;
    let manifest = parse_manifest(&text)
        .map_err(|_| "release manifest 형식이 올바르지 않습니다.".to_string())?;
    validate_manifest_artifacts(&manifest)
        .map_err(|_| "release manifest의 앱 정보가 올바르지 않습니다.".to_string())?;
    let known = catalog().map_err(|_| "앱 카탈로그를 확인할 수 없습니다.".to_string())?;
    if manifest
        .apps
        .iter()
        .any(|entry| !known.iter().any(|target| target.id == entry.id))
    {
        return Err("release manifest에 알 수 없는 앱이 포함되어 있습니다.".to_string());
    }
    Ok(manifest)
}

/// 설치된 앱 목록 (registry.json).
#[tauri::command]
pub fn installed(app: tauri::AppHandle) -> Result<Vec<InstalledAppView>, String> {
    let targets = catalog().map_err(|_| "앱 카탈로그를 확인할 수 없습니다.".to_string())?;
    Ok(read_registry(&app)?
        .iter()
        .filter(|installed| {
            targets.iter().any(|target| {
                target.id == installed.app && target.manager_visible && !target.self_managed
            }) && matches!(installed.mode.as_str(), "portable" | "installer")
                && validate_version_component(&installed.version).is_ok()
        })
        .map(InstalledAppView::from)
        .collect())
}

/// Versioned locator와 source manifest가 증명한 설치 경로를 표시용으로만 반환한다.
/// installer는 실제 설치 완료/위치를 Manager가 소유하지 않으므로 executable/root를
/// 추측하지 않는다. 이 command는 파일·registry·프로세스를 변경하지 않는다.
#[tauri::command]
pub fn install_path(app: tauri::AppHandle, app_id: String) -> Result<InstallPathView, String> {
    ensure_catalog_target(&app_id)?;
    let locator_path = devbox_launch::install_root_registry_path()
        .ok_or_else(|| "설치 경로 출처를 확인할 수 없습니다.".to_string())?;
    let runtime_catalog = devbox_launch::runtime_catalog_path();
    let active_manifest = active_install_location(&app)
        .map_err(|_| "현재 설치 상태의 출처를 확인할 수 없습니다.".to_string())?
        .manifest;
    let details = devbox_launch::installed_path_details_from_paths(
        CATALOG_JSON,
        runtime_catalog.as_deref(),
        &locator_path,
        &active_manifest,
        &app_id,
    )
    .map_err(|_| "검증된 설치 경로 정보를 확인할 수 없습니다.".to_string())?
    .ok_or_else(|| "설치된 앱의 경로 기록이 없습니다.".to_string())?;

    let path_text = |path: std::path::PathBuf| {
        path.into_os_string()
            .into_string()
            .map_err(|_| "설치 경로를 안전하게 표시할 수 없습니다.".to_string())
    };
    Ok(InstallPathView {
        app_id: details.app_id,
        mode: details.mode,
        executable: details.executable.map(&path_text).transpose()?,
        install_root: details.install_root.map(&path_text).transpose()?,
        source_manifest: path_text(details.source_manifest)?,
    })
}

/// 앱 설치/업데이트. Rust가 이미 검증한 manifest에서 대상 asset을 선택한다.
/// - mode=portable: 휴대용 exe를 다운로드해 자체 폴더에 보관
/// - mode=installer: NSIS 설치 패키지를 내려받아 실행 (설치 마법사)
#[tauri::command]
pub async fn install(
    app: tauri::AppHandle,
    app_id: String,
    mode: String,
) -> Result<String, String> {
    ensure_catalog_target(&app_id)?;
    if mode != "portable" && mode != "installer" {
        return Err("지원하지 않는 설치 방식입니다.".to_string());
    }
    let manifest = available().await?;
    let base = install_root_dir(&app)?;
    let client = reqwest::Client::new();
    install_with_manifest(&app, &base, &client, &manifest, app_id, mode).await
}

/// 여러 앱을 순서대로 설치/업데이트한다. Release manifest와 HTTP client는
/// batch 전체에서 한 번만 준비하고 한 항목의 실패는 다음 항목을 막지 않는다.
/// 결과 오류는 app ID와 고정 메시지만 포함하며 lower-level URL/path를 노출하지 않는다.
#[tauri::command]
pub async fn install_many(
    app: tauri::AppHandle,
    requests: Vec<BatchInstallRequest>,
) -> Result<Vec<BatchInstallResult>, String> {
    validate_batch_requests(&requests)?;
    let targets = catalog().map_err(|_| "앱 카탈로그를 확인할 수 없습니다.".to_string())?;
    if requests.iter().any(|request| {
        !targets.iter().any(|target| {
            target.id == request.app_id && target.manager_visible && !target.self_managed
        })
    }) {
        return Err("일괄 작업에 관리할 수 없는 앱이 포함되어 있습니다.".to_string());
    }

    let manifest = match available().await {
        Ok(manifest) => manifest,
        Err(_) => {
            return Ok(requests
                .iter()
                .map(BatchInstallResult::shared_failure)
                .collect());
        }
    };
    let base = match install_root_dir(&app) {
        Ok(base) => base,
        Err(_) => {
            return Ok(requests
                .iter()
                .map(BatchInstallResult::shared_failure)
                .collect());
        }
    };
    let client = reqwest::Client::new();
    let initial_registry = read_registry(&app)?;
    let mut results = Vec::with_capacity(requests.len());
    for request in &requests {
        let available_version = manifest
            .apps
            .iter()
            .find(|entry| entry.id == request.app_id)
            .map(|entry| entry.version.as_str());
        let installed_version = initial_registry
            .iter()
            .find(|entry| entry.app == request.app_id)
            .map(|entry| entry.version.as_str());
        match available_version.ok_or(()).and_then(|available| {
            is_install_or_upgrade(installed_version, available).map_err(|_| ())
        }) {
            Ok(false) => {
                results.push(BatchInstallResult::success(
                    request,
                    "이미 같거나 더 최신 버전이 설치되어 있어 변경하지 않았습니다.".to_string(),
                ));
                continue;
            }
            Err(()) => {
                results.push(BatchInstallResult::retryable_failure(request));
                continue;
            }
            Ok(true) => {}
        }
        let result = install_with_manifest(
            &app,
            &base,
            &client,
            &manifest,
            request.app_id.clone(),
            request.mode.clone(),
        )
        .await;
        results.push(match result {
            Ok(message) => BatchInstallResult::success(request, message),
            Err(_) => BatchInstallResult::retryable_failure(request),
        });
    }
    Ok(results)
}

async fn install_with_manifest(
    app: &tauri::AppHandle,
    base: &std::path::Path,
    client: &reqwest::Client,
    manifest: &ReleaseManifest,
    app_id: String,
    mode: String,
) -> Result<String, String> {
    ensure_catalog_target(&app_id)?;
    if mode != "portable" && mode != "installer" {
        return Err("지원하지 않는 설치 방식입니다.".to_string());
    }
    let app_manifest = manifest
        .apps
        .iter()
        .find(|a| a.id == app_id)
        .ok_or_else(|| format!("manifest에 앱이 없다: {app_id}"))?;
    let asset = select_asset(manifest, &app_id, &mode)?;
    let version = app_manifest.version.clone();
    validate_artifact_coordinates(&manifest.release_tag, &version, &asset.name)
        .map_err(|_| "manifest의 다운로드 경로 정보가 올바르지 않습니다.".to_string())?;
    let url = format!("{DOWNLOAD_ROOT}/{}/{}", manifest.release_tag, asset.name);
    if !is_allowed(&url) {
        return Err("허용되지 않은 다운로드 URL".into());
    }

    if mode == "installer" {
        let dest = prepare_installer_destination(base, &asset.name)
            .map_err(|_| "설치 프로그램 경로를 안전하게 준비할 수 없습니다.".to_string())?;
        // 검증이 끝난 뒤에만 installer를 실행한다.
        download(client, &url, &dest, asset.size, &asset.sha256).await?;
        let original_registry = read_registry(app)?;
        let next_registry = registry_with_entry(
            &original_registry,
            app_id,
            version,
            "installer",
            String::new(),
        );
        // 실행 전에 durable registry 기록을 준비한다. 실행 실패 시 원래 registry를 복구한다.
        write_registry(app, &next_registry)
            .map_err(|_| "설치 상태를 기록할 수 없습니다.".to_string())?;
        if std::process::Command::new(&dest).spawn().is_err() {
            if write_registry(app, &original_registry).is_err() {
                return Err(
                    "설치 프로그램 실행과 상태 복구에 실패했습니다. 앱 상태를 확인하세요."
                        .to_string(),
                );
            }
            sync_runtime_metadata_best_effort(app);
            return Err("설치 프로그램을 실행할 수 없습니다.".to_string());
        }
        sync_runtime_metadata_best_effort(app);
        return Ok("설치 프로그램을 실행했습니다. 화면 안내에 따라 설치하세요.".into());
    }

    // portable
    let exe = prepare_portable_destination(base, &app_id, &version)
        .map_err(|_| "휴대용 앱 설치 폴더를 안전하게 준비할 수 없습니다.".to_string())?;
    download(client, &url, &exe, asset.size, &asset.sha256).await?;

    // current.json 갱신 (직전 정상 버전을 previous로 보존)
    let prev = read_current(base, &app_id);
    let current = crate::core::layout::Current {
        version: version.clone(),
        exe_path: exe.to_string_lossy().into_owned(),
        installed_at: now_ms(),
        previous_version: prev.as_ref().map(|value| value.version.clone()),
    };
    let original_registry = read_registry(app)?;
    let next_registry = registry_with_entry(
        &original_registry,
        app_id.clone(),
        version,
        "portable",
        current.exe_path.clone(),
    );
    write_current(base, &app_id, &current)?;
    if write_registry(app, &next_registry).is_err() {
        if restore_current(base, &app_id, prev.as_ref()).is_err() {
            return Err(
                "설치 상태 기록과 current 복구에 실패했습니다. 앱 상태를 확인하세요.".to_string(),
            );
        }
        return Err("설치 상태를 기록할 수 없습니다.".to_string());
    }
    // 이전 버전 디렉터리는 삭제하지 않는다 (rollback 보존)
    sync_runtime_metadata_best_effort(app);
    Ok("휴대용 앱을 설치했습니다.".into())
}

/// 설치된 휴대용 앱의 current.json을 반환한다 (없으면 None).
#[tauri::command]
pub fn current(app: tauri::AppHandle, app_id: String) -> Result<Option<CurrentView>, String> {
    let installed = portable_registry_entry(&app, &app_id)?;
    let base = install_root_dir(&app)
        .map_err(|_| "Manager 설치 root를 확인할 수 없습니다.".to_string())?;
    let current = read_current(&base, &app_id)
        .filter(|value| {
            value.version == installed.version
                && validate_version_component(&value.version).is_ok()
                && value
                    .previous_version
                    .as_deref()
                    .is_none_or(|version| validate_version_component(version).is_ok())
        })
        .as_ref()
        .map(CurrentView::from);
    Ok(current)
}

/// 직전 정상 버전으로 되돌린다 (portable만).
#[tauri::command]
pub fn rollback(app: tauri::AppHandle, app_id: String) -> Result<String, String> {
    let installed_record = portable_registry_entry(&app, &app_id)?;
    let base = install_root_dir(&app)
        .map_err(|_| "Manager 설치 root를 확인할 수 없습니다.".to_string())?;
    let current = read_current(&base, &app_id)
        .ok_or_else(|| "current.json이 없다 (portable 설치 필요)".to_string())?;
    if current.version != installed_record.version {
        return Err("설치 상태와 현재 버전 정보가 일치하지 않습니다.".to_string());
    }
    let installed = installed_versions_with_exe(&base, &app_id);
    let target = crate::core::layout::pick_rollback_target(&current, &installed)
        .ok_or_else(|| "되돌릴 이전 버전이 없다".to_string())?;
    let target_exe = crate::core::layout::version_exe(&base, &app_id, &target);
    let target_exe_text = target_exe
        .to_str()
        .ok_or_else(|| "이전 버전 경로를 안전하게 표현할 수 없습니다.".to_string())?;
    let target_install = resolve_portable_install(&base, &app_id, &target, target_exe_text)
        .map_err(|_| "검증된 이전 버전 경로를 확인할 수 없습니다.".to_string())?;
    let target_exe_text = target_install
        .executable
        .to_str()
        .ok_or_else(|| "이전 버전 경로를 안전하게 표현할 수 없습니다.".to_string())?;

    let next = crate::core::layout::Current {
        version: target.clone(),
        exe_path: target_exe_text.to_string(),
        installed_at: now_ms(),
        previous_version: Some(current.version.clone()),
    };
    write_current(&base, &app_id, &next)?;

    // registry의 exe_path 갱신
    if let Some(inst) = read_registry(&app)?.into_iter().find(|a| a.app == app_id) {
        let mut reg = read_registry(&app)?;
        reg.retain(|a| a.app != app_id);
        reg.push(InstalledApp {
            app: inst.app,
            version: target,
            mode: "portable".into(),
            exe_path: next.exe_path,
        });
        write_registry(&app, &reg)?;
    }
    Ok(format!(
        "{app_id}를 버전 {}으로 되돌렸습니다.",
        next.version
    ))
}

/// versions/ 아래에서 실행 파일이 실제 존재하는 버전 목록.
fn installed_versions_with_exe(base: &std::path::Path, app_id: &str) -> Vec<String> {
    let mut versions: Vec<String> =
        std::fs::read_dir(crate::core::layout::versions_root(base, app_id))
            .map(|entries| {
                entries
                    .flatten()
                    .filter(|e| e.path().is_dir())
                    .filter_map(|e| e.file_name().into_string().ok())
                    .collect()
            })
            .unwrap_or_default();
    versions.sort();
    versions.retain(|v| crate::core::layout::version_exe(base, app_id, v).exists());
    versions
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn read_current(base: &std::path::Path, app_id: &str) -> Option<crate::core::layout::Current> {
    let path = crate::core::layout::current_json(base, app_id);
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn write_current(
    base: &std::path::Path,
    app_id: &str,
    current: &crate::core::layout::Current,
) -> Result<(), String> {
    let path = crate::core::layout::current_json(base, app_id);
    let json = serde_json::to_string_pretty(current)
        .map_err(|_| "현재 버전 상태를 직렬화할 수 없습니다.".to_string())?;
    devbox_filesystem::atomic_write(path, json.as_bytes())
        .map_err(|_| "현재 버전 상태를 안전하게 기록할 수 없습니다.".to_string())
}

/// 설치된 앱 실행 (휴대용만).
#[tauri::command]
pub fn launch(app: tauri::AppHandle, app_id: String) -> Result<(), String> {
    let install = managed_portable(&app, &app_id)?;
    std::process::Command::new(&install.executable)
        .spawn()
        .map_err(|_| "휴대용 앱을 실행할 수 없습니다.".to_string())?;
    Ok(())
}

/// Manager가 검증한 현재 portable version directory만 연다. registry의 raw
/// path는 frontend로 반환하지 않으며 catalog/layout/canonical identity를 모두
/// 다시 확인한다.
#[tauri::command]
pub fn open_install_folder(app: tauri::AppHandle, app_id: String) -> Result<(), String> {
    let install = managed_portable(&app, &app_id)?;
    let directory = install
        .install_dir()
        .map_err(|_| "검증된 설치 폴더를 확인할 수 없습니다.".to_string())?;
    let directory = directory
        .to_str()
        .ok_or_else(|| "설치 폴더 경로를 안전하게 표현할 수 없습니다.".to_string())?;
    app.opener()
        .open_path(directory, None::<&str>)
        .map_err(|_| "설치 폴더를 열 수 없습니다.".to_string())
}

struct RemovalContext {
    snapshot: RegistrySnapshot,
    record: custom_root::InstallRecord,
    plan: Option<RemovalPlan>,
}

fn removal_error(_error: RemovalError) -> String {
    "제거 대상을 안전하게 확인할 수 없습니다. 설치 상태를 확인한 뒤 다시 시도하세요.".to_string()
}

fn installer_removal_error() -> String {
    "설치 패키지 앱은 Manager가 실제 설치 위치나 제거 프로그램을 소유하지 않아 제거할 수 없습니다."
        .to_string()
}

fn removal_context(app: &tauri::AppHandle, app_id: &str) -> Result<RemovalContext, String> {
    ensure_catalog_target(app_id)?;
    // Prefer the ordinary lifecycle parser. If it rejects only because a
    // portable executable is already gone, the bounded removal parser can
    // still prove the exact app-owned layout and clean the stale record.
    let snapshot =
        read_registry_snapshot(app).or_else(|_| read_registry_snapshot_for_removal(app))?;
    let record = snapshot
        .records
        .iter()
        .find(|record| record.app == app_id)
        .cloned()
        .ok_or_else(|| "설치된 앱의 제거 상태를 찾을 수 없습니다.".to_string())?;
    let plan = if record.mode == "portable" {
        Some(
            inspect_portable_removal(
                &snapshot.location.root,
                &record.app,
                &record.version,
                &record.exe_path,
            )
            .map_err(removal_error)?,
        )
    } else {
        None
    };
    Ok(RemovalContext {
        snapshot,
        record,
        plan,
    })
}

fn removal_preview_view(context: &RemovalContext) -> Result<RemovePreviewView, String> {
    if context.record.mode == "installer" {
        return Ok(RemovePreviewView {
            app_id: context.record.app.clone(),
            mode: context.record.mode.clone(),
            version: context.record.version.clone(),
            state: "unsupported-installer".to_string(),
            can_remove: false,
            registry_revision: context.snapshot.location.registry_revision,
            catalog_revision: context.snapshot.location.catalog_revision,
            root_id: context.snapshot.location.root_id.clone(),
            manifest_digest: context.snapshot.digest.clone(),
            target_path: None,
            owned_entry_count: 0,
            owned_bytes: 0,
            preserves_user_data: true,
        });
    }
    let plan = context
        .plan
        .as_ref()
        .ok_or_else(|| "제거 대상을 안전하게 확인할 수 없습니다.".to_string())?;
    let target_path = plan
        .app_root
        .to_str()
        .ok_or_else(|| "제거 대상을 안전하게 표시할 수 없습니다.".to_string())?
        .to_string();
    Ok(RemovePreviewView {
        app_id: context.record.app.clone(),
        mode: context.record.mode.clone(),
        version: context.record.version.clone(),
        state: plan.state.as_code().to_string(),
        can_remove: true,
        registry_revision: context.snapshot.location.registry_revision,
        catalog_revision: context.snapshot.location.catalog_revision,
        root_id: context.snapshot.location.root_id.clone(),
        manifest_digest: context.snapshot.digest.clone(),
        target_path: Some(target_path),
        owned_entry_count: plan.owned_entry_count,
        owned_bytes: plan.owned_bytes,
        preserves_user_data: true,
    })
}

/// Read-only removal preflight.  It validates the selected app's catalog,
/// locator provenance, app-owned manifest, exact portable path, and bounded
/// tree before the UI offers a separate confirmation action.
#[tauri::command]
pub fn preview_remove_app(
    app: tauri::AppHandle,
    app_id: String,
) -> Result<RemovePreviewView, String> {
    removal_preview_view(&removal_context(&app, &app_id)?)
}

fn removal_request_matches(request: &RemoveAppRequest, context: &RemovalContext) -> bool {
    request.app_id == context.record.app
        && request.expected_registry_revision == context.snapshot.location.registry_revision
        && request.expected_catalog_revision == context.snapshot.location.catalog_revision
        && request.expected_root_id == context.snapshot.location.root_id
        && request.expected_manifest_digest == context.snapshot.digest
        && request.expected_manifest_digest.len() == 64
        && request
            .expected_manifest_digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
}

fn records_to_installed(records: &[custom_root::InstallRecord]) -> Vec<InstalledApp> {
    records
        .iter()
        .map(|record| InstalledApp {
            app: record.app.clone(),
            version: record.version.clone(),
            mode: record.mode.clone(),
            exe_path: record.exe_path.clone(),
        })
        .collect()
}

/// Remove only the app-owned portable tree after a preview token and manifest
/// CAS check.  Installer records deliberately fail closed because Manager
/// owns the installer process invocation, not the installer's final location
/// or uninstaller.  User data is never a target of this command.
#[tauri::command]
pub fn remove_portable_app(
    app: tauri::AppHandle,
    request: RemoveAppRequest,
) -> Result<RemoveResultView, String> {
    let lock = REMOVAL_LOCK.get_or_init(|| Mutex::new(()));
    let _guard = lock
        .lock()
        .map_err(|_| "제거 작업을 시작할 수 없습니다. 잠시 후 다시 시도하세요.".to_string())?;
    let context = removal_context(&app, &request.app_id)?;
    if context.record.mode == "installer" {
        return Err(installer_removal_error());
    }
    if !removal_request_matches(&request, &context) {
        return Err("설치 상태가 바뀌었습니다. 최신 제거 미리 보기를 다시 확인하세요.".to_string());
    }
    let plan = context
        .plan
        .as_ref()
        .ok_or_else(|| "제거 대상을 안전하게 확인할 수 없습니다.".to_string())?;
    let original_records = records_to_installed(&context.snapshot.records);
    let next_records = original_records
        .iter()
        .filter(|record| record.app != request.app_id)
        .cloned()
        .collect::<Vec<_>>();
    if next_records.len() == original_records.len() {
        return Err("설치된 앱의 제거 상태를 찾을 수 없습니다.".to_string());
    }

    // Claim the manifest before filesystem mutation.  If deletion is
    // interrupted, the original bytes can be restored and the same app can
    // be previewed/retried without losing ownership evidence.
    let next_digest = write_registry_if_current(&app, &context.snapshot, &next_records)?;
    let post_write = read_registry_snapshot(&app)?;
    if !same_location(&post_write.location, &context.snapshot.location)
        || post_write.digest != next_digest
    {
        let _ = restore_manifest_if_current(
            &app,
            &context.snapshot.location,
            &next_digest,
            &context.snapshot.bytes,
        );
        return Err("설치 상태가 바뀌었습니다. 최신 제거 미리 보기를 다시 확인하세요.".to_string());
    }

    let outcome = match remove_portable_tree(plan) {
        Ok(outcome) => outcome,
        Err(error) => {
            let restored = restore_manifest_if_current(
                &app,
                &context.snapshot.location,
                &next_digest,
                &context.snapshot.bytes,
            );
            if !restored {
                return Err(
                    "제거 중 안전 검증에 실패했습니다. 남은 파일과 설치 상태를 확인한 뒤 복구하세요."
                        .to_string(),
                );
            }
            return Err(removal_error(error));
        }
    };

    if !outcome.complete {
        let restored = restore_manifest_if_current(
            &app,
            &context.snapshot.location,
            &next_digest,
            &context.snapshot.bytes,
        );
        return Ok(RemoveResultView {
            status: "partial".to_string(),
            message: if restored {
                "앱 파일을 모두 제거하지 못했습니다. 설치 기록은 보존했습니다. 잠금·권한 문제를 해결한 뒤 최신 제거 미리 보기에서 다시 시도하세요.".to_string()
            } else {
                "앱 파일 일부만 제거했고 설치 상태를 안전하게 복구하지 못했습니다. 남은 파일과 설치 상태를 확인한 뒤 Manager를 다시 시작하세요.".to_string()
            },
            removed_entry_count: outcome.removed_entry_count,
            remaining_entry_count: outcome.remaining_entry_count,
            preserves_user_data: true,
        });
    }

    if let Err(error) = sync_runtime_metadata(&app) {
        eprintln!("devbox: runtime metadata sync will retry next launch: {error}");
    }
    Ok(RemoveResultView {
        status: "removed".to_string(),
        message: "휴대용 앱의 Manager 소유 파일을 제거했습니다. 앱 사용자 데이터는 유지됩니다."
            .to_string(),
        removed_entry_count: outcome.removed_entry_count,
        remaining_entry_count: 0,
        preserves_user_data: true,
    })
}

fn registry_with_entry(
    original: &[InstalledApp],
    name: String,
    version: String,
    mode: &str,
    exe_path: String,
) -> Vec<InstalledApp> {
    let mut reg = original.to_vec();
    reg.retain(|a| a.app != name);
    reg.push(InstalledApp {
        app: name,
        version,
        mode: mode.to_string(),
        exe_path,
    });
    reg
}

fn restore_current(
    base: &std::path::Path,
    app_id: &str,
    previous: Option<&crate::core::layout::Current>,
) -> Result<(), String> {
    if let Some(previous) = previous {
        return write_current(base, app_id, previous);
    }
    let path = crate::core::layout::current_json(base, app_id);
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err("current 상태를 복구할 수 없습니다.".to_string()),
    }
}

fn sync_runtime_metadata_best_effort(app: &tauri::AppHandle) {
    if let Err(error) = sync_runtime_metadata(app) {
        eprintln!("devbox: runtime metadata sync will retry next launch: {error}");
    }
}

async fn download(
    client: &reqwest::Client,
    url: &str,
    dest: &std::path::Path,
    expected_size: i64,
    expected_sha: &str,
) -> Result<(), String> {
    validate_download_target(dest)
        .map_err(|_| "다운로드 파일 경로를 안전하게 준비할 수 없습니다.".to_string())?;
    if expected_size < 0 {
        return Err("다운로드 크기 정보가 올바르지 않습니다.".to_string());
    }
    // 1. 요청 전 URL 검증
    if !is_allowed(url) {
        return Err("허용되지 않은 다운로드 URL".into());
    }
    let resp = client
        .get(url)
        .header(USER_AGENT, "devbox-manager")
        .send()
        .await
        .map_err(|_| "다운로드 요청을 완료할 수 없습니다.".to_string())?;
    if !resp.status().is_success() {
        return Err("다운로드 서버가 파일을 제공하지 않았습니다.".to_string());
    }
    // 2. redirect 후 최종 URL을 다시 검증한다 (중간 hop이 아니라 최종 응답의 URL)
    if !is_allowed(resp.url().as_str()) {
        return Err("redirect 후 URL이 허용 범위를 벗어났다".into());
    }
    // 3. Content-Length가 manifest size와 다르면 즉시 중단
    if let Some(cl) = resp.content_length() {
        if cl != expected_size as u64 {
            return Err("다운로드 크기 정보가 manifest와 일치하지 않습니다.".to_string());
        }
    }

    // 4. .partial로 streaming 기록. 청크마다 SHA-256 갱신, 누적 크기 상한 검사
    let partial = partial_path(dest);
    // `create_new` refuses an existing regular partial as well as a link and
    // never follows a symlink between validation and open. Interrupted
    // partials are removed at startup; a retry in the same process is kept
    // fail-closed rather than truncating an attacker-controlled path.
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&partial)
        .map_err(|_| "다운로드 임시 파일을 안전하게 준비할 수 없습니다.".to_string())?;
    let mut hasher = Sha256::new();
    let mut total: u64 = 0;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = match chunk {
            Ok(chunk) => chunk,
            Err(_) => {
                drop(file);
                let _ = std::fs::remove_file(&partial);
                return Err("다운로드 스트림을 읽을 수 없습니다.".to_string());
            }
        };
        let Some(next_total) = total.checked_add(chunk.len() as u64) else {
            drop(file);
            let _ = std::fs::remove_file(&partial);
            return Err("다운로드 크기를 확인할 수 없습니다.".to_string());
        };
        total = next_total;
        if total > expected_size as u64 {
            drop(file);
            let _ = std::fs::remove_file(&partial);
            return Err("다운로드 크기가 manifest 상한을 초과했습니다.".to_string());
        }
        hasher.update(&chunk);
        if file.write_all(&chunk).is_err() {
            drop(file);
            let _ = std::fs::remove_file(&partial);
            return Err("다운로드 파일을 기록할 수 없습니다.".to_string());
        }
    }
    if file.flush().is_err() {
        drop(file);
        let _ = std::fs::remove_file(&partial);
        return Err("다운로드 파일을 완료할 수 없습니다.".to_string());
    }
    drop(file);

    // 5. 완료 후 총 바이트와 digest를 manifest와 대조
    if validate_size(expected_size, total as i64).is_err() {
        let _ = std::fs::remove_file(&partial);
        return Err("다운로드 크기가 manifest와 일치하지 않습니다.".to_string());
    }
    let digest = format!("{:x}", hasher.finalize());
    if validate_digest(expected_sha, &digest).is_err() {
        let _ = std::fs::remove_file(&partial);
        return Err("다운로드 무결성 검증에 실패했습니다.".to_string());
    }

    // 6. 일치하면 최종 경로로 rename
    std::fs::rename(&partial, dest)
        .map_err(|_| "검증된 다운로드 파일을 설치 위치로 옮길 수 없습니다.".to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEST_ROOT: AtomicU64 = AtomicU64::new(0);

    struct TestRoot(PathBuf);

    impl TestRoot {
        fn new() -> Self {
            let nonce = NEXT_TEST_ROOT.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "devbox-manager-batch-{}-{nonce}",
                std::process::id()
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn installed_view_never_serializes_registry_executable_path() {
        let secret_path = r"C:\secret\portable\app.exe";
        let installed = InstalledApp {
            app: "port-manager".to_string(),
            version: "0.4.0".to_string(),
            mode: "portable".to_string(),
            exe_path: secret_path.to_string(),
        };

        let json = serde_json::to_string(&InstalledAppView::from(&installed)).unwrap();

        assert!(!json.contains("secret"));
        assert!(!json.contains("exePath"));
        assert!(!json.contains("exe_path"));
    }

    #[test]
    fn current_view_never_serializes_current_json_executable_path() {
        let secret_path = r"C:\secret\versions\0.4.0\app.exe";
        let current = crate::core::layout::Current {
            version: "0.4.0".to_string(),
            exe_path: secret_path.to_string(),
            installed_at: 1_000,
            previous_version: Some("0.3.0".to_string()),
        };

        let json = serde_json::to_string(&CurrentView::from(&current)).unwrap();

        assert!(!json.contains("secret"));
        assert!(!json.contains("exePath"));
        assert!(json.contains("previousVersion"));
    }

    #[test]
    fn registry_batch_update_replaces_only_the_target_app() {
        let original = vec![
            InstalledApp {
                app: "port-manager".to_string(),
                version: "0.2.1".to_string(),
                mode: "portable".to_string(),
                exe_path: "port-old.exe".to_string(),
            },
            InstalledApp {
                app: "code-pad".to_string(),
                version: "0.3.1".to_string(),
                mode: "portable".to_string(),
                exe_path: "code.exe".to_string(),
            },
        ];

        let updated = registry_with_entry(
            &original,
            "port-manager".to_string(),
            "0.2.2".to_string(),
            "portable",
            "port-new.exe".to_string(),
        );

        assert_eq!(updated.len(), 2);
        assert!(updated.iter().any(|entry| {
            entry.app == "port-manager"
                && entry.version == "0.2.2"
                && entry.exe_path == "port-new.exe"
        }));
        assert_eq!(
            updated.iter().find(|entry| entry.app == "code-pad"),
            original.iter().find(|entry| entry.app == "code-pad")
        );
    }

    #[test]
    fn failed_portable_commit_can_restore_or_remove_current_state() {
        let root = TestRoot::new();
        std::fs::create_dir_all(crate::core::layout::apps_root(&root.0, "port-manager")).unwrap();
        let previous = crate::core::layout::Current {
            version: "0.2.1".to_string(),
            exe_path: "old.exe".to_string(),
            installed_at: 1,
            previous_version: Some("0.2.0".to_string()),
        };
        let attempted = crate::core::layout::Current {
            version: "0.2.2".to_string(),
            exe_path: "new.exe".to_string(),
            installed_at: 2,
            previous_version: Some("0.2.1".to_string()),
        };
        write_current(&root.0, "port-manager", &attempted).unwrap();

        restore_current(&root.0, "port-manager", Some(&previous)).unwrap();
        assert_eq!(read_current(&root.0, "port-manager"), Some(previous));

        restore_current(&root.0, "port-manager", None).unwrap();
        assert!(read_current(&root.0, "port-manager").is_none());
    }

    #[test]
    fn partial_cleanup_removes_only_exact_managed_download_slots() {
        let root = TestRoot::new();
        let version = root.0.join("apps/port-manager/versions/0.4.0");
        std::fs::create_dir_all(version.join("nested")).unwrap();
        let managed = version.join("port-manager.exe.partial");
        let sibling = version.join("user.partial");
        let nested = version.join("nested/user.partial");
        std::fs::write(&managed, b"managed").unwrap();
        std::fs::write(&sibling, b"user").unwrap();
        std::fs::write(&nested, b"user").unwrap();
        let known = HashSet::from(["port-manager".to_string()]);

        cleanup_managed_partials(&root.0, &known);

        assert!(!managed.exists());
        assert_eq!(std::fs::read(sibling).unwrap(), b"user");
        assert_eq!(std::fs::read(nested).unwrap(), b"user");
    }

    #[test]
    fn registry_records_must_belong_to_the_selected_manager_catalog() {
        let records = vec![custom_root::InstallRecord {
            app: "port-manager".to_string(),
            version: "0.4.0".to_string(),
            mode: "installer".to_string(),
            exe_path: String::new(),
        }];
        assert!(registry_apps_are_known(
            &records,
            &HashSet::from(["port-manager".to_string()])
        ));
        assert!(!registry_apps_are_known(
            &records,
            &HashSet::from(["code-pad".to_string()])
        ));
    }

    #[cfg(unix)]
    #[test]
    fn unsafe_exact_partial_aborts_cleanup_without_following_or_deleting_it() {
        use std::os::unix::fs::symlink;

        let root = TestRoot::new();
        let version = root.0.join("apps/port-manager/versions/0.4.0");
        std::fs::create_dir_all(&version).unwrap();
        let outside = root.0.join("outside.partial");
        std::fs::write(&outside, b"outside").unwrap();
        symlink(&outside, version.join("port-manager.exe.partial")).unwrap();
        let known = HashSet::from(["port-manager".to_string()]);

        cleanup_managed_partials(&root.0, &known);

        assert_eq!(std::fs::read(outside).unwrap(), b"outside");
        assert!(version.join("port-manager.exe.partial").exists());
    }
}
