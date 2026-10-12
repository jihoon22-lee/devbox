//! 프로젝트 목록 도메인.
pub mod model;
mod store;

use devbox_core::{Db, Hub, Migration, current_rev, expand_home};
use devbox_protocol::{Done, Exporter, Fail, Internal, method, topic};
pub use model::*;
use std::path::PathBuf;
use std::sync::Arc;

pub const MIGRATIONS: &[Migration] = &[Migration {
    domain: "projects",
    version: 1,
    sql: store::MIGRATION_1,
}];

method!(pub ProjectsList = "projects.list", query, ListProjects => ProjectList, ProjectsError);
method!(pub ProjectsAdd = "projects.add", mutation, AddProject => Project, ProjectsError);
method!(pub ProjectsRemove = "projects.remove", mutation(always), RemoveProject => Done, ProjectsError);
method!(pub ProjectsScan = "projects.scan", query, ScanProjects => ScanResult, ProjectsError);
topic!(pub ProjectsChangedTopic = "projects.changed", ProjectsChanged);

pub fn export<X: Exporter>(x: &mut X) {
    x.method::<ProjectsList>();
    x.method::<ProjectsAdd>();
    x.method::<ProjectsRemove>();
    x.method::<ProjectsScan>();
    x.topic::<ProjectsChangedTopic>();
}

type R<T> = Result<T, Fail<ProjectsError>>;

pub struct ProjectsService {
    db: Db,
    hub: Arc<Hub>,
    home: PathBuf,
    roots: Vec<String>,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn canonical_project(path: String) -> R<Project> {
    let invalid = || ProjectsError::PathInvalid { path: path.clone() };
    let raw = PathBuf::from(&path);
    if !raw.is_absolute() {
        return Err(invalid().into());
    }
    let real = std::fs::canonicalize(raw).map_err(|_| invalid())?;
    if !real.is_dir() {
        return Err(ProjectsError::NotDirectory { path }.into());
    }
    let canonical = real.to_str().ok_or_else(invalid)?.to_owned();
    let name = real
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(&canonical)
        .to_owned();
    Ok(Project {
        id: uuid::Uuid::new_v4().simple().to_string(),
        name,
        path: canonical,
        favorite: false,
        added_ms: now_ms(),
    })
}

fn scan_candidates(
    roots: Vec<String>,
    home: PathBuf,
    registered: std::collections::HashSet<String>,
) -> ScanResult {
    let mut seen_roots = std::collections::HashSet::new();
    let mut seen_paths = std::collections::HashSet::new();
    let mut candidates = Vec::new();
    for root in roots {
        let Ok(dir) = std::fs::canonicalize(expand_home(&root, &home)) else {
            continue;
        };
        if !seen_roots.insert(dir.clone()) {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let filename = entry.file_name();
            let Some(name) = filename.to_str() else {
                continue;
            };
            if name.starts_with('.') || name.ends_with("-agents") {
                continue;
            }
            let Ok(real) = std::fs::canonicalize(entry.path()) else {
                continue;
            };
            if !real.is_dir() {
                continue;
            }
            let Some(path) = real.to_str() else { continue };
            let Some(name) = real.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if name.starts_with('.')
                || name.ends_with("-agents")
                || !seen_paths.insert(path.to_owned())
            {
                continue;
            }
            candidates.push(Candidate {
                path: path.to_owned(),
                name: name.to_owned(),
                is_git: real.join(".git").exists(),
                registered: registered.contains(path),
            });
        }
    }
    candidates.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.path.cmp(&b.path)));
    ScanResult { candidates }
}

fn publish_changed(hub: &Hub, rev: u64) {
    hub.publish::<ProjectsChangedTopic>(
        rev,
        &ProjectsChanged {
            rev: rev.to_string(),
        },
    );
}

impl ProjectsService {
    pub fn new(db: Db, hub: Arc<Hub>, home: PathBuf, roots: Vec<String>) -> Self {
        Self {
            db,
            hub,
            home,
            roots,
        }
    }

    pub async fn list(&self, _: ListProjects) -> R<ProjectList> {
        let (items, rev) = self
            .db
            .read(|tx| Ok((store::all(tx)?, current_rev(tx)?)))
            .await
            .internal("list projects")?;
        Ok(ProjectList {
            rev: rev.to_string(),
            items,
        })
    }

    pub async fn add(&self, p: AddProject) -> R<Project> {
        let project = tokio::task::spawn_blocking(move || canonical_project(p.path))
            .await
            .internal("validate project path")??;
        let row = project.clone();
        let hub = self.hub.clone();
        let _ = self
            .db
            .write_checked_with_commit(
                move |tx| {
                    if store::exists_path(tx, &row.path)? {
                        return Ok(Err(ProjectsError::AlreadyExists {
                            path: row.path.clone(),
                        }));
                    }
                    store::insert(tx, &row)?;
                    Ok(Ok(()))
                },
                move |_, rev| publish_changed(&hub, rev),
            )
            .await
            .internal("add project")??;
        Ok(project)
    }

    pub async fn remove(&self, p: RemoveProject) -> R<Done> {
        let hub = self.hub.clone();
        let (_, rev) = self
            .db
            .write_checked_with_commit(
                move |tx| {
                    if store::delete(tx, &p.id)? == 0 {
                        return Ok(Err(ProjectsError::NotFound { id: p.id }));
                    }
                    Ok(Ok(()))
                },
                move |_, rev| publish_changed(&hub, rev),
            )
            .await
            .internal("remove project")??;
        Ok(Done {
            rev: rev.to_string(),
        })
    }

    pub async fn scan(&self, _: ScanProjects) -> R<ScanResult> {
        let registered: std::collections::HashSet<String> = self
            .db
            .read(store::all)
            .await
            .internal("scan projects")?
            .into_iter()
            .map(|p| p.path)
            .collect();
        let roots = self.roots.clone();
        let home = self.home.clone();
        tokio::task::spawn_blocking(move || scan_candidates(roots, home, registered))
            .await
            .internal("scan project directories")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use devbox_core::{Db, Hub};
    use std::sync::Arc;

    async fn service(dir: &std::path::Path) -> (ProjectsService, Arc<Hub>) {
        let db = Db::open(
            &dir.join("devbox.db"),
            &dir.join("b"),
            "1.0.0-dev",
            MIGRATIONS,
        )
        .expect("open fixture database");
        let hub = Arc::new(Hub::new());
        (
            ProjectsService::new(
                db,
                hub.clone(),
                dir.to_path_buf(),
                vec!["~/projects".into()],
            ),
            hub,
        )
    }

    #[tokio::test]
    async fn add_list_remove_and_publish() {
        let t = tempfile::tempdir().expect("create fixture directory");
        let proj = t.path().join("projects/alpha");
        std::fs::create_dir_all(proj.join(".git")).expect("create fixture directory");
        let (s, hub) = service(t.path()).await;
        let mut events = hub.subscribe("projects.changed");

        let p = s
            .add(AddProject {
                path: proj.display().to_string(),
            })
            .await
            .expect("register fixture project");
        assert_eq!(p.name, "alpha");
        assert!(matches!(
            events.recv().await,
            Some(devbox_core::HubMessage::Event { .. })
        ));

        let list = s
            .list(ListProjects {})
            .await
            .expect("list fixture projects");
        assert_eq!(list.items.len(), 1);
        assert!(list.rev.parse::<u64>().expect("parse fixture value") >= 1);

        let dup = s
            .add(AddProject {
                path: proj.display().to_string(),
            })
            .await
            .expect_err("fixture operation must fail");
        assert!(matches!(
            dup,
            Fail::Domain(ProjectsError::AlreadyExists { .. })
        ));

        s.remove(RemoveProject { id: p.id.clone() })
            .await
            .expect("remove fixture registration");
        let gone = s
            .remove(RemoveProject { id: p.id })
            .await
            .expect_err("fixture operation must fail");
        assert!(matches!(gone, Fail::Domain(ProjectsError::NotFound { .. })));
    }

    #[tokio::test]
    async fn rejects_relative_missing_and_file_paths() {
        let t = tempfile::tempdir().expect("create fixture directory");
        let file = t.path().join("f.txt");
        std::fs::write(&file, "x").expect("write fixture file");
        let (s, _) = service(t.path()).await;
        for (path, want) in [
            ("rel/path", "projects.path_invalid"),
            (
                t.path()
                    .join("missing")
                    .to_str()
                    .expect("fixture path is UTF-8"),
                "projects.path_invalid",
            ),
            (
                file.to_str().expect("fixture path is UTF-8"),
                "projects.not_directory",
            ),
        ] {
            let e = s
                .add(AddProject { path: path.into() })
                .await
                .expect_err("fixture operation must fail");
            let Fail::Domain(e) = e else {
                panic!("domain error expected")
            };
            assert_eq!(devbox_protocol::to_wire(&e).code, want);
        }
    }

    #[tokio::test]
    async fn scan_lists_root_children_and_marks_registered_and_git() {
        let t = tempfile::tempdir().expect("create fixture directory");
        let root = t.path().join("projects");
        std::fs::create_dir_all(root.join("a/.git")).expect("create fixture directory");
        std::fs::create_dir_all(root.join("b")).expect("create fixture directory");
        std::fs::create_dir_all(root.join("a-agents/x")).expect("create fixture directory");
        std::fs::create_dir_all(root.join(".hidden")).expect("create fixture directory");
        let (s, _) = service(t.path()).await;
        s.add(AddProject {
            path: root.join("a").display().to_string(),
        })
        .await
        .expect("register fixture project");
        let r = s
            .scan(ScanProjects {})
            .await
            .expect("scan fixture projects");
        let names: Vec<_> = r
            .candidates
            .iter()
            .map(|c| (c.name.as_str(), c.is_git, c.registered))
            .collect();
        assert_eq!(names, vec![("a", true, true), ("b", false, false)]);
    }
    #[tokio::test]
    async fn concurrent_duplicate_and_missing_remove_do_not_commit_or_publish() {
        let t = tempfile::tempdir().expect("create fixture directory");
        let project = t.path().join("project");
        std::fs::create_dir(&project).expect("create fixture directory");
        let (s, hub) = service(t.path()).await;
        let mut events = hub.subscribe("projects.changed");
        let path = project.to_str().expect("fixture path is UTF-8").to_owned();
        let (a, b) = tokio::join!(
            s.add(AddProject { path: path.clone() }),
            s.add(AddProject { path })
        );
        let (ok, rejected) = if a.is_ok() { (a, b) } else { (b, a) };
        let project = ok.expect("one fixture registration succeeds");
        assert!(matches!(
            rejected,
            Err(Fail::Domain(ProjectsError::AlreadyExists { .. }))
        ));
        assert_eq!(
            s.list(ListProjects {})
                .await
                .expect("list fixture projects")
                .rev,
            "1"
        );
        assert!(matches!(
            events.recv().await,
            Some(devbox_core::HubMessage::Event { rev: 1, .. })
        ));
        assert!(matches!(
            s.remove(RemoveProject {
                id: "missing".into()
            })
            .await,
            Err(Fail::Domain(ProjectsError::NotFound { .. }))
        ));
        assert_eq!(
            s.list(ListProjects {})
                .await
                .expect("list fixture projects")
                .rev,
            "1"
        );
        s.remove(RemoveProject { id: project.id })
            .await
            .expect("remove fixture registration");
        drop(s);
        drop(hub);
        assert!(matches!(
            events.recv().await,
            Some(devbox_core::HubMessage::Event { rev: 2, .. })
        ));
        assert_eq!(events.recv().await, None);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn canonical_scan_deduplicates_roots_and_aliases() {
        let t = tempfile::tempdir().expect("create fixture directory");
        let root = t.path().join("projects");
        std::fs::create_dir_all(root.join("a/.git")).expect("create fixture directory");
        std::fs::create_dir_all(root.join("b")).expect("create fixture directory");
        std::os::unix::fs::symlink(root.join("a"), root.join("alias"))
            .expect("create fixture symlink");
        std::os::unix::fs::symlink(&root, t.path().join("root-alias"))
            .expect("create fixture symlink");
        let (mut s, _) = service(t.path()).await;
        s.roots.extend([
            root.to_str().expect("fixture path is UTF-8").into(),
            "~/root-alias".into(),
        ]);
        s.add(AddProject {
            path: root
                .join("alias")
                .to_str()
                .expect("fixture path is UTF-8")
                .into(),
        })
        .await
        .expect("fixture path is UTF-8");
        let r = s
            .scan(ScanProjects {})
            .await
            .expect("scan fixture projects");
        assert_eq!(
            r.candidates
                .iter()
                .map(|c| (c.name.as_str(), c.is_git, c.registered))
                .collect::<Vec<_>>(),
            vec![("a", true, true), ("b", false, false)]
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn rejects_non_utf8_canonical_target() {
        use std::os::unix::ffi::OsStringExt;
        let t = tempfile::tempdir().expect("create fixture directory");
        let root = t.path().join("projects");
        std::fs::create_dir(&root).expect("create fixture directory");
        let target = root.join(std::ffi::OsString::from_vec(vec![b'a', 0xff]));
        std::fs::create_dir(&target).expect("create fixture directory");
        let alias = root.join("alias");
        std::os::unix::fs::symlink(&target, &alias).expect("create fixture symlink");
        let (s, _) = service(t.path()).await;
        assert!(matches!(
            s.add(AddProject {
                path: alias.to_str().expect("fixture path is UTF-8").into()
            })
            .await,
            Err(Fail::Domain(ProjectsError::PathInvalid { .. }))
        ));
        assert!(
            s.scan(ScanProjects {})
                .await
                .expect("scan fixture projects")
                .candidates
                .is_empty()
        );
    }
    #[tokio::test]
    async fn dropped_remove_waiter_still_publishes_committed_change() {
        use std::future::Future;
        use std::task::{Context, Poll, Waker};
        let t = tempfile::tempdir().expect("create fixture directory");
        let project = t.path().join("project");
        std::fs::create_dir(&project).expect("create fixture directory");
        let (s, hub) = service(t.path()).await;
        let project = s
            .add(AddProject {
                path: project.to_str().expect("fixture path is UTF-8").into(),
            })
            .await
            .expect("fixture path is UTF-8");
        let mut events = hub.subscribe("projects.changed");
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let db = s.db.clone();
        let blocker = tokio::spawn(async move {
            db.write_checked::<(), (), _>(move |_| {
                ready_tx.send(()).expect("send fixture message");
                release_rx.recv().expect("receive fixture message");
                Ok(Err(()))
            })
            .await
            .expect("complete fixture transaction barrier")
        });
        tokio::task::spawn_blocking(move || {
            ready_rx
                .recv_timeout(std::time::Duration::from_secs(5))
                .expect("receive fixture signal before deadline")
        })
        .await
        .expect("receive fixture signal before deadline");
        let mut remove = Box::pin(s.remove(RemoveProject { id: project.id }));
        assert!(matches!(
            remove
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop())),
            Poll::Pending
        ));
        // Dropping the handler after enqueue cannot retract the writer's job.
        drop(remove);
        release_tx.send(()).expect("send fixture message");
        assert_eq!(
            blocker.await.expect("complete fixture transaction barrier"),
            Err(())
        );
        assert!(matches!(
            tokio::time::timeout(std::time::Duration::from_secs(2), events.recv())
                .await
                .expect("receive fixture message"),
            Some(devbox_core::HubMessage::Event { rev: 2, .. })
        ));
        let list = s
            .list(ListProjects {})
            .await
            .expect("list fixture projects");
        assert_eq!(list.rev, "2");
        assert!(list.items.is_empty());
    }
}
