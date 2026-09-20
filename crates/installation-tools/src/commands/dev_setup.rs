//! WinGet Configuration v3 package-only review and guarded apply boundary.
//!
//! External YAML is selected and read natively, normalized through the strict
//! core allowlist, and stored behind a short-lived one-time preview token. The
//! original path and bytes never reach the renderer or WinGet. Apply renders a
//! fresh one-resource configuration for each reviewed package so results remain
//! per-resource and arbitrary DSC resources can never enter the process.

use crate::commands::related_tools::{
    acquire_related_action, run_guarded_winget, GuardedWingetOutcome,
};
use crate::core::dev_setup_configuration::{
    parse_configuration, render_configuration, ConfigurationFailure, PackageDesiredState,
    PackageRequirement, CONFIGURATION_SCHEMA_VERSION, MAX_CONFIGURATION_BYTES,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::ffi::OsString;
use std::fmt::Write as FmtWrite;
use std::io::Read;
#[cfg(windows)]
use std::io::Write as IoWrite;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;
use zeroize::Zeroizing;

const PREVIEW_TTL_MS: u64 = 5 * 60 * 1_000;
const MAX_STORED_PREVIEWS: usize = 4;
const PROBE_TIMEOUT: Duration = Duration::from_secs(30);
const APPLY_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const NO_APPLICATIONS_FOUND_EXIT_CODE: u32 = 0x8A15_0014;
const INVALID_CONFIGURATION: &str =
    "WinGet Configuration v3 package-only 파일을 안전하게 검토할 수 없습니다.";
const PREVIEW_EXPIRED: &str = "Dev Setup 적용 미리 보기가 만료되었거나 없습니다.";
const APPLY_CONFIRMATION_REQUIRED: &str =
    "Dev Setup 적용에는 구성·패키지 약관·권한 및 재부팅 위험 확인이 모두 필요합니다.";
const APPLY_BLOCKED: &str =
    "확인할 수 없는 패키지 상태가 있어 Dev Setup 적용을 시작할 수 없습니다.";
const STATE_ERROR: &str = "Dev Setup 작업 상태를 안전하게 처리할 수 없습니다.";

#[derive(Default)]
pub struct DevSetupConfigurationState {
    previews: Mutex<HashMap<String, StoredPreview>>,
    active_apply: Mutex<Option<Arc<AtomicBool>>>,
}

#[derive(Debug, Clone)]
struct StoredPreview {
    expires_at_ms: u64,
    configuration_digest: String,
    export_content: String,
    packages: Vec<StoredPackagePlan>,
    can_apply: bool,
    has_changes: bool,
}

#[derive(Debug, Clone)]
struct StoredPackagePlan {
    requirement: PackageRequirement,
    observation: PackageObservation,
    action: PackageAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PackageObservation {
    Present,
    Absent,
    UpdateAvailable,
    Unknown,
}

impl PackageObservation {
    fn as_code(self) -> &'static str {
        match self {
            Self::Present => "present",
            Self::Absent => "absent",
            Self::UpdateAvailable => "update-available",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PackageAction {
    None,
    Install,
    Update,
    ReconcileVersion,
    Verify,
}

impl PackageAction {
    fn as_code(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Install => "install",
            Self::Update => "update",
            Self::ReconcileVersion => "reconcile-version",
            Self::Verify => "verify",
        }
    }

    fn changes_system(self) -> bool {
        matches!(self, Self::Install | Self::Update | Self::ReconcileVersion)
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DevSetupConfigurationReviewView {
    pub schema_version: String,
    pub preview_id: String,
    pub expires_at_ms: u64,
    pub configuration_digest: String,
    pub source_trust: String,
    pub mode: String,
    pub can_apply: bool,
    pub has_changes: bool,
    pub requires_agreement_confirmation: bool,
    pub may_require_admin: bool,
    pub may_require_reboot: bool,
    pub packages: Vec<DevSetupPackageReviewView>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DevSetupPackageReviewView {
    pub package_id: String,
    pub desired: String,
    pub version: Option<String>,
    pub current_state: String,
    pub action: String,
    pub requested_agreement_acceptance: bool,
    pub declared_elevation: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DevSetupPreviewRequest {
    pub preview_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DevSetupApplyRequest {
    pub preview_id: String,
    pub confirmed: bool,
    pub accept_package_agreements: bool,
    pub acknowledge_admin_and_reboot: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DevSetupConfigurationExportView {
    pub filename: String,
    pub mime_type: String,
    pub content: String,
    pub byte_count: usize,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DevSetupApplyView {
    pub status: String,
    pub observed_at_ms: u64,
    pub results: Vec<DevSetupPackageApplyView>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DevSetupPackageApplyView {
    pub package_id: String,
    pub status: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ApplyPackageStatus {
    Unchanged,
    Applied,
    Failed,
    TimedOut,
    Cancelled,
    Skipped,
}

impl ApplyPackageStatus {
    fn as_code(self) -> &'static str {
        match self {
            Self::Unchanged => "unchanged",
            Self::Applied => "applied",
            Self::Failed => "failed",
            Self::TimedOut => "timed-out",
            Self::Cancelled => "cancelled",
            Self::Skipped => "skipped",
        }
    }
}

#[tauri::command]
pub async fn import_dev_setup_configuration(
    app: AppHandle,
    state: tauri::State<'_, DevSetupConfigurationState>,
) -> Result<Option<DevSetupConfigurationReviewView>, String> {
    // Starting a new import starts a new review session even when the picker is
    // later cancelled or the selected document is rejected. Revoke every older
    // token first so hidden renderer state cannot leave a stale preview active.
    clear_previews(&state)?;
    let selected = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .add_filter("WinGet Configuration", &["winget", "yaml", "yml"])
            .blocking_pick_file()
    })
    .await
    .map_err(|_| INVALID_CONFIGURATION.to_string())?;
    let Some(selected) = selected else {
        return Ok(None);
    };
    let path = selected
        .into_path()
        .map_err(|_| INVALID_CONFIGURATION.to_string())?;
    let stored = tauri::async_runtime::spawn_blocking(move || build_preview_from_path(&path))
        .await
        .map_err(|_| INVALID_CONFIGURATION.to_string())??;
    let preview_id = random_id("devsetup")?;
    let view = review_view(&preview_id, &stored);
    store_preview(&state, preview_id, stored)?;
    Ok(Some(view))
}

#[tauri::command]
pub fn discard_dev_setup_configuration(
    state: tauri::State<'_, DevSetupConfigurationState>,
    request: DevSetupPreviewRequest,
) -> Result<(), String> {
    validate_preview_id(&request.preview_id)?;
    discard_preview(&state, &request.preview_id)
}

#[tauri::command]
pub fn export_dev_setup_configuration(
    state: tauri::State<'_, DevSetupConfigurationState>,
    request: DevSetupPreviewRequest,
) -> Result<DevSetupConfigurationExportView, String> {
    validate_preview_id(&request.preview_id)?;
    let previews = lock(&state.previews)?;
    let preview = previews
        .get(&request.preview_id)
        .filter(|preview| now_ms() < preview.expires_at_ms)
        .ok_or_else(|| PREVIEW_EXPIRED.to_string())?;
    Ok(DevSetupConfigurationExportView {
        filename: "devbox-packages.winget".to_string(),
        mime_type: "application/yaml;charset=utf-8".to_string(),
        byte_count: preview.export_content.len(),
        content: preview.export_content.clone(),
        sha256: preview.configuration_digest.clone(),
    })
}

#[tauri::command]
pub async fn apply_dev_setup_configuration(
    app: AppHandle,
    state: tauri::State<'_, DevSetupConfigurationState>,
    request: DevSetupApplyRequest,
) -> Result<DevSetupApplyView, String> {
    validate_preview_id(&request.preview_id)?;
    if !request.confirmed
        || !request.accept_package_agreements
        || !request.acknowledge_admin_and_reboot
    {
        return Err(APPLY_CONFIRMATION_REQUIRED.to_string());
    }
    let cache_root = app
        .path()
        .app_cache_dir()
        .map_err(|_| "Dev Setup 임시 파일을 안전하게 준비할 수 없습니다.".to_string())?;

    let cancel = Arc::new(AtomicBool::new(false));
    let preview = {
        let mut active = lock(&state.active_apply)?;
        if active.is_some() {
            return Err("다른 Dev Setup 적용이 진행 중입니다.".to_string());
        }
        let mut previews = lock(&state.previews)?;
        let preview = previews
            .remove(&request.preview_id)
            .ok_or_else(|| PREVIEW_EXPIRED.to_string())?;
        if now_ms() >= preview.expires_at_ms {
            return Err(PREVIEW_EXPIRED.to_string());
        }
        if !preview.can_apply || !preview.has_changes {
            return Err(APPLY_BLOCKED.to_string());
        }
        *active = Some(cancel.clone());
        preview
    };
    let task_cancel = cancel.clone();
    let task = tauri::async_runtime::spawn_blocking(move || {
        let _guard = acquire_related_action()?;
        apply_preview(preview, &cache_root, &task_cancel)
    });
    let result = task
        .await
        .unwrap_or_else(|_| Err("Dev Setup 적용을 완료할 수 없습니다.".to_string()));
    if let Ok(mut active) = state.active_apply.lock() {
        if active
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, &cancel))
        {
            *active = None;
        }
    }
    result
}

#[tauri::command]
pub fn cancel_dev_setup_apply(
    state: tauri::State<'_, DevSetupConfigurationState>,
) -> Result<(), String> {
    if let Some(cancel) = lock(&state.active_apply)?.as_ref() {
        cancel.store(true, Ordering::Relaxed);
    }
    Ok(())
}

fn build_preview_from_path(path: &Path) -> Result<StoredPreview, String> {
    let mut file = devbox_filesystem::open_filesystem_object(path, false)
        .map_err(|_| INVALID_CONFIGURATION.to_string())?
        .0;
    let length = file
        .metadata()
        .map_err(|_| INVALID_CONFIGURATION.to_string())?
        .len();
    if length == 0 || length > MAX_CONFIGURATION_BYTES as u64 {
        return Err(INVALID_CONFIGURATION.to_string());
    }
    // Protect the buffer before the first read so partial bytes are also wiped
    // when a read error or growth-beyond-bound error exits early.
    let mut bytes = Zeroizing::new(Vec::with_capacity(length as usize));
    std::io::Read::by_ref(&mut file)
        .take((MAX_CONFIGURATION_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| INVALID_CONFIGURATION.to_string())?;
    if bytes.len() > MAX_CONFIGURATION_BYTES {
        return Err(INVALID_CONFIGURATION.to_string());
    }
    let input =
        std::str::from_utf8(bytes.as_slice()).map_err(|_| INVALID_CONFIGURATION.to_string())?;
    let configuration = parse_configuration(input).map_err(configuration_error)?;
    let export_content =
        render_configuration(&configuration.packages, false).map_err(configuration_error)?;
    let configuration_digest = digest_hex(export_content.as_bytes());

    let _guard = acquire_related_action()?;
    let mut packages = Vec::with_capacity(configuration.packages.len());
    let mut probe_blocked = false;
    for requirement in configuration.packages {
        let observation = if probe_blocked {
            PackageObservation::Unknown
        } else {
            let (observation, blocked) = probe_package(&requirement);
            probe_blocked = blocked;
            observation
        };
        let action = plan_action(&requirement.desired, observation);
        packages.push(StoredPackagePlan {
            requirement,
            observation,
            action,
        });
    }
    let has_changes = packages
        .iter()
        .any(|package| package.action.changes_system());
    let can_apply = has_changes
        && packages
            .iter()
            .all(|package| package.action != PackageAction::Verify);
    Ok(StoredPreview {
        expires_at_ms: now_ms().saturating_add(PREVIEW_TTL_MS),
        configuration_digest,
        export_content,
        packages,
        can_apply,
        has_changes,
    })
}

fn configuration_error(_error: ConfigurationFailure) -> String {
    INVALID_CONFIGURATION.to_string()
}

/// The boolean indicates a process/source-level failure after which repeating
/// the same bounded probe for every remaining package would only delay a
/// fail-closed review. Remaining packages are marked unknown instead.
fn probe_package(requirement: &PackageRequirement) -> (PackageObservation, bool) {
    let installed = run_guarded_winget(
        vec![
            "list".into(),
            "--id".into(),
            requirement.package_id.clone().into(),
            "--exact".into(),
            "--source".into(),
            "winget".into(),
            "--disable-interactivity".into(),
        ],
        PROBE_TIMEOUT,
        None,
    );
    let (observation, unavailable) = classify_list_probe(installed);
    if observation != PackageObservation::Present
        || !matches!(requirement.desired, PackageDesiredState::Latest)
    {
        return (observation, unavailable);
    }
    let upgrade = run_guarded_winget(
        vec![
            "list".into(),
            "--id".into(),
            requirement.package_id.clone().into(),
            "--exact".into(),
            "--source".into(),
            "winget".into(),
            "--upgrade-available".into(),
            "--disable-interactivity".into(),
        ],
        PROBE_TIMEOUT,
        None,
    );
    match upgrade {
        GuardedWingetOutcome::Exited(0) => (PackageObservation::UpdateAvailable, false),
        GuardedWingetOutcome::Exited(NO_APPLICATIONS_FOUND_EXIT_CODE) => {
            (PackageObservation::Present, false)
        }
        GuardedWingetOutcome::Exited(_)
        | GuardedWingetOutcome::Unavailable
        | GuardedWingetOutcome::FailedToStart
        | GuardedWingetOutcome::TimedOut
        | GuardedWingetOutcome::Cancelled => (PackageObservation::Unknown, true),
    }
}

fn classify_list_probe(outcome: GuardedWingetOutcome) -> (PackageObservation, bool) {
    match outcome {
        GuardedWingetOutcome::Exited(0) => (PackageObservation::Present, false),
        GuardedWingetOutcome::Exited(NO_APPLICATIONS_FOUND_EXIT_CODE) => {
            (PackageObservation::Absent, false)
        }
        GuardedWingetOutcome::Exited(_)
        | GuardedWingetOutcome::Unavailable
        | GuardedWingetOutcome::FailedToStart
        | GuardedWingetOutcome::TimedOut
        | GuardedWingetOutcome::Cancelled => (PackageObservation::Unknown, true),
    }
}

fn plan_action(desired: &PackageDesiredState, observation: PackageObservation) -> PackageAction {
    match observation {
        PackageObservation::Unknown => PackageAction::Verify,
        PackageObservation::Absent => PackageAction::Install,
        PackageObservation::UpdateAvailable => PackageAction::Update,
        PackageObservation::Present => match desired {
            PackageDesiredState::Version(_) => PackageAction::ReconcileVersion,
            PackageDesiredState::Present | PackageDesiredState::Latest => PackageAction::None,
        },
    }
}

fn apply_preview(
    preview: StoredPreview,
    cache_root: &Path,
    cancel: &AtomicBool,
) -> Result<DevSetupApplyView, String> {
    let mut results = Vec::with_capacity(preview.packages.len());
    let mut stop = false;
    for package in preview.packages {
        let status = if stop || cancel.load(Ordering::Relaxed) {
            stop = true;
            ApplyPackageStatus::Skipped
        } else if !package.action.changes_system() {
            ApplyPackageStatus::Unchanged
        } else {
            let content = render_configuration(std::slice::from_ref(&package.requirement), true)
                .map_err(configuration_error)?;
            let guarded = GuardedConfigurationFile::create(cache_root, content.as_bytes())?;
            let args = vec![
                OsString::from("configure"),
                OsString::from("--file"),
                guarded.path().as_os_str().to_os_string(),
                OsString::from("--accept-configuration-agreements"),
                OsString::from("--disable-interactivity"),
                OsString::from("--suppress-initial-details"),
            ];
            match run_guarded_winget(args, APPLY_TIMEOUT, Some(cancel)) {
                GuardedWingetOutcome::Exited(0) => ApplyPackageStatus::Applied,
                GuardedWingetOutcome::TimedOut => {
                    stop = true;
                    ApplyPackageStatus::TimedOut
                }
                GuardedWingetOutcome::Cancelled => {
                    stop = true;
                    ApplyPackageStatus::Cancelled
                }
                GuardedWingetOutcome::Exited(_)
                | GuardedWingetOutcome::Unavailable
                | GuardedWingetOutcome::FailedToStart => ApplyPackageStatus::Failed,
            }
        };
        results.push(DevSetupPackageApplyView {
            package_id: package.requirement.package_id,
            status: status.as_code().to_string(),
        });
    }
    let status = if results
        .iter()
        .any(|result| result.status == ApplyPackageStatus::Cancelled.as_code())
        || (cancel.load(Ordering::Relaxed)
            && results
                .iter()
                .any(|result| result.status == ApplyPackageStatus::Skipped.as_code()))
    {
        "cancelled"
    } else if results
        .iter()
        .all(|result| matches!(result.status.as_str(), "applied" | "unchanged"))
    {
        "complete"
    } else {
        "partial"
    };
    Ok(DevSetupApplyView {
        status: status.to_string(),
        observed_at_ms: now_ms(),
        results,
    })
}

struct GuardedConfigurationFile {
    path: PathBuf,
    #[cfg(windows)]
    _handle: Option<std::fs::File>,
}

impl GuardedConfigurationFile {
    #[cfg(windows)]
    fn create(cache_root: &Path, content: &[u8]) -> Result<Self, String> {
        use std::fs::OpenOptions;
        use std::os::windows::fs::OpenOptionsExt;
        use windows::Win32::Storage::FileSystem::FILE_SHARE_READ;

        let directory = cache_root.join("dev-setup");
        std::fs::create_dir_all(&directory)
            .map_err(|_| "Dev Setup 임시 파일을 안전하게 준비할 수 없습니다.".to_string())?;
        devbox_filesystem::ensure_no_links(&directory)
            .map_err(|_| "Dev Setup 임시 파일을 안전하게 준비할 수 없습니다.".to_string())?;
        let path = directory.join(format!("{}.winget", random_id("apply")?));
        let mut handle = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .share_mode(FILE_SHARE_READ.0)
            .open(&path)
            .map_err(|_| "Dev Setup 임시 파일을 안전하게 준비할 수 없습니다.".to_string())?;
        let prepared = handle
            .write_all(content)
            .and_then(|_| handle.sync_all())
            .and_then(|_| devbox_filesystem::ensure_no_links(&path));
        if prepared.is_err() {
            // The file is not yet wrapped by GuardedConfigurationFile, so clean
            // it explicitly on every preparation failure. Close our handle
            // first because its share mode intentionally denies deletion.
            drop(handle);
            let _ = std::fs::remove_file(&path);
            return Err("Dev Setup 임시 파일을 안전하게 준비할 수 없습니다.".to_string());
        }
        Ok(Self {
            path,
            _handle: Some(handle),
        })
    }

    #[cfg(not(windows))]
    fn create(_cache_root: &Path, _content: &[u8]) -> Result<Self, String> {
        Err("Dev Setup package apply는 Windows에서만 사용할 수 있습니다.".to_string())
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for GuardedConfigurationFile {
    fn drop(&mut self) {
        #[cfg(windows)]
        drop(self._handle.take());
        let _ = std::fs::remove_file(&self.path);
    }
}

fn review_view(preview_id: &str, preview: &StoredPreview) -> DevSetupConfigurationReviewView {
    DevSetupConfigurationReviewView {
        schema_version: CONFIGURATION_SCHEMA_VERSION.to_string(),
        preview_id: preview_id.to_string(),
        expires_at_ms: preview.expires_at_ms,
        configuration_digest: preview.configuration_digest.clone(),
        source_trust: "external-restricted".to_string(),
        mode: "package-only".to_string(),
        can_apply: preview.can_apply,
        has_changes: preview.has_changes,
        requires_agreement_confirmation: true,
        may_require_admin: preview.has_changes,
        may_require_reboot: preview.has_changes,
        packages: preview
            .packages
            .iter()
            .map(|package| DevSetupPackageReviewView {
                package_id: package.requirement.package_id.clone(),
                desired: match package.requirement.desired {
                    PackageDesiredState::Present => "present",
                    PackageDesiredState::Latest => "latest",
                    PackageDesiredState::Version(_) => "version",
                }
                .to_string(),
                version: match &package.requirement.desired {
                    PackageDesiredState::Version(version) => Some(version.clone()),
                    PackageDesiredState::Present | PackageDesiredState::Latest => None,
                },
                current_state: package.observation.as_code().to_string(),
                action: package.action.as_code().to_string(),
                requested_agreement_acceptance: package.requirement.requested_agreement_acceptance,
                declared_elevation: package.requirement.declared_elevation,
            })
            .collect(),
    }
}

fn store_preview(
    state: &DevSetupConfigurationState,
    preview_id: String,
    preview: StoredPreview,
) -> Result<(), String> {
    let now = now_ms();
    let mut previews = lock(&state.previews)?;
    previews.retain(|_, stored| now < stored.expires_at_ms);
    if previews.len() >= MAX_STORED_PREVIEWS {
        if let Some(oldest) = previews
            .iter()
            .min_by_key(|(_, stored)| stored.expires_at_ms)
            .map(|(id, _)| id.clone())
        {
            previews.remove(&oldest);
        }
    }
    previews.insert(preview_id, preview);
    Ok(())
}

fn clear_previews(state: &DevSetupConfigurationState) -> Result<(), String> {
    lock(&state.previews)?.clear();
    Ok(())
}

fn discard_preview(state: &DevSetupConfigurationState, preview_id: &str) -> Result<(), String> {
    lock(&state.previews)?.remove(preview_id);
    Ok(())
}

fn validate_preview_id(value: &str) -> Result<(), String> {
    if value.len() != 73
        || !value.starts_with("devsetup-")
        || !value[9..]
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(PREVIEW_EXPIRED.to_string());
    }
    Ok(())
}

fn random_id(prefix: &str) -> Result<String, String> {
    let mut random = [0_u8; 32];
    getrandom::fill(&mut random).map_err(|_| STATE_ERROR.to_string())?;
    let mut result = String::with_capacity(prefix.len() + 1 + random.len() * 2);
    result.push_str(prefix);
    result.push('-');
    for byte in random {
        write!(&mut result, "{byte:02x}").expect("writing to String cannot fail");
    }
    Ok(result)
}

fn digest_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut result = String::with_capacity(64);
    for byte in digest {
        write!(&mut result, "{byte:02x}").expect("writing to String cannot fail");
    }
    result
}

fn lock<T>(mutex: &Mutex<T>) -> Result<MutexGuard<'_, T>, String> {
    mutex.lock().map_err(|_| STATE_ERROR.to_string())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
        .unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn requirement(desired: PackageDesiredState) -> PackageRequirement {
        PackageRequirement {
            package_id: "Git.Git".into(),
            desired,
            requested_agreement_acceptance: false,
            declared_elevation: false,
        }
    }

    #[test]
    fn only_the_exact_no_applications_exit_code_means_absent() {
        assert_eq!(
            classify_list_probe(GuardedWingetOutcome::Exited(0)),
            (PackageObservation::Present, false)
        );
        assert_eq!(
            classify_list_probe(GuardedWingetOutcome::Exited(
                NO_APPLICATIONS_FOUND_EXIT_CODE
            )),
            (PackageObservation::Absent, false)
        );
        assert_eq!(
            classify_list_probe(GuardedWingetOutcome::Exited(1)),
            (PackageObservation::Unknown, true)
        );
        assert_eq!(
            classify_list_probe(GuardedWingetOutcome::Unavailable),
            (PackageObservation::Unknown, true)
        );
        assert_eq!(
            classify_list_probe(GuardedWingetOutcome::TimedOut),
            (PackageObservation::Unknown, true)
        );
        assert_eq!(
            classify_list_probe(GuardedWingetOutcome::FailedToStart),
            (PackageObservation::Unknown, true)
        );
    }

    #[test]
    fn unknown_never_becomes_an_install_suggestion() {
        for desired in [
            PackageDesiredState::Present,
            PackageDesiredState::Latest,
            PackageDesiredState::Version("1.0.0".into()),
        ] {
            assert_eq!(
                plan_action(&desired, PackageObservation::Unknown),
                PackageAction::Verify
            );
        }
        assert_eq!(
            plan_action(
                &PackageDesiredState::Latest,
                PackageObservation::UpdateAvailable
            ),
            PackageAction::Update
        );
        assert_eq!(
            plan_action(
                &PackageDesiredState::Version("1.0.0".into()),
                PackageObservation::Present
            ),
            PackageAction::ReconcileVersion
        );
    }

    #[test]
    fn review_is_bounded_opaque_and_does_not_copy_external_text() {
        let package = StoredPackagePlan {
            requirement: requirement(PackageDesiredState::Latest),
            observation: PackageObservation::Absent,
            action: PackageAction::Install,
        };
        let preview = StoredPreview {
            expires_at_ms: now_ms() + PREVIEW_TTL_MS,
            configuration_digest: "a".repeat(64),
            export_content: "safe".into(),
            packages: vec![package],
            can_apply: true,
            has_changes: true,
        };
        let id = format!("devsetup-{}", "b".repeat(64));
        let view = review_view(&id, &preview);
        let serialized = serde_json::to_string(&view).unwrap();
        assert!(view.can_apply);
        assert_eq!(view.packages[0].action, "install");
        assert!(!serialized.contains("C:\\"));
        assert!(!serialized.contains("/home/"));
        assert!(!serialized.contains("RunCommandOnSet"));
    }

    #[test]
    fn apply_request_is_strict_and_requires_three_explicit_confirmations() {
        let request: DevSetupApplyRequest = serde_json::from_str(&format!(
            r#"{{"previewId":"devsetup-{}","confirmed":true,"acceptPackageAgreements":true,"acknowledgeAdminAndReboot":true}}"#,
            "a".repeat(64)
        ))
        .unwrap();
        assert!(request.confirmed && request.accept_package_agreements);
        assert!(serde_json::from_str::<DevSetupApplyRequest>(&format!(
            r#"{{"previewId":"devsetup-{}","confirmed":true,"acceptPackageAgreements":true,"acknowledgeAdminAndReboot":true,"path":"C:\\secret"}}"#,
            "a".repeat(64)
        ))
        .is_err());
    }

    #[test]
    fn preview_tokens_have_an_exact_random_shape() {
        let first = random_id("devsetup").unwrap();
        let second = random_id("devsetup").unwrap();
        assert!(validate_preview_id(&first).is_ok());
        assert_ne!(first, second);
        assert!(validate_preview_id("devsetup-not-hex").is_err());
    }

    #[test]
    fn preview_discard_and_new_session_clear_native_tokens() {
        let state = DevSetupConfigurationState::default();
        let preview = || StoredPreview {
            expires_at_ms: now_ms() + PREVIEW_TTL_MS,
            configuration_digest: "a".repeat(64),
            export_content: "safe".into(),
            packages: vec![StoredPackagePlan {
                requirement: requirement(PackageDesiredState::Present),
                observation: PackageObservation::Present,
                action: PackageAction::None,
            }],
            can_apply: false,
            has_changes: false,
        };
        let first = format!("devsetup-{}", "1".repeat(64));
        let second = format!("devsetup-{}", "2".repeat(64));
        store_preview(&state, first.clone(), preview()).unwrap();
        store_preview(&state, second.clone(), preview()).unwrap();

        discard_preview(&state, &first).unwrap();
        assert!(!state.previews.lock().unwrap().contains_key(&first));
        assert!(state.previews.lock().unwrap().contains_key(&second));

        clear_previews(&state).unwrap();
        assert!(state.previews.lock().unwrap().is_empty());
    }
}
