use crate::*;
use devbox_protocol::Internal;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunLogMetadata {
    pub run_id: String,
    pub project_id: String,
    pub attempt_id: Option<String>,
    pub started_ms: u64,
    pub path: PathBuf,
    pub live: bool,
}
pub struct RunLogPage {
    pub items: Vec<RunLogMetadata>,
    pub truncated: bool,
}
impl RuntimeService {
    /// Bounded observation history; no SQL or runtime storage layout escapes to observe.
    pub async fn problem_runs(&self, project_id: &str) -> R<RunLogPage> {
        let project = project_id.to_owned();
        let mut runs = self
            .db
            .read(move |tx| crate::run_store::problem_history(tx, &project))
            .await
            .internal("read problem run metadata")?;
        let truncated = runs.len() > 128;
        runs.truncate(128);
        // Live invocations never disappear behind the completed-history page.
        let live = self.live_unit_runs().await?;
        for row in live.into_iter().filter(|r| r.project_id == project_id) {
            if !runs.iter().any(|r| r.run.id == row.run_id) {
                runs.push(self.stored(&row.run_id).await?);
            }
        }
        let mut items = Vec::with_capacity(runs.len());
        for run in runs {
            items.push(self.log_metadata(run.run)?);
        }
        Ok(RunLogPage { items, truncated })
    }
    pub async fn run_log_metadata(&self, id: &str) -> R<RunLogMetadata> {
        self.log_metadata(self.stored(id).await?.run)
    }
    fn log_metadata(&self, run: Run) -> R<RunLogMetadata> {
        let store = crate::exit_record::ExitStore::new(
            &self.execution()?.paths.data_dir.join("runs"),
            &run.id,
        )
        .internal("resolve observed invocation")?;
        // Durable wrapper identity is authoritative, even before runtime reconciliation.
        let attempt_id = if run.state == RunState::Starting && run.attempt_id.is_none() {
            // Manual restart stores the new generation before the wrapper replaces
            // its durable attempt file. Never attach the previous file to it.
            None
        } else {
            store
                .current_attempt()
                .internal("read observed invocation")?
        };
        Ok(RunLogMetadata {
            run_id: run.id,
            project_id: run.project_id,
            attempt_id,
            started_ms: run.started_ms,
            path: store.log_path(),
            live: run.state.is_live(),
        })
    }
}

/// Original resolved working directory, retained across definition edits.
pub struct RunLocationContext {
    pub project_id: String,
    pub cwd: Option<String>,
    pub windows: Option<WindowsRun>,
}
impl RuntimeService {
    pub async fn location_context(&self, id: &str) -> R<RunLocationContext> {
        let value = self.stored(id).await?;
        Ok(RunLocationContext {
            project_id: value.run.project_id,
            cwd: value.source_cwd,
            windows: value.run.windows,
        })
    }
}
