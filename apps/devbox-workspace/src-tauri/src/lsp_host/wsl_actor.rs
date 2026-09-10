//! Blocking Windows owner for the selected distro's native LSP runtime. Each
//! native startup ticket keeps its Windows permits until native acknowledgement.
use super::{actor::Events, approval::Activities, wsl_approval::Snapshot};
use crate::{core::context_activity::ContextPermit, files_host::FilesHost};
use code_pad_lib::lsp::RequestCancellation;
use product_contract::ProjectContext;
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};
use workspace_wsl::lsp_wire::{
    Command as Method, DocumentMethod, DocumentProof, LifecycleMethod, ProofRequest, Reply,
};
type Result<T> = std::result::Result<T, &'static str>;
struct Request {
    method: String,
    args: Value,
    deadline: u64,
    cancelled: Arc<AtomicBool>,
    reply: tokio::sync::oneshot::Sender<Result<Value>>,
}
#[derive(Clone)]
struct Binding {
    path: String,
    revision: String,
}
pub(super) struct Actor {
    context: ProjectContext,
    sender: mpsc::SyncSender<Request>,
    shutdown: Arc<AtomicBool>,
    confirmed: Arc<AtomicBool>,
    active: Arc<Mutex<Option<Arc<AtomicBool>>>>,
    thread: Mutex<Option<thread::JoinHandle<()>>>,
}
impl Actor {
    pub(super) fn spawn(
        events: Events,
        snapshot: Snapshot,
        activities: Activities,
        files: Arc<Mutex<FilesHost>>,
        owner_shutdown: RequestCancellation,
    ) -> Result<Self> {
        let context = snapshot.context().clone();
        let (sender, receiver) = mpsc::sync_channel::<Request>(32);
        let shutdown = Arc::new(AtomicBool::new(false));
        let confirmed = Arc::new(AtomicBool::new(false));
        let active = Arc::new(Mutex::new(None));
        let thread_shutdown = shutdown.clone();
        let thread_confirmed = confirmed.clone();
        let thread_active = active.clone();
        let thread = thread::Builder::new()
            .name("workspace-wsl-lsp".into())
            .spawn(move || {
                let mut state = State {
                    snapshot,
                    activities,
                    files,
                    events,
                    bindings: BTreeMap::new(),
                    shutdown: thread_shutdown,
                    owner_shutdown,
                };
                while !state.stopping() {
                    let request = match receiver.recv_timeout(Duration::from_millis(50)) {
                        Ok(request) => Some(request),
                        Err(mpsc::RecvTimeoutError::Timeout) => None,
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    };
                    if let Some(request) = request {
                        if request.reply.is_closed() {
                            continue;
                        }
                        if let Ok(mut active) = thread_active.lock() {
                            *active = Some(request.cancelled.clone());
                        }
                        let result = state.call(
                            &request.method,
                            request.args,
                            request.deadline,
                            request.cancelled,
                        );
                        if let Ok(mut active) = thread_active.lock() {
                            *active = None;
                        }
                        let _ = request.reply.send(result);
                    } else {
                        let cancelled = Arc::new(AtomicBool::new(false));
                        if let Ok(mut active) = thread_active.lock() {
                            *active = Some(cancelled.clone());
                        }
                        let _ = state.call("lsp_poll", json!({}), deadline(), cancelled);
                        if let Ok(mut active) = thread_active.lock() {
                            *active = None;
                        }
                    }
                }
                while let Ok(request) = receiver.try_recv() {
                    let _ = request.reply.send(Err("lsp_operation_cancelled"));
                }
                state.retire();
                drop(state);
                thread_confirmed.store(true, Ordering::Release);
            })
            .map_err(|_| "lsp_unavailable")?;
        Ok(Self {
            context,
            sender,
            shutdown,
            confirmed,
            active,
            thread: Mutex::new(Some(thread)),
        })
    }
    pub(super) fn context(&self) -> &ProjectContext {
        &self.context
    }
    pub(super) fn finished(&self) -> bool {
        self.confirmed.load(Ordering::Acquire)
    }
    pub(super) async fn request(
        &self,
        method: &str,
        args: Value,
        deadline: u64,
        cancelled: Option<Arc<AtomicBool>>,
    ) -> Result<Value> {
        Method::parse(method, args.clone())?;
        if self.shutdown.load(Ordering::Acquire) {
            return Err("lsp_operation_cancelled");
        }
        let (reply, result) = tokio::sync::oneshot::channel();
        self.sender
            .try_send(Request {
                method: method.into(),
                args,
                deadline,
                cancelled: cancelled.unwrap_or_else(|| Arc::new(AtomicBool::new(false))),
                reply,
            })
            .map_err(|error| match error {
                mpsc::TrySendError::Full(_) => "lsp_busy",
                mpsc::TrySendError::Disconnected(_) => "lsp_unavailable",
            })?;
        result.await.map_err(|_| "lsp_unavailable")?
    }
    fn cancel(&self) {
        self.shutdown.store(true, Ordering::Release);
        if let Ok(active) = self.active.lock() {
            if let Some(active) = active.as_ref() {
                active.store(true, Ordering::Release);
            }
        }
    }
    pub(super) async fn retire(&self) -> Result<()> {
        self.cancel();
        let until = Instant::now() + Duration::from_secs(12);
        while !self.confirmed.load(Ordering::Acquire) {
            if Instant::now() >= until {
                return Err("lsp_shutdown_unconfirmed");
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        if let Some(thread) = self
            .thread
            .lock()
            .map_err(|_| "lsp_shutdown_unconfirmed")?
            .take()
        {
            thread.join().map_err(|_| "lsp_shutdown_unconfirmed")?;
        }
        Ok(())
    }
}
impl Drop for Actor {
    fn drop(&mut self) {
        self.cancel();
    }
}
pub(super) fn deadline() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX - 29000)) as u64
        + 29000
}
#[derive(Default)]
struct PreparedDocument {
    proof: Option<DocumentProof>,
    _permit: Option<(ContextPermit, ContextPermit)>,
}
struct State {
    snapshot: Snapshot,
    activities: Activities,
    files: Arc<Mutex<FilesHost>>,
    events: Events,
    bindings: BTreeMap<(String, String), Binding>,
    shutdown: Arc<AtomicBool>,
    owner_shutdown: RequestCancellation,
}
impl State {
    fn stopping(&self) -> bool {
        self.shutdown.load(Ordering::Acquire) || self.owner_shutdown.is_cancelled()
    }
    fn check(&self, deadline: u64, cancelled: &AtomicBool) -> Result<()> {
        if self.stopping() || cancelled.load(Ordering::Acquire) {
            return Err("lsp_operation_cancelled");
        }
        crate::files_host::current_deadline(deadline)
    }
    fn retire(&mut self) {
        self.shutdown.store(true, Ordering::Release);
        while self.snapshot.shutdown().is_err() {
            thread::sleep(Duration::from_millis(50));
        }
    }
    fn prepare_document(
        &self,
        method: &Method,
        deadline: u64,
        cancelled: &AtomicBool,
    ) -> Result<PreparedDocument> {
        let Some(method) = method.document() else {
            return Ok(PreparedDocument::default());
        };
        if method.control()
            || matches!(
                method,
                DocumentMethod::CloseLspDocument { .. }
                    | DocumentMethod::RequestLspRename { .. }
                    | DocumentMethod::ApplyLspRename { .. }
            )
        {
            return Ok(PreparedDocument::default());
        }
        let (path, revision, verify_disk) = if let DocumentMethod::OpenLspDocument {
            path,
            native_revision,
            ..
        } = method
        {
            (path.clone(), native_revision.clone(), true)
        } else {
            let (language, uri) = method.target();
            let binding = self
                .bindings
                .get(&(language.into(), uri.into()))
                .ok_or("lsp_document_denied")?;
            let revision = match method {
                DocumentMethod::ChangeLspDocument {
                    native_revision, ..
                }
                | DocumentMethod::ReloadLspDocument {
                    native_revision, ..
                }
                | DocumentMethod::SaveLspDocument {
                    native_revision, ..
                } => native_revision,
                _ => &binding.revision,
            };
            (
                binding.path.clone(),
                revision.clone(),
                matches!(
                    method,
                    DocumentMethod::ReloadLspDocument { .. }
                        | DocumentMethod::SaveLspDocument { .. }
                ),
            )
        };
        loop {
            self.check(deadline, cancelled)?;
            let prepare = || -> Result<_> {
                let context = self
                    .activities
                    .context
                    .enter(false)
                    .map_err(|_| "lsp_busy")?;
                let filesystem = self
                    .activities
                    .filesystem
                    .enter(false)
                    .map_err(|_| "lsp_busy")?;
                let files = self.files.try_lock().map_err(|error| match error {
                    std::sync::TryLockError::WouldBlock => "lsp_busy",
                    _ => "files_unavailable",
                })?;
                let proof = files.wsl_editor_proof(
                    self.snapshot.host(),
                    self.snapshot.context(),
                    ProofRequest {
                        path: path.clone(),
                        native_revision: revision.clone(),
                        verify_disk,
                    },
                    deadline,
                )?;
                Ok(PreparedDocument {
                    proof: Some(proof),
                    _permit: Some((context, filesystem)),
                })
            };
            match prepare() {
                Err("lsp_busy") => thread::sleep(Duration::from_millis(10)),
                result => return result,
            }
        }
    }
    fn call(
        &mut self,
        method: &str,
        args: Value,
        deadline: u64,
        cancelled: Arc<AtomicBool>,
    ) -> Result<Value> {
        let parsed = Method::parse(method, args.clone())?;
        self.check(deadline, &cancelled)?;
        if !parsed.stops() {
            if let Err(error) = self.snapshot.metadata(deadline).and_then(|()| {
                if self.snapshot.approved()? {
                    Ok(())
                } else {
                    Err("lsp_execution_approval_required")
                }
            }) {
                self.retire();
                return Err(error);
            }
        }
        let mut document = self.prepare_document(&parsed, deadline, &cancelled)?;
        let proof = document.proof.take();
        let document_binding = proof.as_ref().map(|proof| Binding {
            path: proof.path.clone(),
            revision: proof.revision.clone(),
        });
        let permits = RefCell::new(Vec::<(ContextPermit, ContextPermit)>::new());
        let authorize = |root: &str| -> Result<()> {
            self.check(deadline, &cancelled)?;
            if root != self.snapshot.root() {
                return Err("lsp_context_changed");
            }
            let context = self
                .activities
                .context
                .enter(false)
                .map_err(|_| "lsp_busy")?;
            let filesystem = self
                .activities
                .filesystem
                .enter(false)
                .map_err(|_| "lsp_busy")?;
            self.snapshot.metadata(deadline)?;
            if !self.snapshot.approved()? {
                return Err("lsp_execution_approval_required");
            }
            permits.borrow_mut().push((context, filesystem));
            Ok(())
        };
        let response =
            self.snapshot
                .request(method, args, proof, deadline, cancelled.clone(), &authorize);
        let raw = match response {
            Ok(raw) => raw,
            Err("lsp_operation_cancelled") => return Err("lsp_operation_cancelled"),
            Err(error) => {
                self.retire();
                return Err(error);
            }
        };
        let reply: Reply = match serde_json::from_value(raw) {
            Ok(reply) => reply,
            Err(_) => {
                self.retire();
                return Err("wsl_protocol_invalid");
            }
        };
        for event in reply.events {
            let mut value = event.value;
            value["nativeContext"] = json!(self.snapshot.context());
            (self.events)(event.name.as_str(), value);
        }
        let result = reply.result.map_err(|error| super::wsl_error(&error))?;
        match parsed {
            Method::Document(DocumentMethod::OpenLspDocument { language_id, .. }) => {
                let uri = result["uri"]
                    .as_str()
                    .filter(|uri| uri.starts_with("file:///") && uri.len() <= 32768)
                    .ok_or("wsl_protocol_invalid")?;
                self.bindings.insert(
                    (language_id, uri.into()),
                    document_binding.ok_or("wsl_protocol_invalid")?,
                );
            }
            Method::Document(DocumentMethod::CloseLspDocument { language_id, uri }) => {
                self.bindings.remove(&(language_id, uri));
            }
            Method::Document(method) => {
                if let Some(binding) = document_binding {
                    let (language, uri) = method.target();
                    self.bindings.insert((language.into(), uri.into()), binding);
                }
            }
            Method::Lifecycle(LifecycleMethod::StopLanguageServer { language_id, .. }) => self
                .bindings
                .retain(|(language, _), _| language != &language_id),
            Method::Lifecycle(LifecycleMethod::StopAllLanguageServers { .. }) => {
                self.bindings.clear()
            }
            _ => {}
        }
        Ok(result)
    }
}
