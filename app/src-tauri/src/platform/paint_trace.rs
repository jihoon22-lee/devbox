//! SC2 acceptance log lines. Both intervals use this process's monotonic `Instant` only, and each
//! includes the renderer's acknowledgement IPC latency, so the logged value is a conservative upper bound.
//! `run` is the app process ID, binding shortcut and cold-start samples to one process run.
use crate::core::{
    connection::IpcError,
    paint_trace::{FirstData, PaletteTraces, TraceEnd},
};
use serde::Deserialize;
use std::{sync::Mutex, time::Instant};
use tauri::{AppHandle, Manager, State, WebviewWindow};

pub struct PaintTrace {
    palette: Mutex<PaletteTraces>,
    first_data: Mutex<FirstData>,
}
impl PaintTrace {
    pub fn new(started: Instant) -> Self {
        Self {
            palette: Mutex::default(),
            first_data: Mutex::new(FirstData::new(started)),
        }
    }
}
pub fn palette_trace(app: &AppHandle, received: Instant) -> u64 {
    let state = app.state::<PaintTrace>();
    let mut traces = state.palette.lock().unwrap_or_else(|e| e.into_inner());
    let (trace, dropped) = traces.allocate(received);
    if let Some(dropped) = dropped {
        eprintln!(
            "palette-trace-dropped run={} trace={dropped}",
            std::process::id()
        );
    }
    trace
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PaletteOutcome {
    Painted,
    AlreadyOpen,
}
#[tauri::command]
pub fn palette_painted(
    window: WebviewWindow,
    state: State<'_, PaintTrace>,
    trace_id: u64,
    outcome: PaletteOutcome,
    deferred: bool,
) -> Result<(), IpcError> {
    super::preferences::main_only(&window)?;
    let now = Instant::now();
    let run = std::process::id();
    let outcome = match outcome {
        PaletteOutcome::Painted => "painted",
        PaletteOutcome::AlreadyOpen => "already-open",
    };
    // A deferred sample waited behind another dialog; the deferral is included and flagged.
    match state
        .palette
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .finish(trace_id, now)
    {
        TraceEnd::Elapsed(elapsed) => eprintln!(
            "palette-painted run={run} trace={trace_id} outcome={outcome} deferred={deferred} elapsed_ms={}",
            elapsed.as_millis()
        ),
        TraceEnd::Stale => {
            eprintln!("palette-painted run={run} trace={trace_id} outcome=stale ignored")
        }
        TraceEnd::Unknown => {
            eprintln!("palette-painted run={run} trace={trace_id} outcome=unknown ignored")
        }
    }
    Ok(())
}
#[tauri::command]
pub fn first_data_painted(
    window: WebviewWindow,
    state: State<'_, PaintTrace>,
) -> Result<(), IpcError> {
    super::preferences::main_only(&window)?;
    let now = Instant::now();
    if let Some(elapsed) = state
        .first_data
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .finish(now)
    {
        // A minimized autostart launch paints hidden; that sample is not a visible cold start.
        let visible = window.is_visible().unwrap_or(false);
        eprintln!(
            "first-data-painted run={} elapsed_ms={} visible={visible}",
            std::process::id(),
            elapsed.as_millis()
        );
    }
    Ok(())
}
