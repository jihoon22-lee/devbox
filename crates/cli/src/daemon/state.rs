use devbox_core::{Db, Hub, Paths};
use devbox_projects::ProjectsService;
use std::sync::Arc;
use std::time::Instant;

pub struct Services {
    pub(super) windows: super::windows_broker::WindowsBroker,
    pub(super) runtime: super::system::Runtime,
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
    pub containers: devbox_runtime::DockerContainers,
    pub secrets: Arc<devbox_secrets::SecretsService>,
    pub documents: Arc<devbox_documents::DocumentsService>,
    pub notes: Arc<devbox_notes::NotesService>,
    pub search: Arc<devbox_search::SearchService>,
    pub activity: Arc<devbox_activity::ActivityService>,
    pub runs: Arc<devbox_runtime::RuntimeService>,
}
