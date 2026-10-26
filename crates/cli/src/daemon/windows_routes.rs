use super::state::Services;
use devbox_protocol::{CommonError, Ctx, DispatchError, StreamOpened, windows::*};
use serde_json::value::RawValue;
use std::sync::Arc;
use zeroize::Zeroize;
pub(super) fn internal(method: &str) -> bool {
    matches!(
        method,
        "windows.prepare"
            | "windows.run"
            | "windows.ready"
            | "windows.next"
            | "windows.output"
            | "windows.probe_next"
            | "windows.probe_output"
    )
}
pub(super) fn native(method: &str) -> bool {
    matches!(
        method,
        "windows.next" | "windows.output" | "windows.probe_next" | "windows.probe_output"
    )
}
fn fail() -> DispatchError {
    DispatchError::common(CommonError::Internal)
}
fn decode<T: serde::de::DeserializeOwned>(raw: &RawValue) -> Result<T, DispatchError> {
    serde_json::from_str(raw.get()).map_err(|_| fail())
}
fn encode<T: serde::Serialize>(value: &T) -> Result<Box<RawValue>, DispatchError> {
    serde_json::value::to_raw_value(value).map_err(|_| fail())
}
fn wipe(raw: Box<RawValue>) {
    let text: Box<str> = raw.into();
    let mut text = String::from(text);
    text.zeroize();
}
pub(super) async fn dispatch(
    s: Arc<Services>,
    method: &str,
    ctx: Ctx,
    generation: u64,
    raw: Box<RawValue>,
) -> Result<Box<RawValue>, DispatchError> {
    match method {
        "windows.next" => encode(
            &s.windows
                .next(generation, ctx.cancel)
                .await
                .map_err(|_| fail())?,
        ),
        "windows.probe_next" => encode(
            &s.windows
                .probe_next(generation, ctx.cancel)
                .await
                .map_err(|_| fail())?,
        ),
        "windows.probe_output" => {
            if raw.get().len() > 16 * 1024 {
                return Err(fail());
            }
            s.windows
                .probe_output(generation, decode::<WindowsProbeReply>(&raw)?)
                .map_err(|_| fail())?;
            encode(&devbox_protocol::Done {
                rev: s.db.rev().to_string(),
            })
        }
        "windows.output" => {
            let event = decode::<WindowsEvent>(&raw);
            wipe(raw);
            let event = event?;
            s.windows
                .output(generation, event)
                .await
                .map_err(|_| fail())?;
            encode(&devbox_protocol::Done {
                rev: s.db.rev().to_string(),
            })
        }
        "windows.prepare" => {
            let id: RunIdentity = decode(&raw)?;
            let prepared = s.runs.prepare_windows(&id).await.map_err(|_| fail())?;
            s.runs
                .clone()
                .monitor_windows(id, ctx.cancel)
                .await
                .map_err(|_| fail())?;
            encode(&prepared)
        }
        "windows.ready" => {
            let id: RunIdentity = decode(&raw)?;
            // A not-yet-owned listener is a typed reply; only failures map to Internal.
            encode(&s.runs.windows_ready(&id).await.map_err(|_| fail())?)
        }
        "windows.run" => {
            if raw.get().len() > MAX_WINDOWS_REQUEST_BYTES {
                wipe(raw);
                return Err(fail());
            }
            let request = decode::<RunWindows>(&raw);
            wipe(raw);
            let request = request?;
            let identity = RunIdentity {
                run_id: request.run_id.clone(),
                attempt_id: request.attempt_id.clone(),
            };
            let expected = s
                .runs
                .prepare_windows(&identity)
                .await
                .map_err(|_| fail())?;
            if request.command != expected.request.command
                || request.cwd != expected.request.cwd
                || request.ports != expected.request.ports
                || request.observation != expected.request.observation
                || !request.tracked()
            {
                return Err(fail());
            }
            s.runs
                .clone()
                .monitor_windows(identity.clone(), ctx.cancel.clone())
                .await
                .map_err(|_| fail())?;
            s.runs
                .claim_windows_native(&identity)
                .await
                .map_err(|_| fail())?;
            let (mut events, native_cancel) = s
                .windows
                .submit(request, ctx.cancel.clone())
                .await
                .map_err(|_| fail())?;
            let ends = ctx.open_stream().map_err(|_| fail())?;
            let id = ends.id;
            tokio::spawn(async move {
                loop {
                    let event = tokio::select! {_=ctx.cancel.cancelled()=>break,_=native_cancel.cancelled()=>break,event=events.recv()=>match event{Some(e)=>e,None=>break}};
                    let finished = matches!(
                        event.event,
                        WindowsOutput::Finished { .. } | WindowsOutput::Failed
                    );
                    let bytes = zeroize::Zeroizing::new(
                        serde_json::to_vec(&event).expect("Windows output DTO"),
                    );
                    let write = async {
                        ends.tx.write(&(bytes.len() as u32).to_le_bytes()).await?;
                        ends.tx.write(&bytes).await
                    };
                    if !matches!(
                        tokio::time::timeout(std::time::Duration::from_secs(10), write).await,
                        Ok(Ok(()))
                    ) {
                        break;
                    }
                    if finished {
                        break;
                    }
                }
                ends.tx.finish().await;
            });
            encode(&StreamOpened { stream_id: id })
        }
        _ => Err(fail()),
    }
}
