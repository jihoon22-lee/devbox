mod debug;
mod dedupe;
mod launch;
mod outbox;
pub mod routes;
pub mod server;
mod socket;
pub mod state;
mod streams;
mod system;
mod trust;

use devbox_core::{Config, Db, Hub, Paths, log};
use state::Services;
use std::sync::Arc;
use std::time::Instant;

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn migrations() -> Vec<devbox_core::Migration> {
    [
        devbox_projects::MIGRATIONS,
        devbox_terminal::MIGRATIONS,
        devbox_agents::MIGRATIONS,
    ]
    .concat()
}

pub fn build_services(paths: Paths) -> anyhow::Result<Arc<Services>> {
    build_services_with_runtime(paths, system::Runtime::new(false))
}

fn build_services_with_runtime(
    paths: Paths,
    runtime: system::Runtime,
) -> anyhow::Result<Arc<Services>> {
    paths.ensure_dirs()?;
    let config = Config::load(&paths)?;
    let db = Db::open(
        &paths.db_path,
        &paths.backups_dir,
        crate::VERSION,
        &migrations(),
    )?;
    let hub = Arc::new(Hub::new());
    let projects = Arc::new(devbox_projects::ProjectsService::new(
        db.clone(),
        hub.clone(),
        paths.home.clone(),
        config.projects.roots.clone(),
    ));
    let terminal = Arc::new(devbox_terminal::TerminalService::new(
        db.clone(),
        hub.clone(),
        paths.clone(),
        projects.clone(),
        config.terminal,
    ));
    let profiles = devbox_agents::profile::configured(&config.agents)?;
    let executable = std::env::current_exe()?;
    let devbox_bin = executable
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("executable path is not UTF-8"))?
        .to_owned();
    let use_scope = std::env::var_os("DEVBOX_NO_SCOPE").is_none();
    let agents = Arc::new(devbox_agents::AgentsService::new(
        db.clone(),
        hub.clone(),
        paths.clone(),
        projects.clone(),
        terminal.clone(),
        profiles,
        devbox_agents::AgentRuntime {
            devbox_bin,
            use_scope,
            cleanup_after_close: config.agents.cleanup_after_close.unwrap_or(true),
        },
    ));
    Ok(Arc::new(Services {
        runtime,
        version: crate::VERSION,
        daemon_id: uuid::Uuid::new_v4().simple().to_string(),
        started: Instant::now(),
        started_ms: now_ms(),
        paths,
        db,
        hub,
        projects,
        terminal,
        agents,
    }))
}

#[cfg(test)]
pub fn build_services_for_test(home: &std::path::Path) -> Arc<Services> {
    struct E(String);
    impl devbox_core::Env for E {
        fn var(&self, k: &str) -> Option<String> {
            (k == "HOME").then(|| self.0.clone())
        }
        fn uid(&self) -> u32 {
            1000
        }
    }
    let paths = Paths::resolve(
        devbox_core::Instance::parse("test-r").expect("parse fixture instance"),
        &E(home.display().to_string()),
    )
    .expect("parse fixture instance");
    build_services(paths).expect("build fixture services")
}

pub async fn run(foreground: bool) -> anyhow::Result<()> {
    let paths = crate::paths()?;
    let _lease = socket::database_lease(&paths.data_dir)?;
    let socket = socket::Socket::acquire(&paths.socket)?;
    paths.ensure_dirs()?;
    let _guard = log::init_logging(&paths, foreground)?;
    log::install_panic_hook(&paths, crate::VERSION);
    let runtime = system::Runtime::new(!foreground);
    let shutdown = runtime.shutdown.clone();
    let services = build_services_with_runtime(paths.clone(), runtime.clone())?;
    socket::notify_ready()?;
    tracing::info!(instance = paths.instance.as_str(), socket = %paths.socket.display(), "daemon ready");
    let s2 = shutdown.clone();
    tokio::spawn(async move {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("sigterm");
        tokio::select! { _ = term.recv() => {}, _ = tokio::signal::ctrl_c() => {} }
        s2.cancel();
    });
    server::serve(socket.listener, services, shutdown).await?;
    tracing::info!("daemon stopped");
    if runtime.restart_requested() {
        anyhow::bail!("intentional daemon restart after environment refresh");
    }
    Ok(())
}
