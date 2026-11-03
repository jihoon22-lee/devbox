use devbox_core::{Db, Hub, Paths};
use devbox_projects::ProjectsService;
use std::sync::Arc;
use std::time::Instant;

pub struct Services {
    pub(super) windows: Arc<super::windows_broker::WindowsBroker>,
    pub(super) runtime: super::system::Runtime,
    pub(super) startup: super::startup::Startup,
    pub version: &'static str,
    pub daemon_id: String,
    pub paths: Paths,
    pub started: Instant,
    pub started_ms: i64,
    pub db: Db,
    pub hub: Arc<Hub>,
    pub projects: Arc<ProjectsService>,
    pub terminal: Arc<devbox_terminal::TerminalService>,
    pub agents: Arc<devbox_agents::AgentsService>,
    pub observe: Arc<devbox_observe::ObserveService>,
    pub git: Arc<devbox_git::service::GitService>,
    pub deps: Arc<devbox_deps::service::DepsService>,
    pub lsp: Arc<devbox_lsp::LspService>,
    pub containers: devbox_runtime::DockerContainers,
    pub secrets: Arc<devbox_secrets::SecretsService>,
    pub documents: Arc<devbox_documents::DocumentsService>,
    pub notes: Arc<devbox_notes::NotesService>,
    pub search: Arc<devbox_search::SearchService>,
    pub activity: Arc<devbox_activity::ActivityService>,
    pub runs: Arc<devbox_runtime::RuntimeService>,
    pub index: devbox_core::IndexDb,
    pub webhooks: Arc<devbox_webhooks::WebhooksService>,
    pub api: Arc<devbox_api::ApiService>,
    pub api_collections: Arc<devbox_api::CollectionsService>,
}

impl Services {
    /// The notes vault, or `None` while notes are off because `[notes]` did not load: the
    /// default vault never stands in for it (log roots, support bundles).
    pub(super) fn notes_vault(&self) -> Option<&std::path::Path> {
        (!self.startup.disabled.notes).then(|| self.notes.vault())
    }
}
