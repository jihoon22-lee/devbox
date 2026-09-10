//! A current-thread LSP runtime driven only by authenticated Windows calls.
//! Retained native ownership outlives cancelled requests and filesystem loss.
use crate::{
    lsp_authority::{Authority, Lease, RequestScope},
    lsp_documents::Documents,
    lsp_review::Review,
    lsp_wire::{Command, DocumentProof, Event, EventName, LifecycleMethod, Reply},
};
use code_pad_lib::lsp::{LspEvent, LspManager, LspManagerError};
use devbox_filesystem::project::ProjectObservation;
use serde_json::Value;
use std::{sync::Arc, time::Duration};
type Result<T> = std::result::Result<T, &'static str>;
const MAX_REPLY: usize = 32 * 1024 * 1024;
pub(crate) fn error(error: LspManagerError) -> &'static str {
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
pub(crate) fn value<T: serde::Serialize>(value: T) -> Result<Value> {
    serde_json::to_value(value).map_err(|_| "lsp_unavailable")
}
pub(crate) struct Runtime {
    pub(crate) root_token: String,
    authority: Arc<Authority>,
    runtime: tokio::runtime::Runtime,
    manager: LspManager,
    documents: Documents,
    events: tokio::sync::broadcast::Receiver<LspEvent>,
    pending_event: Option<Event>,
    // Main's ProcessGate cannot hard-exit until this runtime confirms retirement.
    _retained: Box<dyn Send + Sync>,
}
impl Runtime {
    pub(crate) fn new(
        root_token: String,
        observation: &ProjectObservation,
        review: Arc<Review>,
        retained: Box<dyn Send + Sync>,
    ) -> Result<Self> {
        let observed = ProjectObservation::capture(observation.root(), crate::linux_files::admit)?;
        if observed.root_identity() != observation.root_identity()
            || observed.repository_identity() != observation.repository_identity()
        {
            return Err("project_object_changed");
        }
        observation.revalidate()?;
        let lease = Arc::new(Lease {
            context: review.context().clone(),
            observation: observed,
        });
        let authority = Arc::new(Authority::new(review.clone(), lease.clone()));
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| "lsp_unavailable")?;
        let manager = LspManager::with_reviewed_read_only_execution(
            env!("CARGO_PKG_VERSION"),
            authority.clone(),
            review.reviewed(),
        )
        .map_err(error)?
        .with_linux_supervisor("/proc/self/exe");
        let events = manager.subscribe_events();
        Ok(Self {
            root_token,
            authority,
            runtime,
            manager,
            documents: Documents::new(lease),
            events,
            pending_event: None,
            _retained: retained,
        })
    }
    pub(crate) fn matches(&self, context: &product_contract::ProjectContext, digest: &str) -> bool {
        self.authority.review.context() == context && self.authority.review.digest() == digest
    }
    pub(crate) fn run(
        &mut self,
        command: Command,
        proof: Option<DocumentProof>,
        scope: RequestScope,
    ) -> Result<Reply> {
        scope.check()?;
        self.authority.bind(scope.clone())?;
        let poll = matches!(command, Command::Lifecycle(LifecycleMethod::LspPoll { .. }));
        // Explicit commands never start unrelated automatic retries. A poll
        // uses its own uncancelled ticket and settles all starts before reply.
        self.manager.drive_native_retries(poll);
        let starting = command.starts();
        let manager = &self.manager;
        let documents = &mut self.documents;
        let result = self.runtime.block_on(async {
            let result = if starting {
                // The authority cancellation future makes create_session stop
                // and await its unpublished child. Dropping start() here would
                // release its activity before the process wait task completes.
                execute(manager, documents, command, proof).await
            } else {
                let operation = execute(manager, documents, command, proof);
                tokio::pin!(operation);
                tokio::select! {biased;
                    _=until_cancelled(&scope)=>Err("lsp_operation_cancelled"),
                    result=&mut operation=>result,
                }
            };
            manager.wait_for_startup_retirement().await;
            if scope.check().is_err() {
                Err("lsp_operation_cancelled")
            } else {
                result
            }
        });
        self.manager.drive_native_retries(false);
        self.authority.clear();
        let result = result.map_err(str::to_owned);
        let mut total = serde_json::to_vec(&result)
            .map_err(|_| "lsp_unavailable")?
            .len();
        if total > MAX_REPLY {
            return Err("lsp_response_limit");
        }
        let mut events = Vec::new();
        for _ in 0..128 {
            let event = if let Some(event) = self.pending_event.take() {
                event
            } else {
                match self.events.try_recv() {
                    Ok(LspEvent::Status(status)) => Event {
                        name: EventName::Status,
                        value: value(status)?,
                    },
                    Ok(LspEvent::Diagnostics(diagnostics)) => {
                        if !self
                            .documents
                            .contains(&diagnostics.language_id, &diagnostics.response.metadata.uri)
                        {
                            continue;
                        }
                        Event {
                            name: EventName::Diagnostics,
                            value: value(diagnostics)?,
                        }
                    }
                    Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            };
            if matches!(event.name, EventName::Diagnostics)
                && !self.documents.contains(
                    event.value["languageId"].as_str().unwrap_or(""),
                    event.value["response"]["metadata"]["uri"]
                        .as_str()
                        .unwrap_or(""),
                )
            {
                continue;
            }
            let bytes = serde_json::to_vec(&event)
                .map_err(|_| "lsp_unavailable")?
                .len();
            if bytes > MAX_REPLY {
                return Err("lsp_response_limit");
            }
            if total + bytes + 1024 > MAX_REPLY {
                self.pending_event = Some(event);
                break;
            }
            total += bytes;
            events.push(event);
        }
        Ok(Reply { result, events })
    }
}
async fn until_cancelled(scope: &RequestScope) {
    while scope.check().is_ok() {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}
async fn execute(
    manager: &LspManager,
    documents: &mut Documents,
    command: Command,
    proof: Option<DocumentProof>,
) -> Result<Value> {
    let Command::Lifecycle(method) = command else {
        let Command::Document(method) = command else {
            unreachable!()
        };
        return documents.execute(manager, method, proof).await;
    };
    if proof.is_some() {
        return Err("wsl_request_invalid");
    }
    match method {
        LifecycleMethod::StartLanguageServer { language_id, .. } => manager
            .start(&language_id)
            .await
            .map(|()| Value::Null)
            .map_err(error),
        LifecycleMethod::RestartLanguageServer { language_id, .. } => manager
            .restart(&language_id)
            .await
            .map(|()| Value::Null)
            .map_err(error),
        LifecycleMethod::StopLanguageServer {
            language_id,
            operation_id,
        } => {
            match manager.stop(&language_id).await {
                Err(LspManagerError::NotRunning(_)) if operation_id.is_some() => {}
                result => result.map_err(error)?,
            }
            documents.close_language(Some(&language_id))?;
            Ok(Value::Null)
        }
        LifecycleMethod::StopAllLanguageServers { .. } => {
            manager.stop_all().await.map_err(error)?;
            documents.close_language(None)?;
            Ok(Value::Null)
        }
        LifecycleMethod::LanguageServerStatuses { .. } => value(manager.statuses().await),
        LifecycleMethod::LanguageServerLogs { .. } => value(manager.logs().await),
        LifecycleMethod::LspPoll { .. } => {
            tokio::time::sleep(Duration::from_millis(10)).await;
            Ok(Value::Null)
        }
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        self.authority.shutdown();
        self.manager.drive_native_retries(false);
        loop {
            if self
                .runtime
                .block_on(self.manager.shutdown_for_exit())
                .is_ok()
            {
                break;
            }
            self.runtime.block_on(async {
                tokio::time::sleep(Duration::from_millis(50)).await;
            });
        }
    }
}
