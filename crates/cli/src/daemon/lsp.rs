//! Project, document, problem and terminal adapters for the isolated LSP domain.
use super::state::Services;
use devbox_documents::{DocKey, DocumentsError};
use devbox_lsp::{adapters::DocumentGrant, *};
use devbox_protocol::{Done, Fail, Router};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
type R<T> = Result<T, Fail<LspError>>;
static EVENT_REV: AtomicU64 = AtomicU64::new(0);
async fn root(s: &Services, project: &str) -> R<PathBuf> {
    let project = s
        .projects
        .get(project)
        .await
        .map_err(|_| LspError::ProjectNotFound)?
        .ok_or(LspError::ProjectNotFound)?;
    tokio::fs::canonicalize(project.path)
        .await
        .map_err(|_| LspError::OutsideProject.into())
}
async fn context(s: &Services, p: &OpenLsp) -> R<ProjectContext> {
    let root = root(s, &p.project_id).await?;
    let authority_root = root.clone();
    let documents = s.documents.clone();
    let project_id = p.project_id.clone();
    let access = Arc::new(move |uri: String, lease_id: Option<String>| {
        let root = authority_root.clone();
        let documents = documents.clone();
        let project_id = project_id.clone();
        Box::pin(async move {
            let path = devbox_lsp::uri::file_path(&uri).ok_or(LspError::OutsideProject)?;
            let relative = path
                .strip_prefix(&root)
                .map_err(|_| LspError::OutsideProject)?
                .to_str()
                .ok_or(LspError::InvalidInput)?;
            let key = DocKey {
                root: format!("project:{project_id}"),
                path: relative.into(),
            };
            let disk = documents
                .code_authority_at(
                    &key,
                    lease_id.as_deref(),
                    &root,
                    tokio_util::sync::CancellationToken::new(),
                )
                .await
                .map_err(|error| match error {
                    Fail::Domain(DocumentsError::LeaseLost { .. }) => LspError::LeaseLost,
                    _ => LspError::OutsideProject,
                })?;
            if disk.key.root != key.root {
                return Err(LspError::OutsideProject);
            }
            let uri = devbox_lsp::uri::file_uri(&root.join(disk.key.path))
                .ok_or(LspError::OutsideProject)?;
            Ok(DocumentGrant {
                uri,
                disk_text: disk.text.unwrap_or_default(),
                disk_rev: disk.rev,
                lease_id,
            })
        })
            as std::pin::Pin<
                Box<dyn std::future::Future<Output = Result<DocumentGrant, LspError>> + Send>,
            >
    });
    let documents = s.documents.clone();
    let capture_root = root.clone();
    let capture_project = format!("project:{}", p.project_id);
    let capture: devbox_lsp::adapters::CaptureAccess =
        Arc::new(move |priority: Vec<String>, cancel| {
            let documents = documents.clone();
            let root = capture_root.clone();
            let project = capture_project.clone();
            Box::pin(async move {
                let priority = priority
                    .into_iter()
                    .filter_map(|uri| {
                        let path = devbox_lsp::uri::file_path(&uri)?;
                        let path = path.strip_prefix(&root).ok()?.to_str()?.to_owned();
                        Some(DocKey {
                            root: project.clone(),
                            path,
                        })
                    })
                    .collect();
                documents
                    .capture_code(project, root, priority, cancel)
                    .await
                    .map_err(|_| LspError::OutsideProject)
            })
        });
    let observe = s.observe.clone();
    let hub = s.hub.clone();
    let project_id = p.project_id.clone();
    let server = s
        .lsp
        .selected(p.language)
        .ok_or(LspError::InvalidInput)?
        .command;
    let diagnostics = Arc::new(move |event| {
        match event {
            DiagnosticEvent::Generation { generation, active } => {
                observe.lsp_generation(&project_id, &server, generation, active, &hub)
            }
            DiagnosticEvent::Publish {
                generation,
                diagnostics,
            } => {
                let path = devbox_lsp::uri::file_path(&diagnostics.uri)
                    .and_then(|p| p.to_str().map(str::to_owned))
                    .ok_or(LspError::OutsideProject)?;
                let problems = diagnostics
                    .items
                    .into_iter()
                    .map(|item| devbox_observe::Problem {
                        file: path.clone(),
                        line: item.line,
                        col: Some(item.column_utf16),
                        severity: match item.severity {
                            devbox_lsp::diagnostics::Severity::Error => {
                                devbox_observe::ProblemSeverity::Error
                            }
                            devbox_lsp::diagnostics::Severity::Warning => {
                                devbox_observe::ProblemSeverity::Warning
                            }
                            devbox_lsp::diagnostics::Severity::Info => {
                                devbox_observe::ProblemSeverity::Info
                            }
                        },
                        message: item.message,
                        source: String::new(),
                    })
                    .collect();
                observe.replace_lsp_diagnostics(
                    devbox_observe::LspProblems {
                        project: project_id.clone(),
                        server: server.clone(),
                        uri: diagnostics.uri,
                        generation,
                        problems,
                        overflow: diagnostics.overflow,
                    },
                    &hub,
                )
            }
        }
        .map_err(|_| LspError::LimitExceeded)
    });
    let hub = s.hub.clone();
    let restarted = Arc::new(move |event: Restarted| {
        hub.publish::<LspRestarted>(EVENT_REV.fetch_add(1, Ordering::SeqCst) + 1, &event);
    });
    Ok(ProjectContext {
        project_id: p.project_id.clone(),
        root,
        access,
        capture,
        diagnostics,
        restarted,
    })
}
pub(super) fn register(router: Router<Services>) -> Router<Services> {
    router
        .register::<LspOpen, _, _>(|s, ctx, p| async move {
            let project = context(&s, &p).await?;
            s.lsp.open(ctx, p, project).await
        })
        .register::<LspServers, _, _>(|s, _, p| async move {
            root(&s, &p.project_id).await?;
            Ok(s.lsp.servers(&p.project_id).await)
        })
        .register::<LspRestart, _, _>(|s, _, p| async move {
            let project = context(&s, &p).await?;
            let generation = s.lsp.restart(p, project).await?;
            Ok(Done {
                rev: generation.to_string(),
            })
        })
        .register::<LspInstall, _, _>(|s, _, p| async move {
            let root = root(&s, &p.project_id).await?;
            let terminal = s.terminal.clone();
            let adapter: adapters::InstallAdapter = Arc::new(move |project_id, command| {
                let terminal = terminal.clone();
                let cwd = root.to_string_lossy().into_owned();
                Box::pin(async move {
                    terminal
                        .create(devbox_terminal::CreateSession {
                            project_id: Some(project_id),
                            cwd: Some(cwd),
                            title: Some("언어 서버 설치".into()),
                            command: Some(command),
                            attach_zellij: None,
                        })
                        .await
                        .map(|session| session.id)
                        .map_err(|_| LspError::InstallUnavailable)
                })
            });
            s.lsp.install(p, adapter).await
        })
}
