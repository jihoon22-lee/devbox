//! Installed-only transport. A failed installed scope never becomes portable.
use super::{
    agent_peer::{AgentPeer, Witness},
    component_scope::CapturedScope,
};
use agent_client::{
    AgentClient, AgentError, Connect, ConnectFuture, Connected, LaunchFuture, Transport,
};
use std::{
    os::windows::{io::AsRawHandle, process::CommandExt},
    sync::Arc,
};
use tokio::net::windows::named_pipe::ClientOptions;
use windows::Win32::Foundation::HANDLE;

struct Native {
    scope: Arc<CapturedScope>,
    pipe: String,
}
impl Transport for Native {
    fn generation(&self) -> &str {
        &self.scope.manifest.generation
    }
    fn connect(&self) -> ConnectFuture<'_> {
        Box::pin(async move {
            self.scope.revalidate().map_err(|_| Connect::Unavailable)?;
            let pipe = ClientOptions::new().open(&self.pipe).map_err(|error| {
                if error.raw_os_error() == Some(2) {
                    Connect::Missing
                } else {
                    Connect::Unavailable
                }
            })?;
            let witness =
                Witness::capture(HANDLE(pipe.as_raw_handle())).map_err(|_| Connect::Unavailable)?;
            let scope = self.scope.clone();
            let peer = tokio::task::spawn_blocking(move || AgentPeer::agent(scope, witness))
                .await
                .map_err(|_| Connect::Unavailable)?
                .map_err(|reason| Connect::Rejected(reason.into()))?;
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
            tokio::task::spawn_blocking(move || {
                let executable = scope.agent_image().map_err(|_| AgentError::Unavailable)?;
                let mut child = std::process::Command::new(executable)
                    .creation_flags(0x08000000)
                    .spawn()
                    .map_err(|_| AgentError::Unavailable)?;
                std::thread::spawn(move || {
                    let _ = child.wait();
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
            scope: Arc::new(scope),
            pipe,
        }),
    )
}
