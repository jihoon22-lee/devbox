//! Native-only Windows execution messages. App0 replies never reach a renderer.
use devbox_protocol::{
    frame::{FrameKind, HEADER_LEN, frame_channel},
    secrets::NATIVE_CHANNEL,
    windows::{
        MAX_WINDOWS_PROBE_BYTES, MAX_WINDOWS_REQUEST_BYTES, POWERSHELL_BOOTSTRAP, RunWindows,
        WindowsEvent, WindowsProbeReply, WindowsProbeRequest, valid_windows_identity,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

pub fn powershell_args() -> [&'static str; 4] {
    [
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        POWERSHELL_BOOTSTRAP,
    ]
}

#[derive(Serialize)]
struct Request<'a, T> {
    #[serde(rename = "type")]
    kind: &'static str,
    id: u32,
    method: &'static str,
    params: &'a T,
}

fn request(id: u32, method: &'static str, params: &impl Serialize) -> Vec<u8> {
    // 8 KiB output byte arrays need at most four JSON bytes per input byte.
    let mut frame = Vec::with_capacity(64 * 1024);
    frame.resize(HEADER_LEN, 0);
    serde_json::to_writer(
        &mut frame,
        &Request {
            kind: "request",
            id,
            method,
            params,
        },
    )
    .expect("native Windows DTO serialization");
    let length = (frame.len() - 4) as u32;
    frame[..4].copy_from_slice(&length.to_le_bytes());
    frame[4] = FrameKind::Json as u8;
    frame[5..9].copy_from_slice(&NATIVE_CHANNEL.to_le_bytes());
    frame
}

pub fn next_request() -> Vec<u8> {
    #[derive(Serialize)]
    struct Empty {}
    request(100, "windows.next", &Empty {})
}

pub const PROBE_NEXT_ID: u32 = 99;
pub const PROBE_OUTPUT_ID: u32 = 98;
pub fn probe_next_request() -> Vec<u8> {
    #[derive(Serialize)]
    struct Empty {}
    request(PROBE_NEXT_ID, "windows.probe_next", &Empty {})
}
pub fn probe_output_request(reply: &WindowsProbeReply) -> Vec<u8> {
    request(PROBE_OUTPUT_ID, "windows.probe_output", reply)
}
pub fn output_request(id: u32, event: &WindowsEvent) -> Vec<u8> {
    request(id, "windows.output", event)
}

pub fn finish_run(
    runs: &mut std::collections::HashMap<(String, String), tokio_util::sync::CancellationToken>,
    key: &(String, String),
) {
    if let Some(cancel) = runs.remove(key) {
        cancel.cancel();
    }
}

pub fn receive(frame: &[u8]) -> Option<RunWindows> {
    let request: RunWindows =
        serde_json::from_str(receive_result(frame, 100, MAX_WINDOWS_REQUEST_BYTES)?.get()).ok()?;
    request.tracked().then_some(request)
}
pub fn receive_probe(frame: &[u8]) -> Option<WindowsProbeRequest> {
    // The daemon caps the serialized probe itself at1MiB; envelope/id/header add <1KiB.
    let request: WindowsProbeRequest = serde_json::from_str(
        receive_result(frame, PROBE_NEXT_ID, MAX_WINDOWS_PROBE_BYTES + 1024)?.get(),
    )
    .ok()?;
    (valid_windows_identity(&request.probe_id) && request.probe.valid()).then_some(request)
}
fn receive_result(frame: &[u8], id: u32, limit: usize) -> Option<&RawValue> {
    if frame.len() < HEADER_LEN
        || frame.len() > limit
        || frame_channel(frame) != Some(NATIVE_CHANNEL)
        || frame[4] != FrameKind::Json as u8
        || u32::from_le_bytes(frame[..4].try_into().ok()?) as usize != frame.len() - 4
    {
        return None;
    }
    // Borrow the envelope directly, avoiding a secret-bearing RawValue/String copy.
    #[derive(Deserialize)]
    struct Response<'a> {
        #[serde(rename = "type")]
        kind: &'a str,
        id: u32,
        #[serde(borrow)]
        result: Option<&'a RawValue>,
        #[serde(borrow)]
        error: Option<&'a RawValue>,
    }
    let response: Response<'_> = serde_json::from_slice(&frame[HEADER_LEN..]).ok()?;
    if response.kind != "response" || response.id != id || response.error.is_some() {
        return None;
    }
    response.result
}

#[cfg(all(test, not(windows)))]
#[path = "../platform/windows.rs"]
pub(crate) mod platform_compile;

#[cfg(test)]
#[path = "windows_tests.rs"]
mod tests;
