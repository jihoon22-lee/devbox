//! Webhook Lab command — 서버 시작/중지, history, rule 관리.

use crate::core::history::History;
use crate::core::rules::{matches, ResponseRule};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tiny_http::{Header, Request, Response as HttpResponse, Server};

pub const DEFAULT_BIND: &str = "127.0.0.1";

pub struct ServerState {
    pub running: Mutex<Option<Arc<AtomicBool>>>,
    pub history: Mutex<History>,
    pub rules: Mutex<HashMap<String, ResponseRule>>,
    pub address: Mutex<Option<String>>,
}

pub fn server_state() -> Arc<ServerState> {
    Arc::new(ServerState {
        running: Mutex::new(None),
        history: Mutex::new(History::default()),
        rules: Mutex::new(HashMap::new()),
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
) -> Result<ServerStatus, String> {
    if state.running.lock().unwrap().is_some() {
        return Ok(server_status(state));
    }
    let bind = bind.unwrap_or_else(|| DEFAULT_BIND.to_string());
    if bind != "127.0.0.1" && bind != "localhost" && bind != "::1" {
        // LAN 공개 — 명시적 경고는 프론트에서 별도 표시
        eprintln!("webhook-lab: LAN bind {bind}");
    }
    let address = format!("{bind}:{port}");
    let server = Server::http(&address).map_err(|e| format!("바인드 실패: {e}"))?;
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
    let mut body = String::new();
    if let Ok(read) = request.as_reader().read_to_string(&mut body) {
        let _ = read;
    }
    let received_at = now_ms();

    // history 기록 (마스킹 적용)
    state.history.lock().unwrap().push(
        method.clone(),
        url.clone(),
        headers.clone(),
        body.clone(),
        received_at,
    );

    // rule 매치 → 응답
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
    h.records.iter().rev().cloned().collect()
}

#[tauri::command]
pub fn clear_history(state: tauri::State<'_, Arc<ServerState>>) -> Result<(), String> {
    *state.history.lock().unwrap() = History::default();
    Ok(())
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
) -> Result<(), String> {
    let id = if rule.id.is_empty() {
        uuid::Uuid::new_v4().to_string()
    } else {
        rule.id.clone()
    };
    state.rules.lock().unwrap().insert(id, rule);
    Ok(())
}

#[tauri::command]
pub fn delete_rule(state: tauri::State<'_, Arc<ServerState>>, id: String) -> Result<(), String> {
    state.rules.lock().unwrap().remove(&id);
    Ok(())
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
