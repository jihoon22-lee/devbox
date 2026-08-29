use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{ErrorKind, Read, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tauri::Emitter;

use crate::core::multiplexer::build_session_argv;
use crate::core::workspace::MultiplexerKind;
use crate::runtime_snapshot::{request_snapshot_write, SnapshotCoordinator};

/// 실행 중인 터미널 세션 저장소
pub struct SessionState {
    pub sessions: Mutex<HashMap<String, Arc<Mutex<SessionHandle>>>>,
    pub snapshot_coordinator: Arc<SnapshotCoordinator>,
}

impl SessionState {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            snapshot_coordinator: Arc::new(SnapshotCoordinator::new()),
        }
    }

    /// Take a bounded distro-only count snapshot without exposing session ids, pane keys,
    /// cwd, title or command metadata to the integration producer.
    pub(crate) fn terminal_counts_by_distro(&self) -> Result<BTreeMap<String, usize>, String> {
        let sessions = self
            .sessions
            .lock()
            .map_err(|_| "터미널 상태를 읽을 수 없습니다".to_owned())?;
        let mut counts = BTreeMap::new();
        for handle in sessions.values() {
            let distro = handle
                .lock()
                .map_err(|_| "터미널 상태를 읽을 수 없습니다".to_owned())?
                .distro
                .trim()
                .to_owned();
            if !crate::core::runtime_snapshot::is_safe_distro_name(&distro) {
                return Err("터미널 상태를 읽을 수 없습니다".into());
            }
            let count = counts.entry(distro).or_insert(0usize);
            *count = count.saturating_add(1);
            if *count > crate::core::runtime_snapshot::MAX_TERMINALS_PER_DISTRO {
                return Err("터미널 수 제한을 초과했습니다".into());
            }
        }
        Ok(counts)
    }
}

/// PTY 세션 하나
pub struct SessionHandle {
    pub distro: String,
    pub writer: Box<dyn Write + Send>,
    pub child: Option<Box<dyn portable_pty::Child + Send + Sync>>,
    /// ConPTY(HPCON)를 보관하는 master. drop 되면 ConPTY가 닫히므로
    /// 세션 수명 동안 유지해야 한다 (일찍 닫으면 자식이 0xc0000142로 실패한다).
    /// v0.2.2에서는 보관 용도로만 썼지만, 이제 `resize_session`이
    /// `MasterPty::resize()`를 호출하는 데도 쓰인다.
    pub master: Option<Box<dyn portable_pty::MasterPty>>,
    /// `attach_session`이 리더 스레드를 시작할 때 꺼내 쓰는 PTY 리더.
    /// `start_session`이 세션을 만들 때 채우고, attach 후에는 스레드 클로저로
    /// 이동하므로 `None`으로 남는다.
    pub reader: Option<Box<dyn Read + Send>>,
    /// 리더 스레드가 이미 spawn 됐는지. `attach_session`을 두 번 호출해도
    /// 스레드가 두 번 spawn 되지 않도록 막는 가드.
    pub attached: bool,
}

impl SessionHandle {
    /// 첫 attach 요청에서만 `true`를 반환하고 내부 플래그를 세운다.
    /// 이후 재호출은 `false` — 리더 스레드 중복 spawn을 막는다.
    fn mark_attached(&mut self) -> bool {
        if self.attached {
            false
        } else {
            self.attached = true;
            true
        }
    }

    fn take_resources(&mut self) -> SessionResources {
        SessionResources {
            writer: std::mem::replace(&mut self.writer, Box::new(std::io::sink())),
            child: self.child.take(),
            master: self.master.take(),
            reader: self.reader.take(),
        }
    }
}

struct SessionResources {
    writer: Box<dyn Write + Send>,
    child: Option<Box<dyn portable_pty::Child + Send + Sync>>,
    master: Option<Box<dyn portable_pty::MasterPty>>,
    reader: Option<Box<dyn Read + Send>>,
}

impl SessionResources {
    fn teardown(mut self, kill_child: bool) {
        if kill_child {
            if let Some(child) = self.child.as_mut() {
                let _ = child.kill();
            }
        }
        drop(self.reader.take());
        drop(self.master.take());
        drop(self.writer);
        drop(self.child.take());
    }
}

fn take_session(state: &SessionState, session_id: &str) -> Option<Arc<Mutex<SessionHandle>>> {
    state.sessions.lock().unwrap().remove(session_id)
}

fn teardown_session(handle: Arc<Mutex<SessionHandle>>, kill_child: bool) {
    let resources = {
        let mut handle = handle.lock().unwrap();
        handle.take_resources()
    };
    resources.teardown(kill_child);
}

fn emit_terminal_closed_if_cleanup_won<F>(cleanup_won: bool, emit: F)
where
    F: FnOnce(),
{
    if cleanup_won {
        emit();
    }
}

/// Removes a session only when the reader still owns the handle currently
/// registered for its id. A stale reader must not remove a replacement handle
/// that happens to use the same id.
fn remove_session_if_handle(
    state: &SessionState,
    session_id: &str,
    expected_handle: &Arc<Mutex<SessionHandle>>,
) -> bool {
    let removed = {
        let mut sessions = state.sessions.lock().unwrap();
        let matches_current = matches!(
            sessions.get(session_id),
            Some(current) if Arc::ptr_eq(current, expected_handle)
        );
        if matches_current {
            sessions.remove(session_id)
        } else {
            None
        }
    };
    let Some(handle) = removed else {
        return false;
    };
    teardown_session(handle, false);
    true
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionInfo {
    pub id: String,
    pub distro: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartedSession {
    pub session_id: String,
    pub resumed: bool,
    pub multiplexer: MultiplexerKind,
}

#[derive(Debug, Clone, Serialize)]
pub struct TerminalOutput {
    pub session_id: String,
    pub data: String,
}

/// 프로세스 전역 카운터. 밀리초 타임스탬프는 같은 밀리초에 두 세션이 생기면
/// 충돌한다 — HashMap 삽입이 먼저 생긴 `SessionHandle`을 덮어써 그 ConPTY가
/// 닫히고, 두 리더 스레드가 같은 id로 방출해 두 스트림이 한 xterm에 섞인다.
static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);

/// Returns the Windows build number used by xterm's ConPTY heuristics.
/// Linux/WSL development builds intentionally return `None`.
#[tauri::command]
pub fn windows_build_number() -> Option<u32> {
    #[cfg(target_os = "windows")]
    {
        Some(windows_version::OsVersion::current().build)
    }
    #[cfg(not(target_os = "windows"))]
    {
        None
    }
}

/// 다음 세션 id를 발급한다. 프론트의 `lib/id.ts`의 `makeId`와 같은 패턴
/// (단조 증가 카운터)을 백엔드 쪽에서 쓴다.
fn next_session_id() -> String {
    let n = NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed);
    format!("s{n}")
}

/// PTY 바이트를 UTF-8로 디코드한다. 읽기 경계에 걸린 불완전한 멀티바이트 시퀀스는
/// carry 에 남겨 다음 청크 앞에 이어 붙인다. 진짜 잘못된 바이트만 U+FFFD 로 치환한다.
///
/// 불완전 시퀀스는 최대 3바이트(UTF-8 최대 4바이트 - 1)이므로 carry 는 자연히 유계다.
fn decode_chunk(carry: &mut Vec<u8>, chunk: &[u8]) -> String {
    carry.extend_from_slice(chunk);
    let mut out = String::with_capacity(carry.len());
    let mut start = 0usize;
    loop {
        match std::str::from_utf8(&carry[start..]) {
            Ok(s) => {
                out.push_str(s);
                start = carry.len();
                break;
            }
            Err(e) => {
                let valid = e.valid_up_to();
                out.push_str(std::str::from_utf8(&carry[start..start + valid]).unwrap());
                match e.error_len() {
                    // 진짜 불량 바이트 — U+FFFD 로 치환하고 건너뛴다
                    Some(n) => {
                        out.push('\u{FFFD}');
                        start += valid + n;
                    }
                    // 끝이 잘린 시퀀스 — 다음 청크로 넘긴다
                    None => {
                        start += valid;
                        break;
                    }
                }
            }
        }
    }
    carry.drain(..start);
    out
}

/// Builds the exact argv used for a terminal session.
///
/// Always delegates distro validation and argument construction to the shared
/// WSL builder, including when no cwd is supplied. Keeping this boundary as
/// argv also prevents a path from becoming shell syntax (`bash -lc ...`).
#[cfg(test)]
fn build_session_command(distro: &str, cwd: Option<&str>) -> Result<CommandBuilder, String> {
    build_workspace_session_command(distro, cwd, "ephemeral", MultiplexerKind::Native, None)
}

fn build_workspace_session_command(
    distro: &str,
    cwd: Option<&str>,
    pane_key: &str,
    multiplexer: MultiplexerKind,
    resolved_executable: Option<&str>,
) -> Result<CommandBuilder, String> {
    let argv = build_session_argv(distro, cwd, pane_key, multiplexer, resolved_executable)?;
    let mut command = CommandBuilder::new(&argv[0]);
    command.args(&argv[1..]);
    Ok(command)
}

/// 새 WSL 터미널 세션을 시작한다. `cwd`가 있으면 해당 경로로 열린다.
///
/// PTY 리더 스레드는 여기서 spawn하지 않는다 — `attach_session`이 한다.
/// (프론트가 출력 핸들러를 등록하기 전에 방출을 시작하면, 등록 전 데이터는
/// `App.tsx`의 옵셔널 체이닝으로 조용히 버려지고 이스케이프 시퀀스 중간에서
/// 잘린 첫 청크가 리터럴 쓰레기로 렌더된다.)
#[tauri::command]
pub async fn start_session(
    state: tauri::State<'_, Arc<SessionState>>,
    distro: String,
    cwd: Option<String>,
    pane_key: String,
    multiplexer: MultiplexerKind,
) -> Result<StartedSession, String> {
    let resolved_multiplexer = if multiplexer == MultiplexerKind::Native {
        None
    } else {
        crate::commands::multiplexer::resolve_for_launch(&distro, multiplexer).await
    };
    let actual_multiplexer = resolved_multiplexer
        .as_ref()
        .map_or(MultiplexerKind::Native, |resolved| resolved.kind());
    let resumed = match resolved_multiplexer.as_ref() {
        Some(resolved) => {
            crate::commands::multiplexer::session_is_running(&distro, &pane_key, resolved).await
        }
        None => false,
    };
    let cmd = build_workspace_session_command(
        &distro,
        cwd.as_deref(),
        &pane_key,
        actual_multiplexer,
        resolved_multiplexer
            .as_ref()
            .map(|resolved| resolved.executable()),
    )?;
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: 30,
            cols: 100,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| e.to_string())?;

    // 지정 경로로 바로 열기: wsl.exe -d <distro> [--cd <dir>] --
    // (이전에는 `bash -lc "cd '{dir}' && exec bash"`를 문자열로 조립했다 —
    // 경로에 작은따옴표가 있으면 깨지고 셸 주입 표면이 열려 있었다. `--cd`는
    // wsl.exe가 셸 없이 직접 처리하므로 인용이 필요 없다.)
    let child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
    drop(pair.slave);

    let reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    // Windows에서만 ConPTY DSR 응답을 쓰므로 그 외 OS에서는 mut 불필요
    #[allow(unused_mut)]
    let mut writer = pair.master.take_writer().map_err(|e| e.to_string())?;
    // ConPTY(HPCON)를 보유한 master를 세션 핸들에 보관한다.
    // (reader/writer는 파이프 fd 클론이라 ConPTY 수명을 유지하지 못한다.
    //  master를 drop 하면 ConPTY가 닫히고, 시작 중인 자식이 0xc0000142로 실패한다)
    let master = pair.master;

    #[cfg(target_os = "windows")]
    {
        // ConPTY는 시작 시 커서 위치 조회(ESC[6n)를 보내고 응답이 올 때까지
        // 자식 프로세스를 정지시킨다 (PSEUDOCONSOLE_INHERIT_CURSOR 교착 상태).
        // 커서 위치 보고(ESC[1;1R)를 입력 파이프로 보내 차단을 해제한다.
        let _ = writer.write_all(b"\x1b[1;1R");
        let _ = writer.flush();
    }

    let session_id = next_session_id();

    let handle = Arc::new(Mutex::new(SessionHandle {
        distro: distro.clone(),
        writer,
        child: Some(child),
        master: Some(master),
        reader: Some(reader),
        attached: false,
    }));
    state
        .sessions
        .lock()
        .unwrap()
        .insert(session_id.clone(), handle);
    request_snapshot_write(Arc::clone(state.inner()));

    Ok(StartedSession {
        session_id,
        resumed,
        multiplexer: actual_multiplexer,
    })
}

/// PTY 리더 스레드를 시작한다. 프론트가 출력 핸들러를 등록한 직후 호출해야
/// `start_session`과 `attach_session` 사이의 출력이 유실되지 않는다 — 그 사이의
/// 출력은 ConPTY 내부 버퍼가 보관한다.
///
/// 세션이 이미 attach 됐거나(`SessionHandle::mark_attached`가 막는다) 이미
/// 닫혔으면(맵에 없음) 조용히 무시한다 — `write_session` 등 다른 커맨드와 같은
/// 관례다. attach가 오지 않아도 `close_session`은 세션을 정리할 수 있다
/// (reader/writer/master/child가 전부 `SessionHandle`에 있으므로 drop만으로 정리된다).
#[tauri::command]
pub fn attach_session(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<SessionState>>,
    session_id: String,
) -> Result<(), String> {
    let handle = {
        let sessions = state.sessions.lock().unwrap();
        match sessions.get(&session_id) {
            Some(h) => h.clone(),
            None => return Ok(()),
        }
    };

    let reader = {
        let mut h = handle.lock().unwrap();
        if !h.mark_attached() {
            return Ok(());
        }
        h.reader.take()
    };
    let Some(mut reader) = reader else {
        // attached 와 reader 상태가 어긋난 경우 방어적으로 종료한다 (일어나면 안 됨).
        return Ok(());
    };

    let app_out = app.clone();
    let sid = session_id.clone();
    let state_for_reader = Arc::clone(state.inner());
    std::thread::spawn(move || {
        let mut carry: Vec<u8> = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let data = decode_chunk(&mut carry, &buf[..n]);
                    let _ = app_out.emit(
                        "terminal-output",
                        TerminalOutput {
                            session_id: sid.clone(),
                            data,
                        },
                    );
                }
                // 일시 오류 — EOF가 아니다. 계속 읽는다.
                Err(e)
                    if e.kind() == ErrorKind::Interrupted || e.kind() == ErrorKind::WouldBlock =>
                {
                    continue;
                }
                Err(_) => break,
            }
        }
        drop(reader);
        let cleanup_won = remove_session_if_handle(&state_for_reader, &sid, &handle);
        if cleanup_won {
            request_snapshot_write(Arc::clone(&state_for_reader));
        }
        drop(handle);
        emit_terminal_closed_if_cleanup_won(cleanup_won, || {
            let _ = app_out.emit(
                "terminal-closed",
                TerminalOutput {
                    session_id: sid.clone(),
                    data: String::new(),
                },
            );
        });
    });

    Ok(())
}

/// 세션에 키 입력을 전달한다.
#[tauri::command]
pub fn write_session(
    state: tauri::State<'_, Arc<SessionState>>,
    session_id: String,
    data: String,
) -> Result<(), String> {
    validate_session_id(&session_id)?;
    validate_terminal_input(&data)?;
    let sessions = state.sessions.lock().unwrap();
    if let Some(h) = sessions.get(&session_id) {
        let mut h = h.lock().unwrap();
        h.writer
            .write_all(data.as_bytes())
            .map_err(|e| e.to_string())?;
        h.writer.flush().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// 지정한 세션들에만 동일한 입력을 전달한다 (동시 명령).
/// 탭 도입 전에는 등록된 모든 세션을 대상으로 했지만, 탭이 생기면서
/// 다른 탭의 세션에도 입력이 새는 문제가 됐다. 프론트는 활성 탭의
/// 세션 id만 넘긴다.
fn validate_broadcast_request(session_ids: &[String], data: &str) -> Result<(), String> {
    if session_ids.len() < 2 || session_ids.len() > MAX_BROADCAST_TARGETS {
        return Err("broadcast 입력 범위가 올바르지 않습니다".into());
    }
    for session_id in session_ids {
        validate_session_id(session_id)?;
    }
    validate_terminal_input(data)?;
    if session_ids.iter().collect::<HashSet<_>>().len() != session_ids.len() {
        return Err("broadcast 대상이 중복되었습니다".into());
    }
    Ok(())
}

const MAX_BROADCAST_TARGETS: usize = 32;
const MAX_TERMINAL_INPUT_BYTES: usize = 1_000_000;
const MAX_SESSION_ID_BYTES: usize = 128;

/// Session ids are opaque map keys, never shell fragments. Keep the IPC boundary bounded and
/// reject controls/separators before a malicious renderer can make diagnostics or logs
/// ambiguous. The generated ids are currently `s<number>`, while the wider safe alphabet keeps
/// test fixtures and future reconnect ids compatible.
fn validate_session_id(session_id: &str) -> Result<(), String> {
    if session_id.is_empty()
        || session_id.len() > MAX_SESSION_ID_BYTES
        || !session_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err("터미널 세션 식별자가 올바르지 않습니다".into());
    }
    Ok(())
}

fn validate_terminal_input(data: &str) -> Result<(), String> {
    if data.len() > MAX_TERMINAL_INPUT_BYTES {
        return Err("터미널 입력 범위를 초과했습니다".into());
    }
    Ok(())
}

#[tauri::command]
pub fn broadcast(
    state: tauri::State<'_, Arc<SessionState>>,
    session_ids: Vec<String>,
    data: String,
) -> Result<(), String> {
    validate_broadcast_request(&session_ids, &data)?;
    let sessions = state.sessions.lock().unwrap();
    if session_ids.iter().any(|id| !sessions.contains_key(id)) {
        return Err("broadcast 대상 세션을 찾을 수 없습니다".into());
    }
    for id in &session_ids {
        let mut handle = sessions[id].lock().unwrap();
        handle
            .writer
            .write_all(data.as_bytes())
            .map_err(|_| "broadcast 입력을 일부 세션에 전달하지 못했습니다".to_string())?;
        handle
            .writer
            .flush()
            .map_err(|_| "broadcast 입력을 일부 세션에 전달하지 못했습니다".to_string())?;
    }
    Ok(())
}

/// PTY 크기를 바꾼다. 탭 전환·분할 변경·창 크기 변경 시 프론트가
/// 실제 패인 크기(rows/cols)로 맞춰 호출한다. `openpty`가 세션 시작 시
/// 고정 크기(30x100)로 한 번만 설정하던 것을 세션 생존 동안 갱신 가능하게 한다.
#[tauri::command]
pub fn resize_session(
    state: tauri::State<'_, Arc<SessionState>>,
    session_id: String,
    rows: u16,
    cols: u16,
) -> Result<(), String> {
    let sessions = state.sessions.lock().unwrap();
    if let Some(h) = sessions.get(&session_id) {
        let h = h.lock().unwrap();
        if let Some(master) = &h.master {
            master
                .resize(PtySize {
                    rows,
                    cols,
                    pixel_width: 0,
                    pixel_height: 0,
                })
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[tauri::command]
pub fn close_session(
    state: tauri::State<'_, Arc<SessionState>>,
    session_id: String,
) -> Result<(), String> {
    if let Some(h) = take_session(state.inner().as_ref(), &session_id) {
        teardown_session(h, true);
        request_snapshot_write(Arc::clone(state.inner()));
    }
    Ok(())
}

#[tauri::command]
pub fn list_sessions(state: tauri::State<'_, Arc<SessionState>>) -> Vec<SessionInfo> {
    state
        .sessions
        .lock()
        .unwrap()
        .iter()
        .map(|(id, h)| SessionInfo {
            id: id.clone(),
            distro: h.lock().unwrap().distro.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command_args(command: &CommandBuilder) -> Vec<String> {
        command
            .get_argv()
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn session_command_keeps_cwd_as_a_separate_argument() {
        let command = build_session_command("Ubuntu", Some("/mnt/e/path with 'quote'")).unwrap();
        let args = command_args(&command);
        assert_eq!(
            args,
            vec![
                "wsl.exe",
                "-d",
                "Ubuntu",
                "--cd",
                "/mnt/e/path with 'quote'",
                "--",
            ]
        );
        assert!(args.iter().all(|arg| arg != "bash" && arg != "-lc"));
        assert!(args.iter().all(|arg| !arg.contains("bash -lc")));
    }

    #[test]
    fn session_command_preserves_windows_cwd_as_a_separate_argument() {
        let command =
            build_session_command("Ubuntu", Some(r"E:\projects\path with space")).unwrap();
        assert_eq!(
            command_args(&command),
            vec![
                "wsl.exe",
                "-d",
                "Ubuntu",
                "--cd",
                r"E:\projects\path with space",
                "--",
            ]
        );
    }

    #[test]
    fn session_command_rejects_invalid_distro_without_cwd() {
        assert!(build_session_command("a;b", None).is_err());
    }

    #[test]
    fn session_command_omits_whitespace_only_cwd() {
        let command = build_session_command("Ubuntu", Some("  \t ")).unwrap();
        assert_eq!(
            command_args(&command),
            vec!["wsl.exe", "-d", "Ubuntu", "--"]
        );
    }

    #[test]
    fn broadcast_requires_bounded_unique_explicit_targets() {
        assert!(validate_broadcast_request(&["one".into()], "echo ok").is_err());
        assert!(validate_broadcast_request(&["one".into(), "one".into()], "echo ok").is_err());
        assert!(validate_broadcast_request(&["one".into(), "two".into()], "echo ok").is_ok());
        assert!(
            validate_broadcast_request(&["one".into(), "two".into()], &"x".repeat(1_000_001),)
                .is_err()
        );
        assert!(validate_broadcast_request(&["one\n".into(), "two".into()], "echo ok").is_err());
        assert!(validate_broadcast_request(&["one".into(), "two".into()], "ok").is_ok());
    }

    /// 문자열을 모든 바이트 경계에서 2조각으로 나눠 각각 별도 청크로 넘기고,
    /// 재조립 결과가 원본과 바이트 동일한지 모든 분할 위치에서 확인한다.
    fn assert_roundtrip_all_splits(s: &str) {
        let bytes = s.as_bytes();
        for i in 1..bytes.len() {
            let mut carry = Vec::new();
            let mut out = String::new();
            out.push_str(&decode_chunk(&mut carry, &bytes[..i]));
            out.push_str(&decode_chunk(&mut carry, &bytes[i..]));
            assert_eq!(out, s, "split at byte {i} failed for {s:?}");
            assert!(
                carry.is_empty(),
                "carry not drained after full input, split at {i}"
            );
        }
    }

    #[test]
    fn korean_survives_every_split_point() {
        assert_roundtrip_all_splits("한글 테스트");
    }

    #[test]
    fn box_drawing_survives_every_split_point() {
        assert_roundtrip_all_splits("┌─┬─┐│└┘");
    }

    #[test]
    fn emoji_survives_every_split_point() {
        // 4바이트 이모지
        assert_roundtrip_all_splits("😀😃😄🎉");
    }

    #[test]
    fn three_chunk_splits_reassemble_exactly() {
        let s = "한글 테스트 ┌─┬─┐ 😀😃";
        let bytes = s.as_bytes();
        let len = bytes.len();
        for a in 1..len {
            for b in (a + 1)..len {
                let mut carry = Vec::new();
                let mut out = String::new();
                out.push_str(&decode_chunk(&mut carry, &bytes[..a]));
                out.push_str(&decode_chunk(&mut carry, &bytes[a..b]));
                out.push_str(&decode_chunk(&mut carry, &bytes[b..]));
                assert_eq!(out, s, "3-way split at {a},{b} failed");
                assert!(carry.is_empty(), "carry not drained, split at {a},{b}");
            }
        }
    }

    #[test]
    fn lone_invalid_byte_produces_one_replacement_char_and_resumes() {
        let mut carry = Vec::new();
        // 0xFF는 UTF-8에서 유효한 시작 바이트가 될 수 없다.
        let out = decode_chunk(&mut carry, &[b'a', 0xFF, b'b']);
        assert_eq!(out, "a\u{FFFD}b");
        assert!(carry.is_empty());
    }

    #[test]
    fn lone_continuation_byte_produces_one_replacement_char() {
        let mut carry = Vec::new();
        // 0x9C는 계속 바이트(continuation byte)만 가능 — 단독으로는 시작 바이트가 될 수 없다.
        let out = decode_chunk(&mut carry, &[b'x', 0x9C, b'y']);
        assert_eq!(out, "x\u{FFFD}y");
        assert!(carry.is_empty());
    }

    #[test]
    fn decoding_resumes_correctly_after_invalid_byte_across_chunks() {
        let mut carry = Vec::new();
        let mut out = String::new();
        out.push_str(&decode_chunk(&mut carry, &[0xFF]));
        out.push_str(&decode_chunk(&mut carry, "한글".as_bytes()));
        assert_eq!(out, "\u{FFFD}한글");
        assert!(carry.is_empty());
    }

    #[test]
    fn carry_never_exceeds_three_bytes() {
        let s = "한글 테스트 ┌─┬─┐ 😀😃 mixed ascii";
        let bytes = s.as_bytes();
        let mut carry = Vec::new();
        for chunk in bytes.chunks(1) {
            let _ = decode_chunk(&mut carry, chunk);
            assert!(carry.len() <= 3, "carry grew to {} bytes", carry.len());
        }
    }

    #[test]
    fn chunk_ending_on_char_boundary_leaves_carry_empty() {
        let mut carry = Vec::new();
        let out = decode_chunk(&mut carry, "한글".as_bytes());
        assert_eq!(out, "한글");
        assert!(carry.is_empty());
    }

    #[test]
    fn session_ids_are_unique_across_concurrent_calls() {
        const THREADS: usize = 16;
        const IDS_PER_THREAD: usize = 64;
        let start = Arc::new(std::sync::Barrier::new(THREADS + 1));

        let workers = (0..THREADS)
            .map(|_| {
                let start = Arc::clone(&start);
                std::thread::spawn(move || {
                    start.wait();
                    (0..IDS_PER_THREAD)
                        .map(|_| next_session_id())
                        .collect::<Vec<_>>()
                })
            })
            .collect::<Vec<_>>();

        start.wait();
        let mut ids = std::collections::HashSet::with_capacity(THREADS * IDS_PER_THREAD);
        for worker in workers {
            for id in worker.join().expect("session ID worker panicked") {
                assert!(ids.insert(id), "duplicate session id generated");
            }
        }
        assert_eq!(ids.len(), THREADS * IDS_PER_THREAD);
    }

    fn test_session_handle() -> Arc<Mutex<SessionHandle>> {
        Arc::new(Mutex::new(SessionHandle {
            distro: "Ubuntu".to_string(),
            writer: Box::new(Vec::new()),
            child: None,
            master: None,
            reader: None,
            attached: false,
        }))
    }

    #[test]
    fn terminal_count_view_groups_only_safe_distro_names() {
        let ubuntu_a = test_session_handle();
        let ubuntu_b = test_session_handle();
        let debian = test_session_handle();
        debian.lock().unwrap().distro = " Debian ".into();
        let state = SessionState {
            sessions: Mutex::new(HashMap::from([
                ("s1".into(), ubuntu_a),
                ("s2".into(), ubuntu_b),
                ("s3".into(), debian),
            ])),
            ..SessionState::new()
        };

        assert_eq!(
            state.terminal_counts_by_distro().unwrap(),
            BTreeMap::from([("Debian".into(), 1), ("Ubuntu".into(), 2)])
        );
    }

    #[test]
    fn terminal_count_view_fails_closed_at_the_per_distro_bound() {
        let sessions = (0..=crate::core::runtime_snapshot::MAX_TERMINALS_PER_DISTRO)
            .map(|index| (format!("s{index}"), test_session_handle()))
            .collect();
        let state = SessionState {
            sessions: Mutex::new(sessions),
            ..SessionState::new()
        };

        assert!(state.terminal_counts_by_distro().is_err());
    }

    struct DropProbe(Arc<std::sync::atomic::AtomicUsize>);

    impl Write for DropProbe {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Drop for DropProbe {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn matching_reader_cleanup_drops_resources_with_an_extra_handle_reference() {
        let drops = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let handle = Arc::new(Mutex::new(SessionHandle {
            distro: "Ubuntu".to_string(),
            writer: Box::new(DropProbe(drops.clone())),
            child: None,
            master: None,
            reader: None,
            attached: true,
        }));
        let retained_handle = handle.clone();
        let state = SessionState {
            sessions: Mutex::new(HashMap::from([(String::from("s1"), handle.clone())])),
            ..SessionState::new()
        };

        assert!(remove_session_if_handle(&state, "s1", &handle));
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        drop(retained_handle);
    }

    #[test]
    fn mismatched_reader_cleanup_returns_false_without_emitting_closed_event() {
        let current_handle = test_session_handle();
        let stale_handle = test_session_handle();
        let state = SessionState {
            sessions: Mutex::new(HashMap::from([(String::from("s1"), current_handle)])),
            ..SessionState::new()
        };
        let emitted = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let emitted_for_event = emitted.clone();

        let cleanup_won = remove_session_if_handle(&state, "s1", &stale_handle);
        emit_terminal_closed_if_cleanup_won(cleanup_won, || {
            emitted_for_event.store(true, Ordering::SeqCst);
        });

        assert!(!cleanup_won);
        assert!(!emitted.load(Ordering::SeqCst));
    }

    #[test]
    fn already_removed_reader_cleanup_returns_false_without_emitting_closed_event() {
        let handle = test_session_handle();
        let state = SessionState {
            sessions: Mutex::new(HashMap::from([(String::from("s1"), handle.clone())])),
            ..SessionState::new()
        };
        let emitted = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let emitted_for_event = emitted.clone();

        assert!(remove_session_if_handle(&state, "s1", &handle));
        let cleanup_won = remove_session_if_handle(&state, "s1", &handle);
        emit_terminal_closed_if_cleanup_won(cleanup_won, || {
            emitted_for_event.store(true, Ordering::SeqCst);
        });

        assert!(!cleanup_won);
        assert!(!emitted.load(Ordering::SeqCst));
    }

    #[test]
    fn reader_cleanup_removes_matching_session_once() {
        let handle = test_session_handle();
        let state = SessionState {
            sessions: Mutex::new(HashMap::from([(String::from("s1"), handle.clone())])),
            ..SessionState::new()
        };

        assert!(remove_session_if_handle(&state, "s1", &handle));
        assert!(!remove_session_if_handle(&state, "s1", &handle));
        assert!(state.sessions.lock().unwrap().is_empty());
    }

    #[test]
    fn reader_cleanup_preserves_replacement_with_a_mismatched_handle() {
        let current_handle = test_session_handle();
        let stale_handle = test_session_handle();
        let state = SessionState {
            sessions: Mutex::new(HashMap::from([(
                String::from("s1"),
                current_handle.clone(),
            )])),
            ..SessionState::new()
        };

        assert!(!remove_session_if_handle(&state, "s1", &stale_handle));
        let stored_handle = state.sessions.lock().unwrap().get("s1").cloned();
        assert!(stored_handle.is_some_and(|stored| Arc::ptr_eq(&stored, &current_handle)));
    }

    #[test]
    fn explicit_close_removal_is_idempotent() {
        let handle = test_session_handle();
        let state = SessionState {
            sessions: Mutex::new(HashMap::from([(String::from("s1"), handle)])),
            ..SessionState::new()
        };

        assert!(take_session(&state, "s1").is_some());
        assert!(take_session(&state, "s1").is_none());
    }

    #[test]
    fn attach_marks_session_attached_only_once() {
        let mut handle = SessionHandle {
            distro: "Ubuntu".to_string(),
            writer: Box::new(Vec::new()),
            child: None,
            master: None,
            reader: None,
            attached: false,
        };
        assert!(handle.mark_attached(), "first attach should succeed");
        assert!(!handle.mark_attached(), "second attach must be a no-op");
        assert!(!handle.mark_attached(), "third attach must also be a no-op");
    }
}
