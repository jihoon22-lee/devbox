#![allow(dead_code)]
use devbox_core::{Db, Env, Hub, Instance, Paths};
use devbox_projects::ProjectsService;
use devbox_terminal::TerminalService;
use std::path::PathBuf;
use std::sync::Arc;
pub struct Harness {
    pub home: tempfile::TempDir,
    pub paths: Paths,
    pub db: Db,
    pub hub: Arc<Hub>,
    pub projects: Arc<ProjectsService>,
    pub terminal: TerminalService,
}
impl Harness {
    pub fn new() -> Self {
        let home = tempfile::tempdir().expect("fixture home");
        struct E(PathBuf);
        impl Env for E {
            fn var(&self, name: &str) -> Option<String> {
                match name {
                    "HOME" => Some(self.0.display().to_string()),
                    "XDG_RUNTIME_DIR" => Some(self.0.join("run").display().to_string()),
                    _ => None,
                }
            }
            fn uid(&self) -> u32 {
                1000
            }
        }
        let instance = Instance::parse(&format!(
            "test-{}",
            &uuid::Uuid::new_v4().simple().to_string()[..8]
        ))
        .expect("fixture instance");
        let paths = Paths::resolve(instance, &E(home.path().into())).expect("fixture paths");
        paths.ensure_dirs().expect("fixture dirs");
        std::fs::write(
            paths.instance_config_dir.join("tmux.conf"),
            devbox_terminal::TMUX_CONF,
        )
        .expect("fixture tmux config");
        let migrations = [devbox_projects::MIGRATIONS, devbox_terminal::MIGRATIONS].concat();
        let db = Db::open(&paths.db_path, &paths.backups_dir, "1.0.0-dev", &migrations)
            .expect("fixture database");
        let hub = Arc::new(Hub::new());
        let projects = Arc::new(ProjectsService::new(
            db.clone(),
            hub.clone(),
            paths.home.clone(),
            vec![],
        ));
        let terminal = TerminalService::new(
            db.clone(),
            hub.clone(),
            paths.clone(),
            projects.clone(),
            Default::default(),
        );
        Self {
            home,
            paths,
            db,
            hub,
            projects,
            terminal,
        }
    }
}
impl Drop for Harness {
    fn drop(&mut self) {
        let _ = std::process::Command::new("tmux")
            .args(["-L", self.terminal.tmux().socket(), "kill-server"])
            .env_remove("TMUX_TMPDIR")
            .env_remove("TMUX")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}
