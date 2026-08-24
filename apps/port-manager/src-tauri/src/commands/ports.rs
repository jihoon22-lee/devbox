#[cfg(target_os = "windows")]
use std::process::Command;

use devbox_process::{parse_netstat_output, PortInfo, ProcessSnapshot};
use serde::Serialize;
use sysinfo::{Pid, System};
use tauri_plugin_opener::OpenerExt;

/// 프로세스 상세 정보 (v2 상세 패널용)
#[derive(Debug, Clone, Serialize)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub exe: Option<String>,
    pub start_time: u64,
    pub memory_bytes: u64,
}

/// 포트 + 소유 프로세스명을 묶어 프론트에 전달하는 행 구조체
#[derive(Debug, Clone, Serialize)]
pub struct PortRow {
    #[serde(flatten)]
    pub port: PortInfo,
    pub process_name: Option<String>,
}

/// `netstat`로 현재 열린 포트 목록을 조회한다.
///
/// Windows: `netstat -ano` (PID 포함). 파싱은 공용 process crate가 담당.
/// 그 외 OS: 빈 목록을 반환한다 (이 앱의 타깃은 Windows이며, 타 OS는 컴파일 확인용).
#[tauri::command]
pub fn list_ports() -> Result<Vec<PortRow>, String> {
    let output = run_netstat()?;
    let ports = parse_netstat_output(&output);

    // PID → 프로세스명 매핑을 위한 스냅샷
    let process_snapshot = ProcessSnapshot::new();
    let rows = ports
        .into_iter()
        .map(|port| {
            let process_name = port
                .pid
                .and_then(|pid| process_snapshot.lookup(pid).map(|summary| summary.name));
            PortRow { port, process_name }
        })
        .collect();

    Ok(rows)
}

#[cfg(target_os = "windows")]
fn run_netstat() -> Result<String, String> {
    use std::os::windows::process::CommandExt;

    let mut cmd = Command::new("netstat");
    cmd.args(["-ano"]);
    // 콘솔 프로그램이 새 창을 띄우는 것을 방지 (CREATE_NO_WINDOW)
    cmd.creation_flags(0x0800_0000);
    let output = cmd
        .output()
        .map_err(|e| format!("netstat 실행 실패: {e}"))?;
    // 한국어 Windows에서는 netstat 출력이 OEM 코드페이지(예: CP949)라
    // strict UTF-8 디코딩이 실패할 수 있다. 데이터 행은 ASCII이므로 lossy로 충분하다.
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(not(target_os = "windows"))]
fn run_netstat() -> Result<String, String> {
    // Windows 전용 앱이므로 다른 OS에서는 빈 결과. core 파서는 어디서든 테스트 가능.
    Ok(String::new())
}

/// PID에 해당하는 프로세스를 종료한다.
/// 관리자 권한이 필요한 프로세스는 실패 메시지를 반환한다.
#[tauri::command]
pub fn kill_process(pid: u32) -> Result<(), String> {
    let system = System::new_all();
    let process = system
        .process(Pid::from_u32(pid))
        .ok_or_else(|| format!("PID {pid} 프로세스를 찾을 수 없습니다."))?;

    if process.kill() {
        Ok(())
    } else {
        Err(format!(
            "PID {pid} 프로세스 종료에 실패했습니다. 관리자 권한이 필요한 프로세스일 수 있습니다."
        ))
    }
}

/// PID의 프로세스 상세 정보를 조회한다.
#[tauri::command]
pub fn get_process_info(pid: u32) -> Result<ProcessInfo, String> {
    let system = System::new_all();
    let process = system
        .process(Pid::from_u32(pid))
        .ok_or_else(|| format!("PID {pid} 프로세스를 찾을 수 없습니다."))?;

    Ok(ProcessInfo {
        pid,
        name: process.name().to_string_lossy().into_owned(),
        exe: process.exe().map(|p| p.to_string_lossy().into_owned()),
        start_time: process.start_time(),
        memory_bytes: process.memory(),
    })
}

/// PID를 실행 시점에 다시 조회하고 해당 실행 파일만 탐색기에 표시한다.
/// 프론트엔드가 임의의 파일 경로를 opener에 전달하지 못하게 PID만 입력으로 받는다.
#[tauri::command]
pub async fn reveal_process(app: tauri::AppHandle, pid: u32) -> Result<(), String> {
    let system = System::new_all();
    let process = system
        .process(Pid::from_u32(pid))
        .ok_or_else(|| format!("PID {pid} 프로세스를 찾을 수 없습니다."))?;
    let executable = process
        .exe()
        .ok_or_else(|| format!("PID {pid} 프로세스의 실행 파일 경로를 확인할 수 없습니다."))?;

    app.opener()
        .reveal_item_in_dir(executable)
        .map_err(|e| format!("실행 파일을 탐색기에서 열지 못했습니다: {e}"))
}

/// 기본 브라우저로 URL을 연다.
#[tauri::command]
pub async fn open_browser(app: tauri::AppHandle, url: String) -> Result<(), String> {
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| format!("브라우저 열기 실패: {e}"))
}
