mod source_support;
use source_support::*;
#[tokio::test]
async fn main_and_linked_watch_external_index_commit_and_packed_refs() {
    let r = repo();
    let d = r.path();
    let parent = tempfile::tempdir().expect("create owned temporary directory");
    let linked = parent.path().join("linked");
    git(
        d,
        &[
            "worktree",
            "add",
            "-qb",
            "linked",
            linked.to_str().expect("owned UTF-8 path"),
        ],
    );
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let _watch = devbox_git::watch::watch(&linked, move || {
        let _ = tx.send(());
    })
    .await
    .expect("owned source result");
    std::fs::write(linked.join("indexed"), "indexed").expect("write owned fixture");
    git(&linked, &["add", "."]);
    tokio::time::timeout(std::time::Duration::from_secs(3), rx.recv())
        .await
        .expect("receive owned event")
        .expect("capture owned branch expectation");
    git(&linked, &["commit", "-qm", "external"]);
    tokio::time::timeout(std::time::Duration::from_secs(3), rx.recv())
        .await
        .expect("receive owned event")
        .expect("capture owned branch expectation");
    git(d, &["pack-refs", "--all"]);
    tokio::time::timeout(std::time::Duration::from_secs(3), rx.recv())
        .await
        .expect("receive owned event")
        .expect("capture owned branch expectation");
}

use devbox_git::{
    model::{GitChanged, SourceGitError, SourceTarget},
    service::GitService,
};
use std::time::Duration;
use tokio::sync::mpsc::UnboundedReceiver;
/// The watcher's idle time (`watch::IDLE`), pinned here so that changing it is a decision.
const IDLE: Duration = Duration::from_secs(300);
/// The watcher's poll (`watch::POLL`), pinned here so that changing it is a decision.
const POLL: Duration = Duration::from_millis(500);
/// One poll plus the quiet time (300 ms): the latest a change is reported.
const POLL_AND_QUIET: Duration = Duration::from_millis(800);
/// The same on the real clock, with room for a loaded machine.
const REPORTED_WITHIN: Duration = Duration::from_secs(3);
fn service() -> (GitService, UnboundedReceiver<GitChanged>) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let service = GitService::new(
        std::sync::Arc::new(move |event| {
            let _ = tx.send(event);
        }),
        std::sync::Arc::new(|_| ()),
    );
    (service, rx)
}
fn target(project: &str) -> SourceTarget {
    SourceTarget {
        project_id: project.into(),
        worktree_id: None,
    }
}
async fn next(rx: &mut UnboundedReceiver<GitChanged>, within: Duration) -> String {
    tokio::time::timeout(within, rx.recv())
        .await
        .expect("git.changed in time")
        .expect("open sink")
        .project_id
}
#[tokio::test]
async fn outside_change_is_reported_within_one_poll_and_the_quiet_time() {
    let r = repo();
    let (service, mut rx) = service();
    service
        .observe(r.path(), &target("p"))
        .await
        .expect("owned source result");
    // The watcher's waits run on the paused clock, so the time to the report is exact.
    tokio::time::pause();
    let changed = tokio::time::Instant::now();
    git(r.path(), &["commit", "--allow-empty", "-qm", "outside"]);
    assert_eq!(next(&mut rx, Duration::from_secs(60)).await, "p");
    let waited = changed.elapsed();
    // Not sooner either: the poll is 500 ms (it was 100 ms, a walk of the refs ten times a second).
    assert!(
        waited >= POLL_AND_QUIET - Duration::from_millis(50)
            && waited <= POLL_AND_QUIET + Duration::from_millis(10),
        "{waited:?}"
    );
    // One change is one report.
    assert!(
        tokio::time::timeout(Duration::from_secs(60), rx.recv())
            .await
            .is_err()
    );
    tokio::time::resume();
}
#[tokio::test]
async fn watcher_stops_when_the_checkout_is_gone_and_the_next_call_watches_again() {
    let parent = tempfile::tempdir().expect("create owned temporary directory");
    let dir = parent.path().join("checkout");
    let init = || {
        std::fs::create_dir(&dir).expect("create owned fixture directory");
        git(&dir, &["init", "-qb", "main"]);
        git(&dir, &["commit", "--allow-empty", "-qm", "initial"]);
    };
    init();
    let (service, mut rx) = service();
    service
        .observe(&dir, &target("p"))
        .await
        .expect("owned source result");
    std::fs::remove_dir_all(&dir).expect("remove owned fixture directory");
    // The last report: whoever shows this checkout re-reads it and finds it gone.
    assert_eq!(next(&mut rx, Duration::from_secs(3)).await, "p");
    // The watcher is dropped, not kept on a missing folder: the next call starts a new one.
    assert!(service.observe(&dir, &target("p")).await.is_err());
    init();
    service
        .observe(&dir, &target("p"))
        .await
        .expect("owned source result");
    git(&dir, &["commit", "--allow-empty", "-qm", "outside"]);
    assert_eq!(next(&mut rx, REPORTED_WITHIN).await, "p");
}
/// Puts a folder's mode back when the test ends, so the temporary repository can be removed.
struct Mode<'a>(&'a std::path::Path);
impl Drop for Mode<'_> {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(self.0, std::fs::Permissions::from_mode(0o755));
    }
}
#[tokio::test]
async fn watcher_stops_after_three_failed_stamps_in_a_row_and_watches_again_once_readable() {
    use std::os::unix::fs::PermissionsExt;
    let r = repo();
    let heads = r.path().join(".git/refs/heads");
    let (service, mut rx) = service();
    service
        .observe(r.path(), &target("p"))
        .await
        .expect("owned source result");
    let mode = Mode(&heads);
    std::fs::set_permissions(&heads, std::fs::Permissions::from_mode(0o000))
        .expect("change owned fixture mode");
    if std::fs::read_dir(&heads).is_ok() {
        // The mode does not stop this user (root), so no stamp can fail here.
        return;
    }
    tokio::time::pause();
    let failing = tokio::time::Instant::now();
    // The last report comes after the third failed poll, not the first ...
    assert_eq!(next(&mut rx, Duration::from_secs(60)).await, "p");
    let waited = failing.elapsed();
    assert!(
        waited >= POLL * 3 - Duration::from_millis(50)
            && waited <= POLL * 3 + Duration::from_millis(10),
        "{waited:?}"
    );
    tokio::time::resume();
    // ... and the watcher is gone: a call cannot start one on a checkout it cannot stamp.
    assert!(service.observe(r.path(), &target("p")).await.is_err());
    drop(mode);
    service
        .observe(r.path(), &target("p"))
        .await
        .expect("owned source result");
    git(r.path(), &["commit", "--allow-empty", "-qm", "outside"]);
    assert_eq!(next(&mut rx, REPORTED_WITHIN).await, "p");
}
#[tokio::test]
async fn unused_watcher_stops_after_the_idle_time_with_one_last_report() {
    let r = repo();
    let (service, mut rx) = service();
    service
        .observe(r.path(), &target("p"))
        .await
        .expect("owned source result");
    // Git commands run on real time; only the idle wait below runs on the paused clock.
    tokio::time::pause();
    // A call inside the idle time keeps the watcher.
    tokio::time::advance(IDLE - Duration::from_secs(60)).await;
    service
        .observe(r.path(), &target("p"))
        .await
        .expect("owned source result");
    let used = tokio::time::Instant::now();
    assert_eq!(next(&mut rx, IDLE * 3).await, "p");
    let waited = used.elapsed();
    assert!(
        waited >= IDLE && waited < IDLE + Duration::from_secs(2),
        "{waited:?}"
    );
    // It has stopped: nothing polls and nothing more is reported.
    assert!(tokio::time::timeout(IDLE * 3, rx.recv()).await.is_err());
    tokio::time::resume();
}
#[tokio::test]
async fn change_made_after_the_idle_stop_is_not_lost() {
    let r = repo();
    let (service, mut rx) = service();
    service
        .observe(r.path(), &target("p"))
        .await
        .expect("owned source result");
    tokio::time::pause();
    // The stop itself is reported, so a client that only re-reads on `git.changed` re-reads now ...
    assert_eq!(next(&mut rx, IDLE * 3).await, "p");
    tokio::time::resume();
    // ... and that read, like every `git.*` call, starts a new watcher before it reads: a change
    // made while nothing watched is in the read, and a change after it is reported.
    git(r.path(), &["commit", "--allow-empty", "-qm", "unwatched"]);
    service
        .observe(r.path(), &target("p"))
        .await
        .expect("owned source result");
    assert_eq!(git(r.path(), &["log", "-1", "--format=%s"]), "unwatched\n");
    assert!(
        tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .is_err(),
        "the new watcher starts from the state the call read"
    );
    git(r.path(), &["commit", "--allow-empty", "-qm", "watched"]);
    assert_eq!(next(&mut rx, REPORTED_WITHIN).await, "p");
}
#[tokio::test]
async fn least_recently_used_watcher_is_replaced_at_the_cap() {
    let r = repo();
    let (service, mut rx) = service();
    for i in 0..128 {
        service
            .observe(r.path(), &target(&format!("p{i}")))
            .await
            .expect("owned source result");
    }
    // p0 is used again, so p1 is now the least recently used.
    service
        .observe(r.path(), &target("p0"))
        .await
        .expect("owned source result");
    let added: Result<(), SourceGitError> = service.observe(r.path(), &target("new")).await;
    added.expect("the cap replaces a watcher instead of failing the call");
    // The replaced target is told once, so nobody waits for reports that will not come.
    assert_eq!(next(&mut rx, Duration::from_secs(3)).await, "p1");
    git(r.path(), &["commit", "--allow-empty", "-qm", "outside"]);
    let mut reported = std::collections::BTreeSet::new();
    while reported.len() < 128 {
        reported.insert(next(&mut rx, Duration::from_secs(5)).await);
    }
    assert!(reported.contains("p0") && reported.contains("new"));
    assert!(!reported.contains("p1"), "p1 is still watched");
}
