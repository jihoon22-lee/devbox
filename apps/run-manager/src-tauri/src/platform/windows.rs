use crate::lifecycle::{complete_system_shutdown, RuntimeState};
use std::fs;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::sync::Arc;
use tauri::AppHandle;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Storage::FileSystem::{ReplaceFileW, REPLACEFILE_WRITE_THROUGH};
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{WM_ENDSESSION, WM_NCDESTROY, WM_QUERYENDSESSION};

/// Replace the manifest in one filesystem operation. `std::fs::rename` does
/// not replace an existing destination on Windows, while `ReplaceFileW` does
/// and preserves the crash-safe temp-file + atomic-swap contract.
pub(crate) fn replace_file_atomic(replacement: &Path, destination: &Path) -> io::Result<()> {
    let replacement_wide = wide_path(replacement);
    let destination_wide = wide_path(destination);

    // ReplaceFileW requires an existing destination. The first manifest is a
    // create-only rename; if a racing writer creates the destination, retrying
    // with ReplaceFileW below preserves the replacement contract.
    if !destination.exists() {
        match fs::rename(replacement, destination) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() != io::ErrorKind::AlreadyExists => return Err(error),
            Err(_) => {}
        }
    }

    unsafe {
        ReplaceFileW(
            PCWSTR::from_raw(destination_wide.as_ptr()),
            PCWSTR::from_raw(replacement_wide.as_ptr()),
            PCWSTR::null(),
            REPLACEFILE_WRITE_THROUGH,
            None,
            None,
        )
    }
    .map_err(|error| io::Error::other(error.to_string()))
}

fn wide_path(path: &Path) -> Vec<u16> {
    path.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

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
