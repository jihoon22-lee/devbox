mod source_support;
use devbox_git::hunks::{FileAction, change, snapshot};
use source_support::*;
#[tokio::test]
async fn stage_and_unstage_a_single_hunk() {
    let r = repo();
    let d = r.path();
    edit(d);
    let s = snapshot(d, "file.txt", false)
        .await
        .expect("capture owned source snapshot");
    assert_eq!(s.hunks.len(), 2);
    change(
        d,
        "file.txt",
        Some(&s.hunks[0].id),
        &s.expect,
        FileAction::Stage,
    )
    .await
    .expect("owned source result");
    let index = git(d, &["show", ":file.txt"]);
    assert!(index.contains("first change"));
    assert!(!index.contains("second change"));
    let s = snapshot(d, "file.txt", true)
        .await
        .expect("capture owned source snapshot");
    change(
        d,
        "file.txt",
        Some(&s.hunks[0].id),
        &s.expect,
        FileAction::Unstage,
    )
    .await
    .expect("owned source result");
    assert!(git(d, &["diff", "--cached"]).is_empty());
    assert!(git(d, &["diff"]).contains("second change"));
}
#[tokio::test]
async fn discard_hunk_restores_only_that_hunk() {
    let r = repo();
    let d = r.path();
    edit(d);
    let s = snapshot(d, "file.txt", false)
        .await
        .expect("capture owned source snapshot");
    change(
        d,
        "file.txt",
        Some(&s.hunks[0].id),
        &s.expect,
        FileAction::Discard,
    )
    .await
    .expect("owned source result");
    let disk = std::fs::read_to_string(d.join("file.txt")).expect("read owned fixture text");
    assert!(!disk.contains("first change"));
    assert!(disk.contains("second change"));
    assert!(git(d, &["diff", "--cached"]).is_empty());
}
#[tokio::test]
async fn stale_hunk_or_file_snapshot_changes_nothing() {
    let r = repo();
    let d = r.path();
    edit(d);
    let s = snapshot(d, "file.txt", false)
        .await
        .expect("capture owned source snapshot");
    std::fs::write(d.join("file.txt"), "later external content\n").expect("write owned fixture");
    let before = git(d, &["diff"]);
    assert!(
        change(
            d,
            "file.txt",
            Some(&s.hunks[0].id),
            &s.expect,
            FileAction::Discard
        )
        .await
        .is_err()
    );
    assert_eq!(git(d, &["diff"]), before);
    assert!(git(d, &["diff", "--cached"]).is_empty());
}
#[tokio::test]
async fn unstage_rename_restores_both_index_endpoints_and_preserves_unrelated() {
    let r = repo();
    let d = r.path();
    git(d, &["mv", "file.txt", "renamed"]);
    std::fs::write(d.join("unrelated"), "unrelated").expect("write owned fixture");
    git(d, &["add", "unrelated"]);
    let s = snapshot(d, "renamed", true)
        .await
        .expect("capture owned source snapshot");
    change(d, "renamed", None, &s.expect, FileAction::Unstage)
        .await
        .expect("owned source result");
    let names = git(d, &["diff", "--cached", "--name-only"]);
    assert_eq!(names.trim(), "unrelated");
    assert!(d.join("renamed").exists());
    assert!(!d.join("file.txt").exists());
}
