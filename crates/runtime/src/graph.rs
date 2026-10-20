//! Resolve source, consent and every input before creating the first execution resource.
use crate::{service::project_error, *};
use devbox_projects::trust::LaunchSnapshot;
use devbox_protocol::Internal;
use std::path::{Path, PathBuf};

pub struct PreparedGraph {
    pub(crate) project_id: String,
    pub(crate) schedule: Option<crate::schedule_model::ScheduleClaim>,
    pub(crate) worktree: Option<String>,
    pub(crate) nodes: Vec<RunDef>,
    pub(crate) automatic_ports: std::collections::HashSet<String>,
}
impl PreparedGraph {
    pub fn ids(&self) -> Vec<&str> {
        self.nodes.iter().map(RunDef::id).collect()
    }
}
impl RuntimeService {
    pub async fn authorize_run_graph(&self, p: &StartRun) -> R<PreparedGraph> {
        self.authorize_graph(p, None).await
    }

    pub(crate) async fn authorize_schedule(
        &self,
        project: &str,
        expected: &Definition,
    ) -> R<PreparedGraph> {
        let RunDef::Schedule(schedule) = &expected.def else {
            unreachable!()
        };
        self.authorize_graph(
            &StartRun {
                project_id: project.into(),
                id: schedule.task.clone(),
                worktree: None,
            },
            Some(expected),
        )
        .await
    }

    async fn authorize_graph(
        &self,
        p: &StartRun,
        schedule: Option<&Definition>,
    ) -> R<PreparedGraph> {
        let _guard = self.project_lock(&p.project_id).await;
        let project = self
            .projects
            .get(&p.project_id)
            .await
            .map_err(project_error)?
            .ok_or_else(|| RuntimeError::ProjectNotFound {
                id: p.project_id.clone(),
            })?;
        let state = self
            .projects
            .run_config_status(&p.project_id)
            .await
            .map_err(project_error)?;
        let (local, _) = self.local(&p.project_id).await?;
        let mut merged = merge(state.config.definitions(), local.clone())?;
        let order = order(&merged, &p.id)?;
        if let Some(expected) = schedule {
            if !merged.defs.contains(expected) {
                return Err(RuntimeError::ProjectConfigChanged {
                    id: p.project_id.clone(),
                }
                .into());
            }
        }
        if schedule.is_some_and(|s| s.source == DefinitionSource::ProjectFile)
            || merged.defs.iter().any(|d| {
                order.contains(&d.def.id().to_owned()) && d.source == DefinitionSource::ProjectFile
            })
        {
            let snapshot = self
                .projects
                .run_snapshot(&p.project_id)
                .await
                .map_err(project_error)?;
            if snapshot.canonical_path() != project.path {
                return Err(invalid(&p.id, "project path changed").into());
            }
            merged = merge(snapshot.config().run_config().definitions(), local)?;
            if let Some(expected) = schedule {
                if !merged.defs.contains(expected) {
                    return Err(RuntimeError::ProjectConfigChanged {
                        id: p.project_id.clone(),
                    }
                    .into());
                }
            }
        }
        prepare(self, p, &project.path, merged, std::slice::from_ref(&p.id)).await
    }

    /// The launch capability owns the exact approved bytes for all launch resources.
    pub async fn prepare_with_snapshot(
        &self,
        p: &StartRun,
        snapshot: &LaunchSnapshot,
    ) -> R<PreparedGraph> {
        if snapshot.preparation().project_id() != p.project_id {
            return Err(RuntimeError::ProjectTrustRequired {
                id: p.project_id.clone(),
            }
            .into());
        }
        let _guard = self.project_lock(&p.project_id).await;
        let (local, _) = self.local(&p.project_id).await?;
        let merged = merge(snapshot.run_config().definitions(), local)?;
        prepare(
            self,
            p,
            snapshot.preparation().canonical_path(),
            merged,
            std::slice::from_ref(&p.id),
        )
        .await
    }

    pub async fn prepare_launch_snapshot(
        &self,
        snapshot: &LaunchSnapshot,
    ) -> R<Option<PreparedGraph>> {
        let roots = &snapshot.launch().services;
        let Some(id) = roots.last() else {
            return Ok(None);
        };
        let project_id = snapshot.preparation().project_id().to_owned();
        let _guard = self.project_lock(&project_id).await;
        let (local, _) = self.local(&project_id).await?;
        let merged = merge(snapshot.run_config().definitions(), local)?;
        for id in roots {
            if !merged
                .defs
                .iter()
                .any(|d| d.def.id() == id && matches!(d.def, RunDef::Service(_)))
            {
                return Err(invalid(id, "launch service definition not found").into());
            }
        }
        let p = StartRun {
            project_id,
            id: id.clone(),
            worktree: None,
        };
        prepare(
            self,
            &p,
            snapshot.preparation().canonical_path(),
            merged,
            roots,
        )
        .await
        .map(Some)
    }

    pub async fn start_with_snapshot(&self, p: StartRun, snapshot: &LaunchSnapshot) -> R<Run> {
        let prepared = self.prepare_with_snapshot(&p, snapshot).await?;
        self.start_prepared_graph(prepared).await
    }
}
fn order(merged: &MergedDefinitions, id: &str) -> R<Vec<String>> {
    if !merged.defs.iter().any(|d| d.def.id() == id) {
        return Err(invalid(id, "definition not found").into());
    }
    Ok(merged.order_for(id))
}
fn invalid(id: &str, reason: &str) -> RuntimeError {
    RuntimeError::DefInvalid {
        id: id.into(),
        reason: reason.into(),
    }
}
async fn canonical_directory(id: &str, path: &Path) -> R<PathBuf> {
    let path = tokio::fs::canonicalize(path)
        .await
        .map_err(|_| invalid(id, "working directory does not exist"))?;
    let text = path
        .to_str()
        .ok_or_else(|| invalid(id, "working directory is not UTF-8"))?;
    if text.chars().any(char::is_control) || !path.is_dir() {
        return Err(invalid(id, "invalid working directory").into());
    }
    Ok(path)
}
async fn prepare(
    service: &RuntimeService,
    p: &StartRun,
    project: &str,
    merged: MergedDefinitions,
    roots: &[String],
) -> R<PreparedGraph> {
    if let Some(execution) = &service.execution {
        if !execution.paths.instance_config_dir.join("env").is_file() {
            return Err(
                invalid(&p.id, "captured instance environment is missing; run setup").into(),
            );
        }
        for (_, path) in devbox_core::systemd::path_environment(&execution.paths) {
            devbox_core::systemd::raw_unit_path(path)
                .map_err(|_| invalid(&p.id, "invalid instance environment path"))?;
        }
    }
    let mut ordered = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for id in roots {
        for node in order(&merged, id)? {
            if seen.insert(node.clone()) {
                ordered.push(node);
            }
        }
    }
    let root = canonical_directory(&p.id, Path::new(project)).await?;
    let mut nodes = Vec::new();
    let mut automatic_ports = std::collections::HashSet::new();
    let mut port_guards = Vec::new();
    for id in ordered {
        let mut def = merged
            .defs
            .iter()
            .find(|d| d.def.id() == id)
            .expect("validated graph")
            .def
            .clone();
        validate_definition(&def)?;
        let port_hint = match &def {
            RunDef::Service(d) => d.ports_hint.first().copied(),
            _ => None,
        };
        let command_secret = def.command().is_some_and(|s| s.contains("{{secret:"));
        let (cwd, worktree, target, env, secrets) = match &mut def {
            RunDef::Task(d) => (&mut d.cwd, &d.worktree, d.target, &mut d.env, &d.secrets),
            RunDef::Service(d) => (&mut d.cwd, &d.worktree, d.target, &mut d.env, &d.secrets),
            RunDef::Schedule(_) => {
                return Err(invalid(&id, "schedules are started by the scheduler").into());
            }
        };
        if target != RunTarget::Linux {
            return Err(invalid(&id, "Windows execution is not available yet").into());
        }
        if command_secret {
            return Err(invalid(
                &id,
                "secret references are only supported in environment values",
            )
            .into());
        }
        if secrets.iter().any(|name| {
            !devbox_secrets::valid_name(name)
                || [
                    "HOME",
                    "XDG_CONFIG_HOME",
                    "XDG_DATA_HOME",
                    "XDG_STATE_HOME",
                    "XDG_RUNTIME_DIR",
                    "DEVBOX_INSTANCE",
                    "DEVBOX_PORT",
                    "INVOCATION_ID",
                    "NOTIFY_SOCKET",
                    "LISTEN_FDS",
                    "LISTEN_PID",
                ]
                .contains(&name.as_str())
        }) {
            return Err(invalid(&id, "invalid secret name").into());
        }
        for (key, value) in env.iter() {
            if key.bytes().enumerate().any(|(n, c)| {
                !(c == b'_' || c.is_ascii_alphabetic() || n > 0 && c.is_ascii_digit())
            }) || [
                "HOME",
                "XDG_CONFIG_HOME",
                "XDG_DATA_HOME",
                "XDG_STATE_HOME",
                "XDG_RUNTIME_DIR",
                "DEVBOX_INSTANCE",
                "INVOCATION_ID",
                "NOTIFY_SOCKET",
                "LISTEN_FDS",
                "LISTEN_PID",
            ]
            .contains(&key.as_str())
                || value.chars().any(char::is_control)
            {
                return Err(invalid(&id, "invalid or reserved execution environment").into());
            }
        }
        let mut agent_port = None;
        let base = if let Some(worktree) = p
            .worktree
            .as_ref()
            .or(worktree.as_ref())
            .filter(|id| id.as_str() != "main")
        {
            let resolver = service
                .execution()?
                .worktree
                .as_ref()
                .ok_or_else(|| invalid(&id, "agent worktree resolution is unavailable"))?;
            let resolved = resolver(p.project_id.clone(), worktree.clone()).await?;
            agent_port = Some(resolved.port);
            canonical_directory(&id, Path::new(&resolved.path)).await?
        } else {
            root.clone()
        };
        let selected = canonical_directory(&id, &base.join(cwd.as_deref().unwrap_or("."))).await?;
        *cwd = Some(selected.to_string_lossy().into_owned());
        let explicit = env
            .get("DEVBOX_PORT")
            .map(|v| {
                v.parse::<u16>()
                    .ok()
                    .filter(|v| *v > 0)
                    .ok_or_else(|| invalid(&id, "DEVBOX_PORT must be 1..65535"))
            })
            .transpose()?;
        if port_hint == Some(0) || matches!((explicit, port_hint), (Some(a), Some(b)) if a != b) {
            return Err(invalid(&id, "DEVBOX_PORT and ports_hint disagree").into());
        }
        let port = if let Some(port) = explicit.or(port_hint).or(agent_port) {
            port
        } else {
            let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
                .internal("select execution port")?;
            let port = listener
                .local_addr()
                .internal("read selected execution port")?
                .port();
            port_guards.push(listener);
            automatic_ports.insert(id.clone());
            port
        };
        env.insert("DEVBOX_PORT".into(), port.to_string());
        let _secrets = service.secret_payload(&def).await?;
        let child = launch::child_args(&def);
        devbox_core::input::validate_command_argv(child.iter().map(|arg| arg.as_bytes()))
            .map_err(|_| invalid(&id, "complete child argv exceeds the execution input limit"))?;
        nodes.push(def);
    }
    service
        .execution
        .as_ref()
        .map(|e| e.executable.clone())
        .map(Ok)
        .unwrap_or_else(std::env::current_exe)
        .internal("resolve execution binary")?
        .to_str()
        .filter(|s| !s.chars().any(char::is_control))
        .ok_or_else(|| invalid(&p.id, "invalid execution binary path"))?;
    Ok(PreparedGraph {
        schedule: None,
        project_id: p.project_id.clone(),
        worktree: p.worktree.clone(),
        nodes,
        automatic_ports,
    })
}
