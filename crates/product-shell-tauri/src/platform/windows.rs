//! Keep bare Alt/F10 from entering an empty native menu loop on a WebView host.
use crate::core::window_menu::ignore_empty_menu_key;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    GetMenu, GetWindowThreadProcessId, WM_NCDESTROY, WM_SYSCOMMAND,
};

const MENU_GUARD_ID: usize = 0x4442_4d4b;

pub(crate) fn install_menu_guard(window: &tauri::WebviewWindow) -> Result<(), &'static str> {
    let hwnd = window.hwnd().map_err(|_| "window_menu_guard_unavailable")?;
    install(hwnd)
}

fn install(hwnd: HWND) -> Result<(), &'static str> {
    // SetWindowSubclass must run on the thread that owns the window. Product
    // setup runs there, before restored geometry can activate another product.
    unsafe {
        if GetWindowThreadProcessId(hwnd, None) != GetCurrentThreadId()
            || !SetWindowSubclass(hwnd, Some(menu_guard), MENU_GUARD_ID, 0).as_bool()
        {
            return Err("window_menu_guard_unavailable");
        }
    }
    Ok(())
}

unsafe extern "system" fn menu_guard(
    hwnd: HWND,
    message: u32,
    command: WPARAM,
    detail: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    if message == WM_NCDESTROY {
        let _ = RemoveWindowSubclass(hwnd, Some(menu_guard), MENU_GUARD_ID);
    }
    if message == WM_SYSCOMMAND
        && ignore_empty_menu_key(message, command.0, detail.0, !GetMenu(hwnd).0.is_null())
    {
        return LRESULT(0);
    }
    DefSubclassProc(hwnd, message, command, detail)
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::core::w;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateMenu, CreateWindowExW, DestroyMenu, DestroyWindow, SendMessageW, SetMenu, SC_CLOSE,
        SC_KEYMENU, SC_MINIMIZE, SC_RESTORE, WINDOW_EX_STYLE, WS_OVERLAPPEDWINDOW,
    };

    struct TestWindow(HWND);
    impl Drop for TestWindow {
        fn drop(&mut self) {
            unsafe {
                let _ = DestroyWindow(self.0);
            }
        }
    }
    unsafe extern "system" fn downstream(
        hwnd: HWND,
        message: u32,
        command: WPARAM,
        detail: LPARAM,
        _id: usize,
        _data: usize,
    ) -> LRESULT {
        if message == WM_SYSCOMMAND {
            return LRESULT(77);
        }
        DefSubclassProc(hwnd, message, command, detail)
    }

    #[test]
    fn native_subclass_stops_only_the_empty_menu_loop_and_preserves_dispatch() {
        unsafe {
            let window = TestWindow(
                CreateWindowExW(
                    WINDOW_EX_STYLE::default(),
                    w!("STATIC"),
                    w!("Devbox menu guard fixture"),
                    WS_OVERLAPPEDWINDOW,
                    0,
                    0,
                    100,
                    100,
                    None,
                    None,
                    None,
                    None,
                )
                .unwrap(),
            );
            let raw = window.0 .0 as usize;
            assert_eq!(
                std::thread::spawn(move || install(HWND(raw as *mut _)))
                    .join()
                    .unwrap(),
                Err("window_menu_guard_unavailable")
            );
            assert!(SetWindowSubclass(window.0, Some(downstream), MENU_GUARD_ID + 1, 0).as_bool());
            install(window.0).unwrap();
            for bits in 0..16 {
                assert_eq!(
                    SendMessageW(
                        window.0,
                        WM_SYSCOMMAND,
                        Some(WPARAM((SC_KEYMENU | bits) as usize)),
                        Some(LPARAM(0))
                    ),
                    LRESULT(0)
                );
            }
            for (command, detail) in [
                (SC_KEYMENU, 32),
                (SC_KEYMENU, 'f' as isize),
                (SC_CLOSE, 0),
                (SC_MINIMIZE, 0),
                (SC_RESTORE, 0),
            ] {
                assert_eq!(
                    SendMessageW(
                        window.0,
                        WM_SYSCOMMAND,
                        Some(WPARAM(command as usize)),
                        Some(LPARAM(detail))
                    ),
                    LRESULT(77)
                );
            }
            let menu = CreateMenu().unwrap();
            SetMenu(window.0, Some(menu)).unwrap();
            assert_eq!(
                SendMessageW(
                    window.0,
                    WM_SYSCOMMAND,
                    Some(WPARAM(SC_KEYMENU as usize)),
                    Some(LPARAM(0))
                ),
                LRESULT(77)
            );
            SetMenu(window.0, None).unwrap();
            DestroyMenu(menu).unwrap();
        }
    }
}
