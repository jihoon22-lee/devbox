use reqwest::header::USER_AGENT;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::Manager;

const REPO: &str = "https://api.github.com/repos/jihoon22-lee/devbox";

#[derive(Debug, Clone, Serialize)]
pub struct Asset {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LatestRelease {
    pub tag: String,
    pub published_at: String,
    pub assets: Vec<Asset>,
}

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

/// 최신 릴리스 정보 (태그 + 에셋 목록) 조회.
#[tauri::command]
pub async fn latest() -> Result<LatestRelease, String> {
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
    let tag = json["tag_name"].as_str().unwrap_or("").to_string();
    let published_at = json["published_at"].as_str().unwrap_or("").to_string();
    let assets = json["assets"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|a| Asset {
                    name: a["name"].as_str().unwrap_or("").to_string(),
                    url: a["browser_download_url"].as_str().unwrap_or("").to_string(),
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(LatestRelease {
        tag,
        published_at,
        assets,
    })
}

/// 설치된 앱 목록 (registry.json).
#[tauri::command]
pub fn installed(app: tauri::AppHandle) -> Vec<InstalledApp> {
    read_registry(&app)
}

/// 앱 설치/업데이트.
/// - mode=portable: 휴대용 exe를 다운로드해 자체 폴더에 보관
/// - mode=installer: NSIS 설치 패키지를 내려받아 실행 (설치 마법사)
#[tauri::command]
pub async fn install(
    app: tauri::AppHandle,
    name: String,
    version: String,
    url: String,
    mode: String,
) -> Result<String, String> {
    let base = data_dir(&app)?;
    let client = reqwest::Client::new();

    if mode == "installer" {
        let setup_dir = base.join("installers");
        std::fs::create_dir_all(&setup_dir).map_err(|e| e.to_string())?;
        let file_name = url
            .rsplit('/')
            .next()
            .filter(|s| !s.is_empty())
            .unwrap_or("setup.exe");
        let dest = setup_dir.join(file_name);
        download(&client, &url, &dest).await?;
        // 설치 마법사 실행 (사용자가 진행)
        std::process::Command::new(&dest)
            .spawn()
            .map_err(|e| e.to_string())?;
        upsert_registry(&app, name, version, "installer", String::new())?;
        return Ok("설치 프로그램을 실행했습니다. 화면 안내에 따라 설치하세요.".into());
    }

    // portable
    let app_dir = base.join("apps").join(&name).join(&version);
    std::fs::create_dir_all(&app_dir).map_err(|e| e.to_string())?;
    let exe = app_dir.join(format!("{name}.exe"));
    download(&client, &url, &exe).await?;
    let exe_path = exe.to_string_lossy().into_owned();
    upsert_registry(&app, name, version, "portable", exe_path)?;
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
