use super::*;
use crate::discard_snapshot::probe;
use std::{
    io::Write,
    os::unix::fs::MetadataExt,
    sync::Mutex,
    time::{Duration, SystemTime},
};

/// What the user confirms in the discard dialog.
async fn confirmed(fixture: &Fixture, task: &AgentTask) -> DiscardAgent {
    let check = fixture
        .svc
        .discard_check(&task.id)
        .await
        .expect("discard confirmation");
    DiscardAgent {
        id: task.id.clone(),
        expect: ReviewExpect {
            head_oid: check.head_oid,
            base_oid: None,
            unmerged_commits: Some(check.unmerged_commits),
            discard_fingerprint: check.fingerprint,
        },
    }
}

async fn fingerprint(fixture: &Fixture, task: &AgentTask) -> String {
    confirmed(fixture, task)
        .await
        .expect
        .discard_fingerprint
        .expect("worktree fingerprint")
}

/// New bytes with the same size, mode, inode and modification time.
fn rewrite_keeping_metadata(path: &Path, text: &str) {
    let before = fs::metadata(path).expect("owned file");
    assert_eq!(before.len(), text.len() as u64);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .open(path)
        .expect("owned file");
    file.write_all(text.as_bytes()).expect("owned bytes");
    file.set_modified(before.modified().expect("mtime"))
        .expect("restore mtime");
    let after = fs::metadata(path).expect("owned file");
    assert_eq!(
        (
            after.len(),
            after.mode(),
            after.ino(),
            after.modified().ok()
        ),
        (
            before.len(),
            before.mode(),
            before.ino(),
            before.modified().ok()
        )
    );
}

/// The discard was refused and removed nothing.
async fn refused(fixture: &Fixture, task: &AgentTask, wt: &Path, result: Result<AgentTask>) {
    assert!(
        matches!(result, Err(Fail::Domain(AgentsError::StateChanged { .. }))),
        "{result:?}"
    );
    assert!(wt.join("tracked").is_file(), "worktree kept");
    assert!(
        devbox_git::branches(&fixture.repo)
            .await
            .expect("branches")
            .contains(&"agent/review".into()),
        "branch kept"
    );
    let task = fixture.svc.get(&task.id).await.expect("task");
    assert_ne!(task.state, AgentState::Closed);
    assert_eq!(reservation(fixture, &task).await, Resources::Owned);
}

#[tokio::test]
async fn ignored_files_are_compared_by_metadata_without_reading_them() {
    let (fixture, task, wt) = action_fixture().await;
    fs::write(wt.join(".gitignore"), "/target/\n*.log\n").expect("owned ignore rules");
    fs::create_dir_all(wt.join("target/debug")).expect("owned build directory");
    fs::write(wt.join("target/debug/app"), "build output").expect("owned build output");
    fs::write(wt.join("run.log"), "log line").expect("owned log");
    fs::write(wt.join("notes.txt"), "user notes").expect("owned untracked note");
    let reads = std::sync::Arc::new(Mutex::new(Vec::new()));
    let seen = reads.clone();
    let _probe = probe::watch(&wt, move |path| {
        seen.lock().expect("reads").push(path.to_owned());
    });
    let first = fingerprint(&fixture, &task).await;
    let read = reads.lock().expect("reads").clone();
    // Tracked and untracked files are read; ignored ones are never opened.
    for path in [".gitignore", "notes.txt", "tracked"] {
        assert!(read.contains(&PathBuf::from(path)), "{path}: {read:?}");
    }
    assert!(
        read.iter()
            .all(|path| !path.starts_with("target") && path != Path::new("run.log")),
        "{read:?}"
    );
    // Same size, mode, inode and mtime: new bytes in an ignored file do not count.
    rewrite_keeping_metadata(&wt.join("target/debug/app"), "BUILD OUTPUT");
    rewrite_keeping_metadata(&wt.join("run.log"), "LOG LINE");
    assert_eq!(fingerprint(&fixture, &task).await, first);
    // Its metadata does: a discard confirmed before the log was touched is refused.
    let request = confirmed(&fixture, &task).await;
    fs::OpenOptions::new()
        .write(true)
        .open(wt.join("run.log"))
        .expect("owned log")
        .set_modified(SystemTime::now() + Duration::from_secs(5))
        .expect("owned mtime");
    assert_ne!(fingerprint(&fixture, &task).await, first);
    let result = fixture.svc.discard(request).await;
    refused(&fixture, &task, &wt, result).await;
    // Tracked and untracked files that Git does not ignore are still compared by content.
    for (path, text) in [("notes.txt", "USER NOTES"), ("tracked", "AGENT\n")] {
        let before = fingerprint(&fixture, &task).await;
        rewrite_keeping_metadata(&wt.join(path), text);
        assert_ne!(fingerprint(&fixture, &task).await, before, "{path}");
    }
}

#[tokio::test]
async fn tracked_change_after_confirmation_still_blocks_discard() {
    let (fixture, task, wt) = action_fixture().await;
    // Hashed after `tracked`.
    fs::write(wt.join("zz-last"), "untracked").expect("owned untracked file");
    // A tracked edit after the user confirmed.
    let request = confirmed(&fixture, &task).await;
    fs::write(wt.join("tracked"), "edited after confirmation\n").expect("owned edit");
    let result = fixture.svc.discard(request).await;
    refused(&fixture, &task, &wt, result).await;
    // A tracked edit after the discard hashed it but before the locked check, keeping its
    // size and mtime: the hash still matches, and only the metadata check can refuse.
    let request = confirmed(&fixture, &task).await;
    let tracked = wt.join("tracked");
    let edited = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let done = edited.clone();
    let watching = probe::watch(&wt, move |path| {
        if path == Path::new("zz-last") && !done.swap(true, std::sync::atomic::Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(20));
            rewrite_keeping_metadata(&tracked, "EDITED AFTER CONFIRMATION\n");
        }
    });
    let result = fixture.svc.discard(request).await;
    assert!(edited.load(std::sync::atomic::Ordering::SeqCst));
    refused(&fixture, &task, &wt, result).await;
    assert_eq!(
        fs::read_to_string(wt.join("tracked")).expect("kept edit"),
        "EDITED AFTER CONFIRMATION\n"
    );
    drop(watching);
    // Confirmed again, the unchanged worktree is discarded.
    let request = confirmed(&fixture, &task).await;
    let closed = fixture
        .svc
        .discard(request)
        .await
        .expect("confirmed discard");
    assert_eq!(closed.state, AgentState::Closed);
    assert!(!wt.exists());
}

#[tokio::test]
async fn discard_holds_the_lock_only_for_verification() {
    let (fixture, task, wt) = action_fixture().await;
    fs::write(wt.join("notes.txt"), "user notes").expect("owned untracked note");
    let request = confirmed(&fixture, &task).await;
    let reads = std::sync::Arc::new(Mutex::new(Vec::new()));
    let (seen, svc) = (reads.clone(), fixture.svc.clone());
    let _probe = probe::watch(&wt, move |path| {
        // `actions` is the lifecycle lock; free means no discard holds it during this read.
        let free = svc.actions.try_lock().is_ok();
        seen.lock().expect("reads").push((path.to_owned(), free));
    });
    let closed = fixture.svc.discard(request).await.expect("discard");
    assert_eq!(closed.state, AgentState::Closed);
    assert!(!wt.exists());
    // Every byte was hashed once, outside the lock; under it only metadata was compared.
    assert_eq!(
        *reads.lock().expect("reads"),
        [
            (PathBuf::from("notes.txt"), true),
            (PathBuf::from("tracked"), true)
        ]
    );
}
