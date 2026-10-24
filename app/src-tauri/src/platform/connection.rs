use crate::core::connection::{GateStatus, INSTANCE, IpcError, Notice, State};
use std::sync::{Arc, Mutex};
use tauri::{
    AppHandle, Emitter, Manager, Window,
    ipc::{Channel, InvokeBody, InvokeResponseBody, Request},
};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

pub type FrameChannel = Channel<InvokeResponseBody>;
struct Shared {
    state: Mutex<State<FrameChannel>>,
    attempt: crate::core::maintenance::AttemptControl,
    retry: watch::Sender<Option<String>>,
    stop: CancellationToken,
    task: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    exiting: crate::core::shutdown::Shutdown,
}
#[derive(Clone)]
pub struct Connection(Arc<Shared>);
impl Default for Connection {
    fn default() -> Self {
        let (tx, _) = watch::channel(None);
        Self(Arc::new(Shared {
            state: Mutex::new(State::default()),
            attempt: crate::core::maintenance::AttemptControl::default(),
            retry: tx,
            stop: CancellationToken::new(),
            task: Mutex::new(None),
            exiting: crate::core::shutdown::Shutdown::default(),
        }))
    }
}
impl Connection {
    pub(super) fn operation_cancel(&self) -> CancellationToken {
        self.0.stop.child_token()
    }
    pub(super) async fn pause_for_cleanup(&self) -> Result<(), IpcError> {
        self.0
            .attempt
            .pause(std::time::Duration::from_secs(15))
            .await
            .map_err(|_| IpcError::new("연결을 닫지 못해 WSL 정리를 실행하지 않았습니다"))?;
        if self.0.stop.is_cancelled() {
            return Err(IpcError::new(
                "앱 종료 중에는 WSL 정리를 시작할 수 없습니다",
            ));
        }
        Ok(())
    }

    pub(super) fn state(&self) -> std::sync::MutexGuard<'_, State<FrameChannel>> {
        self.0.state.lock().expect("connection state")
    }
    pub(super) fn gate(
        &self,
        app: &AppHandle,
        generation: u64,
        state: &'static str,
        detail: Option<String>,
        distros: Option<Vec<crate::core::wsl::Distro>>,
    ) {
        let gate = {
            let mut s = self.state();
            if s.generation != generation {
                return;
            }
            s.bump();
            if state != "ready" {
                s.active_distro = None;
            }
            s.gate = GateStatus {
                state,
                detail,
                distros,
                instance: INSTANCE,
                revision: s.revision.to_string(),
            };
            s.gate.clone()
        };
        let _ = app.emit("devbox://gate", gate);
    }
    pub(super) fn broadcast(&self, app: &AppHandle) {
        let notices: Vec<_> = {
            let s = self.state();
            s.channels
                .iter()
                .map(|(label, lease)| (label.clone(), s.notice(&lease.id)))
                .collect()
        };
        for (label, notice) in notices {
            let _ = app.emit_to(label, "devbox://transport", notice);
        }
    }
    pub fn detach_window(&self, app: &AppHandle, label: &str) {
        self.state().detach(label, None);
        self.broadcast(app);
    }
    pub fn begin_exit(&self, app: AppHandle) -> bool {
        use crate::core::shutdown::ExitAction;
        match self.0.exiting.request() {
            ExitAction::Allow => return false,
            ExitAction::Wait => return true,
            ExitAction::StartCleanup => {}
        }
        self.0.stop.cancel();
        self.state().cancel.cancel();
        let task = self.0.task.lock().expect("supervisor task").take();
        let connection = self.clone();
        tauri::async_runtime::spawn(async move {
            if let Some(task) = task {
                let _ = task.await;
            }
            connection.0.exiting.completed();
            app.exit(0);
        });
        true
    }
}
pub fn start(app: AppHandle) {
    let connection = app.state::<Connection>().inner().clone();
    let owned = connection.clone();
    let task = tauri::async_runtime::spawn(async move {
        supervise(app, owned).await;
    });
    *connection.0.task.lock().expect("supervisor task") = Some(task);
}
async fn supervise(app: AppHandle, conn: Connection) {
    crate::core::supervision::run(
        conn.0.retry.subscribe(),
        conn.0.stop.clone(),
        move |generation, forced, cancel| {
            let app = app.clone();
            let conn = conn.clone();
            async move {
                let Some(_completed) = conn.0.attempt.begin(cancel.clone()) else {
                    return;
                };
                conn.state().generation = generation;
                super::supervisor::run(&app, &conn, generation, forced, cancel).await;
            }
        },
    )
    .await;
}
#[tauri::command]
pub fn gate_status(conn: tauri::State<'_, Connection>) -> GateStatus {
    conn.state().gate.clone()
}
#[tauri::command]
pub fn gate_retry(
    distro: Option<String>,
    conn: tauri::State<'_, Connection>,
) -> Result<(), IpcError> {
    if distro
        .as_ref()
        .is_some_and(|s| s.len() > 256 || s.contains('\0'))
    {
        return Err(IpcError::new("잘못된 배포판 이름"));
    }
    let _guard = super::deploy::maintenance_lock()?;
    conn.0.attempt.resume();
    conn.0.retry.send_replace(distro);
    Ok(())
}
#[tauri::command]
pub fn rpc_attach(
    app: AppHandle,
    window: Window,
    on_frame: FrameChannel,
    attachment_id: String,
    conn: tauri::State<'_, Connection>,
) -> Result<Notice, IpcError> {
    let result = conn
        .state()
        .attach(window.label().into(), attachment_id, on_frame);
    conn.broadcast(&app);
    result
}
#[tauri::command]
pub fn rpc_send(
    app: AppHandle,
    window: Window,
    request: Request<'_>,
    conn: tauri::State<'_, Connection>,
) -> Result<(), IpcError> {
    let id = request
        .headers()
        .get("x-devbox-attachment")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| IpcError::new("연결 ID 없음"))?;
    let InvokeBody::Raw(bytes) = request.body() else {
        return Err(IpcError::new("바이너리 프레임이 필요합니다"));
    };
    let result = conn.state().send(window.label(), id, bytes.clone());
    if result.is_err() {
        conn.broadcast(&app);
    }
    result
}
#[tauri::command]
pub fn rpc_detach(
    app: AppHandle,
    window: Window,
    attachment_id: String,
    conn: tauri::State<'_, Connection>,
) {
    conn.state().detach(window.label(), Some(&attachment_id));
    conn.broadcast(&app);
}
