use super::{connection::Connection, deploy, wsl};
use crate::core::{
    bridge::{self, Event},
    connection::INSTANCE,
    process::ShellError,
    settings, wsl as selection,
};
use std::process::Stdio;
use tauri::{AppHandle, Manager, ipc::InvokeResponseBody};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

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
    if let Err(e) = settings::save(
        &path,
        &settings::Settings {
            distro: Some(distro.clone()),
        },
    ) {
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
        let path = app
            .path()
            .resolve("devbox-linux", tauri::path::BaseDirectory::Resource)
            .map_err(|e| ShellError::Invalid(e.to_string()))?;
        let resource_dir = path
            .parent()
            .ok_or_else(|| ShellError::Invalid("내장 리소스 경로 없음".into()))?;
        crate::core::self_check::validate_resources(resource_dir, env!("CARGO_PKG_VERSION"))?;
        let bytes = std::fs::read(path)?;
        if bytes.is_empty() {
            return Err(ShellError::Invalid("내장 바이너리가 비어 있습니다".into()));
        }
        // The script validates the existing digest even when the package version is unchanged.
        deploy::install(&distro, bytes, cancel.clone()).await?;
        // Per-instance setup is independent of the shared binary and restarts only this instance.
        deploy::setup(&distro, INSTANCE, cancel.clone()).await
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
    loop {
        tokio::select! {
            result=&mut drive=>return (result,ready),
            event=incoming.recv()=>match event{
                Some(Event::Ready)=>{
                    let opened={let mut state=conn.state();state.writer=Some(writer.clone());state.open=true;state.bump();let frames=state.mux.reopen_all();frames.into_iter().try_for_each(|f|state.enqueue(f))};
                    if let Err(e)=opened{bridge_cancel.cancel();let _=drive.await;return (Err(ShellError::Invalid(e.to_string())),ready);}
                    ready=true;conn.gate(app,generation,"ready",None,None);conn.broadcast(app);
                }
                Some(Event::Frame(frame))=>{
                    // Only clone routing metadata while locked; IPC never runs inside the mutex.
                    let channel={let state=conn.state();state.mux.inbound(&frame).and_then(|label|state.channels.get(label)).map(|lease|lease.channel.clone())};
                    if let Some(channel)=channel && let Err(e)=channel.send(InvokeResponseBody::Raw(frame)){bridge_cancel.cancel();let _=drive.await;return (Err(ShellError::Invalid(e.to_string())),ready);}
                }
                None=>{bridge_cancel.cancel();let result=drive.await;return (result,ready);}
            }
        }
    }
}
