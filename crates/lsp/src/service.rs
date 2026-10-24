use crate::actor::{Command, Handle};
use crate::adapters::{DocumentAccess, InstallAdapter, RestartSink};
use crate::*;
use std::{
    collections::HashMap,
    ffi::OsString,
    path::PathBuf,
    sync::{Arc, atomic::AtomicU64},
    time::Duration,
};
use tokio::sync::{Mutex, oneshot};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
type R<T> = Result<T, devbox_protocol::Fail<LspError>>;
pub enum DiagnosticEvent {
    Generation {
        generation: u64,
        active: bool,
    },
    Publish {
        generation: u64,
        diagnostics: diagnostics::Diagnostics,
    },
}
pub type DiagnosticSink = Arc<dyn Fn(DiagnosticEvent) -> Result<(), LspError> + Send + Sync>;
#[derive(Clone)]
pub struct ProjectContext {
    pub project_id: String,
    pub root: PathBuf,
    pub access: DocumentAccess,
    pub diagnostics: DiagnosticSink,
    pub restarted: RestartSink,
}
pub struct LspService {
    path: OsString,
    specs: Vec<servers::ServerSpec>,
    handles: Mutex<HashMap<(String, Language), Handle>>,
    generations: Arc<AtomicU64>,
    stop: CancellationToken,
    tasks: TaskTracker,
}
impl Default for LspService {
    fn default() -> Self {
        Self::with_specs(
            std::env::var_os("PATH").unwrap_or_default(),
            servers::catalog(),
        )
    }
}
impl Drop for LspService {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}
impl LspService {
    pub fn with_specs(path: OsString, specs: Vec<servers::ServerSpec>) -> Self {
        Self {
            path,
            specs,
            handles: Mutex::default(),
            generations: Arc::default(),
            stop: CancellationToken::new(),
            tasks: TaskTracker::new(),
        }
    }
    async fn handle(&self, p: &OpenLsp, project: ProjectContext) -> R<Handle> {
        if self.stop.is_cancelled() {
            return Err(LspError::Failed.into());
        }
        if p.project_id != project.project_id || p.project_id.is_empty() || p.project_id.len() > 128
        {
            return Err(LspError::InvalidInput.into());
        }
        let root = tokio::fs::canonicalize(&project.root)
            .await
            .map_err(|_| LspError::OutsideProject)?;
        if root != project.root || !root.is_dir() || crate::uri::file_uri(&root).is_none() {
            return Err(LspError::OutsideProject.into());
        }
        let key = (p.project_id.clone(), p.language);
        let mut handles = self.handles.lock().await;
        if let Some(handle) = handles.get(&key) {
            if handle.root == root && !handle.done.is_cancelled() {
                return Ok(handle.clone());
            }
            handle.stop.cancel();
            tokio::time::timeout(Duration::from_secs(5), handle.done.cancelled())
                .await
                .map_err(|_| LspError::Failed)?;
            handles.remove(&key);
        }
        if handles.len() >= 64 {
            return Err(LspError::LimitExceeded.into());
        }
        let (spec, executable) = servers::detect_in(&self.specs, p.language, Some(&self.path))
            .ok_or(LspError::InvalidInput)?;
        let executable = executable.ok_or(LspError::MissingServer)?;
        let handle = crate::actor::spawn(
            project,
            spec,
            executable,
            self.generations.clone(),
            self.stop.child_token(),
            &self.tasks,
        );
        handles.insert(key, handle.clone());
        Ok(handle)
    }
    pub async fn open(
        &self,
        ctx: devbox_protocol::Ctx,
        p: OpenLsp,
        project: ProjectContext,
    ) -> R<devbox_protocol::StreamOpened> {
        let ends = ctx.open_stream().map_err(|_| LspError::StreamUnavailable)?;
        let id = ends.id;
        let tx = ends.tx.clone();
        let handle = match self.handle(&p, project).await {
            Ok(handle) => handle,
            Err(error) => {
                let _ = tokio::time::timeout(Duration::from_millis(250), tx.finish()).await;
                return Err(error);
            }
        };
        let (reply, response) = oneshot::channel();
        if handle
            .commands
            .try_send(Command::Attach { ends, reply })
            .is_err()
        {
            let _ = tokio::time::timeout(Duration::from_millis(250), tx.finish()).await;
            return Err(LspError::LimitExceeded.into());
        }
        let result = tokio::time::timeout(Duration::from_secs(5), response).await;
        match result {
            Ok(Ok(Ok(()))) => Ok(devbox_protocol::StreamOpened { stream_id: id }),
            Ok(Ok(Err(e))) => Err(e.into()),
            _ => {
                let _ = tokio::time::timeout(Duration::from_millis(250), tx.finish()).await;
                Err(LspError::Failed.into())
            }
        }
    }
    pub fn selected(&self, language: Language) -> Option<servers::ServerSpec> {
        servers::detect_in(&self.specs, language, Some(&self.path)).map(|(spec, _)| spec)
    }
    pub async fn servers(&self, project: &str) -> ServersResult {
        let handles = self.handles.lock().await;
        let mut pids = vec![std::process::id()];
        for handle in handles.values() {
            if let Some(pid) = handle.status.lock().expect("status").pid {
                pids.push(pid);
            }
        }
        let mut items = Vec::new();
        for language in self.specs.iter().map(|s| s.language) {
            if items.iter().any(|s: &ServerInfo| s.language == language) {
                continue;
            }
            if let Some(handle) = handles.get(&(project.into(), language)) {
                items.push(handle.status.lock().expect("status").clone());
            } else if let Some((spec, path)) =
                servers::detect_in(&self.specs, language, Some(&self.path))
            {
                items.push(ServerInfo {
                    language,
                    command: spec.command,
                    args: spec.args,
                    install: spec.install,
                    state: if path.is_some() {
                        ServerState::IdleStopped
                    } else {
                        ServerState::Missing
                    },
                    generation: 0,
                    pid: None,
                    memory_bytes: None,
                    clients: 0,
                });
            }
        }
        drop(handles);
        for item in &mut items {
            if let Some(pid) = item.pid {
                item.memory_bytes = crate::usage::memory(pid).await;
            }
        }
        ServersResult {
            items,
            inotify: crate::usage::inotify(&pids).await,
        }
    }
    pub async fn restart(&self, p: OpenLsp, project: ProjectContext) -> R<u64> {
        let handle = self.handle(&p, project).await?;
        let (reply, response) = oneshot::channel();
        handle
            .commands
            .try_send(Command::Restart { reply })
            .map_err(|_| LspError::LimitExceeded)?;
        tokio::time::timeout(Duration::from_secs(5), response)
            .await
            .map_err(|_| LspError::Failed)?
            .map_err(|_| LspError::Failed)??;
        Ok(handle.status.lock().expect("status").generation)
    }
    /// Only the explicit install RPC calls this adapter, with the same fixed command shown by servers.
    pub async fn install(&self, p: InstallLsp, terminal: InstallAdapter) -> R<Installed> {
        let command = {
            let handles = self.handles.lock().await;
            if let Some(handle) = handles.get(&(p.project_id.clone(), p.language)) {
                handle.status.lock().expect("status").install.clone()
            } else {
                servers::detect_in(&self.specs, p.language, Some(&self.path))
                    .ok_or(LspError::InvalidInput)?
                    .0
                    .install
            }
        }
        .ok_or(LspError::InstallUnavailable)?;
        if p.expected_command != command {
            return Err(LspError::InvalidInput.into());
        }
        let terminal_id = terminal(p.project_id, command).await?;
        Ok(Installed { terminal_id })
    }
    pub async fn shutdown(&self) {
        self.stop.cancel();
        self.tasks.close();
        self.tasks.wait().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn install_only_forwards_the_displayed_fixed_command_to_explicit_terminal_adapter() {
        let service =
            LspService::with_specs("/nonexistent-owned-fixture".into(), servers::catalog());
        let calls = Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured = calls.clone();
        let adapter: InstallAdapter = Arc::new(move |project, command| {
            captured.lock().expect("calls").push((project, command));
            Box::pin(async { Ok("fixture-terminal".into()) })
        });
        let status = service.servers("p").await;
        let displayed = status
            .items
            .iter()
            .find(|s| s.language == Language::Rust)
            .expect("rust")
            .install
            .clone()
            .expect("command");
        assert!(calls.lock().expect("calls").is_empty());
        let result = service
            .install(
                InstallLsp {
                    project_id: "p".into(),
                    language: Language::Rust,
                    expected_command: displayed.clone(),
                },
                adapter.clone(),
            )
            .await
            .expect("explicit install adapter");
        assert!(
            service
                .install(
                    InstallLsp {
                        project_id: "p".into(),
                        language: Language::Rust,
                        expected_command: "not the displayed command".into()
                    },
                    adapter
                )
                .await
                .is_err()
        );
        assert_eq!(result.terminal_id, "fixture-terminal");
        assert_eq!(*calls.lock().expect("calls"), vec![("p".into(), displayed)]);
        assert!(
            service
                .servers("p")
                .await
                .items
                .iter()
                .all(|s| s.pid.is_none())
        );
        service.shutdown().await;
    }
}
