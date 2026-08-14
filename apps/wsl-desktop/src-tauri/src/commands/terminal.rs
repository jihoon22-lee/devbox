use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde::Serialize;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use tauri::Emitter;

/// 실행 중인 터미널 세션 저장소
pub struct SessionState {
    pub sessions: Mutex<HashMap<String, Arc<Mutex<SessionHandle>>>>,
}

/// PTY 세션 하나
pub struct SessionHandle {
    pub distro: String,
    pub writer: Box<dyn Write + Send>,
    pub child: Option<Box<dyn portable_pty::Child + Send + Sync>>,
    /// ConPTY(HPCON)를 보관하는 master. drop 되면 ConPTY가 닫히므로
    /// 세션 수명 동안 유지해야 한다 (일찍 닫으면 자식이 0xc0000142로 실패).
    /// v0.2.2에서는 보관 용도로만 썼지만, 이제 `resize_session`이
    /// `MasterPty::resize()`를 호출하는 데도 쓰인다.
    pub master: Option<Box<dyn portable_pty::MasterPty>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionInfo {
    pub id: String,
    pub distro: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TerminalOutput {
    pub session_id: String,
    pub data: String,
}

/// 새 WSL 터미널 세션을 시작한다. `cwd`가 있으면 해당 경로로 열린다.
#[tauri::command]
pub fn start_session(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<SessionState>>,
    distro: String,
    cwd: Option<String>,
) -> Result<String, String> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: 30,
            cols: 100,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| e.to_string())?;

    let mut cmd = CommandBuilder::new("wsl.exe");
    cmd.args(["-d".to_string(), distro.clone()]);
    if let Some(dir) = cwd.filter(|c| !c.is_empty()) {
        // 지정 경로로 바로 열기: wsl -d <distro> -- bash -lc "cd <dir> && exec bash"
        cmd.args([
            "--".to_string(),
            "bash".to_string(),
            "-lc".to_string(),
            format!("cd '{dir}' && exec bash"),
        ]);
    }

    let child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
    drop(pair.slave);

    let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
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

    let session_id = format!(
        "s{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    );

    let handle = Arc::new(Mutex::new(SessionHandle {
        distro: distro.clone(),
        writer,
        child: Some(child),
        master: Some(master),
    }));
    state
        .sessions
        .lock()
        .unwrap()
        .insert(session_id.clone(), handle.clone());

    // PTY 출력 → 프론트 이벤트
    let app_out = app.clone();
    let sid = session_id.clone();
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let data = String::from_utf8_lossy(&buf[..n]).into_owned();
                    let _ = app_out.emit(
                        "terminal-output",
                        TerminalOutput {
                            session_id: sid.clone(),
                            data,
                        },
                    );
                }
            }
        }
        let _ = app_out.emit(
            "terminal-closed",
            TerminalOutput {
                session_id: sid.clone(),
                data: String::new(),
            },
        );
    });

    Ok(session_id)
}

/// 세션에 키 입력을 전달한다.
#[tauri::command]
pub fn write_session(
    state: tauri::State<'_, Arc<SessionState>>,
    session_id: String,
    data: String,
) -> Result<(), String> {
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
#[tauri::command]
pub fn broadcast(
    state: tauri::State<'_, Arc<SessionState>>,
    session_ids: Vec<String>,
    data: String,
) -> Result<(), String> {
    let sessions = state.sessions.lock().unwrap();
    for id in &session_ids {
        if let Some(h) = sessions.get(id) {
            let mut h = h.lock().unwrap();
            let _ = h.writer.write_all(data.as_bytes());
            let _ = h.writer.flush();
        }
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
    let mut sessions = state.sessions.lock().unwrap();
    if let Some(h) = sessions.remove(&session_id) {
        let mut h = h.lock().unwrap();
        if let Some(mut child) = h.child.take() {
            let _ = child.kill();
        }
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
