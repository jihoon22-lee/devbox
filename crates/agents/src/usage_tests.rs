use super::*;
use std::io::Cursor;
#[test]
fn claude_deduplicates_with_maxima_and_verifies_owner() {
    let lines = concat!(
        "{\"type\":\"assistant\",\"sessionId\":\"s\",\"cwd\":\"/work\",\"message\":{\"id\":\"m\",\"usage\":{\"input_tokens\":10,\"output_tokens\":2,\"cache_read_input_tokens\":8}}}\n",
        "{\"type\":\"assistant\",\"sessionId\":\"s\",\"cwd\":\"/work\",\"message\":{\"id\":\"m\",\"usage\":{\"input_tokens\":3,\"output_tokens\":5,\"cache_creation_input_tokens\":7}}}\n"
    );
    let result = claude_read(Cursor::new(lines), "s", "/work");
    let t = result.tokens.unwrap();
    assert_eq!(
        (t.input, t.output, t.cache_read, t.cache_write),
        (10, 5, 8, 7)
    );
    assert!(!result.truncated);
    assert!(
        claude_read(Cursor::new(lines), "other", "/work")
            .tokens
            .is_none()
    );
    assert!(
        claude_read(Cursor::new(lines), "s", "/other")
            .tokens
            .is_none()
    );
}
fn codex(source: &str, origin: &str) -> String {
    format!(
        "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"s\",\"cwd\":\"/work\",\"source\":\"{source}\",\"thread_source\":\"{origin}\"}}}}\n{{\"type\":\"event_msg\",\"payload\":{{\"type\":\"token_count\",\"info\":{{\"total_token_usage\":{{\"input_tokens\":5,\"output_tokens\":1}}}}}}}}\n{{\"type\":\"event_msg\",\"payload\":{{\"type\":\"token_count\",\"info\":{{\"total_token_usage\":{{\"input_tokens\":9,\"output_tokens\":3,\"cached_input_tokens\":4,\"cache_write_input_tokens\":2,\"reasoning_output_tokens\":99}}}}}}}}\n"
    )
}
#[test]
fn codex_last_cumulative_primary_total_excludes_reasoning_duplication() {
    let result = codex_read(Cursor::new(codex("cli", "user")), "s", "/work");
    let t = result.tokens.unwrap();
    assert_eq!(
        (t.input, t.output, t.cache_read, t.cache_write),
        (9, 3, 4, 2)
    );
    assert!(!result.truncated);
    for (source, origin) in [("cli", "title"), ("exec", "user")] {
        assert!(
            codex_read(Cursor::new(codex(source, origin)), "s", "/work")
                .tokens
                .is_none()
        );
    }
    assert!(
        codex_read(Cursor::new(codex("cli", "user")), "other", "/work")
            .tokens
            .is_none()
    );
    assert!(
        codex_read(Cursor::new(codex("cli", "user")), "s", "/other")
            .tokens
            .is_none()
    );
}
#[test]
fn bounded_records_report_invalid_tail_and_oversized_lines() {
    assert!(records(Cursor::new("{invalid}\n"), |line| {
        serde_json::from_slice::<CodexRecord>(line).is_ok()
    }));
    assert!(records(Cursor::new("{}"), |_| true));
    assert!(records(Cursor::new(vec![b'x'; MAX_LINE + 1]), |_| true));
    assert!(!records(Cursor::new("\n"), |_| true));
}
#[test]
fn claude_project_directory_uses_cli_naming() {
    assert_eq!(
        claude_project_dir(Path::new("/h"), "/home/u/projects/app-agents/x"),
        Path::new("/h/.claude/projects/-home-u-projects-app-agents-x")
    );
}
fn task() -> AgentTask {
    serde_json::from_str(r#"{"id":"a","projectId":"p","title":"x","profile":"codex","baseBranch":"main","branch":null,"worktreePath":null,"workdir":"/work","sessionId":"s","terminalId":null,"state":"preparing","reason":null,"attention":null,"port":20000,"prompt":"","followUps":[],"tokens":null,"prUrl":null,"closedHow":null,"createdMs":0,"updatedMs":0,"closedMs":null}"#).unwrap()
}
#[test]
fn codex_uses_only_exact_binding_and_missing_is_not_truncated() {
    let home = tempfile::tempdir().unwrap();
    let root = home.path().join(".codex/sessions");
    std::fs::create_dir_all(&root).unwrap();
    let exact = root.join("rollout-s.jsonl");
    std::fs::write(&exact, codex("cli", "user")).unwrap();
    std::fs::write(
        root.join("rollout-newer-title.jsonl"),
        codex("cli", "title"),
    )
    .unwrap();
    let task = task();
    let absent = tokens_for(home.path(), &task, Hooks::Codex, None);
    assert!(absent.tokens.is_none());
    assert!(!absent.truncated);
    let mut b = Binding::new(0);
    b.thread_id = Some("s".into());
    b.rollout = Some(exact);
    assert_eq!(
        tokens_for(home.path(), &task, Hooks::Codex, Some(&b))
            .tokens
            .unwrap()
            .input,
        9
    );
    b.rollout = Some(root.join("missing.jsonl"));
    let absent = tokens_for(home.path(), &task, Hooks::Codex, Some(&b));
    assert!(absent.tokens.is_none());
    assert!(!absent.truncated);
}
#[cfg(unix)]
#[test]
fn symlinks_and_special_files_are_rejected_without_reading() {
    let home = tempfile::tempdir().unwrap();
    let dir = claude_project_dir(home.path(), "/work");
    std::fs::create_dir_all(&dir).unwrap();
    std::os::unix::fs::symlink("/dev/zero", dir.join("s.jsonl")).unwrap();
    let report = tokens_for(home.path(), &task(), Hooks::Claude, None);
    assert!(report.tokens.is_none());
    assert!(report.truncated);
}
fn assistant(id: &str, cwd: &str, input: u64) -> String {
    serde_json::json!({"type":"assistant","sessionId":"s","cwd":cwd,"message":{"id":id,"usage":{"input_tokens":input}}}).to_string()+"\n"
}
#[test]
fn unicode_legacy_directory_fallback_is_exact_and_ambiguous_matches_fail() {
    let home = tempfile::tempdir().unwrap();
    let mut task = task();
    task.workdir = "/work/한글".into();
    assert_eq!(
        claude_project_dir(Path::new("/h"), &task.workdir),
        Path::new("/h/.claude/projects/-work---")
    );
    let legacy = home.path().join(".claude/projects/legacy");
    std::fs::create_dir_all(&legacy).unwrap();
    std::fs::write(legacy.join("s.jsonl"), assistant("m", &task.workdir, 7)).unwrap();
    std::fs::write(
        legacy.join("newer.jsonl"),
        assistant("m", &task.workdir, 99),
    )
    .unwrap();
    assert_eq!(
        tokens_for(home.path(), &task, Hooks::Claude, None)
            .tokens
            .unwrap()
            .input,
        7
    );
    let sibling = home.path().join(".claude/projects/sibling");
    std::fs::create_dir_all(&sibling).unwrap();
    std::fs::write(sibling.join("s.jsonl"), assistant("m", "/other", 99)).unwrap();
    let report = tokens_for(home.path(), &task, Hooks::Claude, None);
    assert_eq!(report.tokens.unwrap().input, 7);
    std::fs::write(sibling.join("s.jsonl"), assistant("m", &task.workdir, 99)).unwrap();
    let ambiguous = tokens_for(home.path(), &task, Hooks::Claude, None);
    assert!(ambiguous.tokens.is_none());
    assert!(ambiguous.truncated);
}
#[test]
fn partial_usage_is_retained_and_overflow_is_explicit() {
    let complete = assistant("m", "/work", 8);
    for tail in ["{invalid}\n", "{incomplete"] {
        let report = claude_read(Cursor::new(complete.clone() + tail), "s", "/work");
        assert_eq!(report.tokens.unwrap().input, 8);
        assert!(report.truncated);
    }
    let report = claude_read(
        Cursor::new(assistant("m", "/work", u64::MAX) + &assistant("n", "/work", 1)),
        "s",
        "/work",
    );
    assert_eq!(report.tokens.unwrap().input, u64::MAX);
    assert!(report.truncated);
    let lines = complete + "{\"type\":\"user\",\"message\":{\"content\":\"fixture\"}}\n";
    assert!(!claude_read(Cursor::new(lines), "s", "/work").truncated);
    let lines = codex("cli", "user")
        + "{\"type\":\"event_msg\",\"payload\":{\"type\":\"unrelated\",\"info\":\"fixture\"}}\n";
    assert!(!codex_read(Cursor::new(lines), "s", "/work").truncated);
}
#[test]
fn file_budget_is_bounded_without_allocating_a_full_file() {
    struct Stream {
        remaining: usize,
        chunk: [u8; 8192],
    }
    impl std::io::Read for Stream {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            let count = out.len().min(self.remaining).min(self.chunk.len());
            out[..count].copy_from_slice(&self.chunk[..count]);
            self.remaining -= count;
            Ok(count)
        }
    }
    impl BufRead for Stream {
        fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
            Ok(&self.chunk[..self.remaining.min(self.chunk.len())])
        }
        fn consume(&mut self, count: usize) {
            self.remaining -= count;
        }
    }
    fn stream(bytes: usize) -> Stream {
        let mut chunk = [b' '; 8192];
        chunk[8191] = b'\n';
        Stream {
            remaining: bytes,
            chunk,
        }
    }
    assert!(!records(stream(MAX_BYTES), |_| true));
    assert!(records(stream(MAX_BYTES + 1), |_| true));
}
#[test]
fn unavailable_counters_differ_from_explicitly_measured_zero() {
    for usage in ["{}", "{\"reasoning_output_tokens\":3}"] {
        let lines = format!(
            "{{\"type\":\"assistant\",\"sessionId\":\"s\",\"cwd\":\"/work\",\"message\":{{\"id\":\"m\",\"usage\":{usage}}}}}\n"
        );
        assert!(
            claude_read(Cursor::new(lines), "s", "/work")
                .tokens
                .is_none()
        );
        let lines = codex("cli", "user").lines().next().unwrap().to_owned()
            + &format!(
                "\n{{\"type\":\"event_msg\",\"payload\":{{\"type\":\"token_count\",\"info\":{{\"total_token_usage\":{usage}}}}}}}\n"
            );
        assert!(
            codex_read(Cursor::new(lines), "s", "/work")
                .tokens
                .is_none()
        );
    }
    let lines = assistant("m", "/work", 0);
    let report = claude_read(Cursor::new(lines), "s", "/work");
    assert_eq!(report.tokens.unwrap().input, 0);
    assert!(!report.truncated);
    let lines = codex("cli", "user").lines().next().unwrap().to_owned()
        + "\n{\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"input_tokens\":0,\"output_tokens\":0}}}}\n";
    let report = codex_read(Cursor::new(lines), "s", "/work");
    let zero = report.tokens.unwrap();
    assert_eq!((zero.input, zero.output), (0, 0));
    assert!(!report.truncated);
}
