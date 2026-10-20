use crate::{
    model::*,
    service::{AgentsService, Result, changed, now_ms},
    state::AgentState,
    store,
    tracker::Binding,
};
use devbox_protocol::{Fail, Internal};
use std::{collections::HashSet, path::Path, sync::Arc};

fn allocate_port(tasks: &[AgentTask]) -> std::result::Result<u16, AgentsError> {
    allocate_port_from(tasks, 20000)
}
fn allocate_port_from(tasks: &[AgentTask], start: u16) -> std::result::Result<u16, AgentsError> {
    let used: HashSet<_> = tasks
        .iter()
        .filter(|t| t.state != AgentState::Closed)
        .map(|t| t.port)
        .collect();
    for base in (u32::from(start)..=65520).step_by(10) {
        let base = base as u16;
        if used.contains(&base) {
            continue;
        }
        let listeners: std::io::Result<Vec<_>> = (base..=base + 9)
            .map(|p| std::net::TcpListener::bind(("127.0.0.1", p)))
            .collect();
        if listeners.is_ok() {
            return Ok(base);
        }
    }
    Err(AgentsError::InvalidInput)
}
impl AgentsService {
    pub async fn create(self: &Arc<Self>, p: CreateAgentTask) -> Result<AgentTask> {
        let snapshot = self
            .projects
            .setup_snapshot(&p.project_id)
            .await
            .map_err(project_trust_failure)?;
        self.create_with_snapshot(p, snapshot).await
    }

    /// Composite launch supplies the same approved configuration as its terminals.
    pub async fn create_with_snapshot(
        self: &Arc<Self>,
        p: CreateAgentTask,
        snapshot: devbox_projects::trust::SetupSnapshot,
    ) -> Result<AgentTask> {
        crate::limits::input(&p.prompt)?;
        let title = crate::plan::checked_title(&p.title)?.to_owned();
        let _reservation = self.reservations.lock().await;
        let project = self
            .projects
            .get(&p.project_id)
            .await
            .map_err(|e| Fail::Internal(format!("agent project: {e:?}")))?
            .ok_or_else(|| AgentsError::ProjectNotFound {
                id: p.project_id.clone(),
            })?;
        let profile = crate::profile::resolve(&self.profiles, &p.profile).ok_or_else(|| {
            AgentsError::ProfileNotFound {
                id: p.profile.clone(),
            }
        })?;
        if snapshot.project_id() != p.project_id || snapshot.canonical_path() != project.path {
            return Err(AgentsError::UnsafeWorktree.into());
        }
        let base = match p.base_branch {
            Some(b) => b,
            None => devbox_git::current_branch(Path::new(&project.path))
                .await
                .map_err(git_failure)?
                .ok_or(AgentsError::InvalidInput)?,
        };
        devbox_git::run::resolve(Path::new(&project.path), &base)
            .await
            .map_err(|error| match error {
                devbox_git::GitError::Failed { .. } => AgentsError::InvalidInput.into(),
                other => git_failure(other),
            })?;
        let mut taken: HashSet<String> = devbox_git::branches(Path::new(&project.path))
            .await
            .map_err(git_failure)?
            .into_iter()
            .filter_map(|b| b.strip_prefix("agent/").map(str::to_owned))
            .collect();
        let now = now_ms();
        let id = format!("a{}", uuid::Uuid::new_v4().simple());
        let repo = std::path::PathBuf::from(&project.path);
        let svc = self.clone();
        let hub = self.hub.clone();
        let handle = tokio::runtime::Handle::current();
        let prep_project = project.clone();
        let committed_reservation = self.background.token();
        let committed = self
            .db
            .write_checked_with_commit(
                move |tx| {
                    let project_exists: bool = tx.query_row(
                        "SELECT EXISTS(SELECT 1 FROM projects_projects WHERE id=?1)",
                        [&project.id],
                        |row| row.get(0),
                    )?;
                    if !project_exists {
                        return Ok(Err(AgentsError::ProjectNotFound { id: project.id }));
                    }
                    let existing = store::list(tx, i64::MIN)?;
                    for task in &existing {
                        if task.project_id == project.id
                            && let Some(branch) = &task.branch
                            && let Some(slug) = branch.strip_prefix("agent/")
                        {
                            taken.insert(slug.into());
                        }
                    }
                    let (branch, worktree) = if p.worktree {
                        let base_slug = crate::plan::slug(&title, now as u64);
                        let selected = loop {
                            let slug = match crate::plan::unique_slug(&base_slug, &taken) {
                                Ok(v) => v,
                                Err(e) => return Ok(Err(e)),
                            };
                            let path = match crate::plan::worktree_dir(&repo, &slug) {
                                Ok(v) => v,
                                Err(e) => return Ok(Err(e)),
                            };
                            if std::fs::symlink_metadata(&path).is_ok() {
                                taken.insert(slug);
                                continue;
                            }
                            break (slug, path);
                        };
                        (
                            Some(
                                crate::plan::branch_for(&selected.0)
                                    .map_err(|_| rusqlite::Error::InvalidQuery)?,
                            ),
                            Some(selected.1.to_string_lossy().into_owned()),
                        )
                    } else {
                        (None, None)
                    };
                    let port = match allocate_port(&existing) {
                        Ok(v) => v,
                        Err(e) => return Ok(Err(e)),
                    };
                    let task = AgentTask {
                        id,
                        project_id: project.id,
                        title,
                        profile: profile.id.clone(),
                        base_branch: base,
                        branch,
                        worktree_path: worktree.clone(),
                        workdir: worktree.unwrap_or(project.path),
                        session_id: None,
                        terminal_id: None,
                        tool_exited: false,
                        state: AgentState::Preparing,
                        reason: None,
                        attention: None,
                        port,
                        prompt: p.prompt,
                        follow_ups: vec![],
                        tokens: None,
                        pr_url: None,
                        closed_how: None,
                        created_ms: now,
                        updated_ms: now,
                        closed_ms: None,
                        closed_commits: None,
                        closed_head_oid: None,
                        cleanup_error: None,
                        tokens_truncated: false,
                        activity_seq: 0,
                    };
                    if let Err(error) = crate::limits::serialized(&task, crate::limits::TASK_BYTES)
                    {
                        return Ok(Err(match error {
                            Fail::Domain(error) => error,
                            Fail::Internal(_) => AgentsError::InvalidInput,
                        }));
                    }
                    store::put(tx, &task)?;
                    let mut binding = Binding::new(now);
                    binding.launch_id = Some(uuid::Uuid::new_v4().simple().to_string());
                    store::put_binding(tx, &task.id, &binding)?;
                    Ok(Ok((task, profile)))
                },
                move |(task, profile), rev| {
                    let _tracked_reservation = committed_reservation;
                    changed(&hub, task, rev, false);
                    let task = task.clone();
                    let profile = profile.clone();
                    let background = svc.background.clone();
                    let svc = svc.clone();
                    let _entered = handle.enter();
                    background.spawn(async move {
                        if let Err(error) =
                            svc.prepare(&task, &profile, &prep_project, &snapshot).await
                        {
                            tracing::error!(?error,agent=%task.id,"agent preparation failed");
                            let _ = svc.preparation_failed(&task.id, &error).await;
                        }
                    });
                },
            )
            .await
            .internal("reserve agent")?
            .map_err(Fail::Domain)?;
        Ok(committed.0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocate_port_skips_blocks_with_a_listening_port() {
        let base = allocate_port(&[]).expect("available owned block");
        let listener =
            std::net::TcpListener::bind(("127.0.0.1", base + 3)).expect("owned test port");
        assert!(allocate_port_from(&[], base).expect("owned fixture") >= base + 10);
        drop(listener);
    }
}
