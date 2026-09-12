//! Bounded replacement of native source batches; stale tickets never publish.
use product_contract::ProjectContext;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
type Result<T> = std::result::Result<T, &'static str>;
const MAX_BATCHES: usize = 256;
const MAX_PROBLEMS: usize = 2048;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Severity {
    Error,
    Warning,
    Information,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Target {
    File {
        relative_path: String,
        line: u32,
        column: Option<u32>,
        document_version: Option<i32>,
    },
    Run {
        run_id: String,
        stream: String,
        offset: Option<String>,
    },
    Matcher {
        run_id: String,
        index: u32,
    },
    Route {
        route: String,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Item {
    pub severity: Severity,
    pub message: String,
    pub target: Target,
    #[serde(default)]
    pub log: Option<Target>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Problem {
    pub id: String,
    pub source: String,
    pub revision: String,
    #[serde(flatten)]
    pub item: Item,
    pub stale: bool,
}
#[derive(Clone)]
pub struct Ticket {
    context: ProjectContext,
    key: String,
    sequence: u64,
    source: String,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceState {
    pub source: String,
    pub identity: String,
    pub revision: String,
    pub state: String,
    pub truncated: bool,
}
struct Batch {
    context: ProjectContext,
    sequence: u64,
    source: SourceState,
    problems: Vec<Problem>,
}
#[derive(Default)]
pub struct Store {
    sequence: u64,
    batches: BTreeMap<String, Batch>,
    truncated: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub context: ProjectContext,
    pub initialized: bool,
    pub truncated: bool,
    pub problems: Vec<Problem>,
    pub sources: Vec<SourceState>,
}
fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn bounded(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && !value.contains('\0')
}
fn relative(value: &str) -> bool {
    bounded(value, 4096)
        && !value.chars().any(char::is_control)
        && !value.contains(['\\', ':'])
        && !value.starts_with('/')
        && value
            .split('/')
            .all(|part| !part.is_empty() && !matches!(part, "." | ".."))
}
impl Item {
    fn validate(&self) -> Result<()> {
        if !bounded(&self.message, 4096) {
            return Err("problem_invalid");
        }
        let valid = match &self.target {
            Target::File {
                relative_path,
                line,
                column,
                document_version,
            } => {
                relative(relative_path)
                    && *line > 0
                    && column.is_none_or(|value| value > 0)
                    && document_version.is_none_or(|value| value >= 0)
            }
            Target::Run {
                run_id,
                stream,
                offset,
            } => valid_run(run_id, stream, offset.as_deref()),
            Target::Matcher { run_id, index } => opaque(run_id) && *index < 500,
            Target::Route { route } => matches!(
                route.as_str(),
                "source"
                    | "files"
                    | "dependencies"
                    | "tasks"
                    | "runtime"
                    | "logs"
                    | "terminal"
                    | "overview"
            ),
        };
        if !valid {
            return Err("problem_target_invalid");
        }
        if self.log.as_ref().is_some_and(|target|!matches!(target,Target::Run{run_id,stream,offset} if valid_run(run_id,stream,offset.as_deref()))){return Err("problem_target_invalid");}
        Ok(())
    }
}
fn valid_run(run: &str, stream: &str, offset: Option<&str>) -> bool {
    opaque(run)
        && matches!(stream, "stdout" | "stderr")
        && offset.is_none_or(|value| value.len() <= 20 && value.parse::<u64>().is_ok())
}
fn opaque(value: &str) -> bool {
    bounded(value, 128)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}
impl Store {
    pub fn begin(
        &mut self,
        context: &ProjectContext,
        source: &str,
        identity: &str,
    ) -> Result<Ticket> {
        context.validate().map_err(|_| "problem_context_invalid")?;
        if !opaque(source) || !bounded(identity, 4096) {
            return Err("problem_source_invalid");
        }
        let key = digest(
            &serde_json::to_vec(&(context, source, identity)).map_err(|_| "problem_invalid")?,
        );
        if !self.batches.contains_key(&key) && self.batches.len() >= MAX_BATCHES {
            self.truncated = true;
            return Err("problem_source_limit");
        }
        self.sequence = self.sequence.checked_add(1).ok_or("problem_source_limit")?;
        let sequence = self.sequence;
        let batch = self.batches.entry(key.clone()).or_insert_with(|| Batch {
            context: context.clone(),
            sequence,
            source: SourceState {
                source: source.into(),
                identity: identity.into(),
                revision: String::new(),
                state: "pending".into(),
                truncated: false,
            },
            problems: vec![],
        });
        batch.sequence = sequence;
        batch.source.state = "pending".into();
        for problem in &mut batch.problems {
            problem.stale = true;
        }
        Ok(Ticket {
            context: context.clone(),
            key,
            sequence,
            source: source.into(),
        })
    }
    pub fn finish(
        &mut self,
        ticket: &Ticket,
        revision: &str,
        items: Vec<Item>,
        truncated: bool,
    ) -> Result<()> {
        if !bounded(revision, 128) {
            return Err("problem_revision_invalid");
        }
        let Some(batch) = self.batches.get(&ticket.key) else {
            return Err("problem_source_expired");
        };
        if batch.sequence != ticket.sequence || batch.context != ticket.context {
            return Err("problem_source_superseded");
        }
        let remaining = MAX_PROBLEMS.saturating_sub(
            self.batches
                .values()
                .map(|batch| batch.problems.len())
                .sum::<usize>()
                .saturating_sub(batch.problems.len()),
        );
        let mut problems = Vec::new();
        let mut ids = BTreeSet::new();
        let mut limited = truncated;
        for item in items {
            item.validate()?;
            let id = digest(
                &serde_json::to_vec(&(&ticket.key, revision, &item))
                    .map_err(|_| "problem_invalid")?,
            );
            if !ids.insert(id.clone()) {
                continue;
            }
            if problems.len() >= remaining.min(512) {
                limited = true;
                break;
            }
            problems.push(Problem {
                id,
                source: ticket.source.clone(),
                revision: revision.into(),
                item,
                stale: false,
            });
        }
        let batch = self
            .batches
            .get_mut(&ticket.key)
            .ok_or("problem_source_expired")?;
        batch.source.revision = revision.into();
        batch.source.state = "ready".into();
        batch.source.truncated = limited;
        batch.problems = problems;
        Ok(())
    }
    pub fn unavailable(&mut self, ticket: &Ticket) {
        if let Some(batch) = self
            .batches
            .get_mut(&ticket.key)
            .filter(|batch| batch.sequence == ticket.sequence)
        {
            batch.source.state = "unavailable".into();
            for problem in &mut batch.problems {
                problem.stale = true;
            }
        }
    }
    pub fn source_unavailable(&mut self, context: &ProjectContext, source: &str) {
        for batch in self
            .batches
            .values_mut()
            .filter(|batch| batch.context == *context && batch.source.source == source)
        {
            batch.source.state = "unavailable".into();
            for problem in &mut batch.problems {
                problem.stale = true;
            }
        }
    }
    pub fn clear_all(&mut self, source: &str) {
        self.batches
            .retain(|_, batch| batch.source.source != source);
    }
    pub fn document_changed(&mut self, context: &ProjectContext, identity: &str, closed: bool) {
        if closed {
            self.batches.retain(|_, batch| {
                batch.context != *context
                    || batch.source.source != "lsp"
                    || batch.source.identity != identity
            });
            return;
        }
        for batch in self.batches.values_mut().filter(|batch| {
            batch.context == *context
                && batch.source.source == "lsp"
                && batch.source.identity == identity
        }) {
            batch.source.state = "pending".into();
            for problem in &mut batch.problems {
                problem.stale = true;
            }
        }
    }
    pub fn clear(&mut self, context: &ProjectContext, source: &str) {
        self.batches
            .retain(|_, batch| batch.context != *context || batch.source.source != source);
    }
    pub fn snapshot(&self, context: &ProjectContext) -> Snapshot {
        let batches = self
            .batches
            .values()
            .filter(|batch| batch.context == *context)
            .collect::<Vec<_>>();
        Snapshot {
            context: context.clone(),
            truncated: self.truncated,
            initialized: batches
                .iter()
                .any(|batch| !batch.source.revision.is_empty()),
            problems: batches
                .iter()
                .flat_map(|batch| batch.problems.clone())
                .collect(),
            sources: batches
                .into_iter()
                .map(|batch| batch.source.clone())
                .collect(),
        }
    }
    pub fn select(&self, context: &ProjectContext, id: &str, revision: &str) -> Result<Problem> {
        self.batches
            .values()
            .filter(|batch| batch.context == *context)
            .flat_map(|batch| &batch.problems)
            .find(|problem| problem.id == id && problem.revision == revision && !problem.stale)
            .cloned()
            .ok_or("problem_stale")
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn context(tree: &str) -> ProjectContext {
        ProjectContext {
            project_id: "project".into(),
            worktree_id: tree.into(),
            revision: 1,
            target: product_contract::ExecutionTarget::Windows,
        }
    }
    fn item() -> Item {
        Item {
            severity: Severity::Error,
            message: "Synthetic diagnostic".into(),
            target: Target::File {
                relative_path: "src/main.rs".into(),
                line: 2,
                column: Some(3),
                document_version: Some(2),
            },
            log: None,
        }
    }
    #[test]
    fn newer_scan_replaces_old_diagnostics_and_late_completion_cannot_restore_them() {
        let context = context("one");
        let mut store = Store::default();
        let old = store.begin(&context, "lsp", "file").unwrap();
        store
            .finish(&old, "1", vec![item(), item()], false)
            .unwrap();
        assert_eq!(store.snapshot(&context).problems.len(), 1);
        let first = store.begin(&context, "lsp", "file").unwrap();
        let latest = store.begin(&context, "lsp", "file").unwrap();
        store.finish(&latest, "3", vec![], false).unwrap();
        assert!(store.finish(&first, "2", vec![item()], false).is_err());
        assert!(store.snapshot(&context).problems.is_empty());
    }
    #[test]
    fn stopped_language_server_keeps_rows_but_revokes_navigation() {
        let context = context("one");
        let mut store = Store::default();
        let ticket = store.begin(&context, "lsp", "file").unwrap();
        store.finish(&ticket, "2", vec![item()], false).unwrap();
        let problem = store.snapshot(&context).problems.remove(0);
        store.source_unavailable(&context, "lsp");
        assert_eq!(store.snapshot(&context).sources[0].state, "unavailable");
        assert!(store
            .select(&context, &problem.id, &problem.revision)
            .is_err());
        let refreshed = store.begin(&context, "lsp", "file").unwrap();
        store.finish(&refreshed, "3", vec![], false).unwrap();
        assert!(store.snapshot(&context).problems.is_empty());
    }
    #[test]
    fn unknown_source_retains_stale_evidence_without_cross_worktree_navigation() {
        let one = context("one");
        let two = context("two");
        let mut store = Store::default();
        let ticket = store.begin(&one, "matcher", "run").unwrap();
        store
            .finish(&ticket, "revision", vec![item()], false)
            .unwrap();
        let problem = store.snapshot(&one).problems.remove(0);
        assert!(store.select(&two, &problem.id, &problem.revision).is_err());
        let pending = store.begin(&one, "matcher", "run").unwrap();
        store.unavailable(&pending);
        assert!(store.snapshot(&one).problems[0].stale);
        assert!(store.select(&one, &problem.id, &problem.revision).is_err());
    }
}
