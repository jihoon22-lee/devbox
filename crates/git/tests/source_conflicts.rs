#![cfg(target_os = "linux")]
mod source_support;
use devbox_git::{conflict_model::*, conflicts::*, model::*};
use source_support::*;
use std::{path::Path, process::Command};
fn target() -> SourceTarget {
    SourceTarget {
        project_id: "fixture".into(),
        worktree_id: None,
    }
}
fn conflict() -> tempfile::TempDir {
    conflict_at("file.txt")
}
fn conflict_at(path: &str) -> tempfile::TempDir {
    let r = repo();
    let d = r.path();
    if path != "file.txt" {
        std::fs::create_dir_all(
            d.join(path)
                .parent()
                .expect("owned fixture parent directory"),
        )
        .expect("owned fixture parent directory");
        git(d, &["mv", "file.txt", path]);
        git(d, &["commit", "-m", "move fixture"]);
    }
    git(d, &["checkout", "-b", "side"]);
    std::fs::write(d.join(path), "theirs\n").expect("write owned fixture");
    git(d, &["commit", "-am", "theirs"]);
    git(d, &["checkout", "main"]);
    std::fs::write(d.join(path), "ours\n").expect("write owned fixture");
    git(d, &["commit", "-am", "ours"]);
    let status = Command::new("git")
        .arg("-C")
        .arg(d)
        .args(["merge", "side"])
        .output()
        .expect("run owned Git command");
    assert!(!status.status.success());
    r
}
async fn request(d: &Path) -> ConflictApply {
    ConflictApply {
        target: target(),
        path: "file.txt".into(),
        resolution: Some("my draft\n".into()),
        token: preview(d, &target(), "file.txt")
            .await
            .expect("preview owned source")
            .token,
    }
}
#[tokio::test]
async fn apply_refuses_when_file_changed_after_preview() {
    let r = conflict();
    let d = r.path();
    let p = request(d).await;
    let index = std::fs::read(d.join(".git/index")).expect("read owned fixture");
    std::fs::write(d.join("file.txt"), "external\n").expect("write owned fixture");
    assert!(matches!(
        apply(d, &p).await,
        Err(SourceGitError::ConflictStale { .. })
    ));
    assert_eq!(
        std::fs::read_to_string(d.join("file.txt")).expect("read owned fixture text"),
        "external\n"
    );
    assert_eq!(
        std::fs::read(d.join(".git/index")).expect("read owned fixture"),
        index
    );
}
#[tokio::test]
async fn apply_refuses_when_index_stage_changed_after_preview() {
    let r = conflict();
    let d = r.path();
    let p = request(d).await;
    git(d, &["add", "file.txt"]);
    let index = std::fs::read(d.join(".git/index")).expect("read owned fixture");
    let disk = std::fs::read(d.join("file.txt")).expect("read owned fixture");
    assert!(matches!(
        apply(d, &p).await,
        Err(SourceGitError::ConflictStale { .. })
    ));
    assert_eq!(
        std::fs::read(d.join(".git/index")).expect("read owned fixture"),
        index
    );
    assert_eq!(
        std::fs::read(d.join("file.txt")).expect("read owned fixture"),
        disk
    );
}
#[tokio::test]
async fn apply_resolves_only_selected_file() {
    let r = conflict();
    let d = r.path();
    let p = request(d).await;
    std::fs::write(d.join("other"), "unrelated\n").expect("write owned fixture");
    git(d, &["add", "other"]);
    apply(d, &p).await.expect("apply owned resolution");
    assert!(git(d, &["ls-files", "--unmerged"]).is_empty());
    assert_eq!(git(d, &["show", ":file.txt"]), "my draft\n");
    assert_eq!(git(d, &["show", ":other"]), "unrelated\n");
}
#[tokio::test]
async fn replacement_race_restores_external_bytes_and_preserves_index() {
    let r = conflict();
    let d = r.path();
    let p = request(d).await;
    let index = std::fs::read(d.join(".git/index")).expect("read owned fixture");
    let race =
        || std::fs::write(d.join("file.txt"), "raced after check\n").expect("write owned fixture");
    assert!(matches!(
        apply_with_probe(
            d,
            &p,
            Probe {
                before_replace: Some(&race),
                ..Default::default()
            }
        )
        .await,
        Err(SourceGitError::ConflictStale { .. })
    ));
    assert_eq!(
        std::fs::read_to_string(d.join("file.txt")).expect("read owned fixture text"),
        "raced after check\n"
    );
    assert_eq!(
        std::fs::read(d.join(".git/index")).expect("read owned fixture"),
        index
    );
}
#[tokio::test]
async fn post_exchange_writer_preserves_current_and_displaced_bytes() {
    let r = conflict();
    let d = r.path();
    let p = request(d).await;
    let original = std::fs::read(d.join("file.txt")).expect("read owned fixture");
    let index = std::fs::read(d.join(".git/index")).expect("read owned fixture");
    let race = || {
        std::fs::write(d.join("file.txt"), "post exchange external\n").expect("write owned fixture")
    };
    assert!(matches!(
        apply_with_probe(
            d,
            &p,
            Probe {
                after_replace: Some(&race),
                ..Default::default()
            }
        )
        .await,
        Err(SourceGitError::ConflictPartial { .. })
    ));
    assert_eq!(
        std::fs::read_to_string(d.join("file.txt")).expect("read owned fixture text"),
        "post exchange external\n"
    );
    assert_eq!(
        std::fs::read(d.join(".git/index")).expect("read owned fixture"),
        index
    );
    let copies: Vec<_> = std::fs::read_dir(d)
        .expect("owned source result")
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with(".devbox-conflict-")
        })
        .collect();
    assert_eq!(copies.len(), 1);
    assert_eq!(
        std::fs::read(copies[0].path()).expect("read owned fixture"),
        original
    );
}
#[tokio::test]
async fn index_failure_rolls_back_file_and_keeps_resolution_in_request() {
    let r = conflict();
    let d = r.path();
    let p = request(d).await;
    let disk = std::fs::read(d.join("file.txt")).expect("read owned fixture");
    let index = std::fs::read(d.join(".git/index")).expect("read owned fixture");
    assert!(matches!(
        apply_with_probe(
            d,
            &p,
            Probe {
                fail_index: true,
                ..Default::default()
            }
        )
        .await,
        Err(SourceGitError::ConflictPartial { .. })
    ));
    assert_eq!(
        std::fs::read(d.join("file.txt")).expect("read owned fixture"),
        disk
    );
    assert_eq!(
        std::fs::read(d.join(".git/index")).expect("read owned fixture"),
        index
    );
    assert_eq!(p.resolution.as_deref(), Some("my draft\n"));
    apply(d, &p).await.expect("apply owned resolution");
}
#[tokio::test]
async fn index_lock_is_respected_and_external_index_race_is_preserved() {
    let r = conflict();
    let d = r.path();
    let p = request(d).await;
    let disk = std::fs::read(d.join("file.txt")).expect("read owned fixture");
    std::fs::write(d.join(".git/index.lock"), "external lock").expect("write owned fixture");
    assert!(matches!(
        apply(d, &p).await,
        Err(SourceGitError::SourceBusy)
    ));
    assert_eq!(
        std::fs::read_to_string(d.join(".git/index.lock")).expect("read owned fixture text"),
        "external lock"
    );
    std::fs::remove_file(d.join(".git/index.lock")).expect("remove owned fixture file");
    let mut external = std::fs::read(d.join(".git/index")).expect("read owned fixture");
    external.extend(b"external marker");
    let race = || std::fs::write(d.join(".git/index"), &external).expect("write owned fixture");
    assert!(
        apply_with_probe(
            d,
            &p,
            Probe {
                after_replace: Some(&race),
                ..Default::default()
            }
        )
        .await
        .is_err()
    );
    assert_eq!(
        std::fs::read(d.join(".git/index")).expect("read owned fixture"),
        external
    );
    assert_eq!(
        std::fs::read(d.join("file.txt")).expect("read owned fixture"),
        disk
    );
}
#[tokio::test]
async fn token_cannot_move_between_targets_or_paths() {
    let r = conflict();
    let d = r.path();
    let mut p = request(d).await;
    p.target.worktree_id = Some("another".into());
    assert!(matches!(
        apply(d, &p).await,
        Err(SourceGitError::ConflictStale { .. })
    ));
    p.target = target();
    p.path = "../escape".into();
    assert!(matches!(
        apply(d, &p).await,
        Err(SourceGitError::ConflictStale { .. })
    ));
    assert!(preview(d, &target(), ".git/index").await.is_err());
}
#[cfg(unix)]
#[tokio::test]
async fn mode_and_symlink_replacement_are_safe() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let r = conflict();
    let d = r.path();
    std::fs::set_permissions(d.join("file.txt"), std::fs::Permissions::from_mode(0o755))
        .expect("set owned fixture permissions");
    let p = request(d).await;
    apply(d, &p).await.expect("apply owned resolution");
    assert_eq!(
        std::fs::metadata(d.join("file.txt"))
            .expect("inspect owned fixture metadata")
            .permissions()
            .mode()
            & 0o777,
        0o755
    );
    assert!(git(d, &["ls-files", "--stage", "file.txt"]).starts_with("100755"));
    let r = conflict();
    let d = r.path();
    let p = request(d).await;
    let outside = tempfile::NamedTempFile::new().expect("create owned fixture");
    std::fs::write(outside.path(), "outside").expect("write owned fixture");
    let race = || {
        std::fs::remove_file(d.join("file.txt")).expect("remove owned fixture file");
        symlink(outside.path(), d.join("file.txt")).expect("create owned fixture symlink");
    };
    let index = std::fs::read(d.join(".git/index")).expect("read owned fixture");
    assert!(
        apply_with_probe(
            d,
            &p,
            Probe {
                before_replace: Some(&race),
                ..Default::default()
            }
        )
        .await
        .is_err()
    );
    assert_eq!(
        std::fs::read_link(d.join("file.txt")).expect("owned source result"),
        outside.path()
    );
    assert_eq!(
        std::fs::read_to_string(outside.path()).expect("read owned fixture text"),
        "outside"
    );
    assert_eq!(
        std::fs::read(d.join(".git/index")).expect("read owned fixture"),
        index
    );
}

#[tokio::test]
async fn directory_replacement_never_writes_new_directory() {
    for after in [false, true] {
        let r = conflict_at("nested/file.txt");
        let d = r.path();
        let p = ConflictApply {
            target: target(),
            path: "nested/file.txt".into(),
            resolution: Some("draft".into()),
            token: preview(d, &target(), "nested/file.txt")
                .await
                .expect("preview owned source")
                .token,
        };
        let disk = std::fs::read(d.join("nested/file.txt")).expect("read owned fixture");
        let index = std::fs::read(d.join(".git/index")).expect("read owned fixture");
        let race = || {
            std::fs::rename(d.join("nested"), d.join("old")).expect("rename owned fixture");
            std::fs::create_dir(d.join("nested")).expect("create owned directory");
            std::fs::write(d.join("nested/file.txt"), "external directory")
                .expect("write owned fixture");
        };
        let probe = if after {
            Probe {
                after_replace: Some(&race),
                ..Default::default()
            }
        } else {
            Probe {
                before_replace: Some(&race),
                ..Default::default()
            }
        };
        assert!(apply_with_probe(d, &p, probe).await.is_err());
        assert_eq!(
            std::fs::read_to_string(d.join("nested/file.txt")).expect("read owned fixture text"),
            "external directory"
        );
        assert_eq!(
            std::fs::read(d.join("old/file.txt")).expect("read owned fixture"),
            disk
        );
        assert_eq!(
            std::fs::read(d.join(".git/index")).expect("read owned fixture"),
            index
        );
    }
}
#[path = "source_conflicts/edge_cases.rs"]
mod edge_cases;
