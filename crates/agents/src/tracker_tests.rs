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

const MIB: usize = 1024 * 1024;
/// An owned primary `session_meta` line whose instructions make it `pad` bytes longer.
fn session_meta(pad: usize) -> String {
    format!(
        "{{\"timestamp\":\"2026-10-13T00:00:01.000Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"primary\",\"timestamp\":\"2026-10-13T00:00:01.000Z\",\"cwd\":\"/owned/work\",\"source\":\"cli\",\"thread_source\":\"user\",\"base_instructions\":{{\"text\":\"{}\"}}}}}}\n",
        "i".repeat(pad)
    )
}
/// A `task_started` line padded to `len` bytes including its newline (0: unpadded).
fn turn(id: &str, at: &str, len: usize) -> String {
    let line = |pad: usize| {
        format!(
            "{{\"timestamp\":\"{at}\",\"type\":\"event_msg\",\"payload\":{{\"type\":\"task_started\",\"turn_id\":\"{id}\",\"pad\":\"{}\"}}}}\n",
            " ".repeat(pad)
        )
    };
    let short = line(0);
    line(len.saturating_sub(short.len()))
}
/// Records the tracker does not read, `len` bytes in all, in lines of 50 000–100 000 bytes
/// (one shorter line when `len` is under 100 000).
fn filler(mut len: usize) -> String {
    let (head, tail) = (
        "{\"timestamp\":\"2026-10-13T00:00:02.500Z\",\"type\":\"response_item\",\"payload\":{\"text\":\"",
        "\"}}\n",
    );
    let mut text = String::new();
    while len > 0 {
        let line = if len > 200_000 {
            100_000
        } else if len > 100_000 {
            len / 2
        } else {
            len
        };
        text += &format!("{head}{}{tail}", "x".repeat(line - head.len() - tail.len()));
        len -= line;
    }
    text
}
fn rollout(root: &Path, text: &str) -> PathBuf {
    let dir = root.join("2026/10/13");
    fs::create_dir_all(&dir).expect("owned fixture");
    let path = dir.join("rollout-primary.jsonl");
    fs::write(&path, text).expect("owned fixture");
    path
}
fn launch() -> Binding {
    Binding::new(utc_ms("2026-10-13T00:00:00Z").expect("owned fixture"))
}

#[test]
fn large_rollout_finds_the_latest_turn_in_the_tail() {
    let t = tempfile::tempdir().expect("owned fixture");
    let mut text = session_meta(0) + &turn("early", "2026-10-13T00:00:02.000Z", 0);
    text += &filler(9 * MIB);
    text += &turn("latest", "2026-10-13T00:00:03.000Z", 0);
    text += &filler(4096);
    // The 8 MiB window starts inside a record: that partial line is dropped, not parsed.
    assert_ne!(text.as_bytes()[text.len() - 8 * MIB - 1], b'\n');
    rollout(t.path(), &text);
    let found = refresh(t.path(), "/owned/work", &launch()).expect("long rollout");
    assert!(found.matches("primary", "latest"));
    assert_eq!(found.turn_started_ms, utc_ms("2026-10-13T00:00:03Z"));
    let known = refresh(t.path(), "/owned/work", &found).expect("known long rollout");
    assert!(known.matches("primary", "latest"));
}

#[test]
fn tail_window_is_the_last_eight_mebibytes() {
    for (shift, kept) in [(0, true), (1, false)] {
        let t = tempfile::tempdir().expect("owned fixture");
        let boundary = turn("boundary", "2026-10-13T00:00:03.000Z", 0);
        let mut text = session_meta(0) + &turn("early", "2026-10-13T00:00:02.000Z", 0);
        text += &filler(64 * 1024);
        let at = text.len();
        text += &boundary;
        text += &filler(8 * MIB - boundary.len() + shift);
        assert_eq!(text.len() - 8 * MIB, at + shift);
        rollout(t.path(), &text);
        let found = refresh(t.path(), "/owned/work", &launch());
        if kept {
            // A line that starts exactly at the window is whole and is read.
            assert!(found.expect("boundary turn").matches("primary", "boundary"));
        } else {
            // One byte later it is the window's partial first line; the head is never read.
            assert_eq!(
                found.expect_err("no turn in the window"),
                Diagnostic::Missing
            );
        }
    }
}

#[test]
fn oversized_line_is_skipped_not_fatal() {
    let t = tempfile::tempdir().expect("owned fixture");
    let mut text = session_meta(0) + &turn("one", "2026-10-13T00:00:02.000Z", 0);
    text += &format!(
        "{{\"type\":\"response_item\",\"payload\":\"{}\"}}\n",
        "o".repeat(300 * 1024)
    );
    text += &turn("at-limit", "2026-10-13T00:00:03.000Z", 128 * 1024);
    text += &turn("over-limit", "2026-10-13T00:00:04.000Z", 128 * 1024 + 1);
    // Codex may still be writing a long line: over the limit, it is skipped either way.
    text += &format!(
        "{{\"type\":\"response_item\",\"payload\":\"{}",
        "p".repeat(200 * 1024)
    );
    let path = rollout(t.path(), &text);
    let found = refresh(t.path(), "/owned/work", &launch()).expect("oversized lines skipped");
    assert!(found.matches("primary", "at-limit"));
    let (mut budget, mut skipped) = (u64::MAX, 0);
    let direct = read(&path, "/owned/work", &launch(), &mut budget, &mut skipped)
        .expect("oversized lines skipped")
        .expect("primary rollout");
    assert!(direct.matches("primary", "at-limit"));
    // The 300 KiB record, the turn one byte over the limit and the unfinished last line.
    assert_eq!(skipped, 3);
}

#[test]
fn session_meta_over_128k_is_read() {
    let t = tempfile::tempdir().expect("owned fixture");
    // Codex writes its instructions into session_meta, so its line has its own limit.
    let meta = session_meta(300 * 1024);
    assert!(meta.len() > 128 * 1024);
    rollout(
        t.path(),
        &(meta + &turn("turn", "2026-10-13T00:00:02.000Z", 0)),
    );
    let found = refresh(t.path(), "/owned/work", &launch()).expect("long session_meta");
    assert!(found.matches("primary", "turn"));
}

#[test]
fn session_meta_line_limit_is_one_mebibyte() {
    for (len, allowed) in [(MIB, true), (MIB + 1, false)] {
        let t = tempfile::tempdir().expect("owned fixture");
        let meta = session_meta(len - session_meta(0).len());
        assert_eq!(meta.len(), len);
        rollout(
            t.path(),
            &(meta + &turn("turn", "2026-10-13T00:00:02.000Z", 0)),
        );
        let found = refresh(t.path(), "/owned/work", &launch());
        if allowed {
            assert!(
                found
                    .expect("first line at the limit")
                    .matches("primary", "turn")
            );
        } else {
            assert_eq!(
                found.expect_err("over the limit"),
                Diagnostic::LimitExceeded
            );
        }
    }
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
