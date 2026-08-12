use crate::lifecycle::{complete_system_shutdown, RuntimeState};
use std::sync::Arc;
use tauri::AppHandle;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{WM_ENDSESSION, WM_NCDESTROY, WM_QUERYENDSESSION};

const SESSION_END_SUBCLASS_ID: usize = 0x4456_4258;

struct SessionEndContext {
    state: Arc<RuntimeState>,
    pending: bool,
}

pub fn install_session_end_hook(
    window: &tauri::WebviewWindow,
    _app: &AppHandle,
    state: Arc<RuntimeState>,
) -> Result<(), String> {
    let hwnd = window.hwnd().map_err(|error| error.to_string())?;
    let context = Box::into_raw(Box::new(SessionEndContext {
        state,
        pending: false,
    }));

    let installed = unsafe {
        SetWindowSubclass(
            hwnd,
            Some(session_end_proc),
            SESSION_END_SUBCLASS_ID,
            context as usize,
        )
    };
    if installed.as_bool() {
        Ok(())
    } else {
        unsafe {
            drop(Box::from_raw(context));
        }
        Err("failed to install the Windows session-end hook".to_string())
    }
}

unsafe extern "system" fn session_end_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _subclass_id: usize,
    context: usize,
) -> LRESULT {
    match message {
        WM_QUERYENDSESSION => {
            let context = &mut *(context as *mut SessionEndContext);
            // Another application may cancel the session end. A query records
            // intent but must not stop the scheduler yet.
            context.pending = true;
            LRESULT(1)
        }
        WM_ENDSESSION if wparam.0 != 0 => {
            let context = &mut *(context as *mut SessionEndContext);
            let _was_queried = context.pending;
            context.pending = false;
            // Complete cleanup before tao handles WM_ENDSESSION(TRUE) by
            // destroying the Tauri event loop.
            complete_system_shutdown(&context.state);
            DefSubclassProc(hwnd, message, wparam, lparam)
        }
        WM_ENDSESSION => {
            let context = &mut *(context as *mut SessionEndContext);
            context.pending = false;
            DefSubclassProc(hwnd, message, wparam, lparam)
        }
        WM_NCDESTROY => {
            let _ = RemoveWindowSubclass(hwnd, Some(session_end_proc), SESSION_END_SUBCLASS_ID);
            drop(Box::from_raw(context as *mut SessionEndContext));
            DefSubclassProc(hwnd, message, wparam, lparam)
        }
        _ => DefSubclassProc(hwnd, message, wparam, lparam),
    }
}
