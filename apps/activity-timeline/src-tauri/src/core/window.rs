//! 포그라운드 창 감지. Windows에서만 실제 동작하고, 그 외 OS에서는 None을 반환한다.
//! (이 앱의 타깃은 Windows이며, 다른 OS는 컴파일·테스트 용도)

#[cfg(target_os = "windows")]
mod imp {
    use sysinfo::{Pid, System};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
    };

    /// 현재 포그라운드 창의 (프로세스명, 창 제목)을 반환한다. 감지 실패 시 None.
    pub fn foreground_window() -> Option<(String, String)> {
        unsafe {
            let hwnd: HWND = GetForegroundWindow();
            if hwnd.is_invalid() {
                return None;
            }
            let mut buf = [0u16; 512];
            let len = GetWindowTextW(hwnd, &mut buf);
            let title = String::from_utf16_lossy(&buf[..len as usize])
                .trim()
                .to_string();

            let mut pid: u32 = 0;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            let app = process_name(pid);

            if title.is_empty() && app.is_empty() {
                return None;
            }
            Some((app, title))
        }
    }

    fn process_name(pid: u32) -> String {
        let system = System::new_all();
        system
            .process(Pid::from_u32(pid))
            .map(|p| p.name().to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

#[cfg(not(target_os = "windows"))]
mod imp {
    pub fn foreground_window() -> Option<(String, String)> {
        None
    }
}

pub use imp::foreground_window;
