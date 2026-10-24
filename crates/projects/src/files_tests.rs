use super::*;
use devbox_documents::{DocKey, DocumentsConfig, DocumentsService, Root};
use std::{path::PathBuf, sync::Arc};

struct Fx {
    _tmp: tempfile::TempDir,
    project: PathBuf,
    outside: PathBuf,
    files: FilesService,
}
impl Fx {
    fn new() -> Self {
        let tmp = tempfile::tempdir().expect("owned fixture");
        let project = tmp.path().join("project");
        let outside = tmp.path().join("outside");
        for dir in [&project, &outside] {
            std::fs::create_dir_all(dir).expect("fixture dir");
        }
        std::fs::create_dir_all(project.join("owned-agents")).expect("owned agents fixture");
        let root = project.clone();
        let roots = Arc::new(move || {
            let root = root.clone();
            Box::pin(async move {
                vec![
                    Root {
                        id: "agents:p".into(),
                        path: root.join("owned-agents"),
                    },
                    Root {
                        id: "project:p".into(),
                        path: root,
                    },
                ]
            }) as devbox_protocol::router::BoxFuture<_>
        });
        let documents = Arc::new(DocumentsService::new(DocumentsConfig::new(
            tmp.path().join("data"),
            roots,
            Arc::new(devbox_core::Hub::new()),
        )));
        Self {
            files: FilesService::new(documents),
            _tmp: tmp,
            project,
            outside,
        }
    }
    fn key(&self, path: &str) -> DocKey {
        DocKey {
            root: "project:p".into(),
            path: path.into(),
        }
    }
    fn tree(&self, hidden: bool) -> FileTreeQuery {
        FileTreeQuery {
            project_id: "p".into(),
            dir: "".into(),
            show_hidden: hidden,
            cursor: None,
        }
    }
}

#[tokio::test]
async fn files_tree_filters_internal_entries_and_pages_without_duplicates() {
    let fx = Fx::new();
    for name in [".git", ".trash", ".devbox-tmp-owned", ".hidden", "가😀.ts"] {
        std::fs::write(fx.project.join(name), "").expect("fixture");
    }
    for i in 0..270 {
        std::fs::write(fx.project.join(format!("file-{i:03}.ts")), "").expect("fixture");
    }
    let first = fx.files.tree(fx.tree(false)).await.expect("first page");
    assert_eq!(first.entries.len(), 256);
    let second = fx
        .files
        .tree(FileTreeQuery {
            cursor: first.next_cursor,
            ..fx.tree(false)
        })
        .await
        .expect("second page");
    let names: Vec<_> = first
        .entries
        .iter()
        .chain(&second.entries)
        .map(|e| e.name.as_str())
        .collect();
    assert_eq!(names.len(), 271);
    assert_eq!(
        names
            .iter()
            .copied()
            .collect::<std::collections::HashSet<_>>()
            .len(),
        271
    );
    assert!(!names.iter().any(|name| name.starts_with('.')));
    let hidden = fx.files.tree(fx.tree(true)).await.expect("hidden");
    assert!(hidden.entries.iter().any(|e| e.name == ".hidden"));
    assert!(
        !hidden
            .entries
            .iter()
            .any(|e| e.name == ".git" || e.name == ".trash" || e.name.starts_with(".devbox-tmp-"))
    );
}

#[tokio::test]
async fn files_create_refuses_existing_and_delete_checks_content() {
    let fx = Fx::new();
    let key = fx.key("a.ts");
    let created = fx
        .files
        .create(FileKey { key: key.clone() })
        .await
        .expect("create");
    assert!(fx.files.create(FileKey { key: key.clone() }).await.is_err());
    std::fs::write(fx.project.join("a.ts"), "outside").expect("external edit");
    assert!(
        fx.files
            .delete(DeleteFile {
                key: key.clone(),
                expect: created.rev
            })
            .await
            .is_err()
    );
    assert_eq!(
        std::fs::read_to_string(fx.project.join("a.ts")).expect("preserved"),
        "outside"
    );
    let current = fx
        .files
        .stat(FileKey { key: key.clone() })
        .await
        .expect("stat");
    fx.files
        .delete(DeleteFile {
            key,
            expect: current.rev,
        })
        .await
        .expect("delete current");
    assert!(!fx.project.join("a.ts").exists());
}

#[tokio::test]
async fn files_refuse_root_escape_and_nonempty_directory_delete() {
    let fx = Fx::new();
    std::fs::write(fx.outside.join("keep"), "host").expect("outside fixture");
    std::os::unix::fs::symlink(&fx.outside, fx.project.join("escape")).expect("fixture link");
    for path in [
        "../outside/keep",
        "escape/keep",
        ".git/config",
        "owned-agents/file.ts",
    ] {
        assert!(fx.files.stat(FileKey { key: fx.key(path) }).await.is_err());
        assert!(
            fx.files
                .create(FileKey { key: fx.key(path) })
                .await
                .is_err()
        );
    }
    std::fs::create_dir(fx.project.join("folder")).expect("folder");
    std::fs::write(fx.project.join("folder/child"), "keep").expect("child");
    let key = fx.key("folder");
    let stat = fx
        .files
        .stat(FileKey { key: key.clone() })
        .await
        .expect("folder stat");
    assert!(
        fx.files
            .delete(DeleteFile {
                key: key.clone(),
                expect: stat.rev
            })
            .await
            .is_err()
    );
    assert_eq!(
        std::fs::read_to_string(fx.project.join("folder/child")).expect("child preserved"),
        "keep"
    );
    std::fs::remove_file(fx.project.join("folder/child")).expect("remove own child");
    let stat = fx
        .files
        .stat(FileKey { key: key.clone() })
        .await
        .expect("empty stat");
    fx.files
        .delete(DeleteFile {
            key,
            expect: stat.rev,
        })
        .await
        .expect("empty delete");
    assert!(!fx.project.join("folder").exists());
    assert_eq!(
        std::fs::read_to_string(fx.outside.join("keep")).expect("outside preserved"),
        "host"
    );
}

#[tokio::test]
async fn files_changed_directory_rejects_a_stale_page_cursor() {
    let fx = Fx::new();
    for i in 0..260 {
        std::fs::write(fx.project.join(format!("{i:03}.ts")), b"").expect("fixture");
    }
    let page = fx.files.tree(fx.tree(false)).await.expect("first page");
    std::fs::write(fx.project.join("new.ts"), b"outside").expect("external create");
    assert!(matches!(
        fx.files
            .tree(FileTreeQuery {
                cursor: page.next_cursor,
                ..fx.tree(false)
            })
            .await,
        Err(devbox_protocol::Fail::Domain(FilesError::StateChanged))
    ));
}
