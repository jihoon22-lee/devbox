//! LSP source lifecycle is separate from run attempts and log-reader registration.
use crate::*;
use devbox_core::Hub;
pub struct LspProblems {
    pub project: String,
    pub server: String,
    pub uri: String,
    pub generation: u64,
    pub problems: Vec<Problem>,
    pub overflow: bool,
}
use std::collections::{BTreeMap, BTreeSet};
#[derive(Default)]
pub(crate) struct LspSource {
    generation: u64,
    active: bool,
    pub rows: BTreeMap<String, Vec<Problem>>,
    pub truncated: BTreeSet<String>,
    pub saturated: bool,
}
fn validate(project: &str, server: &str) -> R<()> {
    if project.is_empty()
        || project.len() > 128
        || server.is_empty()
        || server.len() > 128
        || !server
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
    {
        return Err(ObserveError::InvalidInput.into());
    }
    Ok(())
}
fn publish(state: &mut crate::problem_store::ProblemState, project: &str, hub: &Hub) {
    state.rev += 1;
    hub.publish::<ObserveProblemsChanged>(
        state.rev,
        &ProblemsChange {
            rev: state.rev.to_string(),
            project_id: project.into(),
        },
    );
}
impl ObserveService {
    pub fn replace_lsp_diagnostics(&self, batch: LspProblems, hub: &Hub) -> R<()> {
        validate(&batch.project, &batch.server)?;
        if batch.uri.len() > 16384 || !batch.uri.starts_with("file:///") {
            return Err(ObserveError::InvalidInput.into());
        }
        if batch.problems.iter().any(|p| {
            p.line == 0 || p.col == Some(0) || p.message.len() > 4096 || p.file.len() > 4096
        }) {
            return Err(ObserveError::InvalidInput.into());
        }
        let mut state = self.problems.state.lock().expect("problem state");
        let key = (batch.project.clone(), batch.server.clone());
        if !state.lsp.contains_key(&key) && state.lsp.len() >= 64 {
            return Err(ObserveError::LimitExceeded.into());
        }
        let source = state.lsp.entry(key.clone()).or_default();
        if batch.generation < source.generation
            || (batch.generation == source.generation && !source.active)
        {
            return Ok(());
        }
        let mut changed = false;
        if batch.generation > source.generation {
            changed = !source.rows.is_empty() || !source.truncated.is_empty() || source.saturated;
            *source = LspSource {
                generation: batch.generation,
                active: true,
                ..Default::default()
            };
        }
        let previous = source.rows.get(&batch.uri).map_or(0, Vec::len);
        let known = source.rows.contains_key(&batch.uri) || source.truncated.contains(&batch.uri);
        let uris = state
            .lsp
            .values()
            .map(|s| s.rows.len() + s.truncated.len())
            .sum::<usize>();
        if !known && uris >= crate::problem_store::MAX_PROBLEMS {
            if !batch.problems.is_empty() || batch.overflow {
                let source = state.lsp.get_mut(&key).expect("source");
                changed |= !source.saturated;
                source.saturated = true;
            }
            if changed {
                publish(&mut state, &batch.project, hub);
            }
            return Ok(());
        }
        let used = state.rows.len()
            + state
                .lsp
                .values()
                .map(|s| s.rows.values().map(Vec::len).sum::<usize>())
                .sum::<usize>()
            - previous;
        let available = crate::problem_store::MAX_PROBLEMS.saturating_sub(used);
        let mut rows = Vec::new();
        let mut overflow = batch.overflow;
        for mut p in batch.problems {
            p.source = format!("lsp:{}", batch.server);
            if rows.contains(&p) {
                continue;
            }
            if rows.len() >= available {
                overflow = true;
                break;
            }
            rows.push(p);
        }
        rows.sort_by(|a, b| {
            (&a.file, a.line, a.col, &a.message, a.severity as u8).cmp(&(
                &b.file,
                b.line,
                b.col,
                &b.message,
                b.severity as u8,
            ))
        });
        let source = state.lsp.get_mut(&key).expect("inserted source");
        changed |= source.rows.get(&batch.uri).is_some_and(|old| old != &rows)
            || (!rows.is_empty() && !source.rows.contains_key(&batch.uri));
        if rows.is_empty() {
            source.rows.remove(&batch.uri);
        } else {
            source.rows.insert(batch.uri.clone(), rows);
        }
        changed |= if overflow {
            source.truncated.insert(batch.uri)
        } else {
            source.truncated.remove(&batch.uri)
        };
        if changed {
            publish(&mut state, &batch.project, hub);
        }
        Ok(())
    }
    pub fn lsp_generation(
        &self,
        project: &str,
        server: &str,
        generation: u64,
        active: bool,
        hub: &Hub,
    ) -> R<()> {
        validate(project, server)?;
        let mut state = self.problems.state.lock().expect("problem state");
        let key = (project.to_owned(), server.to_owned());
        if !state.lsp.contains_key(&key) && state.lsp.len() >= 64 {
            return Err(ObserveError::LimitExceeded.into());
        }
        let source = state.lsp.entry(key).or_default();
        if generation < source.generation || (generation == source.generation && active) {
            return Ok(());
        }
        let changed = !source.rows.is_empty() || !source.truncated.is_empty() || source.saturated;
        *source = LspSource {
            generation,
            active,
            ..Default::default()
        };
        if changed {
            publish(&mut state, project, hub);
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn row(file: &str, message: &str) -> Problem {
        Problem {
            file: file.into(),
            line: 1,
            col: Some(3),
            severity: ProblemSeverity::Error,
            message: message.into(),
            source: String::new(),
        }
    }
    fn batch(uri: &str, generation: u64, problems: Vec<Problem>) -> LspProblems {
        LspProblems {
            project: "p".into(),
            server: "rust-analyzer".into(),
            uri: uri.into(),
            generation,
            problems,
            overflow: false,
        }
    }
    fn result(s: &ObserveService) -> ProblemsResult {
        s.problem_result(ProblemsQuery {
            project_id: "p".into(),
            limit: None,
            cursor: None,
        })
        .expect("query")
    }
    #[test]
    fn lsp_replacement_empty_and_old_generation_preserve_other_files_and_run_sources() {
        let s = ObserveService::default();
        let hub = Hub::new();
        let run = ProblemRun {
            run_id: "r".into(),
            project_id: "p".into(),
            root: "/owned".into(),
            source_cwd: None,
            windows_directory: None,
            path: "/owned/log".into(),
            attempt_id: Some("a".into()),
            started_ms: 1,
        };
        s.apply_problem_batch(run.clone(), vec![row("run.rs", "build")], false, &hub)
            .expect("run");
        s.replace_lsp_diagnostics(
            batch("file:///owned/a.rs", 1, vec![row("/owned/a.rs", "a")]),
            &hub,
        )
        .expect("a");
        s.replace_lsp_diagnostics(
            batch("file:///owned/b.rs", 1, vec![row("/owned/b.rs", "b")]),
            &hub,
        )
        .expect("b");
        assert_eq!(result(&s).items.len(), 3);
        let rev = s.problem_rev();
        s.replace_lsp_diagnostics(
            batch("file:///owned/a.rs", 1, vec![row("/owned/a.rs", "a")]),
            &hub,
        )
        .expect("same");
        assert_eq!(s.problem_rev(), rev);
        s.replace_lsp_diagnostics(batch("file:///owned/a.rs", 1, vec![]), &hub)
            .expect("empty");
        assert_eq!(result(&s).items.len(), 2);
        s.apply_problem_batch(
            ProblemRun {
                attempt_id: Some("b".into()),
                ..run
            },
            vec![row("run2.rs", "new build")],
            false,
            &hub,
        )
        .expect("restart run");
        assert!(result(&s).items.iter().any(|p| p.message == "b"));
        s.lsp_generation("p", "rust-analyzer", 2, true, &hub)
            .expect("restart LSP");
        assert_eq!(result(&s).items.len(), 1);
        let rev = s.problem_rev();
        s.replace_lsp_diagnostics(
            batch("file:///owned/a.rs", 1, vec![row("/owned/a.rs", "stale")]),
            &hub,
        )
        .expect("ignore stale");
        assert_eq!(s.problem_rev(), rev);
        s.replace_lsp_diagnostics(
            batch("file:///owned/a.rs", 2, vec![row("/owned/a.rs", "new")]),
            &hub,
        )
        .expect("new");
        s.lsp_generation("p", "rust-analyzer", 2, false, &hub)
            .expect("stop");
        s.replace_lsp_diagnostics(
            batch("file:///owned/a.rs", 2, vec![row("/owned/a.rs", "late")]),
            &hub,
        )
        .expect("ignore late");
        assert_eq!(result(&s).items[0].source, "run:r");
        assert_eq!(result(&s).items.len(), 1);
    }
    #[test]
    fn lsp_truncation_and_project_cursor_are_independent_of_run_registration() {
        let s = ObserveService::default();
        let hub = Hub::new();
        s.register_problem_project("p").expect("register");
        s.register_problem_run("r", "p").expect("register run");
        let rows = (0..2050)
            .map(|i| row("/owned/a.rs", &format!("error {i}")))
            .collect();
        s.replace_lsp_diagnostics(batch("file:///owned/a.rs", 1, rows), &hub)
            .expect("bounded");
        let r = result(&s);
        assert!(r.truncated);
        assert_eq!(r.items.len(), 100);
        assert!(r.next_cursor.is_some());
        let other = s
            .problem_result(ProblemsQuery {
                project_id: "q".into(),
                limit: None,
                cursor: None,
            })
            .expect("other");
        assert!(other.items.is_empty());
        s.lsp_generation("p", "rust-analyzer", 1, false, &hub)
            .expect("stop");
        assert!(!result(&s).truncated);
        assert_eq!(s.explicit_problem_runs("p"), vec!["r"]);
        assert_eq!(s.problem_projects(), vec!["p"]);
    }
    #[test]
    fn lsp_sources_share_uri_without_erasing_each_other_and_invalid_replacement_is_atomic() {
        let s = ObserveService::default();
        let hub = Hub::new();
        let uri = "file:///owned/a.rs";
        let a = row("/owned/a.rs", "a");
        let b = row("/owned/a.rs", "b");
        s.replace_lsp_diagnostics(batch(uri, 1, vec![a.clone(), b.clone()]), &hub)
            .expect("first");
        let rev = s.problem_rev();
        s.replace_lsp_diagnostics(batch(uri, 1, vec![b, a]), &hub)
            .expect("reordered same");
        assert_eq!(s.problem_rev(), rev);
        let mut invalid = row("/owned/a.rs", "bad");
        invalid.line = 0;
        assert!(
            s.replace_lsp_diagnostics(batch(uri, 2, vec![invalid]), &hub)
                .is_err()
        );
        assert_eq!(s.problem_rev(), rev);
        assert_eq!(result(&s).items.len(), 2);
        let mut other = batch(uri, 1, vec![row("/owned/a.rs", "other server")]);
        other.server = "other".into();
        s.replace_lsp_diagnostics(other, &hub)
            .expect("other server");
        s.replace_lsp_diagnostics(batch(uri, 1, vec![]), &hub)
            .expect("clear one source");
        assert_eq!(result(&s).items[0].source, "lsp:other");
        assert_eq!(result(&s).items.len(), 1);
    }
}
