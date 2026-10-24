mod source_support;
use source_support::*;
#[tokio::test]
async fn main_and_linked_watch_external_index_commit_and_packed_refs() {
    let r = repo();
    let d = r.path();
    let parent = tempfile::tempdir().unwrap();
    let linked = parent.path().join("linked");
    git(
        d,
        &["worktree", "add", "-qb", "linked", linked.to_str().unwrap()],
    );
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let _watch = devbox_git::watch::watch(&linked, move || {
        let _ = tx.send(());
    })
    .await
    .unwrap();
    std::fs::write(linked.join("indexed"), "indexed").unwrap();
    git(&linked, &["add", "."]);
    tokio::time::timeout(std::time::Duration::from_secs(3), rx.recv())
        .await
        .unwrap()
        .unwrap();
    git(&linked, &["commit", "-qm", "external"]);
    tokio::time::timeout(std::time::Duration::from_secs(3), rx.recv())
        .await
        .unwrap()
        .unwrap();
    git(d, &["pack-refs", "--all"]);
    tokio::time::timeout(std::time::Duration::from_secs(3), rx.recv())
        .await
        .unwrap()
        .unwrap();
}
