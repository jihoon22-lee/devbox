//! Webhook Lab command — 서버 시작/중지, history, rule 관리.

use crate::core::fixtures::{
    fixture_from_request, fixture_path_from_dir, load_document_with_raw, response_rule_draft,
    sorted_fixtures, update_document, CapturedFixture, FixtureDocument, FixtureError, MAX_FIXTURES,
};
use crate::core::handoff::{
    build_api_request_payload, API_REQUEST_HANDOFF_KIND, CONSUMER_APP_ID, HANDOFF_INPUT_ERROR,
    PRODUCER_APP_ID,
};
use crate::core::history::History;
use crate::core::http::{self, ParseError, ParsedRequest, MAX_ACTIVE_CONNECTIONS};
use crate::core::replay::{self, ReplayError, ReplayRateLimiter};
use crate::core::rules::{
    compare_rule_precedence, plan_upsert, select_matching_rule, upsert, ResponseRule,
    ResponseSequenceState, RuleConflictPreview, INVALID_RULE_ERROR,
};
use devbox_applink::{handoff_root_in, CreateHandoff, HandoffError, HandoffStore, OpenRequest};
use serde::Serialize;
use serde_json::to_value;
use std::collections::HashMap;
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

pub const DEFAULT_BIND: &str = "127.0.0.1";
pub const LAN_BIND_CONFIRMATION_ERROR: &str = "LAN 공개를 시작하려면 명시적인 확인이 필요합니다";
pub const INVALID_BIND_ERROR: &str = "허용되지 않은 bind 주소입니다";
pub const INVALID_PORT_ERROR: &str = "포트는 1~65535 범위여야 합니다";
pub const RATE_LIMIT_ERROR: &str = "요청이 너무 많습니다. 잠시 후 다시 시도하세요";
pub const BIND_ERROR: &str = "서버 bind에 실패했습니다";
const REQUEST_TOO_LARGE_ERROR: &str = "요청 크기가 허용 범위를 초과했습니다";
const REQUEST_HEADER_ERROR: &str = "요청 헤더가 허용 범위를 초과했습니다";
const REQUEST_TIMEOUT_ERROR: &str = "요청 시간이 초과되었습니다";
const SERVER_INTERNAL_ERROR: &str = "서버 내부 상태를 읽을 수 없습니다";
pub const REPLAY_SERVER_ERROR: &str =
    "localhost 서버가 실행 중이 아니거나 주소가 유효하지 않습니다";
pub const REPLAY_INPUT_ERROR: &str = "replay 입력이 유효하지 않습니다";
pub const REPLAY_RATE_LIMIT_ERROR: &str = "replay 요청이 너무 많습니다. 잠시 후 다시 시도하세요";
pub const REPLAY_NETWORK_ERROR: &str = "replay 요청을 보내지 못했습니다";
pub const REPLAY_RESPONSE_ERROR: &str = "replay 응답을 읽지 못했습니다";
pub const SEQUENCE_RESET_ERROR: &str = "response sequence를 초기화하지 못했습니다";
pub const RULE_CONFLICT_CONFIRMATION_ERROR: &str =
    "겹치는 규칙을 저장하려면 충돌 확인이 필요합니다";
pub const RUN_DEFINITION_EXPORT_ERROR: &str =
    "실행 중인 loopback 서버만 Run Manager 서비스로 내보낼 수 있습니다";
pub const API_TARGET_UNAVAILABLE_ERROR: &str =
    "API Playground를 사용할 수 없습니다. 설치 또는 업데이트 후 다시 시도하세요. 클립보드로 자동 전환하지 않습니다";
pub const API_LAUNCH_ERROR: &str =
    "API Playground를 실행하지 못했습니다. handoff는 잠시 보관되며 다시 시도할 수 있습니다. 클립보드로 자동 전환하지 않습니다";
pub const HANDOFF_CREATE_ERROR: &str =
    "API Playground handoff를 만들지 못했습니다. 클립보드로 자동 전환하지 않습니다";

pub struct ServerState {
    /// Serializes listener lifecycle transitions. Without this guard two IPC
    /// calls could both observe a stopped server and race to bind, or a new
    /// listener could start while the old accept thread still owns its socket.
    pub lifecycle_lock: Mutex<()>,
    pub running: Mutex<Option<Arc<AtomicBool>>>,
    server_thread: Mutex<Option<JoinHandle<()>>>,
    /// Cloned socket handles let stop_server interrupt a worker blocked in a
    /// bounded header/body read or response write. The map contains only
    /// active connections and is capped before any worker is spawned.
    active_connections: Mutex<HashMap<u64, TcpStream>>,
    next_connection_id: AtomicU64,
    pub history: Mutex<History>,
    pub rules: Mutex<HashMap<String, ResponseRule>>,
    /// The current response position is process-local and intentionally not
    /// persisted.  A cursor is advanced only for matched rules with a
    /// sequence; reset commands remove the entry and start at the base reply.
    pub sequence_cursors: Mutex<ResponseSequenceState>,
    pub replay_rate: Mutex<ReplayRateLimiter>,
    /// Serialize native replay sends so concurrent IPC callers cannot reorder
    /// the local scenario in an unbounded burst. The listener itself remains
    /// available for ordinary webhook clients while a replay is in flight.
    pub replay_lock: Mutex<()>,
    /// Stop requests set this before waiting for the lifecycle lock so an
    /// in-flight replay can cancel its bounded socket I/O promptly.
    pub replay_cancel: AtomicBool,
    /// Monotonic listener epoch used to reject replay IPC calls that were
    /// queued across a stop/start transition. A cancellation flag alone can
    /// be reset by the new listener before an old waiter acquires the lock.
    listener_generation: AtomicU64,
    /// Serializes load/validate/mutate/write so two fixture commands cannot
    /// overwrite one another between their compare-and-swap checks.
    pub fixture_lock: Mutex<()>,
    pub address: Mutex<Option<String>>,
}

pub fn server_state() -> Arc<ServerState> {
    Arc::new(ServerState {
        lifecycle_lock: Mutex::new(()),
        running: Mutex::new(None),
        server_thread: Mutex::new(None),
        active_connections: Mutex::new(HashMap::new()),
        next_connection_id: AtomicU64::new(1),
        history: Mutex::new(History::default()),
        rules: Mutex::new(HashMap::new()),
        sequence_cursors: Mutex::new(ResponseSequenceState::default()),
        replay_rate: Mutex::new(ReplayRateLimiter::default()),
        replay_lock: Mutex::new(()),
        replay_cancel: AtomicBool::new(false),
        listener_generation: AtomicU64::new(0),
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiHandoffDispatch {
    pub handoff_id: String,
    pub producer_id: String,
    pub consumer_id: String,
    pub created_at_ms: u64,
    pub expires_at_ms: u64,
}

#[tauri::command]
pub fn server_status(state: tauri::State<'_, Arc<ServerState>>) -> ServerStatus {
    current_server_status(state.inner())
}

fn current_server_status(state: &Arc<ServerState>) -> ServerStatus {
    let running = state
        .running
        .lock()
        .ok()
        .and_then(|guard| guard.as_ref().map(|flag| flag.load(Ordering::Acquire)))
        .unwrap_or(false);
    ServerStatus {
        running,
        address: if running {
            state
                .address
                .lock()
                .ok()
                .and_then(|address| address.clone())
        } else {
            None
        },
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
    start_server_inner(state.inner(), bind, port, allow_lan)
}

pub(crate) fn start_server_inner(
    state: &Arc<ServerState>,
    bind: Option<String>,
    port: u16,
    allow_lan: Option<bool>,
) -> Result<ServerStatus, String> {
    let _lifecycle = state
        .lifecycle_lock
        .lock()
        .map_err(|_| BIND_ERROR.to_string())?;

    let stale_running = {
        let mut running = state.running.lock().map_err(|_| BIND_ERROR.to_string())?;
        match running.as_ref() {
            Some(flag) if flag.load(Ordering::Acquire) => {
                return Ok(current_server_status(state));
            }
            Some(_) => running.take(),
            None => None,
        }
    };
    // An accept-loop failure can leave its join handle behind. Interrupt all
    // old connection workers before joining it; a partial body or blocked
    // response write must not keep a replacement listener from binding.
    // Also cover an old worker set left by an accept-loop failure where the
    // running flag was already cleared before the command observed it.
    shutdown_active_connections(state);
    if let Some(thread) = state
        .server_thread
        .lock()
        .map_err(|_| BIND_ERROR.to_string())?
        .take()
    {
        let _ = thread.join();
    }
    drop(stale_running);
    if let Ok(mut address) = state.address.lock() {
        *address = None;
    }

    let bind = bind.unwrap_or_else(|| DEFAULT_BIND.to_string());
    // Never let the OS resolve a hostname for the listener. `localhost` is a
    // user-friendly alias, but binding it directly can consult mutable
    // hosts/DNS configuration and is inconsistent with loopback-only replay.
    let bind = if bind.eq_ignore_ascii_case("localhost") {
        DEFAULT_BIND.to_string()
    } else {
        bind
    };
    if port == 0 {
        return Err(INVALID_PORT_ERROR.to_string());
    }
    let lan_bind = matches!(bind.as_str(), "0.0.0.0" | "[::]");
    let loopback_bind = matches!(bind.as_str(), "127.0.0.1" | "::1");
    if !lan_bind && !loopback_bind {
        return Err(INVALID_BIND_ERROR.to_string());
    }
    if lan_bind && allow_lan != Some(true) {
        return Err(LAN_BIND_CONFIRMATION_ERROR.to_string());
    }
    let address = if bind == "::1" {
        // IPv6 literals must be bracketed when combined with a port.  Keep
        // the displayed address parseable by the replay client and curl
        // builder while preserving the existing bind choices.
        format!("[{bind}]:{port}")
    } else {
        format!("{bind}:{port}")
    };
    let listener = TcpListener::bind(&address).map_err(|_| BIND_ERROR.to_string())?;
    listener
        .set_nonblocking(true)
        .map_err(|_| BIND_ERROR.to_string())?;
    let running = Arc::new(AtomicBool::new(true));
    let state_arc = Arc::clone(state);
    let listener_running = Arc::clone(&running);
    let thread = thread::Builder::new()
        .name("webhook-lab-listener".to_string())
        .spawn(move || run_listener(listener, state_arc, listener_running))
        .map_err(|_| BIND_ERROR.to_string())?;
    state.replay_cancel.store(false, Ordering::Release);
    *state.running.lock().map_err(|_| BIND_ERROR.to_string())? = Some(running);
    *state
        .server_thread
        .lock()
        .map_err(|_| BIND_ERROR.to_string())? = Some(thread);
    *state.address.lock().map_err(|_| BIND_ERROR.to_string())? = Some(address);
    advance_listener_generation(state);
    Ok(current_server_status(state))
}

#[tauri::command]
pub fn stop_server(state: tauri::State<'_, Arc<ServerState>>) -> Result<ServerStatus, String> {
    // Set cancellation before acquiring lifecycle_lock. A replay holds that
    // lock while connecting/reading; this lets it observe stop immediately
    // instead of making stop wait for its full network budget.
    state.replay_cancel.store(true, Ordering::Release);
    // Invalidate replay calls that are waiting for lifecycle_lock. The new
    // listener resets replay_cancel, so cancellation alone cannot distinguish
    // an old queued call from a call issued after restart.
    advance_listener_generation(state.inner());
    let _lifecycle = state
        .lifecycle_lock
        .lock()
        .map_err(|_| BIND_ERROR.to_string())?;
    if let Some(running) = state
        .running
        .lock()
        .map_err(|_| BIND_ERROR.to_string())?
        .take()
    {
        running.store(false, Ordering::Release);
    }
    // Closing cloned handles wakes workers blocked in header/body reads or a
    // slow response write. The listener thread joins its bounded worker set
    // before this command returns.
    shutdown_active_connections(state.inner());
    if let Some(thread) = state
        .server_thread
        .lock()
        .map_err(|_| BIND_ERROR.to_string())?
        .take()
    {
        // The accept loop checks the flag at most every 50ms. Joining here
        // makes stop/start deterministic and guarantees the old socket is
        // dropped before a later start attempts to reuse its port.
        let _ = thread.join();
    }
    clear_active_connections(state.inner());
    *state.address.lock().map_err(|_| BIND_ERROR.to_string())? = None;
    Ok(current_server_status(state.inner()))
}

fn parse_error_response(error: ParseError) -> Option<(u16, &'static str)> {
    match error {
        ParseError::Closed | ParseError::Cancelled | ParseError::Io => None,
        ParseError::Malformed => Some((400, "잘못된 HTTP 요청입니다")),
        ParseError::RequestLineTooLarge => Some((414, REQUEST_TOO_LARGE_ERROR)),
        ParseError::HeaderTooLarge => Some((431, REQUEST_HEADER_ERROR)),
        ParseError::BodyTooLarge => Some((413, REQUEST_TOO_LARGE_ERROR)),
        ParseError::Timeout => Some((408, REQUEST_TIMEOUT_ERROR)),
        ParseError::Unsupported => Some((501, "지원하지 않는 HTTP 요청입니다")),
        ParseError::RateLimited => Some((429, RATE_LIMIT_ERROR)),
    }
}

fn configure_connection(stream: &TcpStream) -> Result<(), std::io::Error> {
    let timeout = Some(Duration::from_millis(http::REQUEST_IO_TIMEOUT_MS));
    stream.set_read_timeout(timeout)?;
    stream.set_write_timeout(timeout)?;
    stream.set_nodelay(true)
}

fn next_connection_id(state: &Arc<ServerState>) -> Option<u64> {
    state
        .next_connection_id
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
            current.checked_add(1)
        })
        .ok()
}

fn register_connection(state: &Arc<ServerState>, stream: &TcpStream) -> Option<u64> {
    let id = next_connection_id(state)?;
    let handle = stream.try_clone().ok()?;
    let mut active = state.active_connections.lock().ok()?;
    if active.len() >= MAX_ACTIVE_CONNECTIONS {
        return None;
    }
    active.insert(id, handle);
    Some(id)
}

fn unregister_connection(state: &Arc<ServerState>, id: u64) {
    if let Ok(mut active) = state.active_connections.lock() {
        active.remove(&id);
    }
}

fn shutdown_active_connections(state: &Arc<ServerState>) {
    if let Ok(active) = state.active_connections.lock() {
        for stream in active.values() {
            let _ = stream.shutdown(Shutdown::Both);
        }
    }
}

fn clear_active_connections(state: &Arc<ServerState>) {
    if let Ok(mut active) = state.active_connections.lock() {
        active.clear();
    }
}

fn write_parse_error(stream: &mut TcpStream, error: ParseError) {
    if let Some((status, body)) = parse_error_response(error) {
        let _ = http::write_response(stream, status, &[], body);
    }
}

fn handle_request(
    state: &Arc<ServerState>,
    request: ParsedRequest,
    received_at: i64,
    running: &AtomicBool,
    stream: &mut TcpStream,
) {
    if !running.load(Ordering::Acquire) {
        return;
    }

    // history 기록 (마스킹 적용)
    if let Ok(mut history) = state.history.lock() {
        history.push(
            request.method.clone(),
            request.target.clone(),
            request.headers.clone(),
            request.body.clone(),
            received_at,
        );
    } else {
        let _ = http::write_response_for_method(
            stream,
            500,
            &[],
            SERVER_INTERNAL_ERROR,
            Some(&request.method),
        );
        return;
    }

    // The pure selector owns the same stable precedence used by preview/list.
    let response = match (state.rules.lock(), state.sequence_cursors.lock()) {
        (Ok(rules), Ok(mut cursors)) => {
            select_matching_rule(rules.values(), &request.method, &request.target)
                .map(|rule| cursors.next_response(rule))
        }
        _ => {
            let _ = http::write_response_for_method(
                stream,
                500,
                &[],
                SERVER_INTERNAL_ERROR,
                Some(&request.method),
            );
            return;
        }
    };

    let (status, response_headers, response_body, delay_ms) = match response {
        Some(response) => (
            response.status,
            response.headers,
            response.body,
            response.delay_ms,
        ),
        None => (404, vec![], "Not Found".to_string(), 0),
    };
    if delay_ms > 0 && !sleep_interruptibly(delay_ms, running) {
        // Stopping the listener also cancels an in-flight artificial delay.
        return;
    }

    let _ = http::write_response_for_method(
        stream,
        status,
        &response_headers,
        &response_body,
        Some(&request.method),
    );
}

fn serve_connection(
    state: &Arc<ServerState>,
    running: &Arc<AtomicBool>,
    id: u64,
    mut stream: TcpStream,
) {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let received_at = now_ms();
        let parsed = http::read_request(&mut stream, running, || {
            state
                .history
                .lock()
                .map(|mut history| history.allow_request(received_at))
                .unwrap_or(false)
        });
        match parsed {
            Ok(request) => handle_request(state, request, received_at, running, &mut stream),
            Err(error) => write_parse_error(&mut stream, error),
        }
    }));
    if result.is_err() {
        // A malformed request or a poisoned application lock must not kill the
        // listener. The connection is always removed from the active set.
    }
    unregister_connection(state, id);
}

fn reap_workers(finished: &mpsc::Receiver<u64>, workers: &mut HashMap<u64, JoinHandle<()>>) {
    while let Ok(id) = finished.try_recv() {
        if let Some(worker) = workers.remove(&id) {
            let _ = worker.join();
        }
    }
}

fn run_listener(listener: TcpListener, state: Arc<ServerState>, running: Arc<AtomicBool>) {
    let (finished_sender, finished_receiver) = mpsc::channel::<u64>();
    let mut workers = HashMap::new();

    while running.load(Ordering::Acquire) {
        reap_workers(&finished_receiver, &mut workers);
        match listener.accept() {
            Ok((stream, _peer)) => {
                if !running.load(Ordering::Acquire) {
                    let _ = stream.shutdown(Shutdown::Both);
                    break;
                }
                if configure_connection(&stream).is_err() {
                    let _ = stream.shutdown(Shutdown::Both);
                    continue;
                }
                let Some(id) = register_connection(&state, &stream) else {
                    let mut stream = stream;
                    // The listener thread must not wait the normal 5-second
                    // response budget for an over-cap client that refuses to
                    // read its fixed 503. Active workers remain bounded and
                    // ordinary accepts stay responsive under saturation.
                    let _ = stream.set_write_timeout(Some(Duration::from_millis(100)));
                    let _ = http::write_response(&mut stream, 503, &[], "서버가 바쁩니다");
                    let _ = stream.shutdown(Shutdown::Both);
                    continue;
                };
                let worker_state = Arc::clone(&state);
                let worker_running = Arc::clone(&running);
                let worker_finished = finished_sender.clone();
                let worker = thread::Builder::new()
                    .name("webhook-lab-connection".to_string())
                    .spawn(move || {
                        serve_connection(&worker_state, &worker_running, id, stream);
                        let _ = worker_finished.send(id);
                    });
                match worker {
                    Ok(worker) => {
                        workers.insert(id, worker);
                    }
                    Err(_) => {
                        unregister_connection(&state, id);
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(_) => {
                running.store(false, Ordering::Release);
                shutdown_active_connections(&state);
                break;
            }
        }
    }

    // If the accept loop exits unexpectedly, cancel replay before releasing
    // the listener socket. Holding replay_lock until this function returns
    // keeps the socket alive while an already-running replay observes the
    // cancellation, preventing a freed port from being reused by another
    // local process during that final check.
    advance_listener_generation(&state);
    state.replay_cancel.store(true, Ordering::Release);
    let _replay_exit_guard = match state.replay_lock.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    shutdown_active_connections(&state);
    drop(finished_sender);
    for (_, worker) in workers {
        let _ = worker.join();
    }
    clear_active_connections(&state);
}

/// Wait for a response delay while allowing `stop_server` to complete
/// promptly. A plain sleep could make lifecycle join wait for the full
/// user-configured delay (up to 60 seconds), even after the running flag was
/// cleared. Short slices keep the stop latency bounded without busy-spinning.
fn sleep_interruptibly(delay_ms: u64, running: &AtomicBool) -> bool {
    let deadline = Instant::now() + Duration::from_millis(delay_ms);
    loop {
        if !running.load(Ordering::Acquire) {
            return false;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return true;
        }
        std::thread::sleep(remaining.min(Duration::from_millis(50)));
    }
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayResult {
    /// An opaque source label only; no request body/header or response body is
    /// returned to the renderer.
    pub source_id: String,
    pub status: u16,
}

fn map_replay_error(error: ReplayError) -> String {
    match error {
        ReplayError::InvalidFixture | ReplayError::TooLarge => REPLAY_INPUT_ERROR.to_string(),
        ReplayError::InvalidTarget => REPLAY_SERVER_ERROR.to_string(),
        ReplayError::Cancelled => REPLAY_NETWORK_ERROR.to_string(),
        ReplayError::Network => REPLAY_NETWORK_ERROR.to_string(),
        ReplayError::InvalidResponse => REPLAY_RESPONSE_ERROR.to_string(),
    }
}

fn replay_masked_fixture(
    state: &Arc<ServerState>,
    fixture: CapturedFixture,
    source_id: String,
) -> Result<ReplayResult, String> {
    let expected_generation = state.listener_generation.load(Ordering::Acquire);
    // Keep the listener lifecycle stable for the complete bounded send. A
    // status/address snapshot followed by an unlocked connect would permit a
    // stop/restart (or another local process taking the freed port) to turn a
    // valid replay into a time-of-check/time-of-use destination race.
    // `replay::send` has a finite connect/write/read budget, so lifecycle IPC
    // remains bounded while this guard is held.
    let _lifecycle = state
        .lifecycle_lock
        .lock()
        .map_err(|_| REPLAY_SERVER_ERROR.to_string())?;
    if state.listener_generation.load(Ordering::Acquire) != expected_generation {
        return Err(REPLAY_SERVER_ERROR.to_string());
    }
    let address = {
        let running = state
            .running
            .lock()
            .map_err(|_| REPLAY_SERVER_ERROR.to_string())?
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::Acquire));
        let address = state
            .address
            .lock()
            .map_err(|_| REPLAY_SERVER_ERROR.to_string())?
            .clone();
        if !running {
            return Err(REPLAY_SERVER_ERROR.to_string());
        }
        address.ok_or_else(|| REPLAY_SERVER_ERROR.to_string())?
    };
    // Validate the destination before consuming a replay-rate slot.  This is
    // a second guard in addition to `send`, keeping invalid state from
    // exhausting a caller's bounded action budget.
    replay::loopback_socket(&address).map_err(map_replay_error)?;
    if !state
        .replay_rate
        .lock()
        .map_err(|_| REPLAY_RATE_LIMIT_ERROR.to_string())?
        .allow(now_ms())
    {
        return Err(REPLAY_RATE_LIMIT_ERROR.to_string());
    }
    let _replay_guard = state
        .replay_lock
        .lock()
        .map_err(|_| REPLAY_NETWORK_ERROR.to_string())?;
    let response =
        replay::send(&fixture, &address, &state.replay_cancel).map_err(map_replay_error)?;
    Ok(ReplayResult {
        source_id,
        status: response.status,
    })
}

/// Replay a backend-owned masked history snapshot to the currently running
/// localhost listener.  The frontend supplies only the opaque history ID;
/// raw headers never leave the in-memory history vault.
#[tauri::command]
pub fn replay_history(
    state: tauri::State<'_, Arc<ServerState>>,
    history_id: u64,
) -> Result<ReplayResult, String> {
    let request = state
        .history
        .lock()
        .map_err(|_| REPLAY_INPUT_ERROR.to_string())?
        .masked_record(history_id)
        .ok_or_else(|| REPLAY_INPUT_ERROR.to_string())?;
    let fixture = fixture_from_request(format!("fixture-{history_id}"), &request)
        .map_err(|_| REPLAY_INPUT_ERROR.to_string())?;
    replay_masked_fixture(&state, fixture, format!("history-{history_id}"))
}

/// Replay a validated masked fixture to the currently running localhost
/// listener.  Only the fixture ID crosses IPC; path/body/header values are
/// loaded and revalidated by the backend.
#[tauri::command]
pub fn replay_fixture(
    app: AppHandle,
    state: tauri::State<'_, Arc<ServerState>>,
    id: String,
) -> Result<ReplayResult, String> {
    let _guard = state
        .fixture_lock
        .lock()
        .map_err(|_| REPLAY_INPUT_ERROR.to_string())?;
    let document = load_document_with_raw(&fixture_path(&app)?).map_err(fixture_error)?;
    let fixture = document
        .document
        .fixtures
        .iter()
        .find(|fixture| fixture.id == id)
        .cloned()
        .ok_or_else(|| FixtureError::NotFound.message().to_string())?;
    drop(_guard);
    replay_masked_fixture(&state, fixture, format!("fixture-{id}"))
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
    update_document(&path, |document| {
        if document.fixtures.len() >= MAX_FIXTURES {
            return Err(FixtureError::Size);
        }
        let next_id = document.next_id;
        let fixture_id = format!("fixture-{next_id}");
        let fixture = fixture_from_request(fixture_id, &request)?;
        document.next_id = next_id.checked_add(1).ok_or(FixtureError::Size)?;
        document.fixtures.push(fixture.clone());
        Ok(fixture)
    })
    .map_err(fixture_error)
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
    update_document(&path, |document| {
        let original_len = document.fixtures.len();
        document.fixtures.retain(|fixture| fixture.id != id);
        if document.fixtures.len() == original_len {
            return Err(FixtureError::NotFound);
        }
        Ok(())
    })
    .map_err(fixture_error)
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
    update_document(&path, |document| {
        if document.fixtures.is_empty() {
            return Ok(());
        }
        let next_id = document.next_id;
        *document = FixtureDocument::default();
        document.next_id = next_id;
        Ok(())
    })
    .map_err(fixture_error)
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

/// Publish a masked captured request to API Playground through the shared
/// one-time handoff store.  The target is discovered from the catalog before
/// publishing so a missing/old installation never leaves a misleading
/// clipboard or temporary-file fallback.
#[tauri::command]
pub fn send_history_to_api(
    state: tauri::State<'_, Arc<ServerState>>,
    history_id: u64,
) -> Result<ApiHandoffDispatch, String> {
    let request = state
        .history
        .lock()
        .map_err(|_| HANDOFF_INPUT_ERROR.to_string())?
        .masked_record(history_id)
        .ok_or_else(|| "요청 기록을 찾을 수 없습니다".to_string())?;
    let fixture = fixture_from_request(format!("fixture-{history_id}"), &request)
        .map_err(|_| HANDOFF_INPUT_ERROR.to_string())?;
    publish_api_handoff(fixture)
}

/// Publish a stored masked fixture by opaque fixture ID.  The frontend cannot
/// provide a path, URL, body, or header value to this command.
#[tauri::command]
pub fn send_fixture_to_api(
    app: AppHandle,
    state: tauri::State<'_, Arc<ServerState>>,
    id: String,
) -> Result<ApiHandoffDispatch, String> {
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
        .cloned()
        .ok_or_else(|| FixtureError::NotFound.message().to_string())?;
    drop(_guard);
    publish_api_handoff(fixture)
}

fn publish_api_handoff(fixture: CapturedFixture) -> Result<ApiHandoffDispatch, String> {
    let target_available =
        devbox_launch::installed_targets(&format!("handoff:{API_REQUEST_HANDOFF_KIND}"))
            .into_iter()
            .any(|target| target.id == CONSUMER_APP_ID);
    if !target_available {
        return Err(API_TARGET_UNAVAILABLE_ERROR.to_string());
    }

    let payload =
        build_api_request_payload(&fixture).map_err(|_| HANDOFF_INPUT_ERROR.to_string())?;
    let created_at_ms = handoff_now_ms().ok_or_else(|| HANDOFF_CREATE_ERROR.to_string())?;
    let expires_at_ms = created_at_ms
        .checked_add(devbox_applink::DEFAULT_HANDOFF_TTL_MS)
        .ok_or_else(|| HANDOFF_CREATE_ERROR.to_string())?;
    let store = HandoffStore::new(handoff_root_in(&devbox_integration::common_root()));
    let descriptor = store
        .create(
            CreateHandoff {
                kind: API_REQUEST_HANDOFF_KIND.to_string(),
                source_app: PRODUCER_APP_ID.to_string(),
                target_app: Some(CONSUMER_APP_ID.to_string()),
                payload: to_value(payload).map_err(|_| HANDOFF_CREATE_ERROR.to_string())?,
            },
            created_at_ms,
        )
        .map_err(map_handoff_create_error)?;
    let request = OpenRequest {
        target: descriptor.clone().into(),
        from: Some(PRODUCER_APP_ID.to_string()),
    };
    if devbox_launch::launch_open(CONSUMER_APP_ID, &request).is_err() {
        // The pending envelope is deliberately retained until TTL.  A later
        // explicit retry can consume it, and no alternate data channel is
        // opened when process launch fails.
        return Err(API_LAUNCH_ERROR.to_string());
    }
    Ok(ApiHandoffDispatch {
        handoff_id: descriptor.id,
        producer_id: PRODUCER_APP_ID.to_string(),
        consumer_id: CONSUMER_APP_ID.to_string(),
        created_at_ms,
        expires_at_ms,
    })
}

fn map_handoff_create_error(error: HandoffError) -> String {
    match error {
        HandoffError::InvalidPayload | HandoffError::InvalidRequest | HandoffError::TooLarge => {
            HANDOFF_INPUT_ERROR.to_string()
        }
        HandoffError::UnsafeStorage | HandoffError::Storage | HandoffError::RandomUnavailable => {
            HANDOFF_CREATE_ERROR.to_string()
        }
        HandoffError::Missing
        | HandoffError::AlreadyClaimed
        | HandoffError::WrongTarget
        | HandoffError::WrongKind
        | HandoffError::Expired
        | HandoffError::LeaseExpired
        | HandoffError::TokenMismatch
        | HandoffError::Corrupt => HANDOFF_CREATE_ERROR.to_string(),
    }
}

#[tauri::command]
pub fn list_rules(state: tauri::State<'_, Arc<ServerState>>) -> Vec<ResponseRule> {
    let mut rules: Vec<ResponseRule> = state.rules.lock().unwrap().values().cloned().collect();
    rules.sort_by(compare_rule_precedence);
    rules
}

#[tauri::command]
pub fn preview_rule_conflicts(
    state: tauri::State<'_, Arc<ServerState>>,
    rule: ResponseRule,
) -> Result<RuleConflictPreview, String> {
    let rules = state
        .rules
        .lock()
        .map_err(|_| INVALID_RULE_ERROR.to_string())?;
    plan_upsert(&rules, rule)
        .map(|plan| plan.preview)
        .map_err(|_| INVALID_RULE_ERROR.to_string())
}

#[tauri::command]
pub fn set_rule(
    state: tauri::State<'_, Arc<ServerState>>,
    rule: ResponseRule,
    confirm_conflicts: bool,
) -> Result<String, String> {
    let mut rules = state
        .rules
        .lock()
        .map_err(|_| INVALID_RULE_ERROR.to_string())?;
    // Acquire the cursor lock before mutating the rule map. If either lock is
    // poisoned, the command fails without leaving a new rule whose sequence
    // reset could not be committed.
    let mut cursors = state
        .sequence_cursors
        .lock()
        .map_err(|_| INVALID_RULE_ERROR.to_string())?;
    let plan = plan_upsert(&rules, rule).map_err(|_| INVALID_RULE_ERROR.to_string())?;
    if plan.preview.requires_confirmation && !confirm_conflicts {
        return Err(RULE_CONFLICT_CONFIRMATION_ERROR.to_string());
    }
    let result = upsert(&mut rules, plan.candidate);
    if let Ok(id) = &result {
        // Editing a rule starts a fresh deterministic scenario.  The cursor
        // is never persisted and is removed only after the rule mutation has
        // passed validation.
        cursors.reset(id);
    }
    result.map_err(|_| INVALID_RULE_ERROR.to_string())
}

#[tauri::command]
pub fn delete_rule(state: tauri::State<'_, Arc<ServerState>>, id: String) -> Result<(), String> {
    let mut rules = state
        .rules
        .lock()
        .map_err(|_| "규칙을 찾을 수 없습니다".to_string())?;
    let mut cursors = state
        .sequence_cursors
        .lock()
        .map_err(|_| SEQUENCE_RESET_ERROR.to_string())?;
    if rules.remove(&id).is_some() {
        cursors.reset(&id);
        Ok(())
    } else {
        Err("규칙을 찾을 수 없습니다".to_string())
    }
}

/// Reset one rule's process-local response cursor.  The rule definition and
/// any persisted fixture remain unchanged.
#[tauri::command]
pub fn reset_rule_sequence(
    state: tauri::State<'_, Arc<ServerState>>,
    id: String,
) -> Result<(), String> {
    let rules = state
        .rules
        .lock()
        .map_err(|_| SEQUENCE_RESET_ERROR.to_string())?;
    if !rules.contains_key(&id) {
        return Err("규칙을 찾을 수 없습니다".to_string());
    }
    state
        .sequence_cursors
        .lock()
        .map_err(|_| SEQUENCE_RESET_ERROR.to_string())?
        .reset(&id);
    Ok(())
}

/// Persist the current backend-owned rule set as an app-local service profile
/// and return one disabled Run Manager definition. The renderer supplies no
/// rule JSON, executable path, bind address, or command string.
#[tauri::command]
pub fn export_run_service_definition(
    app: AppHandle,
    state: tauri::State<'_, Arc<ServerState>>,
) -> Result<crate::core::service_profile::RunDefinitionExport, String> {
    // Keep the observed listener generation stable through profile creation
    // and serialize the bounded profile-count check with other exports.
    let _lifecycle = state
        .lifecycle_lock
        .lock()
        .map_err(|_| RUN_DEFINITION_EXPORT_ERROR.to_string())?;
    let address = current_server_status(state.inner())
        .address
        .ok_or_else(|| RUN_DEFINITION_EXPORT_ERROR.to_string())?;
    let socket = address
        .parse::<std::net::SocketAddr>()
        .map_err(|_| RUN_DEFINITION_EXPORT_ERROR.to_string())?;
    let bind = match socket.ip() {
        std::net::IpAddr::V4(address) if address == std::net::Ipv4Addr::LOCALHOST => "127.0.0.1",
        std::net::IpAddr::V6(address) if address == std::net::Ipv6Addr::LOCALHOST => "::1",
        _ => return Err(RUN_DEFINITION_EXPORT_ERROR.to_string()),
    };
    let mut rules = state
        .rules
        .lock()
        .map_err(|_| RUN_DEFINITION_EXPORT_ERROR.to_string())?
        .values()
        .cloned()
        .collect::<Vec<_>>();
    rules.sort_by(compare_rule_precedence);
    let data_root = app
        .path()
        .app_local_data_dir()
        .map_err(|_| crate::core::service_profile::SERVICE_PROFILE_ERROR.to_string())?;
    let executable = std::env::current_exe()
        .map_err(|_| crate::core::service_profile::SERVICE_PROFILE_ERROR.to_string())?;
    let now = handoff_now_ms()
        .ok_or_else(|| crate::core::service_profile::SERVICE_PROFILE_ERROR.to_string())?;
    crate::core::service_profile::export_run_definition_in(
        &data_root,
        &executable,
        bind,
        socket.port(),
        rules,
        now,
    )
}

fn history_not_found() -> String {
    "요청 기록을 찾을 수 없습니다".to_string()
}

fn advance_listener_generation(state: &Arc<ServerState>) {
    state.listener_generation.fetch_add(1, Ordering::AcqRel);
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn handoff_now_ms() -> Option<u64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
        .filter(|now| *now > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    fn spawn_test_listener() -> (
        Arc<ServerState>,
        Arc<AtomicBool>,
        std::net::SocketAddr,
        JoinHandle<()>,
    ) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let state = server_state();
        let running = Arc::new(AtomicBool::new(true));
        let listener_state = Arc::clone(&state);
        let listener_running = Arc::clone(&running);
        let thread =
            thread::spawn(move || run_listener(listener, listener_state, listener_running));
        (state, running, address, thread)
    }

    fn stop_test_listener(
        state: &Arc<ServerState>,
        running: &Arc<AtomicBool>,
        thread: JoinHandle<()>,
    ) {
        running.store(false, Ordering::Release);
        shutdown_active_connections(state);
        thread.join().unwrap();
    }

    #[test]
    fn native_listener_records_a_bounded_request_and_returns_rule_response() {
        let (state, running, address, thread) = spawn_test_listener();
        let rule = ResponseRule {
            id: "rule-1".into(),
            priority: 0,
            method: Some("POST".into()),
            path: "/hook".into(),
            status: 201,
            headers: vec![("X-Test".into(), "ok".into())],
            body: "accepted".into(),
            delay_ms: 0,
            sequence: Vec::new(),
        };
        upsert(&mut state.rules.lock().unwrap(), rule).unwrap();

        let mut client = TcpStream::connect(address).unwrap();
        client
            .write_all(b"POST /hook HTTP/1.1\r\nContent-Length: 7\r\n\r\nbody ok")
            .unwrap();
        let mut response = Vec::new();
        client.read_to_end(&mut response).unwrap();
        let response = String::from_utf8(response).unwrap();
        assert!(response.starts_with("HTTP/1.1 201 Created\r\n"));
        assert!(response.contains("X-Test: ok\r\n"));
        assert!(response.ends_with("\r\n\r\naccepted"));

        let history = state.history.lock().unwrap().list_masked();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].method, "POST");
        assert_eq!(history[0].url, "/hook");
        assert_eq!(history[0].body, "body ok");
        stop_test_listener(&state, &running, thread);
    }

    #[test]
    fn native_listener_omits_body_and_content_length_for_head_requests() {
        let (state, running, address, thread) = spawn_test_listener();
        let rule = ResponseRule {
            id: "rule-head".into(),
            priority: 0,
            method: Some("HEAD".into()),
            path: "/hook".into(),
            status: 200,
            headers: vec![("Content-Type".into(), "text/plain".into())],
            body: "must not be sent".into(),
            delay_ms: 0,
            sequence: Vec::new(),
        };
        upsert(&mut state.rules.lock().unwrap(), rule).unwrap();

        let mut client = TcpStream::connect(address).unwrap();
        client.write_all(b"HEAD /hook HTTP/1.1\r\n\r\n").unwrap();
        let mut response = Vec::new();
        client.read_to_end(&mut response).unwrap();
        let response = String::from_utf8(response).unwrap();
        assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(response.contains("Content-Type: text/plain\r\n"));
        assert!(!response.contains("Content-Length:"));
        assert!(response.ends_with("\r\n\r\n"));
        assert!(!response.contains("must not be sent"));

        stop_test_listener(&state, &running, thread);
    }

    #[test]
    fn stopping_interrupts_a_partial_body_without_waiting_for_the_idle_timeout() {
        let (state, running, address, thread) = spawn_test_listener();
        let mut client = TcpStream::connect(address).unwrap();
        client
            .write_all(b"POST /hook HTTP/1.1\r\nContent-Length: 3\r\n\r\nx")
            .unwrap();
        thread::sleep(Duration::from_millis(30));

        let started = Instant::now();
        stop_test_listener(&state, &running, thread);
        assert!(started.elapsed() < Duration::from_secs(1));
        drop(client);
    }

    #[test]
    fn parser_errors_are_fixed_and_never_enter_history() {
        let (state, running, address, thread) = spawn_test_listener();
        let mut client = TcpStream::connect(address).unwrap();
        client
            .write_all(b"POST /hook HTTP/1.1\r\nContent-Length: 99999999\r\n\r\n")
            .unwrap();
        let mut response = Vec::new();
        client.read_to_end(&mut response).unwrap();
        let response = String::from_utf8(response).unwrap();
        assert!(response.starts_with("HTTP/1.1 413 Payload Too Large\r\n"));
        assert!(!response.contains("99999999"));
        assert!(state.history.lock().unwrap().list_masked().is_empty());
        stop_test_listener(&state, &running, thread);
    }
}
