//! Webhook Lab command — 서버 시작/중지, history, rule 관리.

use crate::core::fixtures::{
    fixture_from_request, fixture_path_from_dir, load_document_with_raw, response_rule_draft,
    save_document_if_current, sorted_fixtures, CapturedFixture, FixtureDocument, FixtureError,
    MAX_FIXTURES,
};
use crate::core::history::{History, MAX_BODY_BYTES};
use crate::core::rules::{matches, upsert, ResponseRule, INVALID_RULE_ERROR};
use serde::Serialize;
use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager};
use tiny_http::{Header, Request, Response as HttpResponse, Server};

pub const DEFAULT_BIND: &str = "127.0.0.1";
pub const LAN_BIND_CONFIRMATION_ERROR: &str = "LAN 공개를 시작하려면 명시적인 확인이 필요합니다";
pub const INVALID_BIND_ERROR: &str = "허용되지 않은 bind 주소입니다";
pub const INVALID_PORT_ERROR: &str = "포트는 1~65535 범위여야 합니다";
pub const RATE_LIMIT_ERROR: &str = "요청이 너무 많습니다. 잠시 후 다시 시도하세요";
pub const BIND_ERROR: &str = "서버 bind에 실패했습니다";

pub struct ServerState {
    pub running: Mutex<Option<Arc<AtomicBool>>>,
    pub history: Mutex<History>,
    pub rules: Mutex<HashMap<String, ResponseRule>>,
    /// Serializes load/validate/mutate/write so two fixture commands cannot
    /// overwrite one another between their compare-and-swap checks.
    pub fixture_lock: Mutex<()>,
    pub address: Mutex<Option<String>>,
}

pub fn server_state() -> Arc<ServerState> {
    Arc::new(ServerState {
        running: Mutex::new(None),
        history: Mutex::new(History::default()),
        rules: Mutex::new(HashMap::new()),
        fixture_lock: Mutex::new(()),
        address: Mutex::new(None),
    })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerStatus {
    pub running: bool,
    pub address: Option<String>,
}

#[tauri::command]
pub fn server_status(state: tauri::State<'_, Arc<ServerState>>) -> ServerStatus {
    ServerStatus {
        running: state.running.lock().unwrap().is_some(),
        address: state.address.lock().unwrap().clone(),
    }
}

/// 서버를 시작한다. bind 기본값 127.0.0.1 (LAN 공개는 명시적 설정).
#[tauri::command]
pub fn start_server(
    state: tauri::State<'_, Arc<ServerState>>,
    bind: Option<String>,
    port: u16,
    allow_lan: Option<bool>,
) -> Result<ServerStatus, String> {
    if state.running.lock().unwrap().is_some() {
        return Ok(server_status(state));
    }
    let bind = bind.unwrap_or_else(|| DEFAULT_BIND.to_string());
    if port == 0 {
        return Err(INVALID_PORT_ERROR.to_string());
    }
    let lan_bind = matches!(bind.as_str(), "0.0.0.0" | "[::]");
    let loopback_bind = matches!(bind.as_str(), "127.0.0.1" | "localhost" | "::1");
    if !lan_bind && !loopback_bind {
        return Err(INVALID_BIND_ERROR.to_string());
    }
    if lan_bind && allow_lan != Some(true) {
        return Err(LAN_BIND_CONFIRMATION_ERROR.to_string());
    }
    let address = format!("{bind}:{port}");
    let server = Server::http(&address).map_err(|_| BIND_ERROR.to_string())?;
    let running = Arc::new(AtomicBool::new(true));
    *state.running.lock().unwrap() = Some(Arc::clone(&running));

    let state_arc = Arc::clone(&state);
    std::thread::spawn(move || loop {
        if !running.load(Ordering::Relaxed) {
            server.unblock();
            break;
        }
        if let Ok(Some(request)) = server.recv_timeout(std::time::Duration::from_millis(50)) {
            handle_request(&state_arc, request);
        }
    });

    *state.address.lock().unwrap() = Some(address);
    Ok(server_status(state))
}

#[tauri::command]
pub fn stop_server(state: tauri::State<'_, Arc<ServerState>>) -> Result<ServerStatus, String> {
    if let Some(running) = state.running.lock().unwrap().take() {
        running.store(false, Ordering::Relaxed);
    }
    *state.address.lock().unwrap() = None;
    Ok(server_status(state))
}

fn handle_request(state: &Arc<ServerState>, mut request: Request) {
    let method = request.method().to_string();
    let url = request.url().to_string();
    let headers: Vec<(String, String)> = request
        .headers()
        .iter()
        .map(|h| (h.field.to_string(), h.value.to_string()))
        .collect();
    let received_at = now_ms();
    if !state.history.lock().unwrap().allow_request(received_at) {
        let response = HttpResponse::from_string(RATE_LIMIT_ERROR).with_status_code(429);
        let _ = request.respond(response);
        return;
    }
    // Read at most one byte beyond the UTF-8/body budget. The previous
    // unbounded read could allocate before History had a chance to truncate
    // a hostile request.
    let mut body_bytes = Vec::new();
    let _ = request
        .as_reader()
        .take((MAX_BODY_BYTES + 1) as u64)
        .read_to_end(&mut body_bytes);
    if body_bytes.len() > MAX_BODY_BYTES {
        body_bytes.truncate(MAX_BODY_BYTES);
    }
    let body = String::from_utf8_lossy(&body_bytes).into_owned();

    // history 기록 (마스킹 적용)
    state.history.lock().unwrap().push(
        method.clone(),
        url.clone(),
        headers.clone(),
        body.clone(),
        received_at,
    );

    // rule 매치 → 응답
    // HashMap iteration order is unspecified. Matching order is not a
    // priority or determinism contract for response rules.
    let rule = state
        .rules
        .lock()
        .unwrap()
        .values()
        .find(|r| matches(r, &method, &url))
        .cloned();

    let (status, response_headers, response_body, delay_ms) = match rule {
        Some(r) => (r.status, r.headers, r.body, r.delay_ms),
        None => (404, vec![], "Not Found".to_string(), 0),
    };
    if delay_ms > 0 {
        std::thread::sleep(std::time::Duration::from_millis(delay_ms));
    }

    let mut builder = HttpResponse::from_data(response_body.into_bytes()).with_status_code(status);
    for (k, v) in response_headers {
        if let Ok(h) = Header::from_bytes(k.as_bytes(), v.as_bytes()) {
            builder = builder.with_header(h);
        }
    }
    let _ = request.respond(builder);
}

#[tauri::command]
pub fn list_history(
    state: tauri::State<'_, Arc<ServerState>>,
) -> Vec<crate::core::history::RequestRecord> {
    let h = state.history.lock().unwrap();
    h.list_masked()
}

#[tauri::command]
pub fn clear_history(state: tauri::State<'_, Arc<ServerState>>) -> Result<(), String> {
    state.history.lock().unwrap().clear();
    Ok(())
}

#[tauri::command]
pub fn copy_masked_history(
    state: tauri::State<'_, Arc<ServerState>>,
    id: u64,
) -> Result<String, String> {
    state
        .history
        .lock()
        .unwrap()
        .masked_copy(id)
        .ok_or_else(history_not_found)
}

/// 사용자가 확인한 일회성 원본 복사에서만 호출한다. 반환값을 저장하거나 로그에 남기지 않는다.
#[tauri::command]
pub fn copy_raw_history(
    state: tauri::State<'_, Arc<ServerState>>,
    id: u64,
) -> Result<String, String> {
    state
        .history
        .lock()
        .unwrap()
        .raw_copy(id)
        .ok_or_else(history_not_found)
}

#[tauri::command]
pub fn copy_history_headers(
    state: tauri::State<'_, Arc<ServerState>>,
    id: u64,
) -> Result<String, String> {
    state
        .history
        .lock()
        .unwrap()
        .masked_headers_copy(id)
        .ok_or_else(history_not_found)
}

#[tauri::command]
pub fn delete_history(state: tauri::State<'_, Arc<ServerState>>, id: u64) -> Result<(), String> {
    if state.history.lock().unwrap().remove(id) {
        Ok(())
    } else {
        Err(history_not_found())
    }
}

fn fixture_path(app: &AppHandle) -> Result<PathBuf, String> {
    let directory = app
        .path()
        .app_local_data_dir()
        .map_err(|_| crate::core::fixtures::FIXTURE_WRITE_ERROR.to_string())?;
    Ok(fixture_path_from_dir(&directory))
}

fn fixture_error(error: FixtureError) -> String {
    error.message().to_string()
}

/// List only validated, masked fixtures from the app-owned store. A missing
/// file is an empty collection; corrupt, oversized, or link-backed files are
/// fixed-error failures and are never repaired implicitly.
#[tauri::command]
pub fn list_fixtures(
    app: AppHandle,
    state: tauri::State<'_, Arc<ServerState>>,
) -> Result<Vec<CapturedFixture>, String> {
    let _guard = state
        .fixture_lock
        .lock()
        .map_err(|_| crate::core::fixtures::FIXTURE_READ_ERROR.to_string())?;
    let document = load_document_with_raw(&fixture_path(&app)?).map_err(fixture_error)?;
    Ok(sorted_fixtures(&document.document))
}

/// Persist one masked history entry. The request body and headers are read
/// from the in-memory history by opaque ID, never from frontend-supplied JSON.
#[tauri::command]
pub fn save_fixture(
    app: AppHandle,
    state: tauri::State<'_, Arc<ServerState>>,
    history_id: u64,
) -> Result<CapturedFixture, String> {
    let request = state
        .history
        .lock()
        .map_err(|_| FixtureError::NotFound.message().to_string())?
        .masked_record(history_id)
        .ok_or_else(|| FixtureError::NotFound.message().to_string())?;

    let _guard = state
        .fixture_lock
        .lock()
        .map_err(|_| crate::core::fixtures::FIXTURE_WRITE_ERROR.to_string())?;
    let path = fixture_path(&app)?;
    let loaded = load_document_with_raw(&path).map_err(fixture_error)?;
    if loaded.document.fixtures.len() >= MAX_FIXTURES {
        return Err(crate::core::fixtures::FIXTURE_SIZE_ERROR.to_string());
    }
    let next_id = loaded.document.next_id;
    let fixture_id = format!("fixture-{next_id}");
    let fixture = fixture_from_request(fixture_id, &request).map_err(fixture_error)?;
    let mut document = loaded.document;
    document.next_id = next_id
        .checked_add(1)
        .ok_or_else(|| crate::core::fixtures::FIXTURE_SIZE_ERROR.to_string())?;
    document.fixtures.push(fixture.clone());
    save_document_if_current(&path, loaded.raw.as_deref(), &document).map_err(fixture_error)?;
    Ok(fixture)
}

#[tauri::command]
pub fn delete_fixture(
    app: AppHandle,
    state: tauri::State<'_, Arc<ServerState>>,
    id: String,
) -> Result<(), String> {
    let _guard = state
        .fixture_lock
        .lock()
        .map_err(|_| crate::core::fixtures::FIXTURE_WRITE_ERROR.to_string())?;
    let path = fixture_path(&app)?;
    let loaded = load_document_with_raw(&path).map_err(fixture_error)?;
    let mut document = loaded.document;
    let original_len = document.fixtures.len();
    document.fixtures.retain(|fixture| fixture.id != id);
    if document.fixtures.len() == original_len {
        return Err(FixtureError::NotFound.message().to_string());
    }
    save_document_if_current(&path, loaded.raw.as_deref(), &document).map_err(fixture_error)
}

#[tauri::command]
pub fn clear_fixtures(
    app: AppHandle,
    state: tauri::State<'_, Arc<ServerState>>,
) -> Result<(), String> {
    let _guard = state
        .fixture_lock
        .lock()
        .map_err(|_| crate::core::fixtures::FIXTURE_WRITE_ERROR.to_string())?;
    let path = fixture_path(&app)?;
    let loaded = load_document_with_raw(&path).map_err(fixture_error)?;
    if loaded.document.fixtures.is_empty() {
        return Ok(());
    }
    let mut document = FixtureDocument::default();
    document.next_id = loaded.document.next_id;
    save_document_if_current(&path, loaded.raw.as_deref(), &document).map_err(fixture_error)
}

/// Return a validated response-rule draft for local editing. This command is
/// intentionally not an API Playground handoff and never writes a rule.
#[tauri::command]
pub fn fixture_to_rule(
    app: AppHandle,
    state: tauri::State<'_, Arc<ServerState>>,
    id: String,
) -> Result<ResponseRule, String> {
    let _guard = state
        .fixture_lock
        .lock()
        .map_err(|_| crate::core::fixtures::FIXTURE_READ_ERROR.to_string())?;
    let document = load_document_with_raw(&fixture_path(&app)?).map_err(fixture_error)?;
    let fixture = document
        .document
        .fixtures
        .iter()
        .find(|fixture| fixture.id == id)
        .ok_or_else(|| FixtureError::NotFound.message().to_string())?;
    response_rule_draft(fixture).map_err(fixture_error)
}

#[tauri::command]
pub fn list_rules(state: tauri::State<'_, Arc<ServerState>>) -> Vec<ResponseRule> {
    let mut rules: Vec<ResponseRule> = state.rules.lock().unwrap().values().cloned().collect();
    rules.sort_by(|a, b| a.id.cmp(&b.id));
    rules
}

#[tauri::command]
pub fn set_rule(
    state: tauri::State<'_, Arc<ServerState>>,
    rule: ResponseRule,
) -> Result<String, String> {
    upsert(&mut state.rules.lock().unwrap(), rule).map_err(|_| INVALID_RULE_ERROR.to_string())
}

#[tauri::command]
pub fn delete_rule(state: tauri::State<'_, Arc<ServerState>>, id: String) -> Result<(), String> {
    if state.rules.lock().unwrap().remove(&id).is_some() {
        Ok(())
    } else {
        Err("규칙을 찾을 수 없습니다".to_string())
    }
}

fn history_not_found() -> String {
    "요청 기록을 찾을 수 없습니다".to_string()
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
