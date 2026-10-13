//! Compose project configuration with terminal and agent services without domain cycles.
use super::state::Services;
use devbox_agents::{CreateAgentTask, state::AgentState};
use devbox_projects::*;
use devbox_protocol::Fail;
use devbox_terminal::{CreateSession, ListSessions};
use futures::{StreamExt, stream};
use std::{path::Path, sync::Arc};

type Result<T> = std::result::Result<T, Fail<ProjectsError>>;

// Only public domain codes enter the launch error. Raw process causes stay in diagnostics.
fn launch_failure<E: devbox_protocol::DomainError>(
    error: Fail<E>,
    result: &LaunchResult,
    resource: &str,
) -> Fail<ProjectsError> {
    let reason = match error {
        Fail::Domain(error) => devbox_protocol::to_wire(&error).code,
        Fail::Internal(reason) => {
            tracing::error!(%reason, resource, "project launch failed");
            "internal".into()
        }
    };
    ProjectsError::LaunchFailed {
        terminals: result.terminals,
        agents: result.agents,
        message: format!("{resource}: {reason}"),
    }
    .into()
}

pub async fn launch(s: Arc<Services>, params: LaunchProject) -> Result<LaunchResult> {
    let project = s
        .projects
        .get(&params.id)
        .await?
        .ok_or_else(|| ProjectsError::NotFound {
            id: params.id.clone(),
        })?;
    let snapshot = s.projects.launch_snapshot(&project.id).await?;
    if snapshot.preparation().canonical_path() != project.path {
        return Err(ProjectsError::PathInvalid { path: project.path }.into());
    }
    let config = snapshot.launch().clone();
    // Preflight the whole configuration before the first terminal or agent is reserved.
    if !tokio::fs::metadata(&project.path)
        .await
        .is_ok_and(|m| m.is_dir())
    {
        return Err(ProjectsError::PathInvalid { path: project.path }.into());
    }
    let profiles = s.agents.profiles();
    for agent in &config.agents {
        if !profiles
            .items
            .iter()
            .any(|profile| profile.id == agent.tool)
        {
            return Err(ProjectsError::LaunchProfileNotFound {
                id: agent.tool.clone(),
            }
            .into());
        }
    }
    let mut result = LaunchResult::default();
    let base = if config.agents.is_empty() {
        None
    } else {
        let repo = Path::new(&project.path);
        let branch = devbox_git::current_branch(repo)
            .await
            .map_err(|error| {
                launch_failure(
                    devbox_agents::model::git_failure(error),
                    &result,
                    "agent preflight",
                )
            })?
            .unwrap_or_else(|| "main".into());
        devbox_git::branches(repo).await.map_err(|error| {
            launch_failure(
                devbox_agents::model::git_failure(error),
                &result,
                "agent preflight",
            )
        })?;
        if config.agents.iter().any(|agent| agent.worktree) {
            devbox_git::run::resolve(repo, &branch)
                .await
                .map_err(|error| {
                    launch_failure(
                        devbox_agents::model::git_failure(error),
                        &result,
                        "worktree preflight",
                    )
                })?;
        }
        Some(branch)
    };
    for (index, terminal) in config.terminals.into_iter().enumerate() {
        let created = s
            .terminal
            .create(CreateSession {
                project_id: Some(project.id.clone()),
                cwd: None,
                title: Some(terminal.name),
                command: terminal.command,
                attach_zellij: None,
            })
            .await
            .map_err(|error| launch_failure(error, &result, &format!("terminal {}", index + 1)))?;
        result.terminal_ids.push(created.id);
        result.terminals += 1;
    }
    for (index, agent) in config.agents.into_iter().enumerate() {
        // Existing agents.create owns branch/slug collision handling and preparation lifecycle.
        s.agents
            .create_with_snapshot(
                CreateAgentTask {
                    project_id: project.id.clone(),
                    title: format!("시작 구성 {}", index + 1),
                    profile: agent.tool,
                    base_branch: base.clone(),
                    worktree: agent.worktree,
                    prompt: agent.prompt.unwrap_or_default(),
                },
                snapshot.preparation().clone(),
            )
            .await
            .map_err(|error| launch_failure(error, &result, &format!("agent {}", index + 1)))?;
        result.agents += 1;
    }
    // Services are intentionally deferred to S2.
    result.rev = s.db.rev().to_string();
    Ok(result)
}

pub async fn summary(s: Arc<Services>, _: SummaryProjects) -> Result<ProjectSummaryList> {
    let projects = s.projects.list(ListProjects {}).await?.items;
    let (terminals, agents) = tokio::try_join!(
        async {
            s.terminal
                .list(ListSessions { project_id: None })
                .await
                .map_err(|error| Fail::Internal(format!("project terminal summary: {error:?}")))
        },
        async {
            s.agents
                .all_tasks(Some(0))
                .await
                .map_err(|error| Fail::Internal(format!("project agent summary: {error:?}")))
        }
    )?;
    let mut items = stream::iter(projects.into_iter().enumerate().map(|(index, project)| {
        let terminals = &terminals.items;
        let agents = &agents;
        async move {
            let mut item = ProjectSummary {
                id: project.id.clone(), branch: None, dirty: None, ahead: None, behind: None,
                terminals: terminals.iter().filter(|terminal| terminal.alive && terminal.project_id.as_deref() == Some(&project.id)).count() as u32,
                agents: ProjectAgentCounts {
                    open: agents.iter().filter(|agent| agent.project_id == project.id && agent.state != AgentState::Closed).count() as u32,
                    attention: agents.iter().filter(|agent| agent.project_id == project.id && agent.state != AgentState::Closed && devbox_agents::state::is_attention(agent.state)).count() as u32,
                }, error: None,
            };
            let path = Path::new(&project.path);
            if !tokio::fs::metadata(path).await.is_ok_and(|m| m.is_dir()) {
                item.error = Some(ProjectSummaryIssue { message: "projects.path_invalid".into() });
            } else {
                match devbox_git::status(path).await {
                    Ok(status) => {
                        item.branch = status.branch;
                        item.dirty = Some(!status.clean);
                        item.ahead = Some(status.ahead);
                        item.behind = Some(status.behind);
                    }
                    Err(devbox_git::GitError::NotRepo)
                        if tokio::fs::symlink_metadata(path.join(".git"))
                            .await
                            .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound) => {
                        item.dirty = Some(false);
                        item.ahead = Some(0);
                        item.behind = Some(0);
                    }
                    Err(error) => {
                        tracing::warn!(project = %project.id, ?error, "project Git summary unavailable");
                        item.error = Some(ProjectSummaryIssue { message: devbox_protocol::to_wire(&error).code });
                    }
                }
            }
            (index, item)
        }
    })).buffer_unordered(4).collect::<Vec<_>>().await;
    items.sort_by_key(|(index, _)| *index);
    Ok(ProjectSummaryList {
        items: items.into_iter().map(|(_, item)| item).collect(),
    })
}
