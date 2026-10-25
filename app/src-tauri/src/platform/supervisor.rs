use super::{connection::Connection, deploy, wsl};
use crate::core::{
    bridge::{self, Event},
    connection::INSTANCE,
    process::ShellError,
    secrets::{self, NativeSecrets},
    settings, wsl as selection,
};
use std::process::Stdio;
use tauri::{AppHandle, Manager, ipc::InvokeResponseBody};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[derive(Default)]
struct NativeQueue(std::collections::VecDeque<Vec<u8>>);
impl Drop for NativeQueue {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        for frame in &mut self.0 {
            frame.zeroize();
        }
    }
}

pub async fn run(
    app: &AppHandle,
    conn: &Connection,
    generation: u64,
    forced: Option<String>,
    cancel: CancellationToken,
) {
    conn.gate(app, generation, "checking", None, None);
    let list = match wsl::distros() {
        Ok(list) => list
            .into_iter()
            .filter(|d| !selection::is_internal(&d.name))
            .collect::<Vec<_>>(),
        Err(e) => {
            conn.gate(app, generation, "setupFailed", Some(e.to_string()), None);
            return;
        }
    };
    if list.is_empty() {
        conn.gate(app, generation, "noWsl", None, None);
        return;
    }
    let path = match app.path().app_local_data_dir() {
        Ok(p) => p.join("settings.json"),
        Err(e) => {
            conn.gate(app, generation, "setupFailed", Some(e.to_string()), None);
            return;
        }
    };
    let saved = match settings::load(&path) {
        Ok(s) => s,
        Err(e) => {
            conn.gate(
                app,
                generation,
                "setupFailed",
                Some(format!("설정을 읽지 못했습니다: {e}")),
                None,
            );
            return;
        }
    };
    let chosen = forced.or_else(|| selection::pick(&list, saved.distro.as_deref()));
    let Some(distro) = chosen else {
        conn.gate(app, generation, "chooseDistro", None, Some(list));
        return;
    };
    if let Err(e) = selection::validate_choice(&list, &distro) {
        let state = if list.iter().any(|d| d.name == distro && d.version == 1) {
            "wsl1"
        } else {
            "chooseDistro"
        };
        conn.gate(
            app,
            generation,
            state,
            Some(if state == "wsl1" {
                distro.clone()
            } else {
                format!("{distro}: {e}")
            }),
            Some(list),
        );
        return;
    }
    if let Err(e) = settings::update(&path, |settings| settings.distro = Some(distro.clone())) {
        conn.gate(
            app,
            generation,
            "setupFailed",
            Some(format!("설정을 저장하지 못했습니다: {e}")),
            None,
        );
        return;
    }
    if let Err(e) = wsl::systemd(&distro, cancel.clone()).await {
        if !cancel.is_cancelled() {
            conn.gate(
                app,
                generation,
                if matches!(e, ShellError::SystemdOff) {
                    "systemdOff"
                } else {
                    "setupFailed"
                },
                Some(if matches!(e, ShellError::SystemdOff) {
                    distro.clone()
                } else {
                    e.to_string()
                }),
                None,
            );
        }
        return;
    }
    conn.gate(app, generation, "deploying", None, None);
    let setup = async {
        let _guard = deploy::maintenance_wait(cancel.clone()).await?;
        let bytes = deploy::bundled_binary(app)?;
        deploy::apply(&distro, INSTANCE, bytes, cancel.clone()).await
    }
    .await;
    if let Err(e) = setup {
        if !cancel.is_cancelled() {
            conn.gate(app, generation, "setupFailed", Some(e.to_string()), None);
        }
        return;
    }
    let mut backoff = devbox_mux::Backoff::default();
    loop {
        if cancel.is_cancelled() {
            return;
        }
        conn.gate(app, generation, "connecting", None, None);
        let (result, was_ready) = bridge_once(app, conn, generation, &distro, cancel.clone()).await;
        if conn.state().close_generation(generation) {
            conn.broadcast(app);
        }
        if cancel.is_cancelled() {
            return;
        }
        if was_ready {
            backoff.reset();
        }
        if let Err(e) = result {
            conn.gate(app, generation, "bridgeFailed", Some(e.to_string()), None);
        }
        tokio::select! {_=cancel.cancelled()=>return,_=tokio::time::sleep(backoff.next_delay())=>{}}
    }
}
async fn bridge_once(
    app: &AppHandle,
    conn: &Connection,
    generation: u64,
    distro: &str,
    cancel: CancellationToken,
) -> (Result<(), ShellError>, bool) {
    let mut c = wsl::command(distro);
    c.args([
        "--exec",
        "env",
        &format!("DEVBOX_INSTANCE={INSTANCE}"),
        "sh",
        "-c",
        &crate::core::deploy::exec_script(),
        "sh",
        "bridge",
    ])
    .kill_on_drop(true)
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    let mut child = match c.spawn() {
        Ok(c) => c,
        Err(e) => return (Err(e.into()), false),
    };
    let _job = match super::job::Job::for_child(&child) {
        Ok(j) => j,
        Err(e) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return (Err(e.into()), false);
        }
    };
    let (writer, writes) = mpsc::channel(16);
    let (events, mut incoming) = mpsc::channel(16);
    let bridge_cancel = cancel.child_token();
    {
        let mut state = conn.state();
        state.cancel = bridge_cancel.clone();
    }
    let drive = bridge::drive(&mut child, writes, events, bridge_cancel.clone());
    tokio::pin!(drive);
    let mut ready = false;
    let mut native = NativeSecrets::default();
    let protector = super::secrets::CurrentUser;
    let (windows_output, mut windows_events) =
        mpsc::channel::<devbox_protocol::windows::WindowsEvent>(16);
    let mut windows_tasks = tokio::task::JoinSet::new();
    let mut windows_waiting = false;
    let (probe_output, mut probe_events) =
        mpsc::channel::<devbox_protocol::windows::WindowsProbeReply>(1);
    let mut probe_waiting = false;
    let mut probe_active: Option<String> = None;
    let mut windows_runs = std::collections::HashMap::<(String, String), CancellationToken>::new();
    let mut native_pending = NativeQueue::default();
    let mut windows_request_id = 101_u32;
    let mut windows_ack: Option<(u32, (String, String), bool)> = None;
    loop {
        tokio::select! {
            result=&mut drive=>return (result,ready),
            Some(_) = windows_tasks.join_next(), if !windows_tasks.is_empty() => {}
            permit = writer.reserve(), if !native_pending.0.is_empty() => {
                match permit {
                    Ok(permit) => permit.send(native_pending.0.pop_front().expect("queued native frame")),
                    Err(_) => {bridge_cancel.cancel();}
                }
            }
            Some(reply) = probe_events.recv(), if native_pending.0.len() < 8 => {
                if probe_active.as_deref()==Some(reply.probe_id.as_str()) {
                    native_pending.0.push_back(crate::core::windows::probe_output_request(&reply));
                }
            }
            Some(event) = windows_events.recv(), if native_pending.0.len() < 8 && windows_ack.is_none() => {
                let finished = matches!(event.event, devbox_protocol::windows::WindowsOutput::Finished{..}|devbox_protocol::windows::WindowsOutput::Failed);
                let key=(event.run_id.clone(),event.attempt_id.clone());
                if !windows_runs.contains_key(&key) {continue;}
                windows_ack = Some((windows_request_id,key,finished));
                native_pending.0.push_back(crate::core::windows::output_request(windows_request_id, &event));
                windows_request_id = match windows_request_id.checked_add(1) { Some(id) => id, None => {bridge_cancel.cancel();return (Err(ShellError::Invalid("Windows 실행 채널을 다시 연결합니다".into())),ready);} };

            }
            event=incoming.recv()=>match event{
                Some(Event::Ready)=>{
                    let opened={let mut state=conn.state();state.active_distro=Some(distro.into());state.writer=Some(writer.clone());state.open=true;state.bump();state.enqueue_reopens()};
                    if let Err(e)=opened{bridge_cancel.cancel();let _=drive.await;return (Err(ShellError::Invalid(e.to_string())),ready);}
                    if let Err(error) = writer.try_send(native.start(INSTANCE)) {
                        use zeroize::Zeroize;
                        error.into_inner().zeroize();
                        native.abort();
                    }
                    ready=true;conn.gate(app,generation,"ready",None,None);conn.broadcast(app);
                }
                Some(Event::Frame(mut frame))=>{
                    if secrets::is_native(&frame) {
                        use zeroize::Zeroize;
                        if let Some(request)=crate::core::windows::receive_probe(&frame) {
                            probe_waiting=false;
                            if probe_active.is_some() {frame.zeroize();bridge_cancel.cancel();continue;}
                            probe_active=Some(request.probe_id.clone());
                            windows_tasks.spawn(super::windows::run_probe(request,probe_output.clone(),bridge_cancel.child_token()));
                            frame.zeroize();continue;
                        }
                        if let Some(request) = crate::core::windows::receive(&frame) {
                            windows_waiting = false;
                            if windows_runs.len() >= 4 {frame.zeroize();bridge_cancel.cancel();continue;}
                            let token=bridge_cancel.child_token();
                            windows_runs.insert((request.run_id.clone(),request.attempt_id.clone()),token.clone());
                            windows_tasks.spawn(super::windows::run_windows(request,windows_output.clone(),token));
                            if windows_runs.len() < 4 {
                                native_pending.0.push_back(crate::core::windows::next_request());
                                windows_waiting = true;
                            }
                            frame.zeroize();
                            continue;
                        }
                        #[derive(serde::Deserialize)]
                        struct Meta<'a> {
                            #[serde(rename="type")] kind: &'a str,
                            id: Option<u32>,
                            #[serde(borrow)] error: Option<&'a serde_json::value::RawValue>,
                        }
                        let meta = serde_json::from_slice::<Meta>(&frame[devbox_protocol::frame::HEADER_LEN..]).ok();
                        let welcome = meta.as_ref().is_some_and(|m| m.kind == "welcome");
                        if let Some(meta)=&meta && meta.kind=="response" && meta.id==Some(crate::core::windows::PROBE_OUTPUT_ID) && probe_active.is_some() {
                            probe_active=None;
                            if !probe_waiting {native_pending.0.push_back(crate::core::windows::probe_next_request());probe_waiting=true;}
                        }
                        if let Some(meta) = &meta && meta.kind == "response" && meta.id == windows_ack.as_ref().map(|a|a.0) && windows_ack.is_some() {
                            let (_,key,finished)=windows_ack.take().expect("pending output ACK");
                            if finished || meta.error.is_some() {
                                crate::core::windows::finish_run(&mut windows_runs,&key);
                                if !windows_waiting {native_pending.0.push_back(crate::core::windows::next_request());windows_waiting=true;}
                            }
                        }

                        if let Some(outgoing) = native.receive(&frame, &protector) { native_pending.0.push_back(outgoing); }
                        if welcome && !probe_waiting {
                            native_pending.0.push_back(crate::core::windows::probe_next_request());probe_waiting=true;
                        }
                        if welcome && !windows_waiting {
                            native_pending.0.push_back(crate::core::windows::next_request());
                            windows_waiting = true;
                        }
                        frame.zeroize();
                        continue;
                    }
                    // Only clone routing metadata while locked; IPC never runs inside the mutex.
                    let channel={let state=conn.state();state.mux.inbound(&frame).and_then(|label|state.channels.get(label)).map(|lease|lease.channel.clone())};
                    if let Some(channel)=channel && let Err(e)=channel.send(InvokeResponseBody::Raw(frame)){bridge_cancel.cancel();let _=drive.await;return (Err(ShellError::Invalid(e.to_string())),ready);}
                }
                None=>{bridge_cancel.cancel();let result=drive.await;return (result,ready);}
            }
        }
    }
}
