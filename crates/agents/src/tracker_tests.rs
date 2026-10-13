use super::*;
use std::fs;
fn fixture(root: &Path, id: &str, source: &str, thread_source: &str, turn: &str) -> PathBuf {
    let dir = root.join("2026/10/13");
    fs::create_dir_all(&dir).expect("owned fixture");
    let path = dir.join(format!("rollout-{id}.jsonl"));
    fs::write(&path,format!("{{\"timestamp\":\"2026-10-13T00:00:01.000Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"{id}\",\"timestamp\":\"2026-10-13T00:00:01.000Z\",\"cwd\":\"/owned/work\",\"source\":\"{source}\",\"thread_source\":\"{thread_source}\"}}}}\n{{\"timestamp\":\"2026-10-13T00:00:02.000Z\",\"type\":\"event_msg\",\"payload\":{{\"type\":\"task_started\",\"turn_id\":\"{turn}\"}}}}\n")).expect("owned fixture");
    path
}
#[test]
fn primary_binding_ignores_title_and_rejects_stale_turn() {
    let t = tempfile::tempdir().expect("owned fixture");
    let root = t.path();
    let path = fixture(root, "primary", "cli", "user", "turn-one");
    fixture(root, "title", "cli", "title", "title-turn");
    let binding = Binding::new(utc_ms("2026-10-13T00:00:00.000Z").expect("owned fixture"));
    let current = refresh(root, "/owned/work", &binding).expect("owned fixture");
    assert_eq!(current.thread_id.as_deref(), Some("primary"));
    assert_eq!(current.turn_id.as_deref(), Some("turn-one"));
    assert!(current.matches("primary", "turn-one"));
    assert!(!current.matches("title", "title-turn"));
    let mut f = fs::OpenOptions::new()
        .append(true)
        .open(path)
        .expect("owned fixture");
    use std::io::Write;
    writeln!(f,"{{\"timestamp\":\"2026-10-13T00:00:03.000Z\",\"type\":\"event_msg\",\"payload\":{{\"type\":\"task_started\",\"turn_id\":\"turn-two\"}}}}").expect("owned fixture");
    let refreshed = refresh(root, "/owned/work", &current).expect("owned fixture");
    assert!(!refreshed.matches("primary", "turn-one"));
    assert!(refreshed.matches("primary", "turn-two"));
}
#[test]
fn missing_ambiguous_resume_and_unsafe_metadata_fail_closed() {
    let t = tempfile::tempdir().expect("owned fixture");
    let root = t.path();
    let launch = utc_ms("2026-10-13T00:00:00Z").expect("owned fixture");
    let b = Binding::new(launch);
    assert_eq!(
        refresh(root, "/owned/work", &b).expect_err("fixture operation must fail"),
        Diagnostic::Missing
    );
    let p = fixture(root, "first", "cli", "user", "t");
    fixture(root, "second", "cli", "user", "t2");
    assert_eq!(
        refresh(root, "/owned/work", &b).expect_err("fixture operation must fail"),
        Diagnostic::Ambiguous
    );
    let mut known = b.clone();
    known.thread_id = Some("first".into());
    let resumed = refresh(root, "/owned/work", &known).expect("owned fixture");
    assert_eq!(resumed.thread_id.as_deref(), Some("first"));
    known.thread_id = Some("unknown".into());
    assert_eq!(
        refresh(root, "/owned/work", &known).expect_err("fixture operation must fail"),
        Diagnostic::Missing
    );
    fs::write(p, "{truncated").expect("owned fixture");
    known.thread_id = Some("first".into());
    assert!(refresh(root, "/owned/work", &known).is_err());
}

#[test]
fn old_history_is_skipped_and_known_exact_rollout_ignores_unrelated_history() {
    let t = tempfile::tempdir().expect("owned fixture");
    let old = t.path().join("2020/01/01");
    fs::create_dir_all(&old).expect("owned fixture");
    let file = fs::File::create(old.join("rollout-old.jsonl")).expect("owned fixture");
    file.set_len(64 * 1024 * 1024).expect("owned fixture");
    fixture(t.path(), "primary", "cli", "user", "turn");
    let binding = Binding::new(utc_ms("2026-10-13T00:00:00Z").expect("owned fixture"));
    let known = refresh(t.path(), "/owned/work", &binding).expect("owned fixture");
    fs::write(
        t.path().join("2026/10/13/rollout-unrelated.jsonl"),
        "broken",
    )
    .expect("owned fixture");
    assert_eq!(
        refresh(t.path(), "/owned/work", &known)
            .expect("owned fixture")
            .thread_id
            .as_deref(),
        Some("primary")
    );
}
#[test]
fn malformed_timestamps_are_bounded_and_rejected() {
    for text in [
        "999999999999999999999-10-13T00:00:00Z",
        "2026-02-30T00:00:00Z",
        "2026-10-13T00:00:00.123456789012Z",
        "2026-00-13T00:00:00Z",
    ] {
        assert!(utc_ms(text).is_none());
    }
    assert_eq!(utc_ms("1970-01-01T00:00:00Z"), Some(0));
    assert_eq!(utc_ms("1970-01-01T00:00:00.12Z"), Some(120));
}

#[test]
fn known_thread_resume_discovers_old_dated_rollout_with_current_launch_turn() {
    let temp = tempfile::tempdir().expect("owned fixture");
    let old = temp.path().join("2026/10/12");
    fs::create_dir_all(&old).expect("owned fixture");
    let path = old.join("rollout-primary.jsonl");
    fs::write(&path,concat!(
      "{\"type\":\"session_meta\",\"payload\":{\"id\":\"primary\",\"timestamp\":\"2026-10-12T00:00:00Z\",\"cwd\":\"/owned/work\",\"source\":\"cli\",\"thread_source\":\"user\"}}\n",
      "{\"timestamp\":\"2026-10-13T00:00:01Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"task_started\",\"turn_id\":\"resumed\"}}\n"
    )).expect("owned fixture");
    let mut known = Binding::new(utc_ms("2026-10-13T00:00:00Z").expect("owned fixture"));
    known.thread_id = Some("primary".into());
    let resumed = refresh(temp.path(), "/owned/work", &known)
        .expect("known thread across earlier dated directories");
    assert!(resumed.matches("primary", "resumed"));
    assert_eq!(resumed.rollout, Some(path));
    known.thread_id = Some("different".into());
    assert_eq!(
        refresh(temp.path(), "/owned/work", &known).expect_err("fixture operation must fail"),
        Diagnostic::Missing
    );
}

#[test]
fn integer_started_at_uses_precise_envelope_launch_boundary() {
    let temp = tempfile::tempdir().expect("owned fixture");
    let path = fixture(temp.path(), "primary", "cli", "user", "turn");
    let text = fs::read_to_string(&path).expect("owned fixture").replace(
        "\"turn_id\":\"turn\"",
        "\"turn_id\":\"turn\",\"started_at\":1791849602",
    );
    fs::write(&path, text).expect("owned fixture");
    let binding = Binding::new(utc_ms("2026-10-13T00:00:01.500Z").expect("owned fixture"));
    // Known resumes may have older session creation. The current turn's envelope
    // timestamp has millisecond precision; the integer is intentionally unused.
    let mut known = binding;
    known.thread_id = Some("primary".into());
    let current = refresh(temp.path(), "/owned/work", &known).expect("integer lifecycle metadata");
    assert!(current.matches("primary", "turn"));
    assert_eq!(current.turn_started_ms, utc_ms("2026-10-13T00:00:02Z"));
}

#[test]
fn non_primary_source_objects_are_skipped_before_strict_primary_metadata() {
    let temp = tempfile::tempdir().expect("owned fixture");
    fixture(temp.path(), "primary", "cli", "user", "turn");
    let unrelated = temp.path().join("2026/10/13/rollout-subagent.jsonl");
    fs::write(&unrelated,concat!(
      "{\"type\":\"session_meta\",\"payload\":{\"source\":{\"subagent\":{\"thread_spawn\":{\"parent_thread_id\":\"other\"}}},\"thread_source\":\"agent\",\"cwd\":\"/elsewhere\",\"timestamp\":\"2026-10-13T00:00:00Z\",\"id\":\"subagent\"}}\n",
      "{synthetic-unrelated-truncated-body"
    )).expect("owned fixture");
    let binding = Binding::new(utc_ms("2026-10-13T00:00:00Z").expect("owned fixture"));
    let primary = refresh(temp.path(), "/owned/work", &binding)
        .expect("unrelated legitimate source enum is not a primary candidate");
    assert!(primary.matches("primary", "turn"));
}
