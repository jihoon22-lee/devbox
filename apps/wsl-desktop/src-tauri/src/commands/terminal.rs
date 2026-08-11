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

/// WSL 배포판 목록
#[tauri::command]
pub fn list_distros() -> Result<Vec<String>, String> {
    #[cfg(target_os = "windows")]
    {
        let output = std::process::Command::new("wsl.exe")
            .args(["-l", "-v"])
            .output()
            .map_err(|e| e.to_string())?;
        Ok(parse_distros(&decode_output(&output.stdout)))
    }
    #[cfg(not(target_os = "windows"))]
    {
        // 개발(리눅스)에서는 기본값만
        Ok(vec!["Ubuntu".to_string()])
    }
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
    let writer = pair.master.take_writer().map_err(|e| e.to_string())?;

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

/// 모든 세션에 동일한 입력을 전달한다 (동시 명령).
#[tauri::command]
pub fn broadcast(state: tauri::State<'_, Arc<SessionState>>, data: String) -> Result<(), String> {
    let sessions = state.sessions.lock().unwrap();
    for h in sessions.values() {
        let mut h = h.lock().unwrap();
        let _ = h.writer.write_all(data.as_bytes());
        let _ = h.writer.flush();
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

/// `wsl -l -v` 출력에서 배포판 이름을 추출한다.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn parse_distros(text: &str) -> Vec<String> {
    text.lines()
        .map(|l| l.trim().trim_start_matches('*').trim())
        .filter(|l| !l.is_empty() && !l.starts_with("NAME") && !l.starts_with("Windows"))
        .filter_map(|l| l.split_whitespace().next().map(|s| s.to_string()))
        .collect()
}

/// 파이프된 wsl.exe 출력은 UTF-16LE(BOM 또는 NUL 다량)로 나올 수 있어 디코딩한다.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn decode_output(bytes: &[u8]) -> String {
    let bom = bytes.starts_with(&[0xFF, 0xFE]);
    let null_ratio = if bytes.is_empty() {
        0
    } else {
        bytes.iter().filter(|b| **b == 0).count() * 4 / bytes.len()
    };
    if bom || (bytes.len() >= 2 && null_ratio >= 1) {
        let start = if bom { 2 } else { 0 };
        let units: Vec<u16> = bytes[start..]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&units)
            .trim_end_matches('\0')
            .to_string()
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_distro_names() {
        let text = "  NAME      STATE           VERSION\n* Ubuntu    Running         2\n  docker-desktop Running     2\n";
        assert_eq!(parse_distros(text), vec!["Ubuntu", "docker-desktop"]);
    }

    #[test]
    fn decodes_utf16le() {
        let mut bytes = vec![0xFF, 0xFE];
        for ch in "Ubuntu".encode_utf16() {
            bytes.extend_from_slice(&ch.to_le_bytes());
        }
        assert_eq!(decode_output(&bytes), "Ubuntu");
    }

    #[test]
    fn keeps_utf8() {
        assert_eq!(decode_output(b"Ubuntu\n"), "Ubuntu\n");
    }
}
