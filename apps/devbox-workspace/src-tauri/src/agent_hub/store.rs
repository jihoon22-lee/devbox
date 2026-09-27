use super::{
    plan::{self, AgentTool},
    AgentIssue,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    sync::Mutex,
};
const TASKS_FILE: &str = "tasks.json";
const MAX_TASKS: usize = 200;
const MAX_BYTES: u64 = 8 * 1024 * 1024;
const MAX_REVISION: u64 = 9_007_199_254_740_991;
static LOCK: Mutex<()> = Mutex::new(());
type Result<T> = std::result::Result<T, AgentIssue>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum AgentTaskState {
    Planned,
    Created,
    Ready,
    Running,
    Merged,
    Discarded,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum AgentOutcome {
    Merged,
    Discarded,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentTask {
    pub id: String,
    pub revision: u64,
    pub project_id: String,
    pub base_worktree_id: String,
    pub title: String,
    pub tool: AgentTool,
    pub command: String,
    pub branch: String,
    pub target_dir: String,
    pub worktree_id: Option<String>,
    pub terminal_id: Option<String>,
    pub state: AgentTaskState,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}
pub enum Change {
    WorktreeCreated { path: String },
    WorktreeBound { worktree_id: String },
    TerminalOpened { terminal_id: String },
    Finished { outcome: AgentOutcome },
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Document {
    schema_version: u32,
    tasks: Vec<AgentTask>,
}
fn id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}
fn terminal_id(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|uuid| uuid.to_string() == value)
}
impl AgentTask {
    fn valid(&self) -> bool {
        use AgentTaskState::*;
        id(&self.id)
            && id(&self.project_id)
            && id(&self.base_worktree_id)
            && (1..=MAX_REVISION).contains(&self.revision)
            && self.created_at_ms <= self.updated_at_ms
            && self.updated_at_ms <= MAX_REVISION
            && plan::checked_title(&self.title).is_ok_and(|title| title == self.title)
            && plan::tool_command(
                self.tool,
                (self.tool == AgentTool::Custom).then_some(self.command.as_str()),
            )
            .is_ok_and(|c| c == self.command)
            && self.branch.strip_prefix("agent/").is_some_and(|s| {
                !s.is_empty()
                    && s.len() <= plan::MAX_SLUG_BYTES
                    && !s.starts_with('-')
                    && !s.ends_with('-')
                    && s.bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            })
            && plan::valid_target_dir(&self.target_dir)
            && self
                .worktree_id
                .as_ref()
                .is_none_or(|value| id(value) && value != &self.base_worktree_id)
            && self.terminal_id.as_deref().is_none_or(terminal_id)
            && match self.state {
                Planned | Created => self.worktree_id.is_none() && self.terminal_id.is_none(),
                Ready => self.worktree_id.is_some() && self.terminal_id.is_none(),
                Running => self.worktree_id.is_some() && self.terminal_id.is_some(),
                Merged => self.worktree_id.is_some(),
                Discarded => true,
            }
    }
}
impl Document {
    fn valid(&self) -> bool {
        let mut ids = HashSet::new();
        let mut branches = HashSet::new();
        self.schema_version == 1
            && self.tasks.len() <= MAX_TASKS
            && self.tasks.iter().all(|task| {
                task.valid()
                    && ids.insert(&task.id)
                    && branches.insert((&task.project_id, &task.branch))
            })
    }
}
pub struct AgentTaskStore {
    directory: PathBuf,
}
impl AgentTaskStore {
    pub fn open(dir: &Path) -> Self {
        Self {
            directory: dir.into(),
        }
    }
    fn load(&self) -> Result<Document> {
        devbox_filesystem::ensure_no_links(&self.directory)
            .map_err(|_| AgentIssue::StoreUnavailable)?;
        let path = self.directory.join(TASKS_FILE);
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Document {
                    schema_version: 1,
                    tasks: vec![],
                })
            }
            Ok(meta) if meta.is_file() && meta.len() <= MAX_BYTES => {}
            _ => return Err(AgentIssue::StoreUnavailable),
        }
        devbox_filesystem::ensure_no_links(&path).map_err(|_| AgentIssue::StoreUnavailable)?;
        let mut bytes = Vec::new();
        File::open(path)
            .map_err(|_| AgentIssue::StoreUnavailable)?
            .take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| AgentIssue::StoreUnavailable)?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(AgentIssue::StoreUnavailable);
        }
        let doc: Document =
            serde_json::from_slice(&bytes).map_err(|_| AgentIssue::StoreUnavailable)?;
        if !doc.valid() {
            return Err(AgentIssue::StoreUnavailable);
        }
        Ok(doc)
    }
    fn save(&self, doc: &Document) -> Result<()> {
        if !doc.valid() {
            return Err(AgentIssue::StateInvalid);
        }
        let bytes = serde_json::to_vec(doc).map_err(|_| AgentIssue::StoreUnavailable)?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(AgentIssue::TaskLimit);
        }
        devbox_filesystem::atomic_write(self.directory.join(TASKS_FILE), &bytes)
            .map_err(|_| AgentIssue::StoreUnavailable)
    }
    pub fn list(&self, project_id: &str) -> Result<Vec<AgentTask>> {
        let _lock = LOCK.lock().map_err(|_| AgentIssue::StoreUnavailable)?;
        let mut tasks = self
            .load()?
            .tasks
            .into_iter()
            .filter(|task| task.project_id == project_id)
            .collect::<Vec<_>>();
        tasks.sort_by(|a, b| {
            b.created_at_ms
                .cmp(&a.created_at_ms)
                .then_with(|| a.id.cmp(&b.id))
        });
        Ok(tasks)
    }
    pub fn get(&self, id: &str) -> Result<AgentTask> {
        let _lock = LOCK.lock().map_err(|_| AgentIssue::StoreUnavailable)?;
        self.load()?
            .tasks
            .into_iter()
            .find(|task| task.id == id)
            .ok_or(AgentIssue::TaskMissing)
    }
    pub fn insert(&self, task: AgentTask) -> Result<AgentTask> {
        let _lock = LOCK.lock().map_err(|_| AgentIssue::StoreUnavailable)?;
        let mut doc = self.load()?;
        if doc.tasks.len() >= MAX_TASKS {
            return Err(AgentIssue::TaskLimit);
        }
        if task.state != AgentTaskState::Planned || task.revision != 1 || !task.valid() {
            return Err(AgentIssue::StateInvalid);
        }
        if doc.tasks.iter().any(|t| {
            t.id == task.id || (t.project_id == task.project_id && t.branch == task.branch)
        }) {
            return Err(AgentIssue::TaskChanged);
        }
        doc.tasks.push(task.clone());
        self.save(&doc)?;
        Ok(task)
    }
    pub fn apply(
        &self,
        id: &str,
        expected_revision: u64,
        change: Change,
        now_ms: u64,
    ) -> Result<AgentTask> {
        let _lock = LOCK.lock().map_err(|_| AgentIssue::StoreUnavailable)?;
        let mut doc = self.load()?;
        let task = doc
            .tasks
            .iter_mut()
            .find(|task| task.id == id)
            .ok_or(AgentIssue::TaskMissing)?;
        if task.revision != expected_revision {
            return Err(AgentIssue::TaskChanged);
        }
        use AgentTaskState::*;
        match (task.state, change) {
            (Planned, Change::WorktreeCreated { path }) if path == task.target_dir => {
                task.state = Created
            }
            (Created, Change::WorktreeBound { worktree_id })
                if self::id(&worktree_id) && worktree_id != task.base_worktree_id =>
            {
                task.worktree_id = Some(worktree_id);
                task.state = Ready;
            }
            (Ready | Running, Change::TerminalOpened { terminal_id })
                if self::terminal_id(&terminal_id) =>
            {
                task.terminal_id = Some(terminal_id);
                task.state = Running;
            }
            (
                Created | Ready | Running,
                Change::Finished {
                    outcome: AgentOutcome::Discarded,
                },
            ) => task.state = Discarded,
            (
                Ready | Running,
                Change::Finished {
                    outcome: AgentOutcome::Merged,
                },
            ) => task.state = Merged,
            _ => return Err(AgentIssue::StateInvalid),
        }
        task.revision = task
            .revision
            .checked_add(1)
            .filter(|v| *v <= MAX_REVISION)
            .ok_or(AgentIssue::TaskChanged)?;
        task.updated_at_ms = task.updated_at_ms.max(now_ms);
        let task = task.clone();
        self.save(&doc)?;
        Ok(task)
    }
    pub fn forget(&self, id: &str, expected_revision: u64) -> Result<()> {
        let _lock = LOCK.lock().map_err(|_| AgentIssue::StoreUnavailable)?;
        let mut doc = self.load()?;
        let task = doc
            .tasks
            .iter()
            .find(|task| task.id == id)
            .ok_or(AgentIssue::TaskMissing)?;
        if task.revision != expected_revision {
            return Err(AgentIssue::TaskChanged);
        }
        if !matches!(
            task.state,
            AgentTaskState::Planned | AgentTaskState::Merged | AgentTaskState::Discarded
        ) {
            return Err(AgentIssue::StateInvalid);
        }
        doc.tasks.retain(|task| task.id != id);
        self.save(&doc)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn task_limit_and_future_documents_preserve_existing_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(TASKS_FILE);
        let tasks = (0..MAX_TASKS)
            .map(|n| {
                let mut t = task(&format!("t{n}"), "fixture");
                t.branch = format!("agent/task-{n}");
                t
            })
            .collect();
        let bytes = serde_json::to_vec(&Document {
            schema_version: 1,
            tasks,
        })
        .unwrap();
        fs::write(&path, &bytes).unwrap();
        let store = AgentTaskStore::open(dir.path());
        assert_eq!(
            store.insert(task("overflow", "fixture")),
            Err(AgentIssue::TaskLimit)
        );
        assert_eq!(fs::read(&path).unwrap(), bytes);
        let future = br#"{"schemaVersion":2,"tasks":[]}"#;
        fs::write(&path, future).unwrap();
        assert_eq!(
            store.insert(task("new", "fixture")),
            Err(AgentIssue::StoreUnavailable)
        );
        assert_eq!(fs::read(&path).unwrap(), future);
    }

    #[test]
    fn separate_store_instances_do_not_lose_concurrent_inserts() {
        let dir = tempfile::tempdir().unwrap();
        std::thread::scope(|scope| {
            for n in 0..4 {
                let root = dir.path();
                scope.spawn(move || {
                    let mut t = task(&format!("t{n}"), "fixture");
                    t.branch = format!("agent/task-{n}");
                    t.created_at_ms = n + 1;
                    t.updated_at_ms = n + 1;
                    AgentTaskStore::open(root).insert(t).unwrap();
                });
            }
        });
        let tasks = AgentTaskStore::open(dir.path()).list("p1").unwrap();
        assert_eq!(
            tasks.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
            ["t3", "t2", "t1", "t0"]
        );
    }

    pub(crate) fn task(id: &str, title: &str) -> AgentTask {
        AgentTask {
            id: id.into(),
            revision: 1,
            project_id: "p1".into(),
            base_worktree_id: "w1".into(),
            title: title.into(),
            tool: AgentTool::ClaudeCode,
            command: "claude".into(),
            branch: "agent/fix-login".into(),
            target_dir: "/home/me/projects/devbox-fix-login".into(),
            worktree_id: None,
            terminal_id: None,
            state: AgentTaskState::Planned,
            created_at_ms: 1,
            updated_at_ms: 1,
        }
    }

    #[test]
    fn tasks_move_forward_one_step_at_a_time_with_revision_checks() {
        let dir = tempfile::tempdir().unwrap();
        let store = AgentTaskStore::open(dir.path());
        let t = store.insert(task("t1", "Fix login")).unwrap();
        assert_eq!(
            store.apply(
                "t1",
                t.revision,
                Change::WorktreeBound {
                    worktree_id: "w2".into()
                },
                2
            ),
            Err(AgentIssue::StateInvalid)
        );
        let t = store
            .apply(
                "t1",
                t.revision,
                Change::WorktreeCreated {
                    path: t.target_dir.clone(),
                },
                2,
            )
            .unwrap();
        assert_eq!((t.state, t.revision), (AgentTaskState::Created, 2));
        assert_eq!(
            store.apply(
                "t1",
                1,
                Change::WorktreeBound {
                    worktree_id: "w2".into()
                },
                3
            ),
            Err(AgentIssue::TaskChanged)
        );
        let t = store
            .apply(
                "t1",
                2,
                Change::WorktreeBound {
                    worktree_id: "w2".into(),
                },
                3,
            )
            .unwrap();
        let t = store
            .apply(
                "t1",
                t.revision,
                Change::TerminalOpened {
                    terminal_id: "00000000-0000-4000-8000-000000000001".into(),
                },
                4,
            )
            .unwrap();
        let t = store
            .apply(
                "t1",
                t.revision,
                Change::TerminalOpened {
                    terminal_id: "00000000-0000-4000-8000-000000000002".into(),
                },
                5,
            )
            .unwrap();
        assert_eq!(t.state, AgentTaskState::Running);
        let t = store
            .apply(
                "t1",
                t.revision,
                Change::Finished {
                    outcome: AgentOutcome::Merged,
                },
                6,
            )
            .unwrap();
        assert_eq!(
            store.apply(
                "t1",
                t.revision,
                Change::Finished {
                    outcome: AgentOutcome::Discarded
                },
                7
            ),
            Err(AgentIssue::StateInvalid)
        );
        store.forget("t1", t.revision).unwrap();
        assert_eq!(store.get("t1"), Err(AgentIssue::TaskMissing));
    }

    #[test]
    fn worktree_path_must_match_the_plan_and_active_tasks_cannot_be_forgotten() {
        let dir = tempfile::tempdir().unwrap();
        let store = AgentTaskStore::open(dir.path());
        let t = store.insert(task("t1", "Fix login")).unwrap();
        assert_eq!(
            store.apply(
                "t1",
                1,
                Change::WorktreeCreated {
                    path: "/elsewhere".into()
                },
                2
            ),
            Err(AgentIssue::StateInvalid)
        );
        let t = store
            .apply(
                "t1",
                t.revision,
                Change::WorktreeCreated {
                    path: t.target_dir.clone(),
                },
                2,
            )
            .unwrap();
        assert_eq!(
            store.forget("t1", t.revision),
            Err(AgentIssue::StateInvalid)
        );
    }

    #[test]
    fn list_is_scoped_to_a_project_and_survives_reopen() {
        let dir = tempfile::tempdir().unwrap();
        AgentTaskStore::open(dir.path())
            .insert(task("t1", "a"))
            .unwrap();
        let mut other = task("t2", "b");
        other.project_id = "p2".into();
        AgentTaskStore::open(dir.path()).insert(other).unwrap();
        let listed = AgentTaskStore::open(dir.path()).list("p1").unwrap();
        assert_eq!(
            listed.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
            vec!["t1"]
        );
    }

    #[test]
    fn a_corrupt_file_is_reported_and_kept() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(TASKS_FILE), b"{not json").unwrap();
        assert_eq!(
            AgentTaskStore::open(dir.path()).list("p1"),
            Err(AgentIssue::StoreUnavailable)
        );
        assert_eq!(
            std::fs::read(dir.path().join(TASKS_FILE)).unwrap(),
            b"{not json"
        );
    }
}
