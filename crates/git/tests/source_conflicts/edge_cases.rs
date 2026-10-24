use super::*;

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
        std::fs::write(d.join("file.txt"), "side content").expect("write owned fixture");
        git(d, &["add", "file.txt"]);
        git(d, &["commit", "-m", "side change"]);
        git(d, &["checkout", "main"]);
        if add {
            std::fs::write(d.join("file.txt"), "main content").expect("write owned fixture");
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
                .expect("run owned Git command")
                .status
                .success()
        );
        let entries = list(d).await.expect("list owned source state");
        assert_eq!(entries.len(), 1);
        assert!(if add {
            matches!(entries[0].kind, ConflictKind::AddAdd)
        } else {
            matches!(entries[0].kind, ConflictKind::DeleteModify)
        });
        let preview = preview(d, &target(), "file.txt")
            .await
            .expect("preview owned source");
        if add {
            assert!(preview.base.is_none());
        } else {
            assert!(preview.ours.is_none());
        }
        let p = request(d).await;
        apply(d, &p).await.expect("apply owned resolution");
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
            .expect("run owned Git command")
            .status
            .success()
    );
    let p = preview(d, &target(), "file.txt")
        .await
        .expect("preview owned source");
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
    std::fs::remove_file(d.join("file.txt")).expect("remove owned fixture file");
    let p = request(d).await;
    apply(d, &p).await.expect("apply owned resolution");
    assert_eq!(git(d, &["show", ":file.txt"]), "my draft\n");
    for bytes in [vec![b'x'; 128 * 1024 + 1], vec![0, 255]] {
        let r = conflict();
        let d = r.path();
        std::fs::write(d.join("file.txt"), &bytes).expect("write owned fixture");
        let preview = preview(d, &target(), "file.txt")
            .await
            .expect("preview owned source");
        assert!(!preview.text_supported);
        assert!(preview.merged.is_none());
        let p = request(d).await;
        assert!(apply(d, &p).await.is_err());
        assert_eq!(
            std::fs::read(d.join("file.txt")).expect("read owned fixture"),
            bytes
        );
    }
}
#[tokio::test]
async fn deleting_a_conflict_removes_only_selected_index_path() {
    let r = conflict();
    let d = r.path();
    let mut p = request(d).await;
    p.resolution = None;
    std::fs::write(d.join("other"), "other").expect("write owned fixture");
    git(d, &["add", "other"]);
    apply(d, &p).await.expect("apply owned resolution");
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
        let original = std::fs::read(d.join("file.txt")).expect("read owned fixture");
        let index = std::fs::read(d.join(".git/index")).expect("read owned fixture");
        let race = || {
            std::fs::write(d.join("file.txt"), "external deletion race")
                .expect("write owned fixture")
        };
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
            std::fs::read(d.join("file.txt")).expect("read owned fixture"),
            if point < 2 {
                b"external deletion race".to_vec()
            } else {
                original
            }
        );
        assert_eq!(
            std::fs::read(d.join(".git/index")).expect("read owned fixture"),
            index
        );
    }
}
#[tokio::test]
async fn absent_file_created_by_external_writer_is_not_overwritten() {
    let r = conflict();
    let d = r.path();
    std::fs::remove_file(d.join("file.txt")).expect("remove owned fixture file");
    let p = request(d).await;
    let index = std::fs::read(d.join(".git/index")).expect("read owned fixture");
    let race =
        || std::fs::write(d.join("file.txt"), "external create").expect("write owned fixture");
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
        "external create"
    );
    assert_eq!(
        std::fs::read(d.join(".git/index")).expect("read owned fixture"),
        index
    );
}
#[tokio::test]
async fn replacing_only_stage_two_oid_refuses_without_touching_worktree() {
    use std::io::Write;
    let r = conflict();
    let d = r.path();
    let p = request(d).await;
    let disk = std::fs::read(d.join("file.txt")).expect("read owned fixture");
    let oid = p.token.stage_oids[2]
        .as_ref()
        .expect("owned optional value");
    let mut child = Command::new("git")
        .arg("-C")
        .arg(d)
        .args(["update-index", "--index-info"])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .expect("create owned fixture");
    writeln!(
        child.stdin.take().expect("owned fixture value"),
        "100644 {oid} 2\tfile.txt"
    )
    .expect("owned fixture value");
    assert!(child.wait().expect("wait for owned child").success());
    let index = std::fs::read(d.join(".git/index")).expect("read owned fixture");
    assert!(matches!(
        apply(d, &p).await,
        Err(SourceGitError::ConflictStale { .. })
    ));
    assert_eq!(
        std::fs::read(d.join("file.txt")).expect("read owned fixture"),
        disk
    );
    assert_eq!(
        std::fs::read(d.join(".git/index")).expect("read owned fixture"),
        index
    );
}
#[cfg(unix)]
#[tokio::test]
async fn resolution_does_not_execute_filters_or_index_hooks() {
    use std::os::unix::fs::PermissionsExt;
    let r = conflict();
    let d = r.path();
    std::fs::write(d.join(".gitattributes"), "file.txt filter=owned\n")
        .expect("write owned fixture");
    git(
        d,
        &["config", "filter.owned.clean", "touch FILTER_RAN; cat"],
    );
    let hook = d.join(".git/hooks/post-index-change");
    std::fs::write(&hook, "#!/bin/sh\ntouch HOOK_RAN\n").expect("write owned fixture");
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755))
        .expect("set owned fixture permissions");
    let p = request(d).await;
    apply(d, &p).await.expect("apply owned resolution");
    assert!(!d.join("FILTER_RAN").exists());
    assert!(!d.join("HOOK_RAN").exists());
    assert_eq!(git(d, &["show", ":file.txt"]), "my draft\n");
}
