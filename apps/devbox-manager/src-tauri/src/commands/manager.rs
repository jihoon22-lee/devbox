use crate::core::asset::select_asset;
use crate::core::catalog::CatalogApp;
use crate::core::manifest::{parse_manifest, ReleaseManifest};
use crate::core::url_policy::is_allowed;
use reqwest::header::USER_AGENT;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::Manager;

const REPO: &str = "https://api.github.com/repos/jihoon22-lee/devbox";
const DOWNLOAD_ROOT: &str = "https://github.com/jihoon22-lee/devbox/releases/download";

/// 빌드 시 임베드된 카탈로그. Manager 자신의 버전이 아는 앱 목록이 명확해지고
/// 오프라인에서도 목록이 보인다. 새 앱은 Manager 업데이트로 반영된다.
const CATALOG_JSON: &str = include_str!("../../../../catalog.json");

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InstalledApp {
    pub app: String,
    pub version: String,
    /// "portable" | "installer"
    pub mode: String,
    pub exe_path: String,
}

fn data_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
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
    std::fs::write(path, json).map_err(|e| e.to_string())
}

/// 번들에 포함된 카탈로그를 반환한다.
#[tauri::command]
pub fn catalog() -> Result<Vec<CatalogApp>, String> {
    let catalog = crate::core::catalog::parse_catalog(CATALOG_JSON)?;
    Ok(catalog.apps)
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
    parse_manifest(&text)
}

/// 설치된 앱 목록 (registry.json).
#[tauri::command]
pub fn installed(app: tauri::AppHandle) -> Vec<InstalledApp> {
    read_registry(&app)
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
    let manifest = available().await?;
    let app_manifest = manifest
        .apps
        .iter()
        .find(|a| a.id == app_id)
        .ok_or_else(|| format!("manifest에 앱이 없다: {app_id}"))?;
    let asset = select_asset(&manifest, &app_id, &mode)?;
    let version = app_manifest.version.clone();
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
        download(&client, &url, &dest).await?;
        // 설치 마법사 실행 (사용자가 진행)
        std::process::Command::new(&dest)
            .spawn()
            .map_err(|e| e.to_string())?;
        upsert_registry(&app, app_id, version, "installer", String::new())?;
        return Ok("설치 프로그램을 실행했습니다. 화면 안내에 따라 설치하세요.".into());
    }

    // portable
    let app_dir = base.join("apps").join(&app_id).join(&version);
    std::fs::create_dir_all(&app_dir).map_err(|e| e.to_string())?;
    let exe = app_dir.join(format!("{app_id}.exe"));
    download(&client, &url, &exe).await?;
    let exe_path = exe.to_string_lossy().into_owned();
    upsert_registry(&app, app_id, version, "portable", exe_path)?;
    Ok("휴대용 앱을 설치했습니다.".into())
}

/// 설치된 앱 실행 (휴대용만).
#[tauri::command]
pub fn launch(app: tauri::AppHandle, name: String) -> Result<(), String> {
    let reg = read_registry(&app);
    let Some(found) = reg.iter().find(|a| a.app == name) else {
        return Err("설치된 앱이 없습니다. 먼저 설치하세요.".into());
    };
    if found.mode != "portable" {
        return Err("설치 패키지 방식으로 설치된 앱은 시작 메뉴에서 실행하세요.".into());
    }
    if found.exe_path.is_empty() || !std::path::Path::new(&found.exe_path).exists() {
        return Err(format!("실행 파일을 찾을 수 없습니다: {}", found.exe_path));
    }
    std::process::Command::new(&found.exe_path)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
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
    write_registry(app, &reg)
}

async fn download(
    client: &reqwest::Client,
    url: &str,
    dest: &std::path::Path,
) -> Result<(), String> {
    let resp = client
        .get(url)
        .header(USER_AGENT, "devbox-manager")
        .send()
        .await
        .map_err(|e| format!("다운로드 실패: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("다운로드 응답 오류: {}", resp.status()));
    }
    let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
    std::fs::write(dest, bytes).map_err(|e| e.to_string())?;
    Ok(())
}
