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
        std::fs::create_dir_all(d.join(path).parent().unwrap()).unwrap();
        git(d, &["mv", "file.txt", path]);
        git(d, &["commit", "-m", "move fixture"]);
    }
    git(d, &["checkout", "-b", "side"]);
    std::fs::write(d.join(path), "theirs\n").unwrap();
    git(d, &["commit", "-am", "theirs"]);
    git(d, &["checkout", "main"]);
    std::fs::write(d.join(path), "ours\n").unwrap();
    git(d, &["commit", "-am", "ours"]);
    let status = Command::new("git")
        .arg("-C")
        .arg(d)
        .args(["merge", "side"])
        .output()
        .unwrap();
    assert!(!status.status.success());
    r
}
async fn request(d: &Path) -> ConflictApply {
    ConflictApply {
        target: target(),
        path: "file.txt".into(),
        resolution: Some("my draft\n".into()),
        token: preview(d, &target(), "file.txt").await.unwrap().token,
    }
}
#[tokio::test]
async fn apply_refuses_when_file_changed_after_preview() {
    let r = conflict();
    let d = r.path();
    let p = request(d).await;
    let index = std::fs::read(d.join(".git/index")).unwrap();
    std::fs::write(d.join("file.txt"), "external\n").unwrap();
    assert!(matches!(
        apply(d, &p).await,
        Err(SourceGitError::ConflictStale { .. })
    ));
    assert_eq!(
        std::fs::read_to_string(d.join("file.txt")).unwrap(),
        "external\n"
    );
    assert_eq!(std::fs::read(d.join(".git/index")).unwrap(), index);
}
#[tokio::test]
async fn apply_refuses_when_index_stage_changed_after_preview() {
    let r = conflict();
    let d = r.path();
    let p = request(d).await;
    git(d, &["add", "file.txt"]);
    let index = std::fs::read(d.join(".git/index")).unwrap();
    let disk = std::fs::read(d.join("file.txt")).unwrap();
    assert!(matches!(
        apply(d, &p).await,
        Err(SourceGitError::ConflictStale { .. })
    ));
    assert_eq!(std::fs::read(d.join(".git/index")).unwrap(), index);
    assert_eq!(std::fs::read(d.join("file.txt")).unwrap(), disk);
}
#[tokio::test]
async fn apply_resolves_only_selected_file() {
    let r = conflict();
    let d = r.path();
    let p = request(d).await;
    std::fs::write(d.join("other"), "unrelated\n").unwrap();
    git(d, &["add", "other"]);
    apply(d, &p).await.unwrap();
    assert!(git(d, &["ls-files", "--unmerged"]).is_empty());
    assert_eq!(git(d, &["show", ":file.txt"]), "my draft\n");
    assert_eq!(git(d, &["show", ":other"]), "unrelated\n");
}
#[tokio::test]
async fn replacement_race_restores_external_bytes_and_preserves_index() {
    let r = conflict();
    let d = r.path();
    let p = request(d).await;
    let index = std::fs::read(d.join(".git/index")).unwrap();
    let race = || std::fs::write(d.join("file.txt"), "raced after check\n").unwrap();
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
        std::fs::read_to_string(d.join("file.txt")).unwrap(),
        "raced after check\n"
    );
    assert_eq!(std::fs::read(d.join(".git/index")).unwrap(), index);
}
#[tokio::test]
async fn post_exchange_writer_preserves_current_and_displaced_bytes() {
    let r = conflict();
    let d = r.path();
    let p = request(d).await;
    let original = std::fs::read(d.join("file.txt")).unwrap();
    let index = std::fs::read(d.join(".git/index")).unwrap();
    let race = || std::fs::write(d.join("file.txt"), "post exchange external\n").unwrap();
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
        std::fs::read_to_string(d.join("file.txt")).unwrap(),
        "post exchange external\n"
    );
    assert_eq!(std::fs::read(d.join(".git/index")).unwrap(), index);
    let copies: Vec<_> = std::fs::read_dir(d)
        .unwrap()
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with(".devbox-conflict-")
        })
        .collect();
    assert_eq!(copies.len(), 1);
    assert_eq!(std::fs::read(copies[0].path()).unwrap(), original);
}
#[tokio::test]
async fn index_failure_rolls_back_file_and_keeps_resolution_in_request() {
    let r = conflict();
    let d = r.path();
    let p = request(d).await;
    let disk = std::fs::read(d.join("file.txt")).unwrap();
    let index = std::fs::read(d.join(".git/index")).unwrap();
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
    assert_eq!(std::fs::read(d.join("file.txt")).unwrap(), disk);
    assert_eq!(std::fs::read(d.join(".git/index")).unwrap(), index);
    assert_eq!(p.resolution.as_deref(), Some("my draft\n"));
    apply(d, &p).await.unwrap();
}
#[tokio::test]
async fn index_lock_is_respected_and_external_index_race_is_preserved() {
    let r = conflict();
    let d = r.path();
    let p = request(d).await;
    let disk = std::fs::read(d.join("file.txt")).unwrap();
    std::fs::write(d.join(".git/index.lock"), "external lock").unwrap();
    assert!(matches!(
        apply(d, &p).await,
        Err(SourceGitError::SourceBusy)
    ));
    assert_eq!(
        std::fs::read_to_string(d.join(".git/index.lock")).unwrap(),
        "external lock"
    );
    std::fs::remove_file(d.join(".git/index.lock")).unwrap();
    let mut external = std::fs::read(d.join(".git/index")).unwrap();
    external.extend(b"external marker");
    let race = || std::fs::write(d.join(".git/index"), &external).unwrap();
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
    assert_eq!(std::fs::read(d.join(".git/index")).unwrap(), external);
    assert_eq!(std::fs::read(d.join("file.txt")).unwrap(), disk);
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
    std::fs::set_permissions(d.join("file.txt"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let p = request(d).await;
    apply(d, &p).await.unwrap();
    assert_eq!(
        std::fs::metadata(d.join("file.txt"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o755
    );
    assert!(git(d, &["ls-files", "--stage", "file.txt"]).starts_with("100755"));
    let r = conflict();
    let d = r.path();
    let p = request(d).await;
    let outside = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(outside.path(), "outside").unwrap();
    let race = || {
        std::fs::remove_file(d.join("file.txt")).unwrap();
        symlink(outside.path(), d.join("file.txt")).unwrap();
    };
    let index = std::fs::read(d.join(".git/index")).unwrap();
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
        std::fs::read_link(d.join("file.txt")).unwrap(),
        outside.path()
    );
    assert_eq!(std::fs::read_to_string(outside.path()).unwrap(), "outside");
    assert_eq!(std::fs::read(d.join(".git/index")).unwrap(), index);
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
                .unwrap()
                .token,
        };
        let disk = std::fs::read(d.join("nested/file.txt")).unwrap();
        let index = std::fs::read(d.join(".git/index")).unwrap();
        let race = || {
            std::fs::rename(d.join("nested"), d.join("old")).unwrap();
            std::fs::create_dir(d.join("nested")).unwrap();
            std::fs::write(d.join("nested/file.txt"), "external directory").unwrap();
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
            std::fs::read_to_string(d.join("nested/file.txt")).unwrap(),
            "external directory"
        );
        assert_eq!(std::fs::read(d.join("old/file.txt")).unwrap(), disk);
        assert_eq!(std::fs::read(d.join(".git/index")).unwrap(), index);
    }
}
#[tokio::test]
async fn add_add_and_delete_modify_have_nullable_stages_and_resolve() {
    for add in [false, true] {
        let r = repo();
        let d = r.path();
        if add {
            git(d, &["rm", "file.txt"]);
            git(d, &["commit", "-m", "remove base"]);
        }
        git(d, &["checkout", "-b", "side"]);
        std::fs::write(d.join("file.txt"), "side content").unwrap();
        git(d, &["add", "file.txt"]);
        git(d, &["commit", "-m", "side change"]);
        git(d, &["checkout", "main"]);
        if add {
            std::fs::write(d.join("file.txt"), "main content").unwrap();
            git(d, &["add", "file.txt"]);
        } else {
            git(d, &["rm", "file.txt"]);
        }
        git(d, &["commit", "-m", "main change"]);
        assert!(
            !Command::new("git")
                .arg("-C")
                .arg(d)
                .args(["merge", "side"])
                .output()
                .unwrap()
                .status
                .success()
        );
        let entries = list(d).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert!(if add {
            matches!(entries[0].kind, ConflictKind::AddAdd)
        } else {
            matches!(entries[0].kind, ConflictKind::DeleteModify)
        });
        let preview = preview(d, &target(), "file.txt").await.unwrap();
        if add {
            assert!(preview.base.is_none());
        } else {
            assert!(preview.ours.is_none());
        }
        let p = request(d).await;
        apply(d, &p).await.unwrap();
        assert_eq!(git(d, &["show", ":file.txt"]), "my draft\n");
    }
}
#[tokio::test]
async fn rebase_labels_match_actual_stage_roles() {
    let r = conflict();
    let d = r.path();
    git(d, &["merge", "--abort"]);
    assert!(
        !Command::new("git")
            .arg("-C")
            .arg(d)
            .args(["rebase", "side"])
            .output()
            .unwrap()
            .status
            .success()
    );
    let p = preview(d, &target(), "file.txt").await.unwrap();
    assert!(p.labels.rebase);
    assert!(p.labels.ours.contains("theirs"));
    assert!(p.labels.theirs.contains("ours"));
    assert_eq!(p.ours.as_deref(), Some("theirs\n"));
    assert_eq!(p.theirs.as_deref(), Some("ours\n"));
}
#[tokio::test]
async fn absent_file_is_created_exclusively_and_oversize_binary_are_disabled() {
    let r = conflict();
    let d = r.path();
    std::fs::remove_file(d.join("file.txt")).unwrap();
    let p = request(d).await;
    apply(d, &p).await.unwrap();
    assert_eq!(git(d, &["show", ":file.txt"]), "my draft\n");
    for bytes in [vec![b'x'; 128 * 1024 + 1], vec![0, 255]] {
        let r = conflict();
        let d = r.path();
        std::fs::write(d.join("file.txt"), &bytes).unwrap();
        let preview = preview(d, &target(), "file.txt").await.unwrap();
        assert!(!preview.text_supported);
        assert!(preview.merged.is_none());
        let p = request(d).await;
        assert!(apply(d, &p).await.is_err());
        assert_eq!(std::fs::read(d.join("file.txt")).unwrap(), bytes);
    }
}
#[tokio::test]
async fn deleting_a_conflict_removes_only_selected_index_path() {
    let r = conflict();
    let d = r.path();
    let mut p = request(d).await;
    p.resolution = None;
    std::fs::write(d.join("other"), "other").unwrap();
    git(d, &["add", "other"]);
    apply(d, &p).await.unwrap();
    assert!(!d.join("file.txt").exists());
    assert!(git(d, &["ls-files", "--stage", "--", "file.txt"]).is_empty());
    assert_eq!(git(d, &["show", ":other"]), "other");
}
#[tokio::test]
async fn deletion_races_and_index_failure_preserve_external_bytes() {
    for point in 0..3 {
        let r = conflict();
        let d = r.path();
        let mut p = request(d).await;
        p.resolution = None;
        let original = std::fs::read(d.join("file.txt")).unwrap();
        let index = std::fs::read(d.join(".git/index")).unwrap();
        let race = || std::fs::write(d.join("file.txt"), "external deletion race").unwrap();
        let probe = match point {
            0 => Probe {
                before_replace: Some(&race),
                ..Default::default()
            },
            1 => Probe {
                after_replace: Some(&race),
                ..Default::default()
            },
            _ => Probe {
                fail_index: true,
                ..Default::default()
            },
        };
        assert!(apply_with_probe(d, &p, probe).await.is_err());
        assert_eq!(
            std::fs::read(d.join("file.txt")).unwrap(),
            if point < 2 {
                b"external deletion race".to_vec()
            } else {
                original
            }
        );
        assert_eq!(std::fs::read(d.join(".git/index")).unwrap(), index);
    }
}
#[tokio::test]
async fn absent_file_created_by_external_writer_is_not_overwritten() {
    let r = conflict();
    let d = r.path();
    std::fs::remove_file(d.join("file.txt")).unwrap();
    let p = request(d).await;
    let index = std::fs::read(d.join(".git/index")).unwrap();
    let race = || std::fs::write(d.join("file.txt"), "external create").unwrap();
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
        std::fs::read_to_string(d.join("file.txt")).unwrap(),
        "external create"
    );
    assert_eq!(std::fs::read(d.join(".git/index")).unwrap(), index);
}
#[tokio::test]
async fn replacing_only_stage_two_oid_refuses_without_touching_worktree() {
    use std::io::Write;
    let r = conflict();
    let d = r.path();
    let p = request(d).await;
    let disk = std::fs::read(d.join("file.txt")).unwrap();
    let oid = p.token.stage_oids[2].as_ref().unwrap();
    let mut child = Command::new("git")
        .arg("-C")
        .arg(d)
        .args(["update-index", "--index-info"])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    writeln!(child.stdin.take().unwrap(), "100644 {oid} 2\tfile.txt").unwrap();
    assert!(child.wait().unwrap().success());
    let index = std::fs::read(d.join(".git/index")).unwrap();
    assert!(matches!(
        apply(d, &p).await,
        Err(SourceGitError::ConflictStale { .. })
    ));
    assert_eq!(std::fs::read(d.join("file.txt")).unwrap(), disk);
    assert_eq!(std::fs::read(d.join(".git/index")).unwrap(), index);
}
#[cfg(unix)]
#[tokio::test]
async fn resolution_does_not_execute_filters_or_index_hooks() {
    use std::os::unix::fs::PermissionsExt;
    let r = conflict();
    let d = r.path();
    std::fs::write(d.join(".gitattributes"), "file.txt filter=owned\n").unwrap();
    git(
        d,
        &["config", "filter.owned.clean", "touch FILTER_RAN; cat"],
    );
    let hook = d.join(".git/hooks/post-index-change");
    std::fs::write(&hook, "#!/bin/sh\ntouch HOOK_RAN\n").unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    let p = request(d).await;
    apply(d, &p).await.unwrap();
    assert!(!d.join("FILTER_RAN").exists());
    assert!(!d.join("HOOK_RAN").exists());
    assert_eq!(git(d, &["show", ":file.txt"]), "my draft\n");
}
