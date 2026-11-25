use super::*;
use std::{fs, io::Cursor, path::PathBuf};

/// The fixture's content: what follows its `#` provenance lines and the `---` line.
fn fixture(text: &'static str) -> &'static str {
    text.split_once("\n---\n")
        .expect("fixture header ends with a --- line")
        .1
}
fn transcript() -> &'static str {
    fixture(include_str!(
        "../tests/fixtures/antigravity-1.5.17/transcript_full.jsonl"
    ))
}
const CONVERSATION: &str = "00000000-0000-4000-8000-000000000001";

fn counts(usage: &AgentUsage) -> Option<(u64, u64, u64, u64)> {
    usage
        .tokens
        .as_ref()
        .map(|t| (t.input, t.output, t.cache_read, t.cache_write))
}

fn task(session: Option<&str>) -> AgentTask {
    serde_json::from_value(serde_json::json!({
        "id":"a","projectId":"p","title":"t","profile":"antigravity","baseBranch":"main",
        "branch":"agent/fix","worktreePath":"/w/fix","workdir":"/w/fix","sessionId":session,
        "terminalId":null,"state":"idle","reason":null,"attention":null,"port":20000,
        "prompt":"prompt","followUps":[],"tokens":null,"prUrl":null,"closedHow":null,
        "createdMs":0,"updatedMs":0,"closedMs":null
    }))
    .expect("owned task")
}

/// An owned home whose agy data holds the conversation's transcript, and the binding that names it.
fn home_with(transcript: &str) -> (tempfile::TempDir, PathBuf, Binding) {
    let home = tempfile::tempdir().expect("owned home");
    let logs = home
        .path()
        .join(".gemini/antigravity-cli/brain")
        .join(CONVERSATION)
        .join(".system_generated/logs");
    fs::create_dir_all(&logs).expect("owned agy data");
    let path = logs.join("transcript_full.jsonl");
    fs::write(&path, transcript).expect("owned transcript");
    let mut binding = Binding::new(1);
    binding.transcript = Some(path.clone());
    (home, path, binding)
}

#[cfg(unix)]
#[test]
fn the_real_transcript_sums_every_model_call() {
    // The sums the spike computed from the same file: 10 model calls.
    let usage = read(Cursor::new(transcript()));
    assert_eq!(counts(&usage), Some((42_546, 2_582, 180_479, 0)));
    assert!(!usage.truncated);
    // The same through the path the binding names.
    let (home, _, binding) = home_with(transcript());
    let read = super::usage(home.path(), &task(Some(CONVERSATION)), Some(&binding));
    assert_eq!(counts(&read), Some((42_546, 2_582, 180_479, 0)));
    assert!(!read.truncated);
}

#[test]
fn only_model_calls_count_and_a_step_written_twice_counts_once() {
    let lines = concat!(
        r#"{"step_index":0,"type":"USER_INPUT","input_tokens":999,"output_tokens":999}"#,
        "\n",
        r#"{"step_index":1,"type":"PLANNER_RESPONSE","input_tokens":10,"cache_read_tokens":4,"output_tokens":2,"content":{"any":["shape"]}}"#,
        "\n",
        r#"{"step_index":1,"type":"PLANNER_RESPONSE","input_tokens":10,"cache_read_tokens":4,"output_tokens":7}"#,
        "\n\n",
        r#"{"step_index":2,"type":"PLANNER_RESPONSE","status":"RUNNING"}"#,
        "\n",
        r#"{"step_index":3,"type":"PLANNER_RESPONSE","output_tokens":1}"#,
        "\n",
    );
    let usage = read(Cursor::new(lines));
    assert_eq!(counts(&usage), Some((10, 8, 4, 0)));
    assert!(!usage.truncated);
    // No model call yet: no total, and nothing is wrong.
    let none = read(Cursor::new("{\"step_index\":0,\"type\":\"USER_INPUT\"}\n"));
    assert_eq!((counts(&none), none.truncated), (None, false));
    assert_eq!(counts(&read(Cursor::new(""))), None);
}

#[test]
fn what_cannot_be_read_is_left_out_and_reported() {
    let call = r#"{"step_index":1,"type":"PLANNER_RESPONSE","input_tokens":5,"output_tokens":1}"#;
    // A line over the limit is skipped without being parsed, and the lines after it still count.
    let long = format!(
        "{{\"step_index\":9,\"type\":\"PLANNER_RESPONSE\",\"input_tokens\":100,\"content\":\"{}\"}}",
        "x".repeat(MAX_LINE)
    );
    let usage = read(Cursor::new(format!("{long}\n{call}\n")));
    assert_eq!(
        (counts(&usage), usage.truncated),
        (Some((5, 1, 0, 0)), true)
    );
    for broken in [
        // Not JSON, a step that is not an object, counts of another type.
        "not json\n".to_owned(),
        "[1,2]\n".to_owned(),
        "{\"step_index\":2,\"type\":\"PLANNER_RESPONSE\",\"input_tokens\":\"many\"}\n".to_owned(),
        "{\"step_index\":2,\"type\":\"PLANNER_RESPONSE\",\"input_tokens\":-1}\n".to_owned(),
        // A model call without its index cannot be told from a rewrite of another.
        "{\"type\":\"PLANNER_RESPONSE\",\"input_tokens\":3}\n".to_owned(),
        // The last line is still being written.
        "{\"step_index\":2,\"type\":\"PLANNER_RESPONSE\",\"input_tokens\":3".to_owned(),
    ] {
        let usage = read(Cursor::new(format!("{call}\n{broken}")));
        assert_eq!(
            (counts(&usage), usage.truncated),
            (Some((5, 1, 0, 0)), true),
            "{broken}"
        );
    }
    // A sum that does not fit is reported, not wrapped.
    let huge = format!(
        "{{\"step_index\":1,\"type\":\"PLANNER_RESPONSE\",\"input_tokens\":{max}}}\n{{\"step_index\":2,\"type\":\"PLANNER_RESPONSE\",\"input_tokens\":{max}}}\n",
        max = u64::MAX
    );
    let usage = read(Cursor::new(huge));
    assert_eq!(
        (counts(&usage), usage.truncated),
        (Some((u64::MAX, 0, 0, 0)), true)
    );
}

#[cfg(unix)]
#[test]
fn only_the_named_regular_file_below_the_users_agy_data_is_opened() {
    let (home, path, binding) = home_with(transcript());
    let known = task(Some(CONVERSATION));
    let read = |binding: Option<&Binding>| super::usage(home.path(), &known, binding);
    // Nothing named yet, or no conversation: no total, and nothing is wrong.
    let unnamed = Binding::new(1);
    for usage in [
        read(None),
        read(Some(&unnamed)),
        super::usage(home.path(), &task(None), Some(&binding)),
    ] {
        assert_eq!((counts(&usage), usage.truncated), (None, false));
    }
    // A path outside agy's data, whatever it holds, is never opened.
    let outside = home.path().join("outside.jsonl");
    fs::write(&outside, transcript()).expect("owned file");
    let data = home.path().join(".gemini/antigravity-cli");
    for named in [
        outside.clone(),
        data.join("brain/../../../outside.jsonl"),
        PathBuf::from("/etc/passwd"),
        PathBuf::from("relative/transcript_full.jsonl"),
    ] {
        let mut other = Binding::new(1);
        other.transcript = Some(named.clone());
        let usage = read(Some(&other));
        assert_eq!((counts(&usage), usage.truncated), (None, true), "{named:?}");
    }
    // A folder, or a symlink to a file outside, in the transcript's place.
    let mut other = Binding::new(1);
    other.transcript = Some(data.join("brain"));
    assert!(read(Some(&other)).truncated);
    fs::remove_file(&path).expect("owned transcript");
    // Gone (agy removed the conversation): no total, and nothing is wrong.
    let gone = read(Some(&binding));
    assert_eq!((counts(&gone), gone.truncated), (None, false));
    std::os::unix::fs::symlink(&outside, &path).expect("owned symlink");
    let linked = read(Some(&binding));
    assert_eq!((counts(&linked), linked.truncated), (None, true));
    // A symlink among the folders on the way.
    let real = home.path().join("real-brain");
    fs::rename(data.join("brain"), &real).expect("owned folder");
    std::os::unix::fs::symlink(&real, data.join("brain")).expect("owned symlink");
    fs::remove_file(
        real.join(CONVERSATION)
            .join(".system_generated/logs/transcript_full.jsonl"),
    )
    .expect("owned symlink");
    fs::write(
        real.join(CONVERSATION)
            .join(".system_generated/logs/transcript_full.jsonl"),
        transcript(),
    )
    .expect("owned transcript");
    let through = read(Some(&binding));
    assert_eq!((counts(&through), through.truncated), (None, true));
}

#[cfg(unix)]
#[test]
fn a_transcript_over_the_size_limit_is_not_read() {
    let (home, path, binding) = home_with(transcript());
    // A sparse file: its length is over the limit and nothing is written.
    let file = fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .expect("owned transcript");
    file.set_len(MAX_BYTES + 1).expect("sparse length");
    let usage = super::usage(home.path(), &task(Some(CONVERSATION)), Some(&binding));
    assert_eq!((counts(&usage), usage.truncated), (None, true));
}
