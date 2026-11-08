use super::*;
fn task() -> AgentTask {
    serde_json::from_str(r#"{"id":"a","projectId":"p","title":"x","profile":"codex","baseBranch":"main","branch":null,"worktreePath":null,"workdir":"/owned","sessionId":null,"terminalId":"term","state":"running","reason":null,"attention":null,"port":20000,"prompt":"","followUps":[],"tokens":null,"prUrl":null,"closedHow":null,"createdMs":0,"updatedMs":0,"closedMs":null}"#).expect("owned fixture")
}
fn snapshot() -> Snapshot {
    Snapshot {
        task: task(),
        binding: Some(Binding::new(1)),
    }
}
#[test]
fn observed_codex_0_162_1_approval_with_wrapped_choices_is_waiting() {
    let builtin = profile::builtin();
    let patterns =
        profile::compiled(&crate::AgentProfiles { items: builtin }).expect("owned fixture");
    // Scrubbed fixture follows the owned S0a 0.162.1 approval capture. The header is
    // 19 physical lines above the bottom and option 2 spans multiple lines.
    let tail = "old conversation\n\nWould you like to run the following command?\n\nEnvironment: local\n\nReason: create an owned fixture\n\n$ mkdir -p /owned/test && printf hi >\n/owned/test/hello.txt\n\n\n› 1. Yes, proceed (y)\n2. Yes, and don't ask again for commands that start with `mkdir -p /owned/long-path\n  /with/more/wrapping/hello.txt` (p)\n3. No, and tell Codex what to do differently (esc)\n\nPress enter to confirm or esc to cancel\n";
    let patterns = &patterns["codex"];
    assert!(matches_waiting(patterns, tail));
    assert!(!matches_waiting(
        patterns,
        "Would you like to run the following command?"
    ));
    assert!(!matches_waiting(
        patterns,
        &format!("{tail}\nworking\n1\n2\n3\n4\n5\n6")
    ));
    // Since S7-13 no row order or footer is required: a choice row marks the prompt on its
    // own, wherever the footer is (Codex 0.164 forms have 2 to 5 choices, see
    // watch_prompt_tests.rs). This swapped pair used to be refused for its order.
    assert!(matches_waiting(
        patterns,
        "Press enter to confirm or esc to cancel\n3. No, and tell Codex what to do differently (esc)"
    ));
}
#[test]
fn quiet_period_requires_two_seconds_of_unchanged_output_and_generation() {
    let now = Instant::now();
    let mut observation = Observation {
        snapshot: snapshot(),
        tail: "prompt".into(),
        since: now,
        suppressed: false,
    };
    assert!(!observation.observe(snapshot(), "prompt".into(), now + Duration::from_secs(1)));
    assert!(observation.observe(snapshot(), "prompt".into(), now + QUIET));
    assert!(!observation.observe(snapshot(), "changed".into(), now + QUIET));
    let mut resumed = snapshot();
    resumed.binding.as_mut().expect("owned fixture").launch_ms = 2;
    assert!(!observation.observe(resumed, "changed".into(), now + QUIET * 2));
}
#[test]
fn sent_input_suppresses_an_unchanged_approval_screen_until_redraw() {
    let now = Instant::now();
    let mut waiting = snapshot();
    apply_event(&mut waiting.task, &AgentEvent::OutputWaiting, 1);
    let mut observation = Observation {
        snapshot: waiting,
        tail: "approval".into(),
        since: now,
        suppressed: false,
    };
    assert!(!observation.observe(snapshot(), "approval".into(), now + QUIET));
    assert!(!observation.observe(snapshot(), "approval".into(), now + QUIET * 4));
    assert!(!observation.observe(snapshot(), "redrawn approval".into(), now + QUIET * 5));
    assert!(observation.observe(snapshot(), "redrawn approval".into(), now + QUIET * 6));
}
#[test]
fn in_flight_output_cannot_replace_new_hook_terminal_or_turn() {
    let old = snapshot();
    let mut current = old.task.clone();
    apply_event(
        &mut current,
        &AgentEvent::NeedsInput {
            message: "permission".into(),
        },
        1,
    );
    assert!(!old.current(&current, &old.binding));
    current = old.task.clone();
    current.terminal_id = Some("resumed".into());
    assert!(!old.current(&current, &old.binding));
    let mut binding = old.binding.clone();
    binding.as_mut().expect("owned fixture").turn_id = Some("new-turn".into());
    assert!(!old.current(&old.task, &binding));
}
#[test]
fn output_observation_only_resumes_its_own_wait() {
    // `apply_event` lets any OutputResumed end a wait; this filter keeps quiet
    // output from ending a hook's permission wait.
    let mut output = task();
    apply_event(&mut output, &AgentEvent::OutputWaiting, 1);
    assert!(eligible(&output));
    let mut hook = task();
    apply_event(
        &mut hook,
        &AgentEvent::NeedsInput {
            message: "permission".into(),
        },
        1,
    );
    assert_eq!(hook.state, AgentState::Waiting);
    assert!(!eligible(&hook));
}
#[test]
fn invalid_profile_regex_is_a_typed_configuration_error() {
    let config: devbox_core::Config = toml::from_str(
        "[[agents.profiles]]\nid='x'\nlabel='X'\ncommand='x'\nhooks='none'\nwaitingPatterns=['[']",
    )
    .expect("owned fixture");
    assert!(matches!(
        profile::configured(&config.agents),
        Err(ProfileError::WaitingPattern { .. })
    ));
}

#[test]
fn same_millisecond_prompt_activity_invalidates_in_flight_observation() {
    let old = snapshot();
    let mut current = old.task.clone();
    assert!(apply_event(
        &mut current,
        &AgentEvent::PromptSubmitted,
        old.task.updated_ms
    ));
    assert_eq!(current.updated_ms, old.task.updated_ms);
    assert_eq!(current.state, old.task.state);
    assert!(!old.current(&current, &old.binding));
}

#[test]
fn review_regression_send_while_running_suppresses_unchanged_approval() {
    let now = Instant::now();
    let mut observation = Observation {
        snapshot: snapshot(),
        tail: "approval".into(),
        since: now,
        suppressed: false,
    };
    let mut instructed = snapshot();
    apply_event(&mut instructed.task, &AgentEvent::UserInstructed, 1);
    assert!(!observation.observe(instructed.clone(), "approval".into(), now + QUIET));
    assert!(!observation.observe(instructed, "approval".into(), now + QUIET * 2));
}

#[test]
fn session_metadata_keeps_initial_approval_detection_eligible() {
    let now = Instant::now();
    let mut observation = Observation {
        snapshot: snapshot(),
        tail: "approval".into(),
        since: now,
        suppressed: false,
    };
    let mut started = snapshot();
    apply_event(
        &mut started.task,
        &AgentEvent::SessionStarted {
            session_id: "session".into(),
        },
        1,
    );
    assert!(!observation.observe(started.clone(), "approval".into(), now + QUIET));
    assert!(observation.observe(started, "approval".into(), now + QUIET * 2));
}

/// Fixtures in `tests/fixtures/codex-0.164.0/` and `tests/fixtures/pi-1.3.0/` start with `#`
/// provenance lines and a `---` line. Only the text after that line is the pane, so the header
/// never reaches the matcher.
fn screen(fixture: &str) -> &str {
    fixture
        .split_once("\n---\n")
        .expect("fixture header ends with a --- line")
        .1
}
fn builtin_patterns(id: &str) -> WaitingPatterns {
    profile::compiled(&crate::AgentProfiles {
        items: profile::builtin(),
    })
    .expect("owned fixture")
    .remove(id)
    .expect("builtin profile")
}
fn codex_patterns() -> WaitingPatterns {
    builtin_patterns("codex")
}

#[test]
fn codex_trust_screen_is_waiting() {
    let patterns = codex_patterns();
    let trust = screen(include_str!("../tests/fixtures/codex-0.164.0/trust.txt"));
    assert!(matches_waiting(&patterns, trust));
    // Moving the cursor to the other choice leaves it the same prompt.
    let moved = trust.replacen("› 1. Trust and continue", "  1. Trust and continue", 1);
    let moved = moved.replacen("  2. Back", "› 2. Back", 1);
    assert_ne!(moved, trust);
    assert!(matches_waiting(&patterns, &moved));
    // The same screen re-wrapped to 60 columns: the question now starts on the 8th non-empty
    // row from the bottom, outside the 6 rows `matches_waiting` reads, so only the choice rows
    // can identify the prompt.
    let narrow = "  Folder access\n  /tmp/devbox-codex-fixture\n\n  Trust this folder? Codex can read, edit, and run files\n  here, subject to your permission settings. Folder\n  settings can run code automatically, even without a\n  model request. Continue only if you trust these files.\n  Your trust decision will be saved.\n\n› 1. Trust and continue\n  2. Back to Agent Command Center\n\n  enter continue · esc back\n";
    let question = WaitingPatterns {
        tail: vec![Regex::new(r"(?m)^\s*Trust this folder\?").expect("owned fixture")],
        screen: Vec::new(),
    };
    assert!(
        !matches_waiting(&question, narrow),
        "holds only while matches_waiting reads 6 non-empty rows: the question is the 8th from the bottom; \
         if that window grows past 8 this screen no longer shows why the pattern anchors on the choice rows"
    );
    assert!(matches_waiting(&patterns, narrow));
    // Output below the prompt means it is no longer the current screen.
    assert!(!matches_waiting(
        &patterns,
        &format!("{trust}\nworking\n1\n2\n3\n4\n5\n6")
    ));
}

#[test]
fn codex_hooks_review_screen_is_waiting() {
    let patterns = codex_patterns();
    let review = screen(include_str!(
        "../tests/fixtures/codex-0.164.0/hooks-review.txt"
    ));
    assert!(matches_waiting(&patterns, review));
    assert!(!matches_waiting(
        &patterns,
        &format!("{review}\nworking\n1\n2\n3\n4\n5\n6")
    ));
}

#[test]
fn codex_working_screen_is_not_waiting() {
    let patterns = codex_patterns();
    let working = screen(include_str!("../tests/fixtures/codex-0.164.0/working.txt"));
    assert!(!matches_waiting(&patterns, working));
    // A hooks notice, or the trust question quoted in output, is not the prompt itself.
    for line in [
        "3 hooks need review before they can run.",
        "Trust this folder? Codex can read, edit, and run files here, subject to your permission settings.",
    ] {
        assert!(
            !matches_waiting(&patterns, &format!("{working}\n{line}")),
            "{line}"
        );
    }
}

#[test]
fn pi_trust_screen_is_waiting() {
    let patterns = builtin_patterns("pi");
    let trust = screen(include_str!("../tests/fixtures/pi-1.3.0/trust.txt"));
    assert!(matches_waiting(&patterns, trust));
    // Moving the selection onto either row the pattern reads leaves it the same prompt.
    let unselected = trust.replacen(" → Trust\n", "   Trust\n", 1);
    assert_ne!(unselected, trust);
    for row in ["Do not trust\n", "Do not trust (this session only)\n"] {
        let moved = unselected.replacen(&format!("   {row}"), &format!(" → {row}"), 1);
        assert_ne!(moved, unselected, "{row}");
        assert!(matches_waiting(&patterns, &moved), "{row}");
    }
    // Output below the prompt means it is no longer the current screen.
    assert!(!matches_waiting(
        &patterns,
        &format!("{trust}\nworking\n1\n2\n3\n4\n5\n6")
    ));
}

#[test]
fn pi_screens_after_the_trust_decision_are_not_waiting() {
    let patterns = builtin_patterns("pi");
    for (name, fixture) in [
        (
            "l1",
            include_str!("../tests/fixtures/pi-1.3.0/after-trust-l1.txt"),
        ),
        (
            "l2",
            include_str!("../tests/fixtures/pi-1.3.0/after-trust-l2.txt"),
        ),
        (
            "l3",
            include_str!("../tests/fixtures/pi-1.3.0/after-trust-l3.txt"),
        ),
    ] {
        let after = screen(fixture);
        assert!(!matches_waiting(&patterns, after), "{name}");
        // The question or one choice quoted in pi's output is not the prompt itself.
        for line in [
            "Trust project folder?",
            " → Trust",
            "Do not trust (this session only)",
        ] {
            assert!(
                !matches_waiting(&patterns, &format!("{after}\n{line}")),
                "{name}: {line}"
            );
        }
    }
}

#[test]
fn an_opencode_draft_in_shell_mode_is_waiting() {
    let patterns = builtin_patterns("opencode");
    // Enter would run the draft, and a send pasted after it, as a shell command.
    let shell = screen(include_str!(
        "../tests/fixtures/opencode-1.19.3/shell-mode.txt"
    ));
    assert!(matches_waiting(&patterns, shell));
    // Output below it means it is no longer the current screen.
    assert!(!matches_waiting(
        &patterns,
        &format!("{shell}\nworking\n1\n2\n3\n4\n5\n6")
    ));
    // The prompt box out of shell mode, and the words in a sentence, are not the mode.
    let normal = shell.replacen("┃  Shell", "┃  Build", 1).replacen(
        "esc exit shell mode",
        "tab agents  ctrl+p commands",
        1,
    );
    assert_ne!(normal, shell);
    assert!(!matches_waiting(&patterns, &normal));
    for line in [
        "• Press esc to exit shell mode, then send the prompt.",
        "  Exit shell mode",
    ] {
        assert!(
            !matches_waiting(&patterns, &format!("{normal}\n{line}")),
            "{line}"
        );
    }
}

#[test]
fn codex_trust_screens_for_untrusted_folders_are_waiting() {
    let patterns = codex_patterns();
    // A folder whose saved trust is "untrusted" opens restricted, and a task that was started
    // while it was trusted opens as it is; Enter picks either.
    for (name, fixture) in [
        (
            "restricted",
            include_str!("../tests/fixtures/codex-0.166.0/trust-restricted.txt"),
        ),
        (
            "existing task",
            include_str!("../tests/fixtures/codex-0.166.0/trust-existing-task.txt"),
        ),
    ] {
        let trust = screen(fixture);
        assert!(matches_waiting(&patterns, trust), "{name}");
        // Without the "Agent Command Center", the other choice quits.
        let quit = trust.replacen("2. Back to Agent Command Center", "2. Quit", 1);
        assert_ne!(quit, trust, "{name}");
        assert!(matches_waiting(&patterns, &quit), "{name}");
        // Output below the prompt means it is no longer the current screen.
        assert!(
            !matches_waiting(&patterns, &format!("{trust}\nworking\n1\n2\n3\n4\n5\n6")),
            "{name}"
        );
    }
}

#[test]
fn every_profile_of_a_tool_family_keeps_its_patterns_and_adds_its_own() {
    let profile = |id: &str, hooks, patterns: &[&str]| profile::ToolProfile {
        id: id.into(),
        label: id.into(),
        command: id.into(),
        hooks,
        waiting_patterns: patterns.iter().map(|&p| p.to_owned()).collect(),
    };
    let compiled = profile::compiled(&crate::AgentProfiles {
        items: vec![
            // A configured `codex` replaces the builtin one; the others are added.
            profile("codex", profile::Hooks::Codex, &[]),
            profile("codex-fast", profile::Hooks::Codex, &["(?m)^ready$"]),
            profile("claude-work", profile::Hooks::Claude, &[]),
            profile("pi-alt", profile::Hooks::Pi, &[]),
            profile("opencode-alt", profile::Hooks::Opencode, &[]),
            profile("mine", profile::Hooks::None, &["(?m)^ready$"]),
        ],
    })
    .expect("owned profiles");
    let network = screen(include_str!(
        "../tests/fixtures/codex-0.166.0/approval-network.txt"
    ));
    let dialog = screen(include_str!(
        "../tests/fixtures/codex-0.166.0/dialog-resume-goal-200x50.txt"
    ));
    let claude = screen(include_str!("../tests/fixtures/claude-2.1.308/trust.txt"));
    let claude_shell = screen(include_str!(
        "../tests/fixtures/claude-2.1.308/shell-mode.txt"
    ));
    let pi = screen(include_str!("../tests/fixtures/pi-1.3.0/trust.txt"));
    let opencode = screen(include_str!(
        "../tests/fixtures/opencode-1.19.3/shell-mode.txt"
    ));
    let waits = |id: &str, shown: &str| matches_waiting(&compiled[id], shown);
    for id in ["codex", "codex-fast"] {
        assert!(waits(id, network), "{id}");
        assert!(waits(id, dialog), "{id}");
        assert!(!waits(id, opencode), "{id}");
        assert!(!waits(id, claude_shell), "{id}");
    }
    assert!(waits("claude-work", claude));
    // Claude's shell mode comes with its family, whatever the profile's id, and with no other.
    assert!(waits("claude-work", claude_shell));
    assert!(!waits("pi-alt", claude_shell));
    assert!(waits("pi-alt", pi));
    assert!(waits("opencode-alt", opencode));
    // A profile's own patterns add to its family's.
    assert!(waits("codex-fast", "ready\n"));
    assert!(!waits("codex", "ready\n"));
    assert!(waits("mine", "ready\n"));
    assert!(!waits("mine", opencode));
    // opencode's plugin reports its prompts, so only its shell mode is read from the screen, and
    // `none` has only its own patterns.
    for id in ["opencode-alt", "mine"] {
        for shown in [network, dialog, claude, pi, claude_shell] {
            assert!(!waits(id, shown), "{id}");
        }
    }
}

/// A launch that resumed Codex's session without an instruction, at its prompt (real capture).
const RESUME_READY: &str = include_str!("../tests/fixtures/codex-0.167.0/resume-ready.txt");

#[tokio::test]
async fn codex_resumed_ready_screen_sets_idle_once_quiet() {
    use devbox_protocol::Topic;
    let ready = screen(RESUME_READY);
    assert!(!matches_waiting(&codex_patterns(), ready));
    let mut watched = fixture::Watched::new(true, None).await;
    let mut attention = watched.svc.hub.subscribe(crate::AgentsAttentionTopic::NAME);
    let start = Instant::now();
    watched.read(ready, start).await;
    watched.read(ready, start + QUIET / 2).await;
    assert_eq!(watched.task().await.state, AgentState::Running, "quiet 1 s");
    watched.read(ready, start + QUIET).await;
    let idle = watched.task().await;
    assert_eq!(idle.state, AgentState::Idle);
    let shown = idle.attention.clone().expect("attention");
    assert_eq!(
        (shown.kind.as_str(), shown.message.as_str()),
        ("idle", "다음 지시를 기다립니다")
    );
    assert!(!watched.binding().await.resumed_session, "judged once");
    // The same screen later changes nothing more.
    for step in [2, 3] {
        watched.read(ready, start + QUIET * step).await;
    }
    assert_eq!(watched.task().await, idle);
    let notices = fixture::notices(&mut attention);
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert!(notices[0].contains(r#""state":"idle""#), "{notices:?}");
}

/// Five passes, 2 s apart, over `shown` as the pane of a fixture task: the task, its binding and
/// the notices published must stay as they were.
async fn unchanged(resumed: bool, turn: Option<&str>, shown: &str) {
    use devbox_protocol::Topic;
    let mut watched = fixture::Watched::new(resumed, turn).await;
    let mut attention = watched.svc.hub.subscribe(crate::AgentsAttentionTopic::NAME);
    let (task, binding) = (watched.task().await, watched.binding().await);
    let start = Instant::now();
    for step in 0..5 {
        watched.read(shown, start + QUIET * step).await;
    }
    assert_eq!(watched.task().await, task, "{resumed} {turn:?}");
    assert_eq!(watched.binding().await, binding, "{resumed} {turn:?}");
    assert_eq!(fixture::notices(&mut attention), Vec::<String>::new());
}

#[tokio::test]
async fn codex_ready_screen_without_marker_or_after_a_turn_keeps_running() {
    let ready = screen(RESUME_READY);
    // A first launch, which hands Codex the task's instruction, and a resumed one whose first
    // turn began.
    unchanged(false, None, ready).await;
    unchanged(true, Some("first-turn"), ready).await;
}

#[tokio::test]
async fn codex_working_screen_is_not_ready() {
    // Codex draws its prompt box and footer while it works too.
    let working = screen(include_str!("../tests/fixtures/codex-0.164.0/working.txt"));
    assert!(!shows_prompt(working));
    unchanged(true, None, working).await;
}

#[tokio::test]
async fn codex_views_over_a_ready_prompt_wait_instead() {
    // The last rows of these are the ready prompt's; the waiting patterns read the rest of the
    // screen. The paused goal's dialog is what `codex resume` opens on its own.
    for (name, capture) in [
        (
            "dialog-resume-goal-200x50",
            include_str!("../tests/fixtures/codex-0.166.0/dialog-resume-goal-200x50.txt"),
        ),
        (
            "form-subagent-thread",
            include_str!("../tests/fixtures/codex-0.166.0/form-subagent-thread.txt"),
        ),
    ] {
        let shown = screen(capture);
        assert!(shows_prompt(shown), "{name}");
        let mut watched = fixture::Watched::new(true, None).await;
        let start = Instant::now();
        for step in 0..3 {
            watched.read(shown, start + QUIET * step).await;
        }
        let task = watched.task().await;
        assert_eq!(task.state, AgentState::Waiting, "{name}");
        assert_eq!(task.attention.expect("attention").kind, "output", "{name}");
        assert!(watched.binding().await.resumed_session, "{name}");
    }
}

#[tokio::test]
async fn codex_ready_with_a_stale_snapshot_after_a_send_keeps_running() {
    use devbox_protocol::Topic;
    let watched = fixture::Watched::new(true, None).await;
    let mut attention = watched.svc.hub.subscribe(crate::AgentsAttentionTopic::NAME);
    // The watcher read the task just before a send's reserve,
    let (mut task, mut binding) = (watched.task().await, watched.binding().await);
    let stale = Snapshot {
        task: task.clone(),
        binding: Some(binding.clone()),
    };
    // which records the instruction and clears the ready marker, as `reserve_instruction` does.
    task.follow_ups.push(crate::model::FollowUp {
        at_ms: now_ms(),
        text: "owned instruction".into(),
        instruction_id: Some("owned".into()),
        delivery: Some(crate::model::InstructionDelivery::Pending),
    });
    assert!(apply_event(
        &mut task,
        &AgentEvent::UserInstructed,
        now_ms()
    ));
    binding.resumed_session = false;
    let reserved = (task.clone(), binding.clone());
    watched
        .svc
        .db
        .write(move |tx| {
            store::put(tx, &task)?;
            store::put_binding(tx, "a", &binding)
        })
        .await
        .expect("owned reserve");
    let judged = CodexTurnEnd::Ready(Box::new(stale));
    watched
        .svc
        .complete_codex("a", judged)
        .await
        .expect("judged");
    assert_eq!((watched.task().await, watched.binding().await), reserved);
    // A current read of the launch, which no longer waits for its first instruction, ends
    // nothing either.
    let seen = Snapshot {
        task: reserved.0.clone(),
        binding: Some(reserved.1.clone()),
    };
    let judged = CodexTurnEnd::Ready(Box::new(seen));
    watched
        .svc
        .complete_codex("a", judged)
        .await
        .expect("judged");
    assert_eq!((watched.task().await, watched.binding().await), reserved);
    assert_eq!(fixture::notices(&mut attention), Vec::<String>::new());
}

#[path = "watch_dialog_tests.rs"]
mod dialogs;
#[path = "watch_fixture.rs"]
mod fixture;
#[path = "watch_form_tests.rs"]
mod forms;
#[path = "watch_prompt_tests.rs"]
mod prompts;
