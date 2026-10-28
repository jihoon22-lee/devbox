use super::*;
use crate::discard_snapshot::probe;
use std::{
    ffi::OsStr,
    io::Write,
    os::unix::{
        ffi::OsStrExt,
        fs::{MetadataExt, PermissionsExt},
    },
    sync::{Arc, Mutex},
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

/// The discard was refused as a change and removed nothing.
async fn refused(fixture: &Fixture, task: &AgentTask, wt: &Path, result: Result<AgentTask>) {
    assert!(
        matches!(result, Err(Fail::Domain(AgentsError::StateChanged { .. }))),
        "{result:?}"
    );
    kept(fixture, task, wt).await;
}

/// The worktree, its branch, the open task and its ownership are all still there.
async fn kept(fixture: &Fixture, task: &AgentTask, wt: &Path) {
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
    // Git lists a directory holding ignored and other files file by file, never whole.
    fs::create_dir(wt.join("mixed")).expect("owned mixed directory");
    fs::write(wt.join("mixed/a.log"), "mixed log").expect("owned ignored file");
    fs::write(wt.join("mixed/b.txt"), "mixed notes").expect("owned untracked file");
    let reads = Arc::new(Mutex::new(Vec::new()));
    let seen = reads.clone();
    let _probe = probe::watch(&wt, move |path| {
        seen.lock().expect("reads").push(path.to_owned());
    });
    let first = fingerprint(&fixture, &task).await;
    let read = reads.lock().expect("reads").clone();
    // Tracked and untracked files are read; ignored ones are never opened.
    for path in [".gitignore", "mixed/b.txt", "notes.txt", "tracked"] {
        assert!(read.contains(&PathBuf::from(path)), "{path}: {read:?}");
    }
    assert!(
        read.iter().all(|path| !path.starts_with("target")
            && path != Path::new("run.log")
            && path != Path::new("mixed/a.log")),
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
    let edited = Arc::new(std::sync::atomic::AtomicBool::new(false));
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
    let reads = Arc::new(Mutex::new(Vec::new()));
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

#[tokio::test]
async fn ignored_file_mode_or_inode_change_refuses_discard() {
    let (fixture, task, wt) = action_fixture().await;
    fs::write(wt.join(".gitignore"), "*.log\n").expect("owned ignore rules");
    let log = wt.join("run.log");
    fs::write(&log, "log line").expect("owned log");
    // chmod after confirmation (the owner's execute bit): the same size, mtime and inode.
    let first = fingerprint(&fixture, &task).await;
    let request = confirmed(&fixture, &task).await;
    let before = fs::metadata(&log).expect("owned log");
    let mode = (before.mode() & 0o777) ^ 0o100;
    fs::set_permissions(&log, fs::Permissions::from_mode(mode)).expect("owned mode");
    let after = fs::metadata(&log).expect("owned log");
    assert_eq!(
        (after.len(), after.ino(), after.modified().ok()),
        (before.len(), before.ino(), before.modified().ok())
    );
    assert_ne!(after.mode(), before.mode());
    assert_ne!(fingerprint(&fixture, &task).await, first);
    let result = fixture.svc.discard(request).await;
    refused(&fixture, &task, &wt, result).await;
    // Replaced by a rename after confirmation: the same size, mtime and mode, another inode.
    let second = fingerprint(&fixture, &task).await;
    let request = confirmed(&fixture, &task).await;
    let before = fs::metadata(&log).expect("owned log");
    let swap = wt.join("swap.log");
    fs::write(&swap, "LOG LINE").expect("owned replacement");
    fs::set_permissions(&swap, before.permissions()).expect("owned mode");
    fs::OpenOptions::new()
        .write(true)
        .open(&swap)
        .expect("owned replacement")
        .set_modified(before.modified().expect("mtime"))
        .expect("owned mtime");
    fs::rename(&swap, &log).expect("owned replace");
    let after = fs::metadata(&log).expect("owned log");
    assert_eq!(
        (after.len(), after.mode(), after.modified().ok()),
        (before.len(), before.mode(), before.modified().ok())
    );
    assert_ne!(after.ino(), before.ino());
    assert_ne!(fingerprint(&fixture, &task).await, second);
    let result = fixture.svc.discard(request).await;
    refused(&fixture, &task, &wt, result).await;
}

#[tokio::test]
async fn ignored_names_need_not_be_utf8() {
    let (fixture, task, wt) = action_fixture().await;
    fs::write(wt.join(".gitignore"), "*.log\n/out-*/\n").expect("owned ignore rules");
    // An ignored file and an ignored directory whose names are not UTF-8.
    fs::write(wt.join(OsStr::from_bytes(b"build-\xff.log")), "log").expect("owned log");
    let out = wt.join(OsStr::from_bytes(b"out-\xfe"));
    fs::create_dir(&out).expect("owned output directory");
    fs::write(out.join("artifact"), "output").expect("owned output");
    let reads = Arc::new(Mutex::new(Vec::new()));
    let seen = reads.clone();
    let _probe = probe::watch(&wt, move |path| {
        seen.lock().expect("reads").push(path.to_owned());
    });
    let request = confirmed(&fixture, &task).await;
    let closed = fixture.svc.discard(request).await.expect("discard");
    assert_eq!(closed.state, AgentState::Closed);
    assert!(!wt.exists());
    // The confirmation and the discard each read only what Git does not ignore.
    assert_eq!(
        *reads.lock().expect("reads"),
        [".gitignore", "tracked", ".gitignore", "tracked"].map(PathBuf::from)
    );
}

#[tokio::test]
async fn listing_over_its_cap_refuses_discard() {
    let (fixture, task, wt) = action_fixture().await;
    fs::write(wt.join(".gitignore"), "*.log\n").expect("owned ignore rules");
    // About 2 KiB of ignored names, while the index lists only `tracked`.
    for index in 0..32 {
        let name = format!("{index:02}-{}.log", "x".repeat(60));
        fs::write(wt.join(name), "log").expect("owned log");
    }
    let request = confirmed(&fixture, &task).await;
    let cap = probe::cap_listings(&wt, 1024);
    let result = fixture.svc.discard(request.clone()).await;
    assert!(
        matches!(
            &result,
            Err(Fail::Domain(AgentsError::DiscardSnapshotUnavailable { reason }))
                if reason == "listing_bytes"
        ),
        "{result:?}"
    );
    kept(&fixture, &task, &wt).await;
    // Within the cap, the same confirmation discards the unchanged worktree.
    drop(cap);
    let closed = fixture.svc.discard(request).await.expect("discard");
    assert_eq!(closed.state, AgentState::Closed);
}

#[tokio::test]
async fn git_failure_before_the_lock_is_reported_not_taken_for_a_change() {
    let (fixture, task, wt) = action_fixture().await;
    let request = confirmed(&fixture, &task).await;
    let head = &request.expect.head_oid;
    let base = devbox_git::run::resolve(&fixture.repo, "main")
        .await
        .expect("base");
    // As in `discard`: hashed before the lock, confirmed under it once the locked checks pass.
    // The base cannot be resolved while the hash runs and is back for the locked checks:
    // nothing changed, but Git failed.
    git(&fixture.repo, &["update-ref", "-d", "refs/heads/main"]);
    let hashed = fixture.svc.hash_for_discard(&request).await;
    git(&fixture.repo, &["update-ref", "refs/heads/main", &base]);
    let result = fixture
        .svc
        .confirm_discard_snapshot(hashed, &wt, head, &base, &request.expect)
        .await
        .err();
    assert!(matches!(result, Some(Fail::Internal(_))), "{result:?}");
    // A task that could not be discarded before the lock but can now did change.
    let partial = Resources::Partial {
        path_exists: true,
        branch_exists: Some(true),
        worktree_registered: Some(true),
    };
    fixture
        .svc
        .record_resources(&task.id, partial)
        .await
        .expect("owned reservation");
    let hashed = fixture.svc.hash_for_discard(&request).await;
    fixture
        .svc
        .record_resources(&task.id, Resources::Owned)
        .await
        .expect("owned reservation");
    let result = fixture
        .svc
        .confirm_discard_snapshot(hashed, &wt, head, &base, &request.expect)
        .await
        .err();
    assert!(
        matches!(result, Some(Fail::Domain(AgentsError::StateChanged { .. }))),
        "{result:?}"
    );
}
