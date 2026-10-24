mod source_support;
use devbox_git::{branches, stash};
use source_support::*;
#[tokio::test]
async fn amend_keeps_author_and_unborn_commit() {
    let r = repo();
    let d = r.path();
    let author = git(d, &["show", "-s", "--format=%an <%ae>", "HEAD"]);
    let e = branches::expect(d)
        .await
        .expect("capture owned branch expectation");
    branches::commit(d, "amended", true, &e)
        .await
        .expect("commit owned change");
    assert_eq!(
        git(d, &["show", "-s", "--format=%an <%ae>", "HEAD"]),
        author
    );
    let r = tempfile::tempdir().expect("create owned temporary directory");
    let d = r.path();
    git(d, &["init", "-qb", "main"]);
    git(d, &["config", "user.name", "Owned"]);
    git(d, &["config", "user.email", "owned@example.invalid"]);
    std::fs::write(d.join("first"), "first").expect("write owned fixture");
    git(d, &["add", "."]);
    let e = branches::expect(d)
        .await
        .expect("capture owned branch expectation");
    branches::commit(d, "first", false, &e)
        .await
        .expect("commit owned change");
    assert_eq!(git(d, &["log", "--format=%s"]).trim(), "first");
}
#[tokio::test]
async fn branch_create_checkout_and_delete_stale() {
    let r = repo();
    let d = r.path();
    let e = branches::expect(d)
        .await
        .expect("capture owned branch expectation");
    branches::checkout(d, "owned", true, &e)
        .await
        .expect("checkout owned branch");
    let oid = git(d, &["rev-parse", "HEAD"]);
    let e = branches::expect(d)
        .await
        .expect("capture owned branch expectation");
    branches::checkout(d, "main", false, &e)
        .await
        .expect("checkout owned branch");
    git(d, &["branch", "-f", "owned", "HEAD"]);
    let e = branches::expect(d)
        .await
        .expect("capture owned branch expectation");
    assert!(
        branches::delete(d, "owned", &"a".repeat(40), true, &e)
            .await
            .is_err()
    );
    assert_eq!(git(d, &["rev-parse", "owned"]), oid);
    branches::delete(d, "owned", oid.trim(), false, &e)
        .await
        .expect("delete owned branch");
}
#[tokio::test]
async fn stash_push_apply_pop_drop_by_oid() {
    let r = repo();
    let d = r.path();
    edit(d);
    let e = branches::expect(d)
        .await
        .expect("capture owned branch expectation");
    stash::push(d, "owned", false, &e)
        .await
        .expect("push owned remote");
    assert!(git(d, &["diff"]).is_empty());
    let item = stash::list(d)
        .await
        .expect("list owned source state")
        .remove(0);
    let e = branches::expect(d)
        .await
        .expect("capture owned branch expectation");
    stash::apply(d, &item.oid, false, &e)
        .await
        .expect("apply owned resolution");
    assert!(git(d, &["diff"]).contains("first change"));
    git(d, &["restore", "file.txt"]);
    let e = branches::expect(d)
        .await
        .expect("capture owned branch expectation");
    stash::apply(d, &item.oid, true, &e)
        .await
        .expect("apply owned resolution");
    assert!(
        stash::list(d)
            .await
            .expect("list owned source state")
            .is_empty()
    );
    let e = branches::expect(d)
        .await
        .expect("capture owned branch expectation");
    stash::push(d, "owned drop", false, &e)
        .await
        .expect("push owned remote");
    let item = stash::list(d)
        .await
        .expect("list owned source state")
        .remove(0);
    let e = branches::expect(d)
        .await
        .expect("capture owned branch expectation");
    stash::drop(d, &item.oid, &e)
        .await
        .expect("owned source result");
    assert!(
        stash::list(d)
            .await
            .expect("list owned source state")
            .is_empty()
    );
}
#[tokio::test]
async fn path_log_and_blame_keep_literal_paths_and_pagination() {
    let r = repo();
    let d = r.path();
    let path = "-literal [x] 한글.txt";
    std::fs::write(d.join(path), "one\ntwo\n").expect("write owned fixture");
    git(d, &["add", "--", path]);
    git(d, &["commit", "-qm", "literal"]);
    git(d, &["commit", "--allow-empty", "-qm", "unrelated"]);
    let log = devbox_git::blame::log(d, Some(path), 10, 0)
        .await
        .expect("read owned commit log");
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].subject, "literal");
    assert!(
        devbox_git::blame::log(d, Some(path), 10, 1)
            .await
            .expect("read owned commit log")
            .is_empty()
    );
    let lines = devbox_git::blame::blame(d, path)
        .await
        .expect("read owned blame");
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[1].text, "two");
    assert_eq!(lines[1].line, 2);
    assert_eq!(lines[0].author, "Original Author");
}
#[tokio::test]
async fn status_and_diff_report_binary_and_rename_as_file_operations() {
    let r = repo();
    let d = r.path();
    std::fs::write(d.join("binary"), b"hello\0binary").expect("write owned fixture");
    git(d, &["add", "binary"]);
    git(d, &["commit", "-qm", "binary"]);
    std::fs::write(d.join("binary"), b"changed\0binary").expect("write owned fixture");
    let snapshot = devbox_git::hunks::snapshot(d, "binary", false)
        .await
        .expect("capture owned source snapshot");
    assert!(snapshot.file_only);
    assert!(snapshot.hunks.is_empty());
    git(d, &["mv", "file.txt", "renamed"]);
    let status = devbox_git::source_queries::status(d)
        .await
        .expect("read owned source status");
    assert_eq!(
        status
            .files
            .iter()
            .find(|f| f.path == "renamed")
            .expect("matching owned result")
            .old_path
            .as_deref(),
        Some("file.txt")
    );
    let snapshot = devbox_git::hunks::snapshot(d, "renamed", true)
        .await
        .expect("capture owned source snapshot");
    assert!(snapshot.file_only);
    assert!(snapshot.hunks.is_empty());
}
#[tokio::test]
async fn stale_branch_oid_returns_current_source_state() {
    let r = repo();
    let d = r.path();
    git(d, &["branch", "owned"]);
    let e = branches::expect(d)
        .await
        .expect("capture owned branch expectation");
    let error = branches::delete(d, "owned", &"a".repeat(40), true, &e)
        .await
        .expect_err("owned query must reject invalid input");
    assert!(
        matches!(error,devbox_git::model::SourceGitError::StateChanged{current}if current==e.state)
    );
}
