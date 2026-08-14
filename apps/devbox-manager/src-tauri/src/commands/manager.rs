use crate::core::asset::select_asset;
use crate::core::catalog::CatalogApp;
use crate::core::download::{is_over_limit, partial_path, validate_digest, validate_size};
use crate::core::manifest::{parse_manifest, ReleaseManifest};
use crate::core::url_policy::is_allowed;
use futures_util::StreamExt;
use reqwest::header::USER_AGENT;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Write;
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
    // 임시 파일 + rename으로 원자 기록 (중간에 깨진 registry 방지)
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
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

    upsert_registry(
        &app,
        app_id,
        version,
        "portable",
        current.exe_path.clone(),
    )?;
    Ok("휴대용 앱을 설치했습니다.".into())
}

/// 설치된 휴대용 앱의 current.json을 반환한다 (없으면 None).
#[tauri::command]
pub fn current(app: tauri::AppHandle, app_id: String) -> Option<crate::core::layout::Current> {
    let base = data_dir(&app).ok()?;
    read_current(&base, &app_id)
}

/// 직전 정상 버전으로 되돌린다 (portable만).
#[tauri::command]
pub fn rollback(
    app: tauri::AppHandle,
    app_id: String,
) -> Result<String, String> {
    let base = data_dir(&app)?;
    let current = read_current(&base, &app_id)
        .ok_or_else(|| "current.json이 없다 (portable 설치 필요)".to_string())?;
    let installed = installed_versions_with_exe(&base, &app_id);
    let target = crate::core::layout::pick_rollback_target(&current, &installed)
        .ok_or_else(|| "되돌릴 이전 버전이 없다".to_string())?;
    let target_exe = crate::core::layout::version_exe(&base, &app_id, &target);

    let next = crate::core::layout::Current {
        version: target.clone(),
        exe_path: target_exe.to_string_lossy().into_owned(),
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
    Ok(format!("{app_id}를 버전 {}으로 되돌렸습니다.", next.version))
}

/// versions/ 아래에서 실행 파일이 실제 존재하는 버전 목록.
fn installed_versions_with_exe(base: &std::path::Path, app_id: &str) -> Vec<String> {
    let mut versions: Vec<String> = std::fs::read_dir(crate::core::layout::versions_root(base, app_id))
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

fn read_current(
    base: &std::path::Path,
    app_id: &str,
) -> Option<crate::core::layout::Current> {
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
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
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
