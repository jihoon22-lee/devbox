//! Installed-only transport. A failed installed scope never becomes portable.
use super::{
    agent_peer::{AgentPeer, Witness},
    component_scope::CapturedScope,
};
use agent_client::{
    AgentClient, AgentError, Connect, ConnectFuture, Connected, ConnectionDiagnostic, LaunchFuture,
    Transport,
};
use product_contract::operation_log::{Entry, OperationLog, Outcome};
use std::{
    os::windows::{io::AsRawHandle, process::CommandExt},
    sync::Arc,
};
use tokio::net::windows::named_pipe::ClientOptions;
use windows::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_BROKEN_PIPE, ERROR_PIPE_BUSY, ERROR_PIPE_NOT_CONNECTED, HANDLE,
};

struct Native {
    scope: Arc<CapturedScope>,
    pipe: String,
    isolate_stdio: bool,
    diagnostics: Option<Arc<OperationLog>>,
    product: String,
}
fn diagnostic_log(scope: &CapturedScope) -> Option<Arc<OperationLog>> {
    use windows::Win32::{
        System::Com::CoTaskMemFree,
        UI::Shell::{FOLDERID_LocalAppData, SHGetKnownFolderPath, KF_FLAG_DEFAULT},
    };
    let folder =
        unsafe { SHGetKnownFolderPath(&FOLDERID_LocalAppData, KF_FLAG_DEFAULT, None) }.ok()?;
    let text = unsafe { folder.to_string() };
    unsafe {
        CoTaskMemFree(Some(folder.0.cast()));
    }
    let root = std::path::PathBuf::from(text.ok()?)
        .join(format!("com.devbox.v08.agent.i{}", scope.installation_key))
        .join("logs");
    OperationLog::open(root).ok().map(Arc::new)
}
fn record(log: Option<&OperationLog>, version: &str, product: &str, event: ConnectionDiagnostic) {
    if let Some(log) = log {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|time| time.as_millis() as u64)
            .unwrap_or(0);
        let outcome = if matches!(
            event,
            ConnectionDiagnostic::ConnectAttempt
                | ConnectionDiagnostic::LaunchAttempt
                | ConnectionDiagnostic::Connected
                | ConnectionDiagnostic::AgentExited
        ) {
            Outcome::Succeeded
        } else {
            Outcome::Failed
        };
        log.append(&Entry::new(
            now,
            version,
            product,
            "agent-connection",
            "connect",
            0,
            outcome,
            Some(event.code()),
        ));
    }
}
impl Transport for Native {
    fn diagnostic(&self, event: ConnectionDiagnostic) {
        record(
            self.diagnostics.as_deref(),
            &self.scope.manifest.suite_version,
            &self.product,
            event,
        );
    }
    fn generation(&self) -> &str {
        &self.scope.manifest.generation
    }
    fn connect(&self) -> ConnectFuture<'_> {
        Box::pin(async move {
            self.scope.revalidate().map_err(|_| {
                self.diagnostic(ConnectionDiagnostic::ScopeInvalid);
                Connect::Unavailable
            })?;
            let pipe = ClientOptions::new().open(&self.pipe).map_err(|error| {
                if error.raw_os_error() == Some(2) {
                    Connect::Missing
                } else {
                    self.diagnostic(match error.raw_os_error().map(|code| code as u32) {
                        Some(code) if code == ERROR_PIPE_BUSY.0 => ConnectionDiagnostic::PipeBusy,
                        Some(code) if code == ERROR_ACCESS_DENIED.0 => {
                            ConnectionDiagnostic::PipeDenied
                        }
                        Some(code)
                            if code == ERROR_PIPE_NOT_CONNECTED.0
                                || code == ERROR_BROKEN_PIPE.0 =>
                        {
                            ConnectionDiagnostic::PipeDisconnected
                        }
                        _ => ConnectionDiagnostic::PipeOther,
                    });
                    Connect::Unavailable
                }
            })?;
            let witness = Witness::capture(HANDLE(pipe.as_raw_handle())).map_err(|_| {
                self.diagnostic(ConnectionDiagnostic::PeerCaptureFailed);
                Connect::Unavailable
            })?;
            let scope = self.scope.clone();
            let peer = tokio::task::spawn_blocking(move || AgentPeer::agent(scope, witness))
                .await
                .map_err(|_| {
                    self.diagnostic(ConnectionDiagnostic::PeerVerificationFailed);
                    Connect::Unavailable
                })?
                .map_err(|reason| {
                    self.diagnostic(ConnectionDiagnostic::PeerVerificationFailed);
                    Connect::Rejected(reason.into())
                })?;
            let peer = Arc::new(peer);
            let retired = peer.clone();
            Ok(Connected {
                stream: Box::new(pipe),
                verify: Arc::new(move || peer.revalidate().map_err(|_| AgentError::Unavailable)),
                exited: Arc::new(move || retired.exited().map_err(|_| AgentError::Unavailable)),
            })
        })
    }
    fn launch(&self) -> LaunchFuture<'_> {
        Box::pin(async move {
            let scope = self.scope.clone();
            let isolate_stdio = self.isolate_stdio;
            let diagnostics = self.diagnostics.clone();
            let product = self.product.clone();
            tokio::task::spawn_blocking(move || {
                let executable = scope.agent_image().map_err(|_| AgentError::Unavailable)?;
                let mut command = std::process::Command::new(executable);
                command.creation_flags(0x08000000);
                if isolate_stdio {
                    command
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null());
                }
                let mut child = command.spawn().map_err(|_| {
                    record(
                        diagnostics.as_deref(),
                        &scope.manifest.suite_version,
                        &product,
                        ConnectionDiagnostic::AgentSpawnFailed,
                    );
                    AgentError::Unavailable
                })?;
                std::thread::spawn(move || {
                    let event = match child.wait() {
                        Ok(status) if status.success() => ConnectionDiagnostic::AgentExited,
                        _ => ConnectionDiagnostic::AgentExitFailed,
                    };
                    record(
                        diagnostics.as_deref(),
                        &scope.manifest.suite_version,
                        &product,
                        event,
                    );
                });
                Ok(())
            })
            .await
            .map_err(|_| AgentError::Unavailable)?
        })
    }
}
pub fn create(product: &str, version: &str) -> AgentClient {
    let Ok(executable) = std::env::current_exe().and_then(|path| path.canonicalize()) else {
        return AgentClient::unavailable();
    };
    let Some(products) = executable
        .parent()
        .and_then(|p| p.parent())
        .filter(|p| p.file_name().is_some_and(|n| n == "products"))
    else {
        return AgentClient::unsupported();
    };
    let Some(generations) = products
        .parent()
        .and_then(|p| p.parent())
        .filter(|p| p.file_name().is_some_and(|n| n == "generations"))
    else {
        return AgentClient::unsupported();
    };
    let Some(root) = generations.parent() else {
        return AgentClient::unavailable();
    };
    let captured = CapturedScope::capture(root, product, &executable, version)
        .and_then(CapturedScope::capture_agent_image);
    let Ok(scope) = captured else {
        return AgentClient::unavailable();
    };
    let pipe = format!(r"\\.\pipe\devbox-agent-{}", scope.installation_key);
    AgentClient::with_transport(
        product,
        Arc::new(Native {
            diagnostics: diagnostic_log(&scope),
            product: product.into(),
            scope: Arc::new(scope),
            pipe,
            isolate_stdio: false,
        }),
    )
}

pub fn create_mcp(root: &std::path::Path) -> Result<(AgentClient, String, String), &'static str> {
    let scope = Arc::new(CapturedScope::capture_current_agent(root)?);
    let installation = scope.installation_key.clone();
    let generation = scope.manifest.generation.clone();
    let pipe = format!(r"\\.\pipe\devbox-agent-{}", installation);
    Ok((
        AgentClient::with_transport(
            "mcp",
            Arc::new(Native {
                diagnostics: diagnostic_log(&scope),
                product: "mcp".into(),
                scope,
                pipe,
                isolate_stdio: true,
            }),
        ),
        installation,
        generation,
    ))
}
