//! A service whose output watcher reads the panes a test gives it, without tmux.
use super::*;
use devbox_core::{Db, Hub, HubReceiver, Instance, Migration, Paths, paths::Env};

struct Home(std::path::PathBuf);
impl Env for Home {
    fn var(&self, key: &str) -> Option<String> {
        let path = match key {
            "HOME" => self.0.clone(),
            "XDG_RUNTIME_DIR" => self.0.join("runtime"),
            _ => return None,
        };
        Some(path.to_string_lossy().into_owned())
    }
    fn uid(&self) -> u32 {
        1000
    }
}

/// Task `a` of the builtin Codex profile, running in terminal `term` on launch `launch`, and the
/// output watcher that reads its pane.
pub(super) struct Watched {
    pub svc: Arc<AgentsService>,
    watcher: Watcher,
    _home: tempfile::TempDir,
}
impl Watched {
    /// `resumed`: the launch resumed Codex's session. `turn`: the turn Codex began since.
    pub async fn new(resumed: bool, turn: Option<&str>) -> Self {
        let home = tempfile::tempdir().expect("owned home");
        let paths = Paths::resolve(
            Instance::parse("test-watch").expect("owned instance"),
            &Home(home.path().into()),
        )
        .expect("owned paths");
        paths.ensure_dirs().expect("owned directories");
        let migrations: Vec<Migration> = crate::MIGRATIONS
            .iter()
            .chain(devbox_projects::MIGRATIONS)
            .chain(devbox_terminal::MIGRATIONS)
            .copied()
            .collect();
        let db = Db::open(&paths.db_path, &paths.backups_dir, "test", &migrations)
            .expect("owned database");
        let hub = Arc::new(Hub::new());
        let projects = Arc::new(devbox_projects::ProjectsService::new(
            db.clone(),
            hub.clone(),
            paths.home.clone(),
            vec![],
        ));
        let terminal = Arc::new(devbox_terminal::TerminalService::new(
            db.clone(),
            hub.clone(),
            paths.clone(),
            projects.clone(),
            Default::default(),
        ));
        let shell = crate::command_path::fixture_shell(&paths.home);
        let runtime = crate::AgentRuntime {
            devbox_bin: "/owned/devbox".into(),
            use_scope: false,
            cleanup_after_close: false,
            shell,
            lookups: Arc::new(crate::command_path::LookupCache::stopped()),
        };
        let svc = Arc::new(AgentsService::new(
            db,
            hub,
            paths,
            projects,
            terminal,
            vec![],
            runtime,
        ));
        let mut binding = Binding::new(1);
        binding.launch_id = Some("launch".into());
        binding.resumed_session = resumed;
        binding.turn_id = turn.map(Into::into);
        let row = task();
        svc.db
            .write(move |tx| {
                store::put(tx, &row)?;
                store::put_binding(tx, &row.id, &binding)
            })
            .await
            .expect("owned launch");
        let watcher = Watcher::new(&svc).expect("builtin waiting patterns");
        Self {
            svc,
            watcher,
            _home: home,
        }
    }
    /// One pass of the watcher at `at`, as its loop makes it, with `shown` as every watched pane.
    pub async fn read(&mut self, shown: &str, at: Instant) {
        let tasks = self.svc.watched_tasks().await.expect("watched tasks");
        self.watcher.keep(&tasks);
        for task in tasks {
            let id = task.id.clone();
            let binding = self
                .svc
                .db
                .read(move |tx| store::binding(tx, &id))
                .await
                .expect("owned binding");
            let snapshot = Snapshot { task, binding };
            self.watcher
                .read(&self.svc, snapshot, shown.into(), at)
                .await;
        }
    }
    pub async fn task(&self) -> AgentTask {
        self.svc.get("a").await.expect("owned task")
    }
    pub async fn binding(&self) -> Binding {
        self.svc.launch_binding("a").await.expect("owned binding")
    }
}

/// The notices published to `attention` so far. Each is published before its write replies.
pub(super) fn notices(attention: &mut HubReceiver) -> Vec<String> {
    use futures::FutureExt;
    let mut notices = Vec::new();
    while let Some(Some(message)) = attention.recv().now_or_never() {
        match message {
            devbox_core::HubMessage::Event { payload, .. } => notices.push(payload.to_string()),
            devbox_core::HubMessage::Resync => panic!("more notices than the queue holds"),
        }
    }
    notices
}
