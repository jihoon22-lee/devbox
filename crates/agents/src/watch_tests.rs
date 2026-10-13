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
fn observed_codex_wrapped_choices_match_only_current_ordered_footer() {
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
    assert!(!matches_waiting(
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
