use crate::core::asset::{
    select_asset, validate_artifact_coordinates, validate_manifest_artifacts,
    validate_version_component,
};
use crate::core::catalog::CatalogApp;
use crate::core::download::{is_over_limit, partial_path, validate_digest, validate_size};
use crate::core::managed_install::{
    remove_portable_install, resolve_portable_install, ManagedPortableInstall,
};
use crate::core::manifest::{parse_manifest, ReleaseManifest};
use crate::core::url_policy::is_allowed;
use futures_util::StreamExt;
use reqwest::header::USER_AGENT;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::PathBuf;
use tauri::Manager;
use tauri_plugin_opener::OpenerExt;

const REPO: &str = "https://api.github.com/repos/jihoon22-lee/devbox";
const DOWNLOAD_ROOT: &str = "https://github.com/jihoon22-lee/devbox/releases/download";

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

pub(crate) fn data_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_local_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn registry_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(data_dir(app)?.join("registry.json"))
}

fn read_registry(app: &tauri::AppHandle) -> Vec<InstalledApp> {
    let path = registry_path(app).ok();
    let Some(path) = path else { return Vec::new() };
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<Vec<InstalledApp>>(&s).ok())
        .unwrap_or_default()
}

fn write_registry(app: &tauri::AppHandle, reg: &[InstalledApp]) -> Result<(), String> {
    let path = registry_path(app)?;
    let json = serde_json::to_string_pretty(reg).map_err(|e| e.to_string())?;
    devbox_filesystem::atomic_write(path, json.as_bytes()).map_err(|e| e.to_string())
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
    let Ok(base) = data_dir(app) else { return };
    let apps_root = base.join("apps");
    let Ok(entries) = std::fs::read_dir(&apps_root) else {
        return;
    };
    for entry in entries.flatten() {
        let app_dir = entry.path();
        if !app_dir.is_dir() {
            continue;
        }
        walk_remove_partials(&app_dir);
    }
}

fn walk_remove_partials(dir: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_remove_partials(&path);
        } else if path
            .file_name()
            .map(|n| n.to_string_lossy().ends_with(".partial"))
            .unwrap_or(false)
        {
            let _ = std::fs::remove_file(&path);
        }
    }
}

/// 번들에 포함된 카탈로그를 반환한다.
#[tauri::command]
pub fn catalog() -> Result<Vec<CatalogApp>, String> {
    let runtime =
        devbox_launch::runtime_catalog_path().and_then(|path| std::fs::read_to_string(path).ok());
    let selected = devbox_catalog::select_catalog(CATALOG_JSON, runtime.as_deref())
        .map_err(|error| error.to_string())?;
    Ok(selected.catalog.apps)
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
    let installed = read_registry(app)
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
        data_dir(app).map_err(|_| "Manager 데이터 위치를 확인할 수 없습니다.".to_string())?;
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
        .map_err(|e| format!("릴리스 조회 실패: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("GitHub 응답 오류: {}", resp.status()));
    }
    let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
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
        .map_err(|e| format!("manifest 다운로드 실패: {e}"))?;
    if !mresp.status().is_success() {
        return Err(format!("manifest 응답 오류: {}", mresp.status()));
    }
    let text = mresp.text().await.map_err(|e| e.to_string())?;
    let manifest = parse_manifest(&text)?;
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
    Ok(read_registry(&app)
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
    let app_manifest = manifest
        .apps
        .iter()
        .find(|a| a.id == app_id)
        .ok_or_else(|| format!("manifest에 앱이 없다: {app_id}"))?;
    let asset = select_asset(&manifest, &app_id, &mode)?;
    let version = app_manifest.version.clone();
    validate_artifact_coordinates(&manifest.release_tag, &version, &asset.name)
        .map_err(|_| "manifest의 다운로드 경로 정보가 올바르지 않습니다.".to_string())?;
    let url = format!("{DOWNLOAD_ROOT}/{}/{}", manifest.release_tag, asset.name);
    if !is_allowed(&url) {
        return Err("허용되지 않은 다운로드 URL".into());
    }

    let base = data_dir(&app)?;
    let client = reqwest::Client::new();

    if mode == "installer" {
        let setup_dir = base.join("installers");
        std::fs::create_dir_all(&setup_dir).map_err(|e| e.to_string())?;
        let dest = setup_dir.join(&asset.name);
        // 검증이 끝난 뒤에만 installer를 실행한다.
        download(&client, &url, &dest, asset.size, &asset.sha256).await?;
        std::process::Command::new(&dest)
            .spawn()
            .map_err(|e| e.to_string())?;
        upsert_registry(&app, app_id, version, "installer", String::new())?;
        return Ok("설치 프로그램을 실행했습니다. 화면 안내에 따라 설치하세요.".into());
    }

    // portable
    let version_root = crate::core::layout::version_dir(&base, &app_id, &version);
    std::fs::create_dir_all(&version_root).map_err(|e| e.to_string())?;
    let exe = crate::core::layout::version_exe(&base, &app_id, &version);
    download(&client, &url, &exe, asset.size, &asset.sha256).await?;

    // current.json 갱신 (직전 정상 버전을 previous로 보존)
    let prev = read_current(&base, &app_id);
    let current = crate::core::layout::Current {
        version: version.clone(),
        exe_path: exe.to_string_lossy().into_owned(),
        installed_at: now_ms(),
        previous_version: prev.map(|p| p.version),
    };
    write_current(&base, &app_id, &current)?;
    // 이전 버전 디렉터리는 삭제하지 않는다 (rollback 보존)

    upsert_registry(&app, app_id, version, "portable", current.exe_path.clone())?;
    Ok("휴대용 앱을 설치했습니다.".into())
}

/// 설치된 휴대용 앱의 current.json을 반환한다 (없으면 None).
#[tauri::command]
pub fn current(app: tauri::AppHandle, app_id: String) -> Result<Option<CurrentView>, String> {
    let installed = portable_registry_entry(&app, &app_id)?;
    let base =
        data_dir(&app).map_err(|_| "Manager 데이터 위치를 확인할 수 없습니다.".to_string())?;
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
    let base =
        data_dir(&app).map_err(|_| "Manager 데이터 위치를 확인할 수 없습니다.".to_string())?;
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
    if let Some(inst) = read_registry(&app).into_iter().find(|a| a.app == app_id) {
        let mut reg = read_registry(&app);
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
    let json = serde_json::to_string_pretty(current).map_err(|e| e.to_string())?;
    devbox_filesystem::atomic_write(path, json.as_bytes()).map_err(|e| e.to_string())
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

/// 기본 Manager root 안의 portable app tree만 제거한다. 대상 앱의 별도
/// app-local user data는 이 tree 밖에 있으므로 보존된다. Installer uninstall과
/// custom root removal은 별도 기능이 소유한다.
#[tauri::command]
pub fn remove_portable_app(app: tauri::AppHandle, app_id: String) -> Result<String, String> {
    let install = managed_portable(&app, &app_id)?;
    let original_registry = read_registry(&app);
    let mut next_registry = original_registry.clone();
    next_registry.retain(|entry| entry.app != app_id);
    if next_registry.len() == original_registry.len() {
        return Err("설치 상태에서 제거할 앱을 찾을 수 없습니다.".to_string());
    }

    write_registry(&app, &next_registry)
        .map_err(|_| "제거 전 설치 상태를 갱신할 수 없습니다.".to_string())?;
    if let Err(_error) = remove_portable_install(&install) {
        let _ = write_registry(&app, &original_registry);
        let _ = sync_runtime_metadata(&app);
        return Err("휴대용 앱 파일을 안전하게 제거할 수 없습니다.".to_string());
    }
    if let Err(error) = sync_runtime_metadata(&app) {
        eprintln!("devbox: runtime metadata sync will retry next launch: {error}");
    }
    Ok("휴대용 앱을 제거했습니다. 앱 사용자 데이터는 유지됩니다.".to_string())
}

fn upsert_registry(
    app: &tauri::AppHandle,
    name: String,
    version: String,
    mode: &str,
    exe_path: String,
) -> Result<(), String> {
    let mut reg = read_registry(app);
    reg.retain(|a| a.app != name);
    reg.push(InstalledApp {
        app: name,
        version,
        mode: mode.to_string(),
        exe_path,
    });
    write_registry(app, &reg)?;
    if let Err(error) = sync_runtime_metadata(app) {
        eprintln!("devbox: runtime metadata sync will retry next launch: {error}");
    }
    Ok(())
}

async fn download(
    client: &reqwest::Client,
    url: &str,
    dest: &std::path::Path,
    expected_size: i64,
    expected_sha: &str,
) -> Result<(), String> {
    // 1. 요청 전 URL 검증
    if !is_allowed(url) {
        return Err("허용되지 않은 다운로드 URL".into());
    }
    let resp = client
        .get(url)
        .header(USER_AGENT, "devbox-manager")
        .send()
        .await
        .map_err(|e| format!("다운로드 실패: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("다운로드 응답 오류: {}", resp.status()));
    }
    // 2. redirect 후 최종 URL을 다시 검증한다 (중간 hop이 아니라 최종 응답의 URL)
    if !is_allowed(resp.url().as_str()) {
        return Err("redirect 후 URL이 허용 범위를 벗어났다".into());
    }
    // 3. Content-Length가 manifest size와 다르면 즉시 중단
    if let Some(cl) = resp.content_length() {
        if cl != expected_size as u64 {
            return Err(format!(
                "Content-Length 불일치: 기대 {expected_size}바이트, 서버 {cl}바이트"
            ));
        }
    }

    // 4. .partial로 streaming 기록. 청크마다 SHA-256 갱신, 누적 크기 상한 검사
    let partial = partial_path(dest);
    let mut file = std::fs::File::create(&partial).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut total: u64 = 0;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("스트림 오류: {e}"))?;
        total += chunk.len() as u64;
        if is_over_limit(total as i64, expected_size) {
            drop(file);
            let _ = std::fs::remove_file(&partial);
            return Err(format!(
                "크기 초과: 기대 {expected_size}바이트를 넘었다 ({total}바이트)"
            ));
        }
        hasher.update(&chunk);
        file.write_all(&chunk).map_err(|e| e.to_string())?;
    }
    file.flush().map_err(|e| e.to_string())?;
    drop(file);

    // 5. 완료 후 총 바이트와 digest를 manifest와 대조
    if let Err(e) = validate_size(expected_size, total as i64) {
        let _ = std::fs::remove_file(&partial);
        return Err(e);
    }
    let digest = format!("{:x}", hasher.finalize());
    if let Err(e) = validate_digest(expected_sha, &digest) {
        let _ = std::fs::remove_file(&partial);
        return Err(e);
    }

    // 6. 일치하면 최종 경로로 rename
    std::fs::rename(&partial, dest).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
