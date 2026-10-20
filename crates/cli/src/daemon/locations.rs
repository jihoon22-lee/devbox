//! Resolve clicked log locations without opening a shell or interpreting a URL.
use super::state::Services;
use devbox_observe::{FileLocation, LogSource, ObserveError, ResolveFileLocation};
use devbox_protocol::Fail;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
type R<T> = Result<T, Fail<ObserveError>>;

pub(super) async fn resolve(s: Arc<Services>, p: ResolveFileLocation) -> R<FileLocation> {
    if p.line == 0
        || p.column == Some(0)
        || p.path.len() > 4096
        || p.path.chars().any(char::is_control)
    {
        return Err(ObserveError::InvalidInput.into());
    }
    let run_context = if let LogSource::Run { run_id } = &p.source {
        Some(
            s.runs
                .location_context(run_id)
                .await
                .map_err(|_| ObserveError::RunNotFound)?,
        )
    } else {
        None
    };
    // Merged run logs can belong to another project. Their recorded source wins
    // over the currently selected UI project, never its edited definition.
    let project_id = run_context
        .as_ref()
        .map(|c| c.project_id.as_str())
        .unwrap_or(&p.project_id);
    let project = s
        .projects
        .get(project_id)
        .await
        .map_err(|error| Fail::Internal(format!("resolve location project: {error:?}")))?
        .ok_or(ObserveError::ProjectNotFound)?;
    let root = PathBuf::from(project.path);
    let mut allowed = vec![root.clone()];
    let (base, windows) = match (p.source, run_context) {
        (LogSource::Run { .. }, Some(context)) => {
            let cwd = match context.cwd {
                Some(cwd) => PathBuf::from(cwd),
                // Old records predate cwd capture. Absolute Linux paths remain
                // unambiguous; never guess a relative file in the project root.
                None if Path::new(&p.path).is_absolute() => root.clone(),
                None => return Err(ObserveError::SourceUnavailable.into()),
            };
            (cwd, context.windows.map(|w| w.directory))
        }
        (LogSource::File { path }, _) => {
            let source = tokio::fs::canonicalize(path)
                .await
                .map_err(|_| ObserveError::SourceUnavailable)?;
            let mut source_allowed = false;
            for log_root in super::logs::roots(&s).await? {
                if let Ok(log_root) = tokio::fs::canonicalize(log_root).await
                    && source.starts_with(&log_root)
                {
                    source_allowed = true;
                    allowed.push(log_root);
                }
            }
            if !source_allowed || !source.is_file() {
                return Err(ObserveError::FileOutsideRoots.into());
            }
            (source.parent().unwrap_or(&root).to_path_buf(), None)
        }
        _ => (root.clone(), None),
    };
    let target = location_path(&base, windows.as_deref(), &p.path)?;
    let canonical = tokio::fs::canonicalize(target)
        .await
        .map_err(|_| ObserveError::SourceUnavailable)?;
    // Run cwd may be an owned worktree outside the registered main directory.
    if !allowed.iter().any(|root| canonical.starts_with(root)) && !canonical.starts_with(&base) {
        return Err(ObserveError::FileOutsideRoots.into());
    }
    if !canonical.is_file() {
        return Err(ObserveError::SourceUnavailable.into());
    }
    Ok(FileLocation {
        path: canonical.to_str().ok_or(ObserveError::InvalidInput)?.into(),
        line: p.line,
        column: p.column.unwrap_or(1),
    })
}

fn location_path(base: &Path, windows: Option<&str>, text: &str) -> R<PathBuf> {
    if text.is_empty() || text.contains("://") {
        return Err(ObserveError::InvalidInput.into());
    }
    let bytes = text.as_bytes();
    let relative = if bytes.len() > 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        let windows = windows.ok_or(ObserveError::InvalidInput)?;
        // Match only a directory boundary, never C:\\repo-other for C:\\repo.
        let normalized = text.replace('\\', "/");
        let prefix = format!("{}/", windows.replace('\\', "/").trim_end_matches('/'));
        if !normalized
            .get(..prefix.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(&prefix))
        {
            return Err(ObserveError::FileOutsideRoots.into());
        }
        normalized[prefix.len()..].to_owned()
    } else {
        text.replace('\\', "/")
    };
    if relative.contains(':') {
        return Err(ObserveError::InvalidInput.into());
    }
    Ok(base.join(relative))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn file_locations_resolve_relative_absolute_and_windows_copy_paths() {
        let base = Path::new("/owned/repo/src");
        assert_eq!(
            location_path(base, None, "file name.rs").expect("fixture result"),
            base.join("file name.rs")
        );
        assert_eq!(
            location_path(base, None, "/owned/repo/main.rs").expect("fixture result"),
            PathBuf::from("/owned/repo/main.rs")
        );
        assert_eq!(
            location_path(
                base,
                Some(r"C:\dev\repo\src"),
                r"C:\dev\repo\src\file name.rs"
            )
            .expect("fixture result"),
            base.join("file name.rs")
        );
        assert!(location_path(base, Some(r"C:\dev\repo"), r"C:\dev\repo-other\file.rs").is_err());
        assert!(
            location_path(
                base,
                Some("C:/dev/İİ/"),
                "C:/dev/i\u{0307}i\u{0307}/file.rs"
            )
            .is_err()
        );
        assert!(location_path(base, None, "https://example.com/x.rs").is_err());
        assert!(location_path(base, None, r"C:\dev\repo\file.rs").is_err());
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    #[tokio::test]
    async fn file_location_query_uses_file_parent_and_rejects_escape_and_missing_run() {
        let home = tempfile::tempdir().expect("fixture result");
        let repo = home.path().join("repo");
        std::fs::create_dir_all(repo.join("logs")).expect("fixture result");
        std::fs::write(repo.join("logs/output.log"), b"owned").expect("fixture result");
        std::fs::write(repo.join("source.rs"), b"owned").expect("fixture result");
        std::fs::write(home.path().join("outside.rs"), b"owned outside").expect("fixture result");
        let services = crate::daemon::build_services_for_test(home.path());
        let project = services
            .projects
            .add(devbox_projects::AddProject {
                path: repo.display().to_string(),
            })
            .await
            .expect("fixture result");
        let request = |path: &str| ResolveFileLocation {
            project_id: project.id.clone(),
            source: LogSource::File {
                path: repo.join("logs/output.log").display().to_string(),
            },
            path: path.into(),
            line: 12,
            column: Some(3),
        };
        let location = resolve(services.clone(), request("../source.rs"))
            .await
            .expect("fixture result");
        assert_eq!(location.path, repo.join("source.rs").display().to_string());
        assert_eq!((location.line, location.column), (12, 3));
        assert!(matches!(
            resolve(services.clone(), request("../../outside.rs")).await,
            Err(Fail::Domain(ObserveError::FileOutsideRoots))
        ));
        assert!(matches!(
            resolve(services.clone(), request("missing.rs")).await,
            Err(Fail::Domain(ObserveError::SourceUnavailable))
        ));
        let mut missing = request("source.rs");
        missing.source = LogSource::Run {
            run_id: "missing".into(),
        };
        assert!(matches!(
            resolve(services, missing).await,
            Err(Fail::Domain(ObserveError::RunNotFound))
        ));
    }
    #[tokio::test]
    async fn merged_run_location_uses_original_source_project_cwd_and_windows_mapping() {
        let home = tempfile::tempdir().expect("fixture result");
        let repo = home.path().join("source");
        let other = home.path().join("selected");
        std::fs::create_dir_all(repo.join("old")).expect("fixture result");
        std::fs::create_dir_all(repo.join("new")).expect("fixture result");
        std::fs::create_dir_all(&other).expect("fixture result");
        for dir in ["old", "new"] {
            std::fs::write(repo.join(dir).join("source.rs"), dir).expect("fixture result");
        }
        let services = crate::daemon::build_services_for_test(home.path());
        let project = services
            .projects
            .add(devbox_projects::AddProject {
                path: repo.display().to_string(),
            })
            .await
            .expect("fixture result");
        let selected = services
            .projects
            .add(devbox_projects::AddProject {
                path: other.display().to_string(),
            })
            .await
            .expect("fixture result");
        let project_id = project.id.clone();
        let original = repo.join("old").display().to_string();
        services.db.write(move |tx| {
            let run = devbox_runtime::Run {
                id: "old-run".into(), project_id: project_id.clone(), def_id: "task".into(),
                kind: devbox_runtime::RunKind::Task, state: devbox_runtime::RunState::Exited,
                exit: None, started_ms: 1, ended_ms: Some(2), restarts: 0, unit: "owned-fixture.service".into(),
                ready: false, pid: None, port: None, attempt_id: None,
                windows: Some(devbox_runtime::WindowsRun { directory: r"C:\dev\source\old".into(), notice: "owned fixture".into() }),
            };
            let body = serde_json::json!({ "run": run, "source_cwd": original, "restart": "no", "start_limit": null, "worktree": null, "start_limit_notified": false }).to_string();
            tx.execute("INSERT INTO runtime_runs(id,project_id,started_ms,body) VALUES('old-run',?1,1,?2)", [&project_id, &body])?;
            Ok(())
        }).await.expect("fixture result");
        services
            .runs
            .define(devbox_runtime::DefineRun {
                project_id: project.id,
                save_to: devbox_runtime::SaveTo::ThisPc,
                def: devbox_runtime::RunDef::Task(devbox_runtime::TaskDef {
                    id: "task".into(),
                    command: "true".into(),
                    cwd: Some("new".into()),
                    ..Default::default()
                }),
            })
            .await
            .expect("fixture result");
        for path in ["source.rs", r"C:\dev\source\old\source.rs"] {
            let location = resolve(
                services.clone(),
                ResolveFileLocation {
                    project_id: selected.id.clone(),
                    source: LogSource::Run {
                        run_id: "old-run".into(),
                    },
                    path: path.into(),
                    line: 1,
                    column: None,
                },
            )
            .await
            .expect("fixture result");
            assert_eq!(
                location.path,
                repo.join("old/source.rs").display().to_string()
            );
        }
        services.db.write(|tx| {
            tx.execute("UPDATE runtime_runs SET body=json_remove(body,'$.source_cwd') WHERE id='old-run'", [])?;
            Ok(())
        }).await.expect("fixture result");
        assert!(matches!(
            resolve(
                services,
                ResolveFileLocation {
                    project_id: selected.id,
                    source: LogSource::Run {
                        run_id: "old-run".into()
                    },
                    path: "source.rs".into(),
                    line: 1,
                    column: None,
                }
            )
            .await,
            Err(Fail::Domain(ObserveError::SourceUnavailable))
        ));
    }
}
