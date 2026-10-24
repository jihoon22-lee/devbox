mod source_support;
use devbox_git::{branches, stash};
use source_support::*;
#[tokio::test]
async fn amend_keeps_author_and_unborn_commit() {
    let r = repo();
    let d = r.path();
    let author = git(d, &["show", "-s", "--format=%an <%ae>", "HEAD"]);
    let e = branches::expect(d).await.unwrap();
    branches::commit(d, "amended", true, &e).await.unwrap();
    assert_eq!(
        git(d, &["show", "-s", "--format=%an <%ae>", "HEAD"]),
        author
    );
    let r = tempfile::tempdir().unwrap();
    let d = r.path();
    git(d, &["init", "-qb", "main"]);
    git(d, &["config", "user.name", "Owned"]);
    git(d, &["config", "user.email", "owned@example.invalid"]);
    std::fs::write(d.join("first"), "first").unwrap();
    git(d, &["add", "."]);
    let e = branches::expect(d).await.unwrap();
    branches::commit(d, "first", false, &e).await.unwrap();
    assert_eq!(git(d, &["log", "--format=%s"]).trim(), "first");
}
#[tokio::test]
async fn branch_create_checkout_and_delete_stale() {
    let r = repo();
    let d = r.path();
    let e = branches::expect(d).await.unwrap();
    branches::checkout(d, "owned", true, &e).await.unwrap();
    let oid = git(d, &["rev-parse", "HEAD"]);
    let e = branches::expect(d).await.unwrap();
    branches::checkout(d, "main", false, &e).await.unwrap();
    git(d, &["branch", "-f", "owned", "HEAD"]);
    let e = branches::expect(d).await.unwrap();
    assert!(
        branches::delete(d, "owned", &"a".repeat(40), true, &e)
            .await
            .is_err()
    );
    assert_eq!(git(d, &["rev-parse", "owned"]), oid);
    branches::delete(d, "owned", oid.trim(), false, &e)
        .await
        .unwrap();
}
#[tokio::test]
async fn stash_push_apply_pop_drop_by_oid() {
    let r = repo();
    let d = r.path();
    edit(d);
    let e = branches::expect(d).await.unwrap();
    stash::push(d, "owned", false, &e).await.unwrap();
    assert!(git(d, &["diff"]).is_empty());
    let item = stash::list(d).await.unwrap().remove(0);
    let e = branches::expect(d).await.unwrap();
    stash::apply(d, &item.oid, false, &e).await.unwrap();
    assert!(git(d, &["diff"]).contains("first change"));
    git(d, &["restore", "file.txt"]);
    let e = branches::expect(d).await.unwrap();
    stash::apply(d, &item.oid, true, &e).await.unwrap();
    assert!(stash::list(d).await.unwrap().is_empty());
    let e = branches::expect(d).await.unwrap();
    stash::push(d, "owned drop", false, &e).await.unwrap();
    let item = stash::list(d).await.unwrap().remove(0);
    let e = branches::expect(d).await.unwrap();
    stash::drop(d, &item.oid, &e).await.unwrap();
    assert!(stash::list(d).await.unwrap().is_empty());
}
#[tokio::test]
async fn path_log_and_blame_keep_literal_paths_and_pagination() {
    let r = repo();
    let d = r.path();
    let path = "-literal [x] 한글.txt";
    std::fs::write(d.join(path), "one\ntwo\n").unwrap();
    git(d, &["add", "--", path]);
    git(d, &["commit", "-qm", "literal"]);
    git(d, &["commit", "--allow-empty", "-qm", "unrelated"]);
    let log = devbox_git::blame::log(d, Some(path), 10, 0).await.unwrap();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].subject, "literal");
    assert!(
        devbox_git::blame::log(d, Some(path), 10, 1)
            .await
            .unwrap()
            .is_empty()
    );
    let lines = devbox_git::blame::blame(d, path).await.unwrap();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[1].text, "two");
    assert_eq!(lines[1].line, 2);
    assert_eq!(lines[0].author, "Original Author");
}
#[tokio::test]
async fn status_and_diff_report_binary_and_rename_as_file_operations() {
    let r = repo();
    let d = r.path();
    std::fs::write(d.join("binary"), b"hello\0binary").unwrap();
    git(d, &["add", "binary"]);
    git(d, &["commit", "-qm", "binary"]);
    std::fs::write(d.join("binary"), b"changed\0binary").unwrap();
    let snapshot = devbox_git::hunks::snapshot(d, "binary", false)
        .await
        .unwrap();
    assert!(snapshot.file_only);
    assert!(snapshot.hunks.is_empty());
    git(d, &["mv", "file.txt", "renamed"]);
    let status = devbox_git::source_queries::status(d).await.unwrap();
    assert_eq!(
        status
            .files
            .iter()
            .find(|f| f.path == "renamed")
            .unwrap()
            .old_path
            .as_deref(),
        Some("file.txt")
    );
    let snapshot = devbox_git::hunks::snapshot(d, "renamed", true)
        .await
        .unwrap();
    assert!(snapshot.file_only);
    assert!(snapshot.hunks.is_empty());
}
#[tokio::test]
async fn stale_branch_oid_returns_current_source_state() {
    let r = repo();
    let d = r.path();
    git(d, &["branch", "owned"]);
    let e = branches::expect(d).await.unwrap();
    let error = branches::delete(d, "owned", &"a".repeat(40), true, &e)
        .await
        .unwrap_err();
    assert!(
        matches!(error,devbox_git::model::SourceGitError::StateChanged{current}if current==e.state)
    );
}
