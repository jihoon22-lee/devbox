//! pi usage from the recorded session file and from owned session folders.
use super::*;
use crate::model::Tokens;
use std::{fs, time::Duration};

const SESSION: &str = include_str!("../tests/fixtures/pi-1.3.0/session.jsonl");
const ID: &str = "00000000-0000-4000-8000-000000000001";
const CWD: &str = "/tmp/devbox-pi-fixture/repo";
type Read = (Option<Tokens>, bool);

fn later() -> Instant {
    Instant::now() + Duration::from_secs(60)
}
/// `pi_read` as (tokens, truncated).
fn read(body: &str, session: &str, cwd: &str) -> Read {
    let usage = pi_read(body.as_bytes(), session, cwd, later());
    (usage.tokens, usage.truncated)
}
fn tokens(input: u64, output: u64, cache_read: u64, cache_write: u64) -> Option<Tokens> {
    Some(Tokens {
        input,
        output,
        cache_read,
        cache_write,
    })
}

#[test]
fn pi_session_fixture_sums_usage() {
    // The answer of run 1 and the one of its resume (run 2). pi counts reasoning in the output
    // already, and the cost is not summed.
    assert_eq!(
        read(SESSION, ID, CWD),
        (tokens(890 + 980, 8 + 9, 210 + 220, 0), false)
    );
    // Another session's file, or one made in another folder, is not this task's.
    for (session, cwd) in [
        ("00000000-0000-4000-8000-000000000003", CWD),
        (ID, "/elsewhere"),
    ] {
        assert_eq!(read(SESSION, session, cwd), (None, true), "{session} {cwd}");
    }
    // A file still being written is read up to its last whole line, and says so.
    let cut = &SESSION[..SESSION.len() - 20];
    assert_eq!(read(cut, ID, CWD), (tokens(890, 8, 210, 0), true));
    // A file that does not start with this session's header is never read as its usage.
    for broken in ["", "not json\n", "{\"type\":\"message\"}\n", "[]\n"] {
        assert_eq!(read(broken, ID, CWD), (None, true), "{broken:?}");
    }
}

/// Assistant usage is recorded (the fixture). Tool-result usage and the `usage`, `compaction` and
/// `branch_summary` entries are 타입 기반: built from pi 1.3.0's session-format.md and
/// `getSessionStats`, not recorded.
#[test]
fn pi_counts_what_pi_bills_once_and_reports_what_it_cannot_read() {
    let usage = |input: u64, output: u64, read: u64, write: u64| {
        format!(
            r#"{{"input":{input},"output":{output},"cacheRead":{read},"cacheWrite":{write},"reasoning":99,"totalTokens":1,"cost":{{"total":5}}}}"#
        )
    };
    let lines = [
        r#"{"type":"session","version":3,"id":"s","timestamp":"t","cwd":"/w"}"#.to_owned(),
        // The user's prompt and an extension's own entry are not billed.
        r#"{"type":"message","message":{"role":"user","content":"hi"}}"#.to_owned(),
        r#"{"type":"custom","customType":"x","data":{"usage":{"input":7}}}"#.to_owned(),
        format!(
            r#"{{"type":"message","message":{{"role":"assistant","usage":{}}}}}"#,
            usage(1, 2, 3, 4)
        ),
        format!(
            r#"{{"type":"message","message":{{"role":"toolResult","usage":{}}}}}"#,
            usage(10, 20, 30, 40)
        ),
        r#"{"type":"message","message":{"role":"toolResult"}}"#.to_owned(),
        format!(
            r#"{{"type":"usage","kind":"cache_warm","usage":{}}}"#,
            usage(100, 200, 300, 400)
        ),
        format!(
            r#"{{"type":"compaction","summary":"s","usage":{}}}"#,
            usage(1000, 2000, 3000, 4000)
        ),
        r#"{"type":"compaction","summary":"s"}"#.to_owned(),
        format!(
            r#"{{"type":"branch_summary","summary":"s","usage":{}}}"#,
            usage(10000, 20000, 30000, 40000)
        ),
    ];
    let body = lines.join("\n") + "\n";
    assert_eq!(
        read(&body, "s", "/w"),
        (tokens(11111, 22222, 33333, 44444), false)
    );
    // A session that has not answered yet has no usage, which is complete.
    assert_eq!(read(&format!("{}\n", lines[0]), "s", "/w"), (None, false));
    // An entry pi bills that cannot be read is reported; the rest still counts.
    for bad in [
        r#"{"type":"message","message":{"role":"assistant"}}"#,
        r#"{"type":"message","message":{"role":"assistant","usage":{"input":1}}}"#,
        r#"{"type":"usage","kind":"cache_warm"}"#,
        r#"{"type":"usage","usage":{"input":-1,"output":0,"cacheRead":0,"cacheWrite":0}}"#,
        r#"{"type":"message"}"#,
        "not json",
    ] {
        let body = format!("{}\n{}\n{bad}\n", lines[0], lines[3]);
        assert_eq!(read(&body, "s", "/w"), (tokens(1, 2, 3, 4), true), "{bad}");
    }
    // Sums saturate instead of wrapping.
    let huge = r#"{"type":"usage","usage":{"input":18446744073709551615,"output":0,"cacheRead":0,"cacheWrite":0}}"#;
    let (summed, truncated) = read(&format!("{}\n{huge}\n{huge}\n", lines[0]), "s", "/w");
    assert!(truncated);
    assert_eq!(summed.expect("tokens").input, u64::MAX);
}

#[test]
fn pi_session_dir_follows_the_variables_pi_reads() {
    let dir = |agent: Option<&str>, sessions: Option<&str>| {
        pi_session_dir(
            Path::new("/home/u"),
            agent.map(OsStr::new),
            sessions.map(OsStr::new),
            "/w/x y:z",
        )
    };
    let project = "--w-x y-z--";
    assert_eq!(
        dir(None, None),
        Path::new("/home/u/.pi/agent/sessions").join(project)
    );
    // pi reads an empty variable as unset.
    assert_eq!(dir(Some(""), Some("")), dir(None, None));
    assert_eq!(
        dir(Some("/a"), None),
        Path::new("/a/sessions").join(project)
    );
    assert_eq!(
        dir(Some("~"), None),
        Path::new("/home/u/sessions").join(project)
    );
    assert_eq!(
        dir(Some("~/pi"), None),
        Path::new("/home/u/pi/sessions").join(project)
    );
    // A relative folder is one in pi's working folder, the task's.
    assert_eq!(
        dir(Some("rel"), None),
        Path::new("/w/x y:z/rel/sessions").join(project)
    );
    // A session folder of its own holds every project's sessions, side by side.
    assert_eq!(dir(Some("/a"), Some("~/s")), Path::new("/home/u/s"));
    assert_eq!(dir(None, Some("/s")), Path::new("/s"));
}

#[test]
fn only_ids_pi_makes_are_looked_up() {
    for id in [ID, "019a2b3c-7d8e-7f00-8000-000000000005", "a", "x.y_z-1"] {
        assert!(pi_session_id(id), "{id}");
    }
    let long = "a".repeat(129);
    for id in [
        "", "-h", ".x", "x.", "_x", "a/b", "../x", "a b", "x\n", &long,
    ] {
        assert!(!pi_session_id(id), "{id:?}");
    }
}

#[cfg(unix)]
#[test]
fn only_the_task_session_file_in_its_folder_is_read() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().expect("owned sessions");
    let dir = root.path().join("sessions");
    fs::create_dir(&dir).expect("owned folder");
    let file = |name: String, body: &str| fs::write(dir.join(name), body).expect("owned file");
    let usage = |dir: &Path, session: &str, until: Instant| {
        let usage = usage_in(dir, session, CWD, until);
        (usage.tokens, usage.truncated)
    };
    let summed = tokens(1870, 17, 430, 0);
    // No folder or no file for the session yet: nothing to read, which is complete.
    assert_eq!(
        usage(&root.path().join("missing"), ID, later()),
        (None, false)
    );
    assert_eq!(usage(&dir, ID, later()), (None, false));
    file(format!("2026-10-28T05-08-56-172Z_{ID}.jsonl"), SESSION);
    assert_eq!(usage(&dir, ID, later()), (summed.clone(), false));
    // Files of other sessions are never opened, even one whose name ends in the id.
    file(
        format!("2026-10-28T05-09-00-000Z_x_{ID}.jsonl"),
        "unreadable",
    );
    file("2026-10-28T05-09-00-000Z_other.jsonl".into(), "unreadable");
    file(format!("{ID}.jsonl"), "unreadable");
    assert_eq!(usage(&dir, ID, later()), (summed.clone(), false));
    // Two files naming the session make it ambiguous.
    let second = format!("2026-10-28T06-00-00-000Z_{ID}.jsonl");
    file(second.clone(), SESSION);
    assert_eq!(usage(&dir, ID, later()), (None, true));
    fs::remove_file(dir.join(second)).expect("owned file");
    // A link is never followed, whether it is the file or the folder.
    let other = "00000000-0000-4000-8000-000000000003";
    let target = root.path().join("target.jsonl");
    fs::write(&target, SESSION.replace(ID, other)).expect("owned file");
    symlink(
        &target,
        dir.join(format!("2026-10-28T07-00-00-000Z_{other}.jsonl")),
    )
    .expect("owned link");
    assert_eq!(usage(&dir, other, later()), (None, true));
    let linked = root.path().join("linked");
    symlink(&dir, &linked).expect("owned link");
    assert_eq!(usage(&linked, ID, later()), (None, true));
    // The read gives up at its deadline, in the folder and in the file.
    assert_eq!(usage(&dir, ID, Instant::now()), (None, true));
    let late = pi_read(SESSION.as_bytes(), ID, CWD, Instant::now());
    assert_eq!((late.tokens, late.truncated), (None, true));
}

#[cfg(unix)]
#[test]
fn a_session_pi_has_not_written_is_told_apart_from_one_that_cannot_be_read() {
    use std::os::unix::fs::symlink;
    let home = tempfile::tempdir().expect("owned home");
    let mut task: AgentTask = serde_json::from_value(serde_json::json!({
        "id": "a", "projectId": "p", "title": "t", "profile": "pi", "baseBranch": "main",
        "branch": null, "worktreePath": null, "workdir": CWD, "sessionId": ID,
        "terminalId": null, "state": "review", "reason": null, "attention": null, "port": 1,
        "prompt": "", "followUps": [], "tokens": null, "prUrl": null, "closedHow": null,
        "createdMs": 1, "updatedMs": 1, "closedMs": null,
    }))
    .expect("owned task");
    let written = |task: &AgentTask| pi_session_written(home.path(), None, None, task);
    // No folder for the project yet, then a folder without the session's file.
    assert_eq!(written(&task), Some(false));
    let dir = pi_session_dir(home.path(), None, None, CWD);
    fs::create_dir_all(&dir).expect("owned sessions");
    fs::write(dir.join("2026-10-28T05-09-00-000Z_other.jsonl"), "x").expect("owned file");
    assert_eq!(written(&task), Some(false));
    fs::write(
        dir.join(format!("2026-10-28T05-08-56-172Z_{ID}.jsonl")),
        SESSION,
    )
    .expect("owned");
    assert_eq!(written(&task), Some(true));
    // When the folder cannot be read safely, nobody can tell.
    fs::write(
        dir.join(format!("2026-10-28T06-00-00-000Z_{ID}.jsonl")),
        SESSION,
    )
    .expect("owned");
    assert_eq!(written(&task), None);
    let moved = home.path().join("moved");
    fs::rename(&dir, &moved).expect("owned folder");
    symlink(&moved, &dir).expect("owned link");
    assert_eq!(written(&task), None);
    // A task without a session pi accepts has nothing pi could have written.
    task.session_id = Some("../x".into());
    assert_eq!(written(&task), None);
}
