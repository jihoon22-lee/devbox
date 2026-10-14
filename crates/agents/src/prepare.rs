use crate::{
    model::{AgentTask, AgentsError},
    profile::{Hooks, ToolProfile},
    service::{AgentsService, Result, apply_event, changed, now_ms},
    state::AgentEvent,
};
use devbox_protocol::Fail;
use std::{fs::OpenOptions, io::Write, path::Path};
pub(crate) fn private_dir(path: &Path) -> std::io::Result<()> {
    for ancestor in path.ancestors() {
        if std::fs::symlink_metadata(ancestor).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(std::io::Error::other("symlink private directory"));
        }
    }
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}
pub(crate) fn private_file(path: &Path, body: &str, executable: bool) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        for ancestor in parent.ancestors() {
            if std::fs::symlink_metadata(ancestor).is_ok_and(|m| m.file_type().is_symlink()) {
                return Err(std::io::Error::other("symlink private file ancestor"));
            }
        }
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(if executable { 0o700 } else { 0o600 });
    }
    let mut file = options.open(path)?;
    file.write_all(body.as_bytes())?;
    file.sync_all()
}
fn internal(context: &str, error: impl std::fmt::Display) -> Fail<AgentsError> {
    Fail::Internal(format!("{context}: {error}"))
}
#[derive(Debug)]
pub(crate) enum PreparationFailure {
    Failed {
        error: Box<Fail<AgentsError>>,
        launch_id: Option<String>,
    },
    Superseded,
    CleanupIncomplete {
        error: Box<Fail<AgentsError>>,
        cleanup: Box<Fail<AgentsError>>,
        launch_id: String,
        terminal_id: String,
    },
}
impl From<Fail<AgentsError>> for PreparationFailure {
    fn from(error: Fail<AgentsError>) -> Self {
        Self::Failed {
            error: Box::new(error),
            launch_id: None,
        }
    }
}
type PreparationResult = std::result::Result<(), PreparationFailure>;
impl AgentsService {
    pub(crate) async fn prepare(
        &self,
        task: &AgentTask,
        profile: &ToolProfile,
        project: &devbox_projects::Project,
        config: &devbox_projects::trust::SetupSnapshot,
    ) -> PreparationResult {
        // Capture correlation before asynchronous preparation or terminal creation.
        let launch_id = self
            .launch_binding(&task.id)
            .await?
            .launch_id
            .ok_or_else(|| internal("agent launch", "missing launch identity"))?;
        self.prepare_owned(task, profile, project, config, &launch_id)
            .await
            .map_err(|failure| match failure {
                PreparationFailure::Failed { error, .. } => PreparationFailure::Failed {
                    error,
                    launch_id: Some(launch_id),
                },
                other => other,
            })
    }
    async fn prepare_owned(
        &self,
        task: &AgentTask,
        profile: &ToolProfile,
        project: &devbox_projects::Project,
        config: &devbox_projects::trust::SetupSnapshot,
        launch_id: &str,
    ) -> PreparationResult {
        let repo = Path::new(&project.path);
        if let (Some(path), Some(branch)) = (&task.worktree_path, &task.branch) {
            if let Err(error) =
                devbox_git::mutate::worktree_add(repo, Path::new(path), branch, &task.base_branch)
                    .await
            {
                if let devbox_git::GitError::WorktreeAddPartial {
                    branch: observed_branch,
                    path: observed_path,
                    path_exists,
                    branch_exists,
                    worktree_registered,
                    ..
                } = &error
                    && observed_branch == branch
                    && observed_path == path
                {
                    self.record_resources(
                        &task.id,
                        crate::tracker::Resources::Partial {
                            path_exists: *path_exists,
                            branch_exists: *branch_exists,
                            worktree_registered: *worktree_registered,
                        },
                    )
                    .await?;
                }
                return Err(crate::model::git_failure(error).into());
            }
            self.record_resources(&task.id, crate::tracker::Resources::Owned)
                .await?;
            devbox_projects::config::copy_files(repo, Path::new(path), config.copy())
                .map_err(|e| internal("agent copy", e))?;
        }
        let agents_dir = self.paths.data_dir.join("agents");
        private_dir(&agents_dir).map_err(|e| internal("agent data directory", e))?;
        let data = agents_dir.join(&task.id);
        // create_dir is the task's ownership reservation. Never accept an existing foreign directory.
        std::fs::create_dir(&data).map_err(|e| internal("reserve agent data", e))?;
        private_dir(&data).map_err(|e| internal("agent data permissions", e))?;
        let prompt = data.join("prompt.md");
        let script = data.join("start.sh");
        private_file(&prompt, &task.prompt, false).map_err(|e| internal("agent prompt", e))?;
        let phase = self.phase_path(task, launch_id);
        private_file(&phase, "starting\n", false).map_err(|e| internal("agent phase", e))?;
        let settings = if profile.hooks == Hooks::Claude {
            let _hook_files = self.hook_files.lock().await;
            let hooks = self.paths.data_dir.join("agent-hooks");
            private_dir(&hooks).map_err(|e| internal("agent hooks directory", e))?;
            let path = hooks.join("claude.json");
            let body = crate::hooks::claude_settings(&self.devbox_bin);
            match private_file(&path, &body, false) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    if !std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_file())
                        || std::fs::read_to_string(&path)
                            .map_err(|e| internal("read agent hooks", e))?
                            != body
                    {
                        return Err(internal("agent hooks", "existing file differs").into());
                    }
                }
                Err(e) => return Err(internal("agent hooks", e).into()),
            };
            Some(path.to_string_lossy().into_owned())
        } else {
            None
        };
        let plan = crate::start::StartPlan {
            devbox: self.devbox_bin.clone(),
            instance: self.paths.instance.as_str().into(),
            agent_id: task.id.clone(),
            launch_id: launch_id.to_owned(),
            phase_file: phase.to_string_lossy().into_owned(),
            port: task.port,
            workdir: task.workdir.clone(),
            setup: config.setup().to_vec(),
            tool_command: profile.command.clone(),
            hooks: profile.hooks,
            claude_settings: settings,
            prompt_file: Some(prompt.to_string_lossy().into_owned()),
            resume_session: None,
            use_scope: self.use_scope,
        };
        private_file(&script, &crate::start::render_start_script(&plan), true)
            .map_err(|e| internal("agent start script", e))?;
        let env = vec![
            ("DEVBOX_BIN".into(), self.devbox_bin.clone()),
            ("DEVBOX_AGENT_ID".into(), task.id.clone()),
            (
                "DEVBOX_INSTANCE".into(),
                self.paths.instance.as_str().into(),
            ),
            ("DEVBOX_PORT".into(), task.port.to_string()),
            (
                "DEVBOX_PROMPT_FILE".into(),
                prompt.to_string_lossy().into_owned(),
            ),
        ];
        // Keep the exact fresh identity even if tmux replies ambiguously or DB publication fails.
        let terminal_uuid = uuid::Uuid::new_v4();
        let terminal_id = format!("t{}", terminal_uuid.simple());
        let launched = self
            .terminal
            .create_internal_with_id(
                (devbox_terminal::SessionKind::Agent, terminal_uuid),
                Some(task.project_id.clone()),
                task.workdir.clone().into(),
                task.title.clone(),
                format!("bash {}", crate::plan::sh_quote(&script.to_string_lossy())),
                &env,
            )
            .await;
        // Do not hold this lock during creation: synchronous setup hooks need it.
        let _guard = self.actions.lock().await;
        let error = match launched {
            Err(error) => internal("agent terminal", format!("{error:?}")),
            Ok(session) => {
                let key = task.id.clone();
                let generation = launch_id.to_owned();
                let hub = self.hub.clone();
                let published = self
                    .db
                    .write_checked_with_commit(
                        move |tx| {
                            let Some(mut current) = crate::store::get(tx, &key)? else {
                                return Ok(Err(()));
                            };
                            let Some(binding) = crate::store::binding(tx, &key)? else {
                                return Ok(Err(()));
                            };
                            if binding.launch_id.as_deref() != Some(&generation)
                                || current.state == crate::state::AgentState::Closed
                            {
                                return Ok(Err(()));
                            }
                            current.terminal_id = Some(session.id);
                            current.updated_ms = now_ms();
                            crate::store::put(tx, &current)?;
                            Ok(Ok(current))
                        },
                        move |current, rev| changed(&hub, current, rev, false),
                    )
                    .await;
                match published {
                    Ok(Ok(_)) => return Ok(()),
                    Ok(Err(())) => internal("agent terminal binding", "launch superseded"),
                    Err(error) => internal("agent terminal binding", error),
                }
            }
        };
        let current = self.launch_binding(&task.id).await;
        let same_generation = current
            .as_ref()
            .is_ok_and(|b| b.launch_id.as_deref() == Some(launch_id));
        let cleanup = if same_generation {
            let mut owned = task.clone();
            owned.terminal_id = Some(terminal_id.clone());
            self.quiesce(&owned).await
        } else {
            // The task's scope name may already belong to a newer launch. Only the old UUID is safe.
            self.terminal
                .close_internal(&terminal_id)
                .await
                .map(|_| ())
                .map_err(|e| internal("close superseded preparation terminal", format!("{e:?}")))
        };
        if let Err(cleanup) = cleanup {
            return Err(PreparationFailure::CleanupIncomplete {
                error: Box::new(error),
                cleanup: Box::new(cleanup),
                launch_id: launch_id.into(),
                terminal_id,
            });
        }
        match current {
            Err(cleanup) => Err(PreparationFailure::CleanupIncomplete {
                error: Box::new(error),
                cleanup: Box::new(cleanup),
                launch_id: launch_id.into(),
                terminal_id,
            }),
            Ok(_) if !same_generation => Err(PreparationFailure::Superseded),
            Ok(_) => Err(error.into()),
        }
    }
    pub(crate) async fn record_resources(
        &self,
        id: &str,
        resources: crate::tracker::Resources,
    ) -> Result<()> {
        let id = id.to_owned();
        self.db
            .write(move |tx| {
                let mut binding =
                    crate::store::binding(tx, &id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
                binding.resources = resources;
                crate::store::put_binding(tx, &id, &binding)
            })
            .await
            .map_err(|e| internal("record agent resources", e))?;
        Ok(())
    }
    pub(crate) async fn reconcile_resources(&self, task: &AgentTask) -> Result<()> {
        let (Some(path), Some(branch)) = (&task.worktree_path, &task.branch) else {
            return Ok(());
        };
        let Some(project) = self
            .projects
            .get(&task.project_id)
            .await
            .map_err(|e| internal("reconcile project", format!("{e:?}")))?
        else {
            return Ok(());
        };
        // Creation reserved an absent deterministic path/ref. Inspect exactly those resources;
        // never remove partial hook-written files or infer rollback from a process error.
        let path_exists = std::fs::symlink_metadata(path).is_ok();
        let branch_exists = devbox_git::branches(Path::new(&project.path))
            .await
            .ok()
            .map(|refs| refs.iter().any(|r| r == branch));
        let registered = devbox_git::run::output(
            Path::new(&project.path),
            &["worktree", "list", "--porcelain", "-z"],
        )
        .await
        .ok()
        .and_then(|(code, out, _)| {
            if code == 0 {
                Some(
                    out.split_terminator('\0')
                        .any(|field| field == format!("worktree {path}")),
                )
            } else {
                None
            }
        });
        self.record_resources(
            &task.id,
            crate::tracker::Resources::Partial {
                path_exists,
                branch_exists,
                worktree_registered: registered,
            },
        )
        .await?;
        if !path_exists && branch_exists == Some(false) && registered == Some(false) {
            self.update(&task.id, move |t| {
                t.branch = None;
                t.worktree_path = None;
                t.workdir = project.path;
                t.updated_ms = now_ms();
                true
            })
            .await?;
        }
        Ok(())
    }
    pub(crate) async fn preparation_failed(
        &self,
        id: &str,
        failure: &PreparationFailure,
    ) -> Result<()> {
        let (error, generation) = match failure {
            PreparationFailure::Failed {
                error,
                launch_id: Some(generation),
            } => (error, generation.clone()),
            PreparationFailure::CleanupIncomplete {
                error,
                cleanup,
                launch_id,
                terminal_id,
            } => {
                tracing::error!(?error, ?cleanup, agent=id, launch=%launch_id, terminal=%terminal_id, "preparation cleanup incomplete; retained possibly live generation");
                return Ok(());
            }
            // Without a captured generation no failure may overwrite a current launch.
            _ => return Ok(()),
        };
        let _guard = self.actions.lock().await;
        let reason = match error.as_ref() {
            Fail::Domain(AgentsError::WorktreeAddPartial { .. }) => "worktree_add_partial",
            Fail::Domain(AgentsError::BranchExists { .. }) => "branch_exists",
            _ => "preparation_failed",
        }
        .to_owned();
        let task = self.get(id).await?;
        let key = id.to_owned();
        let resources = self
            .db
            .read(move |tx| crate::store::binding(tx, &key))
            .await
            .map_err(|e| internal("read resource ownership", e))?
            .map(|b| b.resources)
            .unwrap_or_default();
        let clear =
            matches!(resources, crate::tracker::Resources::Planned) && task.worktree_path.is_some();
        let repo_path = if clear {
            self.projects
                .get(&task.project_id)
                .await
                .map_err(|e| internal("failure project", format!("{e:?}")))?
                .map(|p| p.path)
        } else {
            None
        };
        let id = id.to_owned();
        let hub = self.hub.clone();
        let _ = self
            .db
            .write_checked_with_commit(
                move |tx| {
                    let Some(mut task) = crate::store::get(tx, &id)? else {
                        return Ok(Err(()));
                    };
                    let Some(binding) = crate::store::binding(tx, &id)? else {
                        return Ok(Err(()));
                    };
                    if binding.launch_id.as_deref() != Some(&generation)
                        || task.state == crate::state::AgentState::Closed
                    {
                        return Ok(Err(()));
                    }
                    let previous = task.state;
                    let event = if task.state == crate::state::AgentState::Preparing {
                        AgentEvent::SetupFailed { code: -1 }
                    } else {
                        AgentEvent::Exited { code: -1 }
                    };
                    let updated = apply_event(&mut task, &event, now_ms());
                    if updated {
                        task.terminal_id = None;
                        task.reason = Some(reason.clone());
                        if let Some(attention) = &mut task.attention {
                            attention.message = reason;
                        }
                        if clear {
                            task.branch = None;
                            task.worktree_path = None;
                            if let Some(repo) = repo_path {
                                task.workdir = repo;
                            }
                        }
                    }
                    if !updated {
                        return Ok(Err(()));
                    }
                    crate::store::put(tx, &task)?;
                    Ok(Ok((task, previous)))
                },
                move |(task, previous), rev| changed(&hub, task, rev, task.state != *previous),
            )
            .await
            .map_err(|e| internal("record preparation failure", e))?;
        Ok(())
    }
}
