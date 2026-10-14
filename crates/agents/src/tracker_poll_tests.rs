use super::*;
use std::{cell::Cell, fs, io::Write, path::PathBuf, time::Duration};

fn fixture() -> (tempfile::TempDir, PathBuf, Binding) {
    let root = tempfile::tempdir().expect("owned metadata");
    let path = root.path().join("rollout-primary.jsonl");
    fs::write(&path, concat!(
        "{\"type\":\"session_meta\",\"payload\":{\"id\":\"primary\",\"timestamp\":\"2026-10-13T00:00:01Z\",\"cwd\":\"/owned/work\",\"source\":\"cli\",\"thread_source\":\"user\"}}\n",
        "{\"timestamp\":\"2026-10-13T00:00:02Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"task_started\",\"turn_id\":\"one\"}}\n"
    )).expect("owned rollout");
    let launch = super::super::utc_ms("2026-10-13T00:00:00Z").expect("fixture time");
    let binding = super::super::refresh(root.path(), "/owned/work", &Binding::new(launch))
        .expect("verified primary binding");
    (root, path, binding)
}

fn append_turn(path: &Path, turn: &str) {
    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(path)
        .expect("owned rollout");
    writeln!(file, "{{\"timestamp\":\"2026-10-13T00:00:03Z\",\"type\":\"event_msg\",\"payload\":{{\"type\":\"task_started\",\"turn_id\":\"{turn}\"}}}}").expect("append current turn");
}

fn counted<'a>(
    reads: &'a Cell<usize>,
) -> impl FnOnce(&Path, &str, &Binding) -> Result<Binding, Diagnostic> + 'a {
    move |root, workdir, binding| {
        reads.set(reads.get() + 1);
        super::super::refresh(root, workdir, binding)
    }
}

#[test]
fn unchanged_bound_rollout_is_not_parsed_again() {
    let (root, path, binding) = fixture();
    // Include a nontrivial real message body: the counter guards parser invocations,
    // not merely whether the resulting binding changed.
    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(path)
        .expect("owned rollout");
    for _ in 0..8 {
        writeln!(
            file,
            "{{\"type\":\"response_item\",\"payload\":{{\"text\":\"{}\"}}}}",
            "x".repeat(64 * 1024)
        )
        .expect("owned body");
    }
    let now = Instant::now();
    let mut poll = Poll::default();
    let reads = Cell::new(0);
    assert!(
        poll.refresh_with(now, root.path(), "/owned/work", &binding, counted(&reads))
            .expect("first read")
            .is_some()
    );
    for seconds in [1, 2, 4, 60] {
        assert!(
            poll.refresh_with(
                now + Duration::from_secs(seconds),
                root.path(),
                "/owned/work",
                &binding,
                counted(&reads)
            )
            .expect("unchanged poll")
            .is_none()
        );
    }
    assert_eq!(
        reads.get(),
        1,
        "unchanged metadata must not reread message bodies"
    );
}

#[test]
fn growing_rollouts_use_a_bounded_fallback_but_explicit_reads_remain_immediate() {
    let (root, path, binding) = fixture();
    let now = Instant::now();
    let mut poll = Poll::default();
    let reads = Cell::new(0);
    poll.refresh_with(now, root.path(), "/owned/work", &binding, counted(&reads))
        .expect("first read");
    append_turn(&path, "two");
    assert!(!poll.due(now + Duration::from_millis(250)));
    assert!(
        poll.refresh_with(
            now + Duration::from_millis(250),
            root.path(),
            "/owned/work",
            &binding,
            counted(&reads)
        )
        .expect("throttled poll")
        .is_none()
    );
    assert!(
        super::super::refresh(root.path(), "/owned/work", &binding)
            .expect("notify/resume exact read")
            .matches("primary", "two")
    );
    append_turn(&path, "three");
    assert!(
        poll.refresh_with(
            now + Duration::from_secs(1),
            root.path(),
            "/owned/work",
            &binding,
            counted(&reads)
        )
        .expect("throttled growth")
        .is_none()
    );
    assert_eq!(reads.get(), 1);
    assert!(poll.due(now + Duration::from_secs(2)));
    assert!(
        poll.refresh_with(
            now + Duration::from_secs(2),
            root.path(),
            "/owned/work",
            &binding,
            counted(&reads)
        )
        .expect("fallback read")
        .expect("changed file")
        .matches("primary", "three")
    );
    assert_eq!(reads.get(), 2);
}

#[cfg(unix)]
#[test]
fn replacement_and_launch_identity_changes_require_a_new_read() {
    let (root, path, binding) = fixture();
    let now = Instant::now();
    let mut poll = Poll::default();
    let reads = Cell::new(0);
    poll.refresh_with(now, root.path(), "/owned/work", &binding, counted(&reads))
        .expect("first read");
    let modified = fs::metadata(&path)
        .expect("metadata")
        .modified()
        .expect("mtime");
    let replacement = root.path().join("replacement");
    fs::write(&replacement, fs::read(&path).expect("owned content")).expect("replacement");
    fs::File::open(&replacement)
        .expect("replacement file")
        .set_times(fs::FileTimes::new().set_modified(modified))
        .expect("same mtime");
    fs::rename(replacement, &path).expect("replace same-sized file");
    assert!(
        poll.refresh_with(
            now + Duration::from_secs(2),
            root.path(),
            "/owned/work",
            &binding,
            counted(&reads)
        )
        .expect("identity replacement")
        .is_some()
    );
    let mut resumed = binding.clone();
    resumed.launch_id = Some("another-launch".into());
    assert!(
        poll.refresh_with(
            now + Duration::from_secs(4),
            root.path(),
            "/owned/work",
            &resumed,
            counted(&reads)
        )
        .expect("new launch")
        .is_some()
    );
    assert_eq!(reads.get(), 3);
    assert_eq!(
        poll.refresh_with(
            now + Duration::from_secs(6),
            root.path(),
            "/different",
            &resumed,
            counted(&reads)
        ),
        Err(Diagnostic::Missing)
    );
    let target = root.path().join("original");
    fs::rename(&path, &target).expect("owned move");
    std::os::unix::fs::symlink(target, &path).expect("owned alias");
    assert_eq!(
        poll.refresh_with(
            now + Duration::from_secs(8),
            root.path(),
            "/owned/work",
            &resumed,
            counted(&reads)
        ),
        Err(Diagnostic::UnsafePath)
    );
}

#[test]
fn errors_retry_at_the_fallback_deadline_and_do_not_cache_invalid_metadata() {
    let (root, path, binding) = fixture();
    let original = fs::read(&path).expect("owned content");
    fs::write(&path, "{broken\n").expect("invalid fixture");
    let now = Instant::now();
    let mut poll = Poll::default();
    let reads = Cell::new(0);
    assert_eq!(
        poll.refresh_with(now, root.path(), "/owned/work", &binding, counted(&reads)),
        Err(Diagnostic::Invalid)
    );
    assert!(
        poll.refresh_with(
            now + Duration::from_millis(250),
            root.path(),
            "/owned/work",
            &binding,
            counted(&reads)
        )
        .expect("throttled invalid file")
        .is_none()
    );
    assert_eq!(
        poll.refresh_with(
            now + Duration::from_secs(2),
            root.path(),
            "/owned/work",
            &binding,
            counted(&reads)
        ),
        Err(Diagnostic::Invalid)
    );
    assert_eq!(reads.get(), 2);
    fs::write(path, original).expect("repair fixture");
    assert!(
        poll.refresh_with(
            now + Duration::from_secs(4),
            root.path(),
            "/owned/work",
            &binding,
            counted(&reads)
        )
        .expect("recovered metadata")
        .expect("new read")
        .matches("primary", "one")
    );
}

#[test]
fn growth_during_a_read_is_not_mistaken_for_an_unchanged_snapshot() {
    let (root, path, binding) = fixture();
    let now = Instant::now();
    let mut poll = Poll::default();
    poll.refresh_with(
        now,
        root.path(),
        "/owned/work",
        &binding,
        |root, workdir, binding| {
            let result = super::super::refresh(root, workdir, binding)?;
            append_turn(&path, "two");
            Ok(result)
        },
    )
    .expect("initial read");
    assert!(
        poll.refresh(
            now + Duration::from_secs(2),
            root.path(),
            "/owned/work",
            &binding
        )
        .expect("read appended metadata")
        .expect("changed file")
        .matches("primary", "two")
    );
}
