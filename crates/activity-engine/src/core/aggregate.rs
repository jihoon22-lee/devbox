use crate::core::models::{GitDay, ProjectCommit};
use std::sync::atomic::AtomicBool;

/// Use the same exact-range, worktree-deduplicated history as digest/export.
/// Native Git waits never occupy the shared async command executor.
pub async fn collect_git(projects: &[String], day_start: i64, day_end: i64) -> GitDay {
    let owned = projects.to_vec();
    let rows = tokio::task::spawn_blocking(move || {
        crate::core::git_activity::collect(&owned, day_start, day_end, &AtomicBool::new(false))
    })
    .await;
    let projects = match rows {
        Ok(Ok(rows)) => rows
            .into_iter()
            .map(|row| ProjectCommit {
                path: row.path,
                commits: row.timestamps.len() as u32,
                error_code: row.error_code,
            })
            .collect(),
        _ => projects
            .iter()
            .map(|path| ProjectCommit {
                path: path.clone(),
                commits: 0,
                error_code: Some("git_failed".into()),
            })
            .collect::<Vec<_>>(),
    };
    GitDay {
        total_commits: projects
            .iter()
            .fold(0u32, |sum, project| sum.saturating_add(project.commits)),
        projects,
    }
}
