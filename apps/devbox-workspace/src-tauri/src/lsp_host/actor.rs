//! One owned OS thread keeps native LSP work and automatic retries off Tauri's
//! shared async executor. Retirement retains the thread until confirmed cleanup.
use super::{approval::Snapshot, documents, input};
use code_pad_lib::lsp::{
    LspEvent, LspManager, LspManagerError, ManagedInstaller, RequestCancellation,
};
use product_contract::ProjectContext;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::JoinHandle,
    time::Duration,
};
use tokio::sync::{mpsc, oneshot};
type Result<T> = std::result::Result<T, &'static str>;
pub(super) type Events = Arc<dyn Fn(&str, Value) + Send + Sync>;

tokio::task_local! {static START_REQUEST: (Arc<AtomicBool>,RequestCancellation,u64);}
pub(super) fn request_issue() -> Option<&'static str> {
    START_REQUEST
        .try_with(|(cancelled, owner, deadline)| {
            if cancelled.load(Ordering::Acquire) || owner.is_cancelled() {
                Some("lsp_operation_cancelled")
            } else {
                crate::files_host::current_deadline(*deadline).err()
            }
        })
        .ok()
        .flatten()
}
pub(super) fn request_cancelled() -> bool {
    request_issue().is_some()
}
pub(super) async fn start_scope<T>(
    cancelled: Arc<AtomicBool>,
    owner: RequestCancellation,
    deadline: u64,
    operation: impl std::future::Future<Output = T>,
) -> T {
    START_REQUEST
        .scope((cancelled, owner, deadline), operation)
        .await
}

pub(super) fn allowed(method: &str) -> bool {
    documents::allowed(method)
        || matches!(
            method,
            "start_language_server"
                | "restart_language_server"
                | "stop_language_server"
                | "stop_all_language_servers"
                | "language_server_statuses"
                | "language_server_logs"
        )
}
pub(super) fn starts(method: &str) -> bool {
    matches!(method, "start_language_server" | "restart_language_server")
}
pub(super) fn stops(method: &str) -> bool {
    matches!(
        method,
        "stop_language_server"
            | "stop_all_language_servers"
            | "cancel_lsp_rename"
            | "discard_lsp_rename"
    )
}

#[derive(Deserialize)]
#[serde(
    tag = "method",
    content = "args",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum Method {
    #[serde(skip)]
    Document(documents::Method),
    StartLanguageServer {
        language_id: String,
        operation_id: String,
    },
    RestartLanguageServer {
        language_id: String,
        operation_id: String,
    },
    StopLanguageServer {
        language_id: String,
        #[serde(default)]
        operation_id: Option<String>,
    },
    StopAllLanguageServers {
        #[serde(default)]
        operation_ids: Vec<String>,
    },
    LanguageServerStatuses {},
    LanguageServerLogs {},
}
impl Method {
    fn stop(&self) -> bool {
        matches!(self, Self::Document(method) if method.control())
            || matches!(
                self,
                Self::StopLanguageServer { .. } | Self::StopAllLanguageServers { .. }
            )
    }
    fn start(&self) -> bool {
        matches!(
            self,
            Self::StartLanguageServer { .. } | Self::RestartLanguageServer { .. }
        )
    }
    fn parse(method: &str, args: Value) -> Result<Self> {
        if documents::allowed(method) {
            return documents::Method::parse(method, args).map(Self::Document);
        }
        let value: Self = input(json!({"method":method,"args":args}))?;
        let language = match &value {
            Self::StartLanguageServer { language_id, .. }
            | Self::RestartLanguageServer { language_id, .. }
            | Self::StopLanguageServer { language_id, .. } => Some(language_id),
            _ => None,
        };
        if language.is_some_and(|language| {
            language.is_empty()
                || language.len() > 64
                || !language
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'+'))
        }) {
            return Err("invalid_request");
        }
        Ok(value)
    }
}
pub(super) fn admission(method: &str, args: Value) -> Result<(Option<String>, Vec<String>)> {
    match Method::parse(method, args)? {
        Method::StartLanguageServer { operation_id, .. }
        | Method::RestartLanguageServer { operation_id, .. } => Ok((Some(operation_id), vec![])),
        Method::StopLanguageServer { operation_id, .. } => {
            Ok((None, operation_id.into_iter().collect()))
        }
        Method::StopAllLanguageServers { operation_ids } if operation_ids.len() <= 64 => {
            Ok((None, operation_ids))
        }
        Method::Document(_) | Method::LanguageServerStatuses {} | Method::LanguageServerLogs {} => {
            Ok((None, vec![]))
        }
        _ => Err("invalid_request"),
    }
}
pub(super) fn idle(method: &str, args: Value) -> Result<Value> {
    match Method::parse(method, args)? {
        Method::LanguageServerStatuses {} | Method::LanguageServerLogs {} => Ok(json!([])),
        Method::StopAllLanguageServers { .. } | Method::StopLanguageServer { .. } => {
            Ok(Value::Null)
        }
        _ => Err("lsp_execution_approval_required"),
    }
}
pub(super) fn control_error(error: LspManagerError) -> &'static str {
    match error {
        LspManagerError::ExecutionApprovalRequired => "lsp_execution_approval_required",
        LspManagerError::ExecutionBusy => "lsp_busy",
        LspManagerError::AlreadyRunning(_) => "lsp_already_running",
        LspManagerError::StartInProgress(_) => "lsp_start_in_progress",
        LspManagerError::NotRunning(_) => "lsp_not_running",
        LspManagerError::Disabled => "lsp_disabled",
        LspManagerError::UnsupportedFeature { .. } => "lsp_feature_unsupported",
        _ => "lsp_unavailable",
    }
}
pub(super) fn value<T: serde::Serialize>(value: T) -> Result<Value> {
    serde_json::to_value(value).map_err(|_| "lsp_unavailable")
}
async fn execute(
    manager: &LspManager,
    documents: &tokio::sync::Mutex<documents::Documents>,
    method: Method,
    deadline: u64,
) -> Result<Value> {
    match method {
        Method::Document(documents::Method::CancelLspRename { plan_id }) => {
            value(manager.cancel_rename(&plan_id).await)
        }
        Method::Document(documents::Method::DiscardLspRename { plan_id }) => {
            value(manager.discard_rename(&plan_id).await)
        }
        Method::Document(method) => {
            let mut documents = documents.lock().await;
            crate::files_host::current_deadline(deadline)?;
            documents.execute(manager, method, deadline).await
        }
        Method::StartLanguageServer {
            language_id,
            operation_id,
        } => {
            let _ = operation_id;
            manager
                .start(&language_id)
                .await
                .map(|()| Value::Null)
                .map_err(control_error)
        }
        Method::RestartLanguageServer {
            language_id,
            operation_id,
        } => {
            let _ = operation_id;
            manager
                .restart(&language_id)
                .await
                .map(|()| Value::Null)
                .map_err(control_error)
        }
        Method::StopLanguageServer {
            language_id,
            operation_id,
        } => {
            let _ = operation_id;
            let result = manager.stop(&language_id).await;
            wait_for_starts(manager).await?;
            result.map(|()| Value::Null).map_err(control_error)
        }
        Method::StopAllLanguageServers { operation_ids } => {
            let _ = operation_ids;
            let result = manager.stop_all().await;
            wait_for_starts(manager).await?;
            result.map(|()| Value::Null).map_err(control_error)
        }
        Method::LanguageServerStatuses {} => value(manager.statuses().await),
        Method::LanguageServerLogs {} => value(manager.logs().await),
    }
}
async fn wait_for_starts(manager: &LspManager) -> Result<()> {
    tokio::time::timeout(
        Duration::from_secs(12),
        manager.wait_for_startup_retirement(),
    )
    .await
    .map_err(|_| "lsp_shutdown_unconfirmed")
}
struct Command {
    method: Method,
    deadline: u64,
    cancelled: Option<Arc<AtomicBool>>,
    reply: oneshot::Sender<Result<Value>>,
}
pub(super) struct Actor {
    context: ProjectContext,
    snapshot: Arc<Snapshot>,
    sender: mpsc::Sender<Command>,
    shutdown: RequestCancellation,
    confirmed: Arc<AtomicBool>,
    thread: Mutex<Option<JoinHandle<()>>>,
}
impl Actor {
    pub(super) fn spawn(
        events: Events,
        snapshot: Snapshot,
        installer: Arc<ManagedInstaller>,
        owner_shutdown: RequestCancellation,
        files: Arc<Mutex<crate::files_host::FilesHost>>,
    ) -> Result<Self> {
        let context = snapshot.context().clone();
        snapshot.bind_owner(owner_shutdown.clone());
        snapshot.bind_files(files.clone());
        let snapshot = Arc::new(snapshot);
        let (sender, receiver) = mpsc::channel(16);
        let shutdown = RequestCancellation::new();
        let confirmed = Arc::new(AtomicBool::new(false));
        let thread_snapshot = snapshot.clone();
        let thread_shutdown = shutdown.clone();
        let thread_confirmed = confirmed.clone();
        let thread = std::thread::Builder::new()
            .name("workspace-lsp".into())
            .spawn(move || {
                let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                else {
                    thread_confirmed.store(true, Ordering::Release);
                    return;
                };
                let _result = runtime.block_on(async move {
                    let manager = LspManager::with_reviewed_execution(
                        thread_snapshot.data_path(),
                        env!("CARGO_PKG_VERSION"),
                        installer,
                        thread_snapshot.clone(),
                        thread_snapshot.reviewed(),
                    )
                    .map_err(control_error)?;
                    run(
                        events,
                        Arc::new(manager),
                        thread_snapshot,
                        files,
                        receiver,
                        thread_shutdown,
                        owner_shutdown,
                    )
                    .await;
                    Ok::<_, &'static str>(())
                });
                drop(runtime);
                // A constructor error precedes any child creation. run() itself
                // returns only after manager and request retirement are confirmed.
                thread_confirmed.store(true, Ordering::Release);
            })
            .map_err(|_| "lsp_unavailable")?;
        Ok(Self {
            context,
            snapshot,
            sender,
            shutdown,
            confirmed,
            thread: Mutex::new(Some(thread)),
        })
    }
    pub(super) fn context(&self) -> &ProjectContext {
        &self.context
    }
    pub(super) fn uses_installation(&self, id: &str, version: &str) -> bool {
        self.snapshot.config().server_by_language.values().any(|server|matches!(server,code_pad_lib::lsp::ServerRef::Managed {manifest_id,version:configured,..} if manifest_id==id&&configured==version))
    }
    pub(super) async fn request(
        &self,
        method: &str,
        args: Value,
        deadline: u64,
        cancelled: Option<Arc<AtomicBool>>,
    ) -> Result<Value> {
        if self.shutdown.is_cancelled() {
            return Err("lsp_operation_cancelled");
        }
        let method = Method::parse(method, args)?;
        let (reply, result) = oneshot::channel();
        self.sender
            .try_send(Command {
                method,
                deadline,
                cancelled,
                reply,
            })
            .map_err(|_| "lsp_busy")?;
        result.await.map_err(|_| "lsp_unavailable")?
    }
    pub(super) async fn retire(&self) -> Result<()> {
        self.snapshot.active.store(false, Ordering::Release);
        self.shutdown.cancel();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            {
                let mut thread = self.thread.lock().map_err(|_| "lsp_shutdown_unconfirmed")?;
                if thread.as_ref().is_none_or(JoinHandle::is_finished) {
                    if let Some(thread) = thread.take() {
                        let _ = thread.join();
                    }
                    return if self.confirmed.load(Ordering::Acquire) {
                        Ok(())
                    } else {
                        Err("lsp_shutdown_unconfirmed")
                    };
                }
            }
            if tokio::time::Instant::now() >= deadline {
                return Err("lsp_shutdown_unconfirmed");
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}
impl Drop for Actor {
    fn drop(&mut self) {
        self.snapshot.active.store(false, Ordering::Release);
        self.shutdown.cancel();
    }
}

async fn run(
    events_sink: Events,
    manager: Arc<LspManager>,
    snapshot: Arc<Snapshot>,
    files: Arc<Mutex<crate::files_host::FilesHost>>,
    mut receiver: mpsc::Receiver<Command>,
    shutdown: RequestCancellation,
    owner_shutdown: RequestCancellation,
) {
    let documents = Arc::new(tokio::sync::Mutex::new(documents::Documents::new(
        files,
        snapshot.clone(),
    )));
    let mut events = manager.subscribe_events();
    let mut tasks = tokio::task::JoinSet::new();
    loop {
        tokio::select! {
            biased;
            _=shutdown.cancelled()=>break,
            _=owner_shutdown.cancelled()=>break,
            result=events.recv()=>{
                if !snapshot.active.load(Ordering::Acquire){continue;}
                let event=match result {
                    Ok(LspEvent::Diagnostics(payload))=>value(payload).map(|value|("lsp/diagnostics",value)),
                    Ok(LspEvent::Status(status))=>value(status).map(|value|("lsp/status",value)),
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_))=>continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed)=>break,
                };
                if let Ok((name,mut value))=event {value["nativeContext"]=json!(snapshot.context());events_sink(name,value);}
            },
            Some(command)=receiver.recv()=>{
                if command.reply.is_closed(){continue;}
                if let Err(issue)=crate::files_host::current_deadline(command.deadline){let _=command.reply.send(Err(issue));continue;}
                if tasks.len()>=8 && (!command.method.stop()||tasks.len()>=16){let _=command.reply.send(Err("lsp_busy"));continue;}
                let manager=manager.clone();
                let documents=documents.clone();
                let owner_shutdown=owner_shutdown.clone();
                tasks.spawn(async move {
                    let starting=command.method.start();
                    let operation=async move {
                        if let Some(issue)=request_issue(){return Err(issue);}
                        let result=execute(&manager,&documents,command.method,command.deadline).await;
                        if let Some(issue)=request_issue(){Err(issue)}else{result}
                    };
                    let result=if starting {if let Some(cancelled)=command.cancelled {start_scope(cancelled,owner_shutdown,command.deadline,operation).await}else{Err("invalid_request")}}else{operation.await};
                    let _=command.reply.send(result);
                });
            },
            _=tasks.join_next(),if !tasks.is_empty()=>{},
            else=>break,
        }
    }
    snapshot.active.store(false, Ordering::Release);
    receiver.close();
    while let Ok(command) = receiver.try_recv() {
        let _ = command.reply.send(Err("lsp_operation_cancelled"));
    }
    // Missing native completion keeps this owner alive. The caller can retry
    // retirement, and the product must not exit or replace this context yet.
    loop {
        if manager.shutdown_for_exit().await.is_ok() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    while tasks.join_next().await.is_some() {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lsp_host::{
        approval::{tests::Fixture, Approvals},
        settings::Settings,
    };
    use std::{
        fs,
        path::{Path, PathBuf},
    };
    fn node() -> PathBuf {
        let filename = if cfg!(windows) { "node.exe" } else { "node" };
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .map(|path| path.join(filename))
            .filter(|path| path.is_absolute() && crate::platform::windows_path::admit(path).is_ok())
            .find_map(|path| fs::canonicalize(path).ok().filter(|path| path.is_file()))
            .expect("Node used by the repository test environment must be available")
    }
    fn fixture(mode: &str) -> (Fixture, PathBuf) {
        let fixture = Fixture::new();
        let script = fixture.path().join("fixture.mjs");
        fs::write(
            &script,
            include_str!("../../../../../.github/fixtures/workspace-lsp-server.mjs"),
        )
        .unwrap();
        let marker = fixture.path().join("child.pid");
        let settings = Settings::open(&fixture.host, &fixture.context).unwrap();
        let mut view = settings.view().unwrap();
        let mut args = vec![marker.to_string_lossy().into_owned()];
        if !mode.is_empty() {
            args.push(mode.to_owned());
        }
        view["config"]["server_by_language"] = json!({});
        view["config"]["custom_servers"] = json!([{"language_ids":["rust"],"executable":script,"args":args,"runtime":{"kind":"node","executable":node(),"min_version":null},"source":"synthetic fixture","license":"UNLICENSED","version":"1"}]);
        settings.save(&fixture.host,json!({"config":view["config"],"nativeRevision":view["nativeRevision"],"recoverInvalid":false}),u64::MAX).unwrap();
        let mut approvals = Approvals::default();
        let preview = approvals.preview(fixture.capture(false).unwrap()).unwrap();
        approvals
            .approve(
                &fixture.context,
                preview["previewId"].as_str().unwrap(),
                u64::MAX,
            )
            .unwrap();
        (fixture, marker)
    }
    async fn child(marker: &Path) -> Child {
        let pid = tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                if let Ok(value) = fs::read_to_string(marker) {
                    if let Ok(pid) = value.parse::<u32>() {
                        break pid;
                    }
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("native fixture did not publish its PID");
        Child::open(pid)
    }
    #[cfg(unix)]
    struct Child(u32);
    #[cfg(unix)]
    impl Child {
        fn open(pid: u32) -> Self {
            assert!(Path::new("/proc").join(pid.to_string()).exists());
            Self(pid)
        }
        fn exited(&self) {
            assert!(!Path::new("/proc").join(self.0.to_string()).exists());
        }
    }
    #[cfg(windows)]
    struct Child(usize);
    #[cfg(windows)]
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut std::ffi::c_void;
        fn WaitForSingleObject(handle: *mut std::ffi::c_void, milliseconds: u32) -> u32;
        fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
    }
    #[cfg(windows)]
    impl Child {
        fn open(pid: u32) -> Self {
            // Retain only synchronization access to the exact fixture child.
            let handle = unsafe { OpenProcess(0x0010_0000, 0, pid) };
            assert!(!handle.is_null());
            assert_eq!(unsafe { WaitForSingleObject(handle, 0) }, 258);
            Self(handle as usize)
        }
        fn exited(&self) {
            assert_eq!(unsafe { WaitForSingleObject(self.0 as _, 0) }, 0);
        }
    }
    #[cfg(windows)]
    impl Drop for Child {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0 as _);
            }
        }
    }

    #[tokio::test]
    async fn owned_actor_documents_require_native_grants_and_saved_disk_revisions() {
        use code_pad_lib::commands::file::{OpenFileRequest, SaveFileRequest};
        let (fixture, marker) = fixture("documents");
        let path = fixture.path().join("main.rs");
        fs::write(&path, b"let value = 1;\r\n").unwrap();
        let lease = fixture
            .host
            .projects()
            .unwrap()
            .admit(&fixture.context)
            .unwrap();
        let mut owner = crate::file_owner::FileOwner::default();
        let opened = owner
            .open(
                Some((&fixture.context, &lease)),
                OpenFileRequest {
                    path: path.to_string_lossy().into_owned(),
                    encoding: None,
                },
            )
            .unwrap();
        let revision = owner.document_revision(&opened.path).unwrap();
        let notes_path = fixture.path().join("notes.txt");
        fs::write(&notes_path, b"let value = 1;\r\n").unwrap();
        let notes = owner
            .open(
                Some((&fixture.context, &lease)),
                OpenFileRequest {
                    path: notes_path.to_string_lossy().into_owned(),
                    encoding: None,
                },
            )
            .unwrap();
        let notes_revision = owner.document_revision(&notes.path).unwrap();
        let files = Arc::new(Mutex::new(
            crate::files_host::FilesHost::from_editor_fixture(owner),
        ));
        let actor = Actor::spawn(
            Arc::new(|_, _| {}),
            fixture.capture(true).unwrap(),
            fixture.installer.clone(),
            RequestCancellation::new(),
            files.clone(),
        )
        .unwrap();
        actor
            .request(
                "start_language_server",
                json!({"languageId":"rust","operationId":"documents-start"}),
                u64::MAX,
                Some(Arc::new(AtomicBool::new(false))),
            )
            .await
            .unwrap();
        let child = child(&marker).await;
        let request = |revision: &str| json!({"languageId":"rust","path":opened.path,"text":opened.text,"nativeRevision":revision});
        assert_eq!(
            actor
                .request("open_lsp_document", request("stale"), u64::MAX, None)
                .await
                .unwrap_err(),
            "file_snapshot_changed"
        );
        let unowned = fixture.path().join("unopened.rs");
        fs::write(&unowned, b"unopened").unwrap();
        let mut denied = request(&revision);
        denied["path"] = json!(unowned);
        assert_eq!(
            actor
                .request("open_lsp_document", denied, u64::MAX, None)
                .await
                .unwrap_err(),
            "file_selection_required"
        );
        let native = actor
            .request("open_lsp_document", request(&revision), u64::MAX, None)
            .await
            .unwrap();
        let uri = native["uri"].as_str().unwrap();
        let changed=actor.request("change_lsp_document",json!({"languageId":"rust","uri":uri,"text":"let value = 2;\n","dirty":false,"nativeRevision":revision}),u64::MAX,None).await.unwrap();
        assert_eq!(changed["version"], 2);
        let rename_args = json!({"languageId":"rust","uri":uri,"position":{"line":0,"character":4},"newName":"renamed"});
        assert!(actor
            .request("request_lsp_rename", rename_args.clone(), u64::MAX, None)
            .await
            .is_err());

        assert_eq!(
            actor
                .request(
                    "save_lsp_document",
                    json!({"languageId":"rust","uri":uri,"nativeRevision":revision}),
                    u64::MAX,
                    None
                )
                .await
                .unwrap_err(),
            "file_snapshot_changed"
        );
        assert_eq!(actor.request("reload_lsp_document",json!({"languageId":"rust","uri":uri,"text":"fake clean","nativeRevision":revision}),u64::MAX,None).await.unwrap_err(),"file_snapshot_changed");
        let hover = actor
            .request(
                "request_lsp_hover",
                json!({"languageId":"rust","uri":uri,"position":{"line":0,"character":1}}),
                u64::MAX,
                None,
            )
            .await
            .unwrap();
        assert!(!hover["stale"].as_bool().unwrap());
        // A newer editor acknowledgement can reach LSP while an earlier disk
        // save is still in progress. Publishing the save must retain that buffer.
        actor.request("change_lsp_document",json!({"languageId":"rust","uri":uri,"text":"let value = 99;\n","dirty":true,"nativeRevision":revision}),u64::MAX,None).await.unwrap();
        // The native Files owner commits and rotates its revision before didSave.
        let saved = {
            let mut files = files.lock().unwrap();
            files
                .execute_editor_save_fixture(
                    Some((&fixture.context, &lease)),
                    SaveFileRequest {
                        path: opened.path.clone(),
                        text: "let value = 2;\n".into(),
                        encoding: opened.encoding,
                        line_ending: opened.line_ending,
                        expected_mtime_nanos: opened.mtime_nanos.clone(),
                        expected_size: opened.size,
                        expected_content_hash: opened.content_hash.clone(),
                        source_lossy: false,
                    },
                )
                .unwrap()
        };
        assert_eq!(
            actor
                .request(
                    "save_lsp_document",
                    json!({"languageId":"rust","uri":uri,"nativeRevision":saved}),
                    u64::MAX,
                    None
                )
                .await
                .unwrap_err(),
            "file_snapshot_changed"
        );
        assert_eq!(actor.request("save_lsp_document",json!({"languageId":"rust","uri":uri,"nativeRevision":saved,"text":"forged saved text"}),u64::MAX,None).await.unwrap_err(),"file_snapshot_changed");
        // An editor mirror/session write may hold Files metadata after disk save.
        // Expired admission must not publish didSave; a live one waits for release.
        let (locked_tx, locked_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let held_files = files.clone();
        let holder = std::thread::spawn(move || {
            let _guard = held_files.lock().unwrap();
            locked_tx.send(()).unwrap();
            let _ = release_rx.recv_timeout(Duration::from_secs(5));
        });
        locked_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let save_args =
            json!({"languageId":"rust","uri":uri,"nativeRevision":saved,"text":"let value = 2;\n"});
        let deadline = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
            + 75;
        assert_eq!(
            actor
                .request("save_lsp_document", save_args.clone(), deadline, None)
                .await
                .unwrap_err(),
            "request_expired"
        );
        let pending_save = actor.request("save_lsp_document", save_args, u64::MAX, None);
        tokio::pin!(pending_save);
        assert!(
            tokio::time::timeout(Duration::from_millis(75), &mut pending_save)
                .await
                .is_err()
        );
        release_tx.send(()).unwrap();
        holder.join().unwrap();
        pending_save.await.unwrap();
        assert_eq!(
            files
                .lock()
                .unwrap()
                .guard_editor_write(&fixture.context, &opened.path),
            Err("lsp_dirty_editor_document")
        );
        actor.request("change_lsp_document",json!({"languageId":"rust","uri":uri,"text":"let value = 2;\n","dirty":false,"nativeRevision":saved}),u64::MAX,None).await.unwrap();
        assert_eq!(actor.request("change_lsp_document",json!({"languageId":"rust","uri":uri,"text":"old","dirty":true,"nativeRevision":revision}),u64::MAX,None).await.unwrap_err(),"file_snapshot_changed");
        actor
            .request(
                "pull_lsp_diagnostics",
                json!({"languageId":"rust","uri":uri}),
                u64::MAX,
                None,
            )
            .await
            .unwrap();
        files
            .lock()
            .unwrap()
            .sync_editor(
                &fixture.context,
                &notes.path,
                &notes_revision,
                "unsaved notes",
            )
            .unwrap();
        assert!(actor
            .request("request_lsp_rename", rename_args.clone(), u64::MAX, None)
            .await
            .is_err());
        files
            .lock()
            .unwrap()
            .sync_editor(&fixture.context, &notes.path, &notes_revision, &notes.text)
            .unwrap();
        let preview = actor
            .request("request_lsp_rename", rename_args.clone(), u64::MAX, None)
            .await
            .unwrap();
        assert_eq!(preview["files"].as_array().unwrap().len(), 2);
        files
            .lock()
            .unwrap()
            .sync_editor(
                &fixture.context,
                &notes.path,
                &notes_revision,
                "edited after preview",
            )
            .unwrap();
        let blocked = actor
            .request(
                "apply_lsp_rename",
                json!({"planId":preview["planId"]}),
                u64::MAX,
                None,
            )
            .await
            .unwrap();
        assert_eq!(blocked["success"], false);
        assert_eq!(fs::read(&path).unwrap(), b"let value = 2;\r\n");
        assert_eq!(fs::read(&notes_path).unwrap(), b"let value = 1;\r\n");
        files
            .lock()
            .unwrap()
            .sync_editor(&fixture.context, &notes.path, &notes_revision, &notes.text)
            .unwrap();
        let preview = actor
            .request("request_lsp_rename", rename_args, u64::MAX, None)
            .await
            .unwrap();
        let applied = actor
            .request(
                "apply_lsp_rename",
                json!({"planId":preview["planId"]}),
                u64::MAX,
                None,
            )
            .await
            .unwrap();
        assert_eq!(applied["success"], true, "{applied}");
        assert_eq!(applied["documents"].as_array().unwrap().len(), 2);
        for file in applied["files"].as_array().unwrap() {
            assert!(file["nativeRevision"].as_str().is_some());
        }
        assert_eq!(fs::read(&path).unwrap(), b"let renamed = 2;\r\n");
        assert_eq!(fs::read(&notes_path).unwrap(), b"let renamed = 1;\r\n");
        assert!(files
            .lock()
            .unwrap()
            .sync_editor(
                &fixture.context,
                &notes.path,
                &notes_revision,
                "stale notes"
            )
            .is_err());
        actor
            .request(
                "request_lsp_hover",
                json!({"languageId":"rust","uri":uri,"position":{"line":0,"character":4}}),
                u64::MAX,
                None,
            )
            .await
            .unwrap();
        assert!(actor
            .request(
                "apply_lsp_rename",
                json!({"planId":preview["planId"]}),
                u64::MAX,
                None
            )
            .await
            .is_err());
        actor
            .request(
                "close_lsp_document",
                json!({"languageId":"rust","uri":uri}),
                u64::MAX,
                None,
            )
            .await
            .unwrap();
        assert_eq!(
            actor
                .request(
                    "request_lsp_hover",
                    json!({"languageId":"rust","uri":uri,"position":{"line":0,"character":1}}),
                    u64::MAX,
                    None
                )
                .await
                .unwrap_err(),
            "lsp_document_denied"
        );
        actor.retire().await.unwrap();
        child.exited();
    }

    #[tokio::test]
    async fn owned_actor_emits_context_and_retires_the_actual_server_before_joining() {
        let (fixture, marker) = fixture("");
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = events.clone();
        let actor = Actor::spawn(
            Arc::new(move |name, value| sink.lock().unwrap().push((name.to_owned(), value))),
            fixture.capture(true).unwrap(),
            fixture.installer.clone(),
            RequestCancellation::new(),
            Default::default(),
        )
        .unwrap();
        assert_eq!(
            actor
                .request(
                    "start_language_server",
                    json!({"languageId":"rust","operationId":"expired-fixture"}),
                    0,
                    Some(Arc::new(AtomicBool::new(false)))
                )
                .await
                .unwrap_err(),
            "request_expired"
        );
        assert!(!marker.exists());
        actor
            .request(
                "start_language_server",
                json!({"languageId":"rust","operationId":"start-fixture"}),
                u64::MAX,
                Some(Arc::new(AtomicBool::new(false))),
            )
            .await
            .unwrap();
        let child = child(&marker).await;
        let statuses = actor
            .request("language_server_statuses", json!({}), u64::MAX, None)
            .await
            .unwrap();
        assert_eq!(statuses.as_array().unwrap().len(), 1);
        assert!(events
            .lock()
            .unwrap()
            .iter()
            .any(|(name, value)| name == "lsp/status"
                && value["nativeContext"] == json!(fixture.context)));
        actor.retire().await.unwrap();
        child.exited();
        assert!(actor
            .request(
                "start_language_server",
                json!({"languageId":"rust","operationId":"retired"}),
                u64::MAX,
                Some(Arc::new(AtomicBool::new(false)))
            )
            .await
            .is_err());
    }
    #[tokio::test]
    async fn cancelled_initialization_cleans_up_before_the_start_request_returns() {
        let (fixture, marker) = fixture("hang-initialize");
        let actor = Arc::new(
            Actor::spawn(
                Arc::new(|_, _| {}),
                fixture.capture(true).unwrap(),
                fixture.installer.clone(),
                RequestCancellation::new(),
                Default::default(),
            )
            .unwrap(),
        );
        let cancelled = Arc::new(AtomicBool::new(false));
        let request_actor = actor.clone();
        let request_cancelled = cancelled.clone();
        let start = tokio::spawn(async move {
            request_actor
                .request(
                    "start_language_server",
                    json!({"languageId":"rust","operationId":"cancel-fixture"}),
                    u64::MAX,
                    Some(request_cancelled),
                )
                .await
        });
        let child = child(&marker).await;
        cancelled.store(true, Ordering::Release);
        let result = tokio::time::timeout(Duration::from_secs(5), start)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(result.unwrap_err(), "lsp_operation_cancelled");
        child.exited();
        actor.retire().await.unwrap();
    }
}
