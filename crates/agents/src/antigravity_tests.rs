//! The payloads and screens are real ones of agy 1.5.17, redacted
//! (`tests/fixtures/antigravity-1.5.17/README.md`).
use super::*;
use crate::state::AgentEvent as E;

/// The fixture's content: what follows its `#` provenance lines and the `---` line.
fn fixture(text: &'static str) -> &'static str {
    text.split_once("\n---\n")
        .expect("fixture header ends with a --- line")
        .1
}
fn session_start() -> &'static str {
    fixture(include_str!(
        "../tests/fixtures/antigravity-1.5.17/hook-session-start.json"
    ))
}
fn pre_invocation() -> &'static str {
    fixture(include_str!(
        "../tests/fixtures/antigravity-1.5.17/hook-pre-invocation.json"
    ))
}
fn stop() -> &'static str {
    fixture(include_str!(
        "../tests/fixtures/antigravity-1.5.17/hook-stop.json"
    ))
}
fn post_invocation() -> &'static str {
    fixture(include_str!(
        "../tests/fixtures/antigravity-1.5.17/hook-post-invocation.json"
    ))
}
fn pre_tool_use() -> &'static str {
    fixture(include_str!(
        "../tests/fixtures/antigravity-1.5.17/hook-pre-tool-use.json"
    ))
}
const CONVERSATION: &str = "00000000-0000-4000-8000-000000000001";
const TRANSCRIPT: &str = "/owned/home/.gemini/antigravity-cli/brain/00000000-0000-4000-8000-000000000001/.system_generated/logs/transcript_full.jsonl";

fn started() -> AgentEvent {
    E::SessionStarted {
        session_id: CONVERSATION.into(),
    }
}

#[test]
fn hooks_dir_is_the_work_folders_sibling_and_sorts_after_it() {
    let dir = hooks_dir("/home/u/projects/devbox-agents/fix-login");
    assert_eq!(
        dir,
        Path::new("/home/u/projects/devbox-agents/fix-login.agy-hooks")
    );
    assert_eq!(
        hooks_dir("/w/x y/"),
        Path::new("/w/x y.agy-hooks"),
        "never inside the work folder"
    );
    // agy runs commands in the first of its folders: the work folder's name is a prefix of the
    // hooks folder's, so it sorts first.
    for workdir in ["/w/fix", "/w/fix-2", "/w/Fix.A", "/w/한글"] {
        let hooks = hooks_dir(workdir);
        assert!(workdir < hooks.to_str().expect("UTF-8"), "{workdir}");
        assert_eq!(hooks.parent(), Path::new(workdir).parent(), "{workdir}");
        assert!(!hooks.starts_with(workdir), "{workdir}");
    }
}

#[test]
fn hooks_json_registers_exactly_three_events_with_the_report_command() {
    let hooks: serde_json::Value =
        serde_json::from_str(&hooks_json("/b/it's devbox")).expect("JSON");
    let groups = hooks.as_object().expect("groups");
    assert_eq!(groups.keys().collect::<Vec<_>>(), ["devbox"]);
    let events = groups["devbox"].as_object().expect("events");
    let mut names: Vec<_> = events.keys().map(String::as_str).collect();
    names.sort_unstable();
    // Never PreToolUse or PostToolUse: a PreToolUse hook that prints `{}` denies the tool call.
    assert_eq!(names, ["PreInvocation", "SessionStart", "Stop"]);
    for (name, entries) in events {
        assert_eq!(
            entries,
            &serde_json::json!([{
                "type": "command",
                "command": "'/b/it'\\''s devbox' agent event --from antigravity",
                "timeout": 10,
            }]),
            "{name}"
        );
    }
    assert!(!hooks_json("/b/devbox").contains("ToolUse"));
}

#[cfg(unix)]
#[test]
fn the_hook_command_reaches_devbox_as_one_word_whatever_its_path_holds() {
    use std::{fs, os::unix::fs::PermissionsExt, process::Command};
    let root = tempfile::tempdir().expect("owned folder");
    // A devbox whose path a shell would split, expand or end a quote on.
    let dir = root.path().join("it's a \"dev box\" $HOME `id`");
    fs::create_dir(&dir).expect("owned folder");
    let devbox = dir.join("dev box");
    let receipt = root.path().join("argv");
    fs::write(
        &devbox,
        "#!/bin/sh\nprintf '%s\\n' \"$0\" \"$@\" > \"$DEVBOX_HOOK_RECEIPT\"\necho '{}'\n",
    )
    .expect("owned devbox");
    fs::set_permissions(&devbox, fs::Permissions::from_mode(0o700)).expect("mode");
    let devbox = devbox.to_str().expect("UTF-8");
    let hooks: serde_json::Value = serde_json::from_str(&hooks_json(devbox)).expect("JSON");
    for event in ["SessionStart", "PreInvocation", "Stop"] {
        let command = hooks["devbox"][event][0]["command"]
            .as_str()
            .expect("command");
        // As agy runs it: the command string through a shell.
        let output = Command::new("/bin/sh")
            .args(["-c", command])
            .current_dir(root.path())
            .env("DEVBOX_HOOK_RECEIPT", &receipt)
            .output()
            .expect("owned hook");
        assert!(output.status.success(), "{event}");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "{}\n", "{event}");
        assert_eq!(
            fs::read_to_string(&receipt).expect("receipt"),
            format!("{devbox}\nagent\nevent\n--from\nantigravity\n"),
            "{event}"
        );
    }
}

#[test]
fn real_payloads_map_to_session_running_and_turn_end() {
    assert_eq!(events(session_start()), [started()]);
    assert_eq!(events(pre_invocation()), [started(), E::PromptSubmitted]);
    assert_eq!(
        events(stop()),
        [started(), E::TurnComplete { message: None }]
    );
    for payload in [session_start(), pre_invocation(), stop()] {
        let report = report(payload).expect("report");
        assert_eq!(report.conversation, CONVERSATION);
        assert_eq!(report.transcript.as_deref(), Some(TRANSCRIPT));
    }
    assert_eq!(report(stop()).expect("stop").event, HookEvent::Stop);
    // The same through the hook parser the service calls.
    assert_eq!(
        crate::hooks::parse(crate::hooks::HookSource::Antigravity, stop(), None),
        events(stop())
    );
    // Another family's parser reads nothing from them.
    assert!(crate::hooks::parse(crate::hooks::HookSource::Claude, stop(), None).is_empty());
}

#[test]
fn other_events_and_malformed_payloads_mean_nothing() {
    // Events devbox does not register, should agy send them anyway, and ones it does not know.
    for payload in [post_invocation(), pre_tool_use()] {
        assert!(report(payload).is_none());
        assert!(events(payload).is_empty());
    }
    let unknown = stop().replace("\"Stop\"", "\"SessionEnd\"");
    assert_ne!(unknown, stop());
    assert!(events(&unknown).is_empty());
    for payload in [
        "",
        "not json",
        "[]",
        "{}",
        r#"{"hookEventName":"Stop"}"#,
        r#"{"hookEventName":"Stop","conversationId":""}"#,
        r#"{"hookEventName":"Stop","conversationId":7}"#,
        r#"{"hookEventName":7,"conversationId":"c"}"#,
        r#"{"hook_event_name":"Stop","session_id":"c"}"#,
        r#"{"conversationId":"c"}"#,
    ] {
        assert!(events(payload).is_empty(), "{payload}");
    }
    // The rest of the payload is not read, whatever it holds.
    let odd = r#"{"lastUserInput":{"x":1},"cwd":7,"workspacePaths":"x","hookEventName":"PreInvocation","conversationId":"c","toolCall":[1]}"#;
    assert_eq!(
        events(odd),
        [
            E::SessionStarted {
                session_id: "c".into()
            },
            E::PromptSubmitted
        ]
    );
    assert_eq!(report(odd).expect("report").transcript, None);
}

#[cfg(unix)]
#[test]
fn only_a_transcript_below_the_users_agy_data_is_taken() {
    let home = Path::new("/owned/home");
    assert_eq!(
        transcript_path(home, TRANSCRIPT),
        Some(PathBuf::from(TRANSCRIPT))
    );
    let long = format!(
        "/owned/home/.gemini/antigravity-cli/{}/t.jsonl",
        "x".repeat(MAX_PATH_BYTES)
    );
    for named in [
        "",
        "transcript_full.jsonl",
        "brain/x/transcript_full.jsonl",
        "~/.gemini/antigravity-cli/brain/x/t.jsonl",
        "/etc/passwd",
        "/owned/home/.ssh/id_ed25519",
        "/owned/home/.gemini/antigravity/brain/x/t.jsonl",
        "/owned/home/.gemini/antigravity-cli",
        "/owned/home/.gemini/antigravity-cli/",
        "/owned/home/.gemini/antigravity-cli-other/t.jsonl",
        "/owned/home/.gemini/antigravity-cli/../../.ssh/id_ed25519",
        "/owned/home/.gemini/antigravity-cli/brain/../../../.ssh/id_ed25519",
        "/owned/home/.gemini/antigravity-cli/./brain/../x",
        "/other/home/.gemini/antigravity-cli/brain/x/t.jsonl",
        long.as_str(),
    ] {
        assert_eq!(transcript_path(home, named), None, "{named}");
    }
}

fn screen(fixture: &str) -> &str {
    fixture
        .split_once("\n---\n")
        .expect("fixture header ends with a --- line")
        .1
}

#[test]
fn an_empty_prompt_with_nothing_running_shows_the_prompt() {
    for (name, idle) in [
        (
            "idle",
            include_str!("../tests/fixtures/antigravity-1.5.17/idle.txt"),
        ),
        (
            "idle-default-statusline",
            include_str!("../tests/fixtures/antigravity-1.5.17/idle-default-statusline.txt"),
        ),
        (
            "idle-resumed",
            include_str!("../tests/fixtures/antigravity-1.5.17/idle-resumed.txt"),
        ),
        (
            "idle-interrupted",
            include_str!("../tests/fixtures/antigravity-1.5.17/idle-interrupted.txt"),
        ),
        (
            "idle-after-trust",
            include_str!("../tests/fixtures/antigravity-1.5.17/idle-after-trust.txt"),
        ),
    ] {
        assert!(shows_prompt(screen(idle)), "{name}");
    }
    for (name, busy) in [
        (
            "working",
            include_str!("../tests/fixtures/antigravity-1.5.17/working.txt"),
        ),
        (
            "working-tip",
            include_str!("../tests/fixtures/antigravity-1.5.17/working-tip.txt"),
        ),
        (
            "working-statusline",
            include_str!("../tests/fixtures/antigravity-1.5.17/working-statusline.txt"),
        ),
        (
            "queued",
            include_str!("../tests/fixtures/antigravity-1.5.17/queued.txt"),
        ),
        (
            "trust",
            include_str!("../tests/fixtures/antigravity-1.5.17/trust.txt"),
        ),
        (
            "approval",
            include_str!("../tests/fixtures/antigravity-1.5.17/approval.txt"),
        ),
        (
            "shell-mode",
            include_str!("../tests/fixtures/antigravity-1.5.17/shell-mode.txt"),
        ),
        (
            "slash-draft",
            include_str!("../tests/fixtures/antigravity-1.5.17/slash-draft.txt"),
        ),
        (
            "draft",
            include_str!("../tests/fixtures/antigravity-1.5.17/draft.txt"),
        ),
        (
            "login",
            include_str!("../tests/fixtures/antigravity-1.5.17/login.txt"),
        ),
    ] {
        assert!(!shows_prompt(screen(busy)), "{name}");
    }
    assert!(!shows_prompt(""));
    // A turn in progress is told by its spinner row alone, wherever the box is.
    for (name, shown, turn) in [
        (
            "working",
            include_str!("../tests/fixtures/antigravity-1.5.17/working.txt"),
            true,
        ),
        (
            "working-tip",
            include_str!("../tests/fixtures/antigravity-1.5.17/working-tip.txt"),
            true,
        ),
        (
            "queued",
            include_str!("../tests/fixtures/antigravity-1.5.17/queued.txt"),
            true,
        ),
        (
            "idle",
            include_str!("../tests/fixtures/antigravity-1.5.17/idle.txt"),
            false,
        ),
        (
            "trust",
            include_str!("../tests/fixtures/antigravity-1.5.17/trust.txt"),
            false,
        ),
        (
            "approval",
            include_str!("../tests/fixtures/antigravity-1.5.17/approval.txt"),
            false,
        ),
        (
            "draft",
            include_str!("../tests/fixtures/antigravity-1.5.17/draft.txt"),
            false,
        ),
    ] {
        assert_eq!(shows_turn(screen(shown)), turn, "{name}");
    }
    assert!(!shows_turn("⣽ one blank\n⣽text\n  ⣽  indented\n"));
    // The box is agy's own: a full rule above and below a row that is `>` alone or agy's hint.
    let rule = "─".repeat(60);
    let boxed = |row: &str| format!("  OK\n{rule}\n{row}\n{rule}\nstatus\n");
    assert!(shows_prompt(&boxed(">")));
    assert!(shows_prompt(&boxed("> ")));
    assert!(shows_prompt(&boxed(
        "> Auto mode: edits & safe commands auto-approved (shift+tab to cycle)"
    )));
    for row in [
        "> fix it",
        "> Auto mode: is what I asked about",
        "!",
        "  >",
        "",
    ] {
        assert!(!shows_prompt(&boxed(row)), "{row:?}");
    }
    // NOT real captures: agy's hint for another permission mode than the recorded `Auto mode`.
    // Only the hint's words differ; it is taken by its ending.
    for hint in [
        "> Plan mode: read-only (shift+tab to cycle)",
        "> Ask mode: every command needs approval (shift+tab to cycle)",
        "> (shift+tab to cycle)",
    ] {
        assert!(shows_prompt(&boxed(hint)), "{hint}");
    }
    let recorded = screen(include_str!(
        "../tests/fixtures/antigravity-1.5.17/idle-default-statusline.txt"
    ));
    let other_mode = recorded.replacen(
        "> Auto mode: edits & safe commands auto-approved (shift+tab to cycle)",
        "> Plan mode: edits & safe commands auto-approved (shift+tab to cycle)",
        1,
    );
    assert_ne!(other_mode, recorded);
    assert!(shows_prompt(&other_mode));
    // The hint without its ending is a draft the user typed.
    assert!(!shows_prompt(&boxed("> Plan mode: read-only")));
    assert!(!shows_prompt(&boxed(">Plan mode (shift+tab to cycle)")));
    // A short rule is a row of the answer, and a rule on one side only is not a box.
    let short = "─".repeat(19);
    assert!(!shows_prompt(&format!("{short}\n>\n{short}\nstatus\n")));
    assert!(!shows_prompt(&format!("{rule}\n>\nstatus\n")));
    assert!(!shows_prompt(&format!(">\n{rule}\nstatus\n")));
    // A turn's spinner row above the box, with a tip and a queued prompt between them.
    assert!(!shows_prompt(&format!(
        "⣽  Generating...\n└ Tip: x\n▸ next (alt+m to send immediately)\n{rule}\n>\n{rule}\nstatus\n"
    )));
    // The box above more than three rows is no longer among the rows read.
    assert!(!shows_prompt(&format!("{rule}\n>\n{rule}\n1\n2\n3\n4\n")));
    // The pane after agy ended: the start script's last rows, no prompt box.
    assert!(!shows_prompt(
        "Resume with -c (or command below):\nagy --conversation=x\n[devbox] 에이전트가 끝났습니다(종료 코드 0).\n"
    ));
}

macro_rules! shown {
    ($name:literal) => {
        (
            $name,
            screen(include_str!(concat!(
                "../tests/fixtures/antigravity-1.5.17/",
                $name,
                ".txt"
            ))),
        )
    };
}

#[test]
fn a_prompt_already_given_in_this_pane_is_told_by_its_echo_above_the_box() {
    // agy echoes a prompt it was given under a short rule, and leaves it above the prompt box.
    for (name, after_turn) in [
        shown!("idle"),
        shown!("idle-after-user-turn"),
        shown!("idle-interrupted"),
        shown!("idle-after-decline"),
        shown!("idle-after-approve"),
        shown!("working"),
        shown!("working-tip"),
        shown!("queued"),
        // A resumed conversation shows its earlier turns the same way. A resumed launch never
        // has a queued first instruction, so that is no concern.
        shown!("idle-resumed"),
    ] {
        assert!(shows_earlier_prompt(after_turn), "{name}");
    }
    // Before a launch's first prompt there is none: straight after start or after the trust
    // question, after a panel was opened and closed (its command row has no rule above it),
    // after a draft was typed and cleared, with a draft in the box, or at a prompt.
    for (name, fresh) in [
        shown!("idle-after-trust"),
        shown!("idle-default-statusline"),
        shown!("idle-after-panel"),
        shown!("idle-after-clearing-draft"),
        shown!("draft"),
        shown!("shell-mode"),
        shown!("shell-mode-draft"),
        shown!("slash-draft"),
        shown!("slash-menu"),
        shown!("selection-hooks"),
        shown!("trust"),
        shown!("trust-default-statusline"),
        shown!("login"),
    ] {
        assert!(!shows_earlier_prompt(fresh), "{name}");
    }
    assert!(!shows_earlier_prompt(""));
    // The box's own rules never count, whatever its input row holds.
    let rule = "─".repeat(60);
    for input in [
        ">",
        "> draft",
        "> Auto mode: x (shift+tab to cycle)",
        "!",
        "! ls",
    ] {
        assert!(
            !shows_earlier_prompt(&format!("banner\n{rule}\n{input}\n{rule}\nstatus\n")),
            "{input}"
        );
    }
    assert!(shows_earlier_prompt(&format!(
        "{rule}\n> fix it\n  done\n{rule}\n>\n{rule}\nstatus\n"
    )));
    // NOT a real capture: what a pane resized before the first prompt could leave, a piece of
    // the older box with its hint above the box drawn anew. A hint row is never an echo.
    let hint = "> Auto mode: x (shift+tab to cycle)";
    let remnant = format!("banner\n{rule}\n{hint}\n{rule}\n{hint}\n{rule}\nstatus\n");
    assert!(!shows_earlier_prompt(&remnant));
    assert!(shows_prompt(&remnant));
    // NOT a real capture: a rule over something other than a prompt (a table in an answer).
    assert!(!shows_earlier_prompt(&format!(
        "{rule}\n  a table row\n{rule}\n>\n{rule}\nstatus\n"
    )));
}

#[test]
fn only_the_trust_and_sign_in_screens_come_before_any_prompt() {
    for (name, before) in [
        shown!("trust"),
        shown!("trust-default-statusline"),
        shown!("login"),
    ] {
        assert!(shows_before_first_prompt(before), "{name}");
    }
    // Any other screen may have had a prompt typed into it already: the prompt box, a turn, a
    // menu or an approval, a draft.
    for (name, later) in [
        shown!("idle"),
        shown!("idle-after-trust"),
        shown!("idle-default-statusline"),
        shown!("idle-after-user-turn"),
        shown!("idle-after-panel"),
        shown!("working"),
        shown!("queued"),
        shown!("approval"),
        shown!("approval-six-options"),
        shown!("selection-model"),
        shown!("selection-hooks"),
        shown!("slash-menu"),
        shown!("slash-draft"),
        shown!("shell-mode"),
        shown!("draft"),
    ] {
        assert!(!shows_before_first_prompt(later), "{name}");
    }
    assert!(!shows_before_first_prompt(""));
    // NOT real captures: the trust choices quoted in an answer above the prompt box.
    let (_, idle) = shown!("idle");
    let quoted = idle.replacen(
        "  OK. The prompt had 3 lines.",
        "> Yes, I trust this folder\n  No, exit",
        1,
    );
    assert_ne!(quoted, idle);
    assert!(!shows_before_first_prompt(&quoted));
    // NOT real captures: one of the two choices alone is not the question.
    let (_, trust) = shown!("trust");
    for choice in ["Yes, I trust this folder", "No, exit"] {
        let one = trust.replacen(choice, "something else", 1);
        assert_ne!(one, trust);
        assert!(!shows_before_first_prompt(&one), "{choice}");
    }
}

#[cfg(unix)]
#[path = "antigravity_folder_tests.rs"]
mod folder;
