use super::support::{TestDaemon, bounded};
use devbox_cli::daemon;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub struct Server {
    pub services: Arc<daemon::state::Services>,
    cancel: CancellationToken,
    task: Option<tokio::task::JoinHandle<std::io::Result<()>>>,
}
impl Server {
    pub async fn start(d: &TestDaemon, slice: Option<String>) -> Self {
        let mut services = daemon::build_services(d.paths.clone()).unwrap();
        let secrets = Arc::new(
            devbox_secrets::SecretsService::with_key(
                services.db.clone(),
                d.paths.clone(),
                devbox_secrets::crypto::Key::from_bytes([7; 32]),
            )
            .await
            .unwrap(),
        );
        let mut execution = devbox_runtime::ExecutionConfig::new(d.paths.clone());
        execution.executable = env!("CARGO_BIN_EXE_devbox").into();
        execution.slice = slice;
        let runs = Arc::new(
            devbox_runtime::RuntimeService::new(
                services.db.clone(),
                services.hub.clone(),
                services.projects.clone(),
            )
            .with_execution(execution)
            .with_secrets(secrets.clone()),
        );
        let inner = Arc::get_mut(&mut services).unwrap();
        inner.secrets = secrets;
        inner.runs = runs;
        if d.socket.exists() {
            std::fs::remove_file(&d.socket).unwrap();
        }
        let listener = tokio::net::UnixListener::bind(&d.socket).unwrap();
        let cancel = CancellationToken::new();
        let task = tokio::spawn(daemon::server::serve(
            listener,
            services.clone(),
            cancel.clone(),
        ));
        Self {
            services,
            cancel,
            task: Some(task),
        }
    }
    pub async fn seed(&self, d: &TestDaemon, project: &str, id: &str, started: u64, text: &str) {
        let store = devbox_runtime::exit_record::ExitStore::new(&d.paths.data_dir.join("runs"), id)
            .unwrap();
        store.begin("owned").unwrap();
        std::fs::write(
            store.log_path(),
            format!(
                "2026-10-20T01:02:03.004Z\ts\t{}\n{text}",
                devbox_core::run_log::attempt_marker("owned")
            ),
        )
        .unwrap();
        let row=serde_json::json!({"run":{"id":id,"projectId":project,"defId":"fixture","kind":"task","state":"stopped","exit":null,"startedMs":started,"endedMs":started+1,"restarts":0,"unit":"devbox-test-mcp.service","ready":false,"pid":null,"port":null,"attemptId":"owned"},"source_cwd":d.home.path().join("project").display().to_string(),"restart":"no","start_limit":null,"worktree":null,"start_limit_notified":false}).to_string();
        let id = id.to_owned();
        let project = project.to_owned();
        self.services
            .db
            .write(move |tx| {
                tx.execute(
                    "INSERT INTO runtime_runs(id,project_id,started_ms,body) VALUES(?1,?2,?3,?4)",
                    (&id, &project, started, &row),
                )?;
                Ok(())
            })
            .await
            .unwrap();
    }
    pub async fn stop(mut self) {
        self.cancel.cancel();
        bounded(self.task.take().unwrap()).await.unwrap().unwrap();
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}
pub fn record(text: &str) -> String {
    format!("2026-10-20T01:02:03.004Z\to\t{text}\n")
}
