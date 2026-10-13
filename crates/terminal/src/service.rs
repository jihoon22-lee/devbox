use crate::{
    TerminalChangedTopic,
    model::*,
    store,
    tmux::{Tmux, TmuxError},
};
use devbox_core::{Db, Hub, Paths, config::TerminalConfig, current_rev, sh_quote};
use devbox_projects::ProjectsService;
use devbox_protocol::{Done, Fail, Internal};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;
type Result<T> = std::result::Result<T, Fail<TerminalError>>;
pub struct TerminalService {
    db: Db,
    hub: Arc<Hub>,
    paths: Paths,
    projects: Arc<ProjectsService>,
    tmux: Tmux,
    config: TerminalConfig,
    mutations: Mutex<()>,
    attachments: crate::attach::Attachments,
}
fn changed(hub: &Hub, rev: u64) {
    hub.publish::<TerminalChangedTopic>(
        rev,
        &TerminalChanged {
            rev: rev.to_string(),
        },
    );
}
fn tmux_error(error: TmuxError) -> Fail<TerminalError> {
    Fail::Internal(format!("terminal tmux: {error}"))
}
async fn canonical_cwd(path: PathBuf) -> Result<String> {
    let original = path.to_string_lossy().into_owned();
    if !path.is_absolute() {
        return Err(TerminalError::CwdInvalid { path: original }.into());
    }
    let canonical = tokio::fs::canonicalize(path)
        .await
        .map_err(|_| TerminalError::CwdInvalid {
            path: original.clone(),
        })?;
    let path = canonical
        .to_str()
        .filter(|path| !path.chars().any(char::is_control))
        .ok_or_else(|| TerminalError::CwdInvalid {
            path: original.clone(),
        })?;
    if !tokio::fs::metadata(&canonical)
        .await
        .map_err(|_| TerminalError::CwdInvalid {
            path: original.clone(),
        })?
        .is_dir()
    {
        return Err(TerminalError::CwdInvalid { path: original }.into());
    }
    Ok(path.into())
}
impl TerminalService {
    pub fn new(
        db: Db,
        hub: Arc<Hub>,
        paths: Paths,
        projects: Arc<ProjectsService>,
        config: TerminalConfig,
    ) -> Self {
        Self {
            tmux: Tmux::for_paths(&paths),
            db,
            hub,
            paths,
            projects,
            config,
            mutations: Mutex::new(()),
            attachments: Default::default(),
        }
    }
    pub async fn attach(
        &self,
        ctx: devbox_protocol::Ctx,
        p: AttachSession,
    ) -> Result<devbox_protocol::StreamOpened> {
        crate::attach::validate(p.cols, p.rows)?;
        let id = p.id.clone();
        if self
            .db
            .read(move |tx| store::get(tx, &id))
            .await
            .internal("find terminal")?
            .is_none()
            || !self
                .tmux
                .list()
                .await
                .map_err(tmux_error)?
                .iter()
                .any(|session| session.name == p.id)
        {
            return Err(TerminalError::NotFound { id: p.id }.into());
        }
        let history = self
            .tmux
            .capture_history(&p.id, 2000)
            .await
            .map_err(tmux_error)?;
        let dimensions = crate::attach::dimensions(&self.tmux, &p).await?;
        if ctx.cancel.is_cancelled() {
            return Err(TerminalError::StreamUnavailable {}.into());
        }
        let ends = ctx
            .open_stream()
            .map_err(|_| TerminalError::StreamUnavailable {})?;
        let stream_id = ends.id;
        let tx = ends.tx.clone();
        if let Err(error) =
            crate::attach::start(&self.tmux, &self.attachments, p, history, ends, dimensions)
        {
            tx.finish().await;
            return Err(error);
        }
        Ok(devbox_protocol::StreamOpened { stream_id })
    }
    pub async fn resize(&self, ctx: devbox_protocol::Ctx, p: ResizeStream) -> Result<Done> {
        crate::attach::validate(p.cols, p.rows)?;
        if !ctx
            .streams
            .as_ref()
            .is_some_and(|streams| streams.owns(p.stream_id))
        {
            return Err(TerminalError::NotFound {
                id: p.stream_id.to_string(),
            }
            .into());
        }
        self.attachments.resize(p.stream_id, p.cols, p.rows)?;
        let rev = self
            .db
            .read(|tx| current_rev(tx))
            .await
            .internal("terminal revision")?;
        Ok(Done {
            rev: rev.to_string(),
        })
    }
    pub fn tmux(&self) -> &Tmux {
        &self.tmux
    }
    pub async fn list(&self, params: ListSessions) -> Result<SessionList> {
        let (mut items, rev) = self
            .db
            .read(move |tx| {
                Ok((
                    store::all(tx, params.project_id.as_deref())?,
                    current_rev(tx)?,
                ))
            })
            .await
            .internal("list terminals")?;
        let live = self.tmux.list().await.map_err(tmux_error)?;
        for item in &mut items {
            if let Some(session) = live.iter().find(|session| session.name == item.id) {
                item.alive = true;
                item.command.clone_from(&session.command);
            }
        }
        Ok(SessionList {
            rev: rev.to_string(),
            items,
        })
    }
    pub fn profiles(&self) -> TerminalProfiles {
        let mut items = vec![TerminalProfile {
            name: "기본 셸".into(),
            command: "bash -l".into(),
        }];
        items.extend(self.config.profiles.clone());
        TerminalProfiles { items }
    }
    pub async fn create(&self, params: CreateSession) -> Result<TerminalSession> {
        let project = match &params.project_id {
            Some(id) => Some(
                self.projects
                    .get(id)
                    .await
                    .map_err(|error| match error {
                        Fail::Internal(reason) => Fail::Internal(reason),
                        Fail::Domain(error) => {
                            Fail::Internal(format!("resolve terminal project: {error}"))
                        }
                    })?
                    .ok_or_else(|| TerminalError::ProjectNotFound { id: id.clone() })?,
            ),
            None => None,
        };
        let cwd = params
            .cwd
            .map(PathBuf::from)
            .or_else(|| project.map(|p| PathBuf::from(p.path)))
            .unwrap_or_else(|| self.paths.home.clone());
        let cwd = canonical_cwd(cwd).await?;
        let title = params.title.unwrap_or_else(|| match &params.attach_zellij {
            Some(name) => format!("zellij: {name}"),
            None => Path::new(&cwd)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or(&cwd)
                .into(),
        });
        let command = params
            .attach_zellij
            .map(|name| format!("zellij attach {}", sh_quote(&name)))
            .unwrap_or_else(|| params.command.unwrap_or_else(|| "bash -l".into()));
        self.create_internal(
            SessionKind::Shell,
            params.project_id,
            PathBuf::from(cwd),
            title,
            command,
            &[],
        )
        .await
    }
    pub async fn create_internal(
        &self,
        kind: SessionKind,
        project_id: Option<String>,
        cwd: PathBuf,
        title: String,
        command: String,
        env: &[(String, String)],
    ) -> Result<TerminalSession> {
        let _guard = self.mutations.lock().await;
        let cwd = canonical_cwd(cwd).await?;
        if let Some(id) = &project_id
            && self
                .projects
                .get(id)
                .await
                .map_err(|error| match error {
                    Fail::Internal(reason) => Fail::Internal(reason),
                    Fail::Domain(error) => {
                        Fail::Internal(format!("resolve terminal project: {error}"))
                    }
                })?
                .is_none()
        {
            return Err(TerminalError::ProjectNotFound { id: id.clone() }.into());
        }
        let session = TerminalSession {
            id: format!("t{}", uuid::Uuid::new_v4().simple()),
            project_id,
            title,
            cwd,
            kind,
            command,
            alive: true,
            created_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as i64,
        };
        self.tmux
            .ensure_server(&self.paths.instance)
            .await
            .map_err(tmux_error)?;
        self.tmux
            .new_session(
                &session.id,
                Path::new(&session.cwd),
                Some(&session.command),
                env,
            )
            .await
            .map_err(tmux_error)?;
        let row = session.clone();
        let hub = self.hub.clone();
        let stored = self
            .db
            .write_checked_with_commit(
                move |tx| {
                    store::insert(tx, &row)?;
                    Ok(Ok::<_, std::convert::Infallible>(()))
                },
                move |_, rev| changed(&hub, rev),
            )
            .await;
        match stored {
            Ok(Ok(_)) => Ok(session),
            Ok(Err(never)) => match never {},
            Err(error) => {
                self.tmux
                    .kill_session(&session.id)
                    .await
                    .internal("remove unpersisted terminal")?;
                Err(Fail::Internal(format!("persist terminal: {error}")))
            }
        }
    }
    pub async fn rename(&self, params: RenameSession) -> Result<Done> {
        let _guard = self.mutations.lock().await;
        let hub = self.hub.clone();
        let (_, rev) = self
            .db
            .write_checked_with_commit(
                move |tx| {
                    if store::rename(tx, &params.id, &params.title)? == 0 {
                        return Ok(Err(TerminalError::NotFound { id: params.id }));
                    }
                    Ok(Ok(()))
                },
                move |_, rev| changed(&hub, rev),
            )
            .await
            .internal("rename terminal")??;
        Ok(Done {
            rev: rev.to_string(),
        })
    }
    /// Agent-owned terminal cleanup also removes records whose tmux session was lost.
    pub async fn close_internal(&self, id: &str) -> Result<Done> {
        let _guard = self.mutations.lock().await;
        if self
            .tmux
            .list()
            .await
            .map_err(tmux_error)?
            .iter()
            .any(|s| s.name == id)
        {
            self.tmux.kill_session(id).await.map_err(tmux_error)?;
        }
        let id = id.to_owned();
        let hub = self.hub.clone();
        let (_, rev) = self
            .db
            .write_checked_with_commit(
                move |tx| {
                    store::delete(tx, &id)?;
                    Ok(Ok::<_, std::convert::Infallible>(()))
                },
                move |_, rev| changed(&hub, rev),
            )
            .await
            .internal("remove agent terminal")?
            .unwrap();
        Ok(Done {
            rev: rev.to_string(),
        })
    }
    pub async fn close(&self, params: CloseSession) -> Result<Done> {
        let _guard = self.mutations.lock().await;
        let id = params.id.clone();
        let existing = self
            .db
            .read(move |tx| store::get(tx, &id))
            .await
            .internal("find terminal")?;
        if existing.is_none() {
            return Err(TerminalError::NotFound { id: params.id }.into());
        }
        self.tmux
            .kill_session(&params.id)
            .await
            .map_err(tmux_error)?;
        let hub = self.hub.clone();
        let (_, rev) = self
            .db
            .write_checked_with_commit(
                move |tx| {
                    if store::delete(tx, &params.id)? == 0 {
                        return Ok(Err(TerminalError::NotFound { id: params.id }));
                    }
                    Ok(Ok(()))
                },
                move |_, rev| changed(&hub, rev),
            )
            .await
            .internal("remove terminal")??;
        Ok(Done {
            rev: rev.to_string(),
        })
    }
    pub async fn zellij_list(&self) -> ZellijSessions {
        let mut command = tokio::process::Command::new("zellij");
        command
            .args(["list-sessions", "--short", "--no-formatting"])
            .kill_on_drop(true);
        let items = match tokio::time::timeout(std::time::Duration::from_secs(10), command.output())
            .await
        {
            Ok(Ok(output)) if output.status.success() => String::from_utf8_lossy(&output.stdout)
                .lines()
                .filter(|name| !name.is_empty())
                .map(str::to_owned)
                .collect(),
            _ => vec![],
        };
        ZellijSessions { items }
    }
}

#[cfg(test)]
mod error_tests {
    use super::*;
    use devbox_protocol::{Ctx, Router};
    #[tokio::test]
    async fn terminal_io_causes_are_private_and_receive_router_diagnostics() {
        for error in [
            crate::attach::pty_error(std::io::Error::other("private-pty-marker")),
            tmux_error(TmuxError::Spawn(std::io::Error::other(
                "private-spawn-marker",
            ))),
            tmux_error(TmuxError::Failed {
                operation: "attach".into(),
                stderr: "private-stderr-marker".into(),
            }),
        ] {
            let error = Mutex::new(Some(error));
            let router =
                Router::new(Arc::new(error)).register::<crate::TerminalResize, _, _>(
                    |error, _, _| async move {
                        Err(error.lock().await.take().expect("fixture failure"))
                    },
                );
            let failure = router
                .dispatch(
                    "terminal.resize",
                    Ctx {
                        channel: 0,
                        cancel: Default::default(),
                        streams: None,
                    },
                    serde_json::value::RawValue::from_string(
                        "{\"streamId\":1,\"cols\":80,\"rows\":24}".into(),
                    )
                    .expect("params"),
                )
                .await
                .expect_err("failure");
            let wire = serde_json::to_string(&failure.error).expect("wire");
            assert!(!wire.contains("private-"), "private cause leaked: {wire}");
            assert_eq!(failure.error.code, "internal");
            assert!(failure.diagnostic_id.is_some());
        }
    }
}
