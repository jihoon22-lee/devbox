use crate::*;
use devbox_core::Hub;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::Mutex,
};

pub(crate) const MAX_PROBLEMS: usize = 2048;
pub(crate) const MAX_RUNS: usize = 2048;
pub(crate) const MAX_LIVE_READERS: usize = 128;
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProblemRun {
    pub run_id: String,
    pub project_id: String,
    pub root: PathBuf,
    pub source_cwd: Option<PathBuf>,
    pub windows_directory: Option<String>,
    pub path: PathBuf,
    pub attempt_id: Option<String>,
    pub started_ms: u64,
}
#[derive(Default)]
pub(crate) struct ProblemState {
    pub rev: u64,
    projects: HashSet<String>,
    sources: HashMap<String, ProblemRun>,
    explicit: HashMap<String, String>,
    rows: Vec<(String, Problem)>,
    truncated: HashSet<String>,
}
#[derive(Default)]
pub(crate) struct ProblemCollection {
    pub state: Mutex<ProblemState>,
    pub gate: tokio::sync::Mutex<()>,
    pub readers: tokio::sync::Mutex<HashMap<String, crate::problem_ingest::ProblemReader>>,
    pub wake: tokio::sync::Notify,
    pub watching: std::sync::atomic::AtomicBool,
    pub completed:
        tokio::sync::Mutex<HashMap<String, (ProblemRun, crate::problem_ingest::FileStamp)>>,
}
impl ObserveService {
    pub fn problem_rev(&self) -> u64 {
        self.problems.state.lock().expect("problem state").rev
    }
    pub fn problem_projects(&self) -> Vec<String> {
        self.problems
            .state
            .lock()
            .expect("problem state")
            .projects
            .iter()
            .cloned()
            .collect()
    }
    pub fn register_problem_project(&self, project: &str) -> R<()> {
        let mut state = self.problems.state.lock().expect("problem state");
        if !state.projects.contains(project) && state.projects.len() >= 8 {
            return Err(ObserveError::LimitExceeded.into());
        }
        state.projects.insert(project.into());
        self.problems.wake.notify_one();
        Ok(())
    }
    pub fn register_problem_run(&self, run: &str, project: &str) -> R<()> {
        let mut state = self.problems.state.lock().expect("problem state");
        if !state.explicit.contains_key(run) && state.explicit.len() >= 32 {
            return Err(ObserveError::LimitExceeded.into());
        }
        state.explicit.insert(run.into(), project.into());
        self.problems.wake.notify_one();
        Ok(())
    }
    pub fn explicit_problem_runs(&self, project: &str) -> Vec<String> {
        self.problems
            .state
            .lock()
            .expect("problem state")
            .explicit
            .iter()
            .filter(|(_, p)| p.as_str() == project)
            .map(|(id, _)| id.clone())
            .collect()
    }
    pub async fn problem_refresh_lock(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.problems.gate.lock().await
    }
    pub fn problem_result(&self, p: ProblemsQuery) -> R<ProblemsResult> {
        let limit = p.limit.unwrap_or(100);
        if limit == 0 || limit > 100 {
            return Err(ObserveError::InvalidInput.into());
        }
        let state = self.problems.state.lock().expect("problem state");
        let rev = state.rev.to_string();
        let offset = match p.cursor {
            Some(c) if c.rev == rev => c.offset as usize,
            Some(_) => return Err(ObserveError::InvalidCursor.into()),
            None => 0,
        };
        let rows: Vec<_> = state
            .rows
            .iter()
            .filter(|(project, _)| project == &p.project_id)
            .collect();
        if offset > rows.len() {
            return Err(ObserveError::InvalidCursor.into());
        }
        let mut items = Vec::new();
        let mut bytes = 256; // envelope, revision and cursor
        for (_, problem) in rows.iter().skip(offset).take(limit as usize) {
            let size = serde_json::to_vec(problem)
                .expect("problem serializes")
                .len()
                + 1;
            if bytes + size > 768 * 1024 {
                break;
            }
            bytes += size;
            items.push(problem.clone());
        }
        let next = offset + items.len();
        Ok(ProblemsResult {
            rev: rev.clone(),
            items,
            next_cursor: (next < rows.len()).then_some(ProblemsCursor {
                rev,
                offset: next as u32,
            }),
            truncated: state.truncated.contains(&p.project_id),
        })
    }
    pub fn apply_problem_batch(
        &self,
        run: ProblemRun,
        problems: Vec<Problem>,
        overflow: bool,
        hub: &Hub,
    ) -> R<()> {
        let mut state = self.problems.state.lock().expect("problem state");
        if !state.sources.contains_key(&run.run_id) && state.sources.len() >= MAX_RUNS {
            return Err(ObserveError::LimitExceeded.into());
        }
        let source = format!("run:{}", run.run_id);
        let previous = state.sources.insert(run.run_id.clone(), run.clone());
        let restarted = previous.as_ref().is_some_and(|old| {
            old.attempt_id != run.attempt_id
                || old.started_ms != run.started_ms
                || old.root != run.root
        });
        let mut changed = false;
        if restarted {
            state.rows.retain(|(_, p)| p.source != source);
            // A restart is itself a lifecycle change, including an empty previous source.
            changed = true;
        }
        if overflow && state.truncated.insert(run.project_id.clone()) {
            changed = true;
        }
        for mut problem in problems {
            problem.source = source.clone();
            if state.rows.iter().any(|(_, p)| p == &problem) {
                continue;
            }
            if state.rows.len() == MAX_PROBLEMS {
                changed |= state.truncated.insert(run.project_id.clone());
                break;
            }
            state.rows.push((run.project_id.clone(), problem));
            changed = true;
        }
        if changed {
            state.rev += 1;
            hub.publish::<ObserveProblemsChanged>(
                state.rev,
                &ProblemsChange {
                    rev: state.rev.to_string(),
                    project_id: run.project_id,
                },
            );
        }
        Ok(())
    }
    pub fn mark_problem_history_truncated(&self, project: &str, hub: &Hub) {
        let mut state = self.problems.state.lock().expect("problem state");
        if state.truncated.insert(project.into()) {
            state.rev += 1;
            hub.publish::<ObserveProblemsChanged>(
                state.rev,
                &ProblemsChange {
                    rev: state.rev.to_string(),
                    project_id: project.into(),
                },
            );
        }
    }
}
