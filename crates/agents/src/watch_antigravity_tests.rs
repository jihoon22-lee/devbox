//! Antigravity's screens: every pattern on a real capture of agy 1.5.17, redacted
//! (`tests/fixtures/antigravity-1.5.17/README.md`), and what the output watcher makes of them.
use super::*;
use devbox_protocol::Topic;

macro_rules! capture {
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

/// The screens where a send must wait: a paste is dropped and Enter answers, or the paste becomes
/// a shell or slash command.
fn waiting() -> Vec<(&'static str, &'static str)> {
    vec![
        capture!("trust"),
        capture!("trust-default-statusline"),
        capture!("approval"),
        capture!("approval-six-options"),
        capture!("selection-model"),
        capture!("selection-hooks"),
        capture!("slash-menu"),
        capture!("slash-draft"),
        capture!("shell-mode"),
        capture!("shell-mode-draft"),
        capture!("queued"),
        capture!("login"),
    ]
}

/// The screens where a send goes to the agent as a prompt.
fn open() -> Vec<(&'static str, &'static str)> {
    vec![
        capture!("idle"),
        capture!("idle-default-statusline"),
        capture!("idle-resumed"),
        capture!("idle-interrupted"),
        // A declined and an approved tool call: agy removes the approval's rows once answered.
        capture!("idle-after-decline"),
        capture!("idle-after-approve"),
        capture!("idle-after-trust"),
        capture!("working"),
        capture!("working-tip"),
        capture!("working-statusline"),
        capture!("draft"),
    ]
}

fn patterns() -> WaitingPatterns {
    builtin_patterns("antigravity")
}

/// A set with only the pattern `index` of the family, to show which screen each one reads.
fn only(index: usize) -> WaitingPatterns {
    let all = patterns();
    WaitingPatterns {
        tail: vec![all.tail[index].clone()],
        screen: Vec::new(),
    }
}

#[test]
fn every_antigravity_prompt_is_waiting_and_no_idle_or_working_screen_is() {
    let patterns = patterns();
    // All of the family's patterns read the last rows: agy draws its prompts at the bottom.
    assert_eq!(patterns.tail.len(), 6);
    assert!(patterns.screen.is_empty());
    for (name, shown) in waiting() {
        assert!(matches_waiting(&patterns, shown), "{name}");
    }
    for (name, shown) in open() {
        assert!(!matches_waiting(&patterns, shown), "{name}");
    }
}

#[test]
fn each_antigravity_pattern_reads_its_own_screens_only() {
    // (pattern, the captures it matches). Every other capture, waiting or not, must not match.
    let cases: [(usize, &[&str]); 6] = [
        (0, &["trust", "trust-default-statusline"]),
        // "1. Google OAuth" of the sign-in choice has no "Yes," or "No,": only the list's hint
        // row marks it.
        (1, &["approval", "approval-six-options"]),
        (
            2,
            &[
                "trust",
                "trust-default-statusline",
                "approval",
                "approval-six-options",
                "selection-model",
                "selection-hooks",
                "slash-menu",
                "login",
            ],
        ),
        (3, &["shell-mode", "shell-mode-draft"]),
        (4, &["slash-draft"]),
        (5, &["queued"]),
    ];
    let all: Vec<_> = waiting().into_iter().chain(open()).collect();
    for (index, matching) in cases {
        let pattern = only(index);
        for (name, shown) in &all {
            assert_eq!(
                matches_waiting(&pattern, shown),
                matching.contains(name),
                "pattern {index} on {name}"
            );
        }
    }
}

#[test]
fn an_antigravity_prompt_is_waiting_whatever_its_cursor_and_status_rows() {
    let patterns = patterns();
    let (_, trust) = capture!("trust");
    // The cursor on the other choice.
    let moved = trust
        .replacen(
            "> Yes, I trust this folder",
            "  Yes, I trust this folder",
            1,
        )
        .replacen("  No, exit", "> No, exit", 1);
    assert_ne!(moved, trust);
    assert!(matches_waiting(&patterns, &moved));
    let (_, approval) = capture!("approval-six-options");
    let moved = approval
        .replacen("> 1. Yes, run command", "  1. Yes, run command", 1)
        .replacen("  4. No, cancel", "> 4. No, cancel", 1);
    assert_ne!(moved, approval);
    assert!(matches_waiting(&patterns, &moved));
    // A status line of two rows instead of one still leaves each prompt among the rows read.
    for (name, shown) in waiting() {
        let rows: Vec<&str> = shown.lines().collect();
        let last = rows
            .iter()
            .rposition(|row| !row.trim().is_empty())
            .expect("status row");
        let mut taller = rows[..=last].to_vec();
        taller.push("status two");
        assert!(matches_waiting(&patterns, &taller.join("\n")), "{name}");
    }
    // The queued prompt sits highest, on the fifth row from the bottom: a third status row
    // pushes it out of the rows read, and the others still match.
    for (name, shown) in waiting() {
        let rows: Vec<&str> = shown.lines().collect();
        let last = rows
            .iter()
            .rposition(|row| !row.trim().is_empty())
            .expect("status row");
        let mut taller = rows[..=last].to_vec();
        taller.extend(["status two", "status three"]);
        assert_eq!(
            matches_waiting(&patterns, &taller.join("\n")),
            name != "queued",
            "{name}"
        );
    }
    // Newer output below a prompt means it is no longer the current screen.
    assert!(!matches_waiting(
        &patterns,
        &format!("{trust}\nworking\n1\n2\n3\n4\n5\n6")
    ));
}

#[test]
fn antigravity_output_that_mentions_a_prompt_is_not_waiting() {
    let patterns = patterns();
    let (_, idle) = capture!("idle");
    // Sentences an answer may hold, indented as agy draws an answer.
    for row in [
        "  Do you trust the contents of this project?",
        "  The screen offers Yes, I trust this folder and No, exit.",
        "  Requesting permission for:",
        "  Run this command?",
        "  1. Yes: run the command",
        "  ! is how shell mode starts, and /help lists commands.",
        "  (alt+m to send immediately)",
    ] {
        let shown = idle.replacen("  OK. The prompt had 3 lines.", row, 1);
        assert_ne!(shown, idle, "{row}");
        assert!(!matches_waiting(&patterns, &shown), "{row}");
    }
    // A shell command and a slash command that ran are in the history, not in the prompt box.
    for row in ["! ls", "> /hooks"] {
        let shown = idle.replacen("  OK. The prompt had 3 lines.", row, 1);
        assert!(!matches_waiting(&patterns, &shown), "{row}");
    }
    // NOT real captures: rows drawn exactly like a prompt's own, as an answer that quotes them
    // or a list of its own, above the idle prompt box. A prompt is drawn in the box's place, so
    // with the box below them they are not one, and a send goes through.
    for rows in [
        "  ↑/↓ Navigate · enter Confirm",
        "  1. Yes, run command\n  2. No, cancel",
        "> 1. Yes, run command\n  2. Yes, and always allow\n  4. No, cancel\n  ↑/↓ Navigate · tab Amend",
        "> Yes, I trust this folder\n  No, exit\n  ↑/↓ Navigate · enter Confirm",
    ] {
        let quoted = idle.replacen("  OK. The prompt had 3 lines.", rows, 1);
        assert_ne!(quoted, idle, "{rows}");
        assert!(!matches_waiting(&patterns, &quoted), "{rows}");
    }
    // The same rows with no box below them are the prompt itself.
    for rows in [
        "> 1. Yes, run command\n  4. No, cancel\n  ↑/↓ Navigate · tab Amend\nstatus",
        "> Yes, I trust this folder\n  No, exit\n  ↑/↓ Navigate · enter Confirm\nstatus",
        "  1. Yes, run command\nstatus one\nstatus two",
    ] {
        assert!(matches_waiting(&patterns, rows), "{rows}");
    }
}

/// Task `a` as a task of the builtin Antigravity profile.
async fn watched(awaits_instruction: bool) -> fixture::Watched {
    let watched = fixture::Watched::new(awaits_instruction, None).await;
    watched
        .svc
        .update("a", |task| {
            task.profile = "antigravity".into();
            true
        })
        .await
        .expect("owned task");
    watched
}

#[tokio::test]
async fn the_trust_screen_turns_a_launch_waiting_and_the_empty_prompt_after_it_idle() {
    // Every launch of agy begins without an instruction (`Binding::resumed_session`).
    let mut watched = watched(true).await;
    let mut attention = watched.svc.hub.subscribe(crate::AgentsAttentionTopic::NAME);
    let (_, trust) = capture!("trust");
    let (_, idle) = capture!("idle-after-trust");
    let start = Instant::now();
    watched.read(trust, start).await;
    watched.read(trust, start + QUIET).await;
    let waiting = watched.task().await;
    assert_eq!(waiting.state, AgentState::Waiting);
    assert_eq!(
        waiting.attention.as_ref().expect("attention").kind,
        "output"
    );
    // Still waiting for its first instruction: the trust screen is not one.
    assert!(watched.binding().await.resumed_session);
    // The user trusts the folder in the terminal: agy shows its empty prompt.
    watched.read(idle, start + QUIET * 2).await;
    watched.read(idle, start + QUIET * 3).await;
    assert_eq!(watched.task().await.state, AgentState::Running);
    watched.read(idle, start + QUIET * 4).await;
    watched.read(idle, start + QUIET * 5).await;
    let ready = watched.task().await;
    assert_eq!(ready.state, AgentState::Idle);
    let shown = ready.attention.clone().expect("attention");
    assert_eq!(
        (shown.kind.as_str(), shown.message.as_str()),
        ("idle", "다음 지시를 기다립니다")
    );
    assert!(!watched.binding().await.resumed_session);
    assert_eq!(fixture::notices(&mut attention).len(), 2);
}

#[tokio::test]
async fn a_working_or_prompted_launch_is_never_judged_idle_from_its_pane() {
    let (_, idle) = capture!("idle");
    let (_, working) = capture!("working");
    let start = Instant::now();
    // The launch still waits for its first instruction, and agy works: a prompt typed in the pane.
    let mut busy = watched(true).await;
    busy.read(working, start).await;
    busy.read(working, start + QUIET).await;
    busy.read(working, start + QUIET * 2).await;
    assert_eq!(busy.task().await.state, AgentState::Running);
    // The launch got its instruction: only agy's Stop hook ends its turn, not the empty prompt
    // box, which agy also draws while it works.
    let mut prompted = watched(false).await;
    prompted.read(idle, start).await;
    prompted.read(idle, start + QUIET).await;
    prompted.read(idle, start + QUIET * 2).await;
    assert_eq!(prompted.task().await.state, AgentState::Running);
}
