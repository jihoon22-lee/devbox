//! Prompts that stop a tool until the user decides. Each must read as waiting, so that a send
//! cannot answer it; the same screen with newer output below it must not.
use super::*;

/// Every Codex prompt that waits for a decision at the bottom of the screen. The fixtures are drawn
/// by the Codex TUI source, which is the same at rust-v0.164.0, rust-v0.165.0 and rust-v0.166.0
/// (see their headers). The `decision-` ones are lists Codex opens on its own when a turn ends or
/// stalls.
const CODEX_PROMPTS: [(&str, &str); 27] = [
    (
        "approval-exec",
        include_str!("../tests/fixtures/codex-0.166.0/approval-exec.txt"),
    ),
    (
        "approval-exec-prefix",
        include_str!("../tests/fixtures/codex-0.166.0/approval-exec-prefix.txt"),
    ),
    (
        "approval-network",
        include_str!("../tests/fixtures/codex-0.166.0/approval-network.txt"),
    ),
    (
        "approval-permissions",
        include_str!("../tests/fixtures/codex-0.166.0/approval-permissions.txt"),
    ),
    (
        "approval-additional-permissions",
        include_str!("../tests/fixtures/codex-0.166.0/approval-additional-permissions.txt"),
    ),
    (
        "approval-patch",
        include_str!("../tests/fixtures/codex-0.166.0/approval-patch.txt"),
    ),
    (
        "approval-write-stdin",
        include_str!("../tests/fixtures/codex-0.166.0/approval-write-stdin.txt"),
    ),
    (
        "approval-cross-thread",
        include_str!("../tests/fixtures/codex-0.166.0/approval-cross-thread.txt"),
    ),
    (
        "approval-mcp",
        include_str!("../tests/fixtures/codex-0.166.0/approval-mcp.txt"),
    ),
    (
        "approval-mcp-tool",
        include_str!("../tests/fixtures/codex-0.166.0/approval-mcp-tool.txt"),
    ),
    (
        "approval-mcp-message",
        include_str!("../tests/fixtures/codex-0.166.0/approval-mcp-message.txt"),
    ),
    (
        "approval-mcp-form",
        include_str!("../tests/fixtures/codex-0.166.0/approval-mcp-form.txt"),
    ),
    (
        "approval-user-verification",
        include_str!("../tests/fixtures/codex-0.166.0/approval-user-verification.txt"),
    ),
    (
        "approval-question",
        include_str!("../tests/fixtures/codex-0.166.0/approval-question.txt"),
    ),
    (
        "approval-question-confirm",
        include_str!("../tests/fixtures/codex-0.166.0/approval-question-confirm.txt"),
    ),
    (
        "approval-url",
        include_str!("../tests/fixtures/codex-0.166.0/approval-url.txt"),
    ),
    (
        "feedback-note",
        include_str!("../tests/fixtures/codex-0.166.0/feedback-note.txt"),
    ),
    (
        "decision-plan",
        include_str!("../tests/fixtures/codex-0.166.0/decision-plan.txt"),
    ),
    (
        "decision-rate-limit",
        include_str!("../tests/fixtures/codex-0.166.0/decision-rate-limit.txt"),
    ),
    (
        "decision-daybreak",
        include_str!("../tests/fixtures/codex-0.166.0/decision-daybreak.txt"),
    ),
    (
        "decision-misalignment",
        include_str!("../tests/fixtures/codex-0.166.0/decision-misalignment.txt"),
    ),
    (
        "decision-safety-retry",
        include_str!("../tests/fixtures/codex-0.166.0/decision-safety-retry.txt"),
    ),
    (
        "startup-update",
        include_str!("../tests/fixtures/codex-0.166.0/startup-update.txt"),
    ),
    (
        "startup-model",
        include_str!("../tests/fixtures/codex-0.166.0/startup-model.txt"),
    ),
    (
        "startup-cwd",
        include_str!("../tests/fixtures/codex-0.166.0/startup-cwd.txt"),
    ),
    (
        "startup-unarchive",
        include_str!("../tests/fixtures/codex-0.166.0/startup-unarchive.txt"),
    ),
    (
        "startup-daemon",
        include_str!("../tests/fixtures/codex-0.166.0/startup-daemon.txt"),
    ),
];

/// Dialogs Codex opens on its own, drawn over a list or the history as Codex's own tests draw
/// them. They are read over the whole screen, so output below them does not end them; they end
/// when they close (see watch_dialog_tests.rs).
const CODEX_DIALOG_PROMPTS: [(&str, &str); 2] = [
    (
        "decision-misalignment-review",
        include_str!("../tests/fixtures/codex-0.166.0/decision-misalignment-review.txt"),
    ),
    (
        "decision-safety-confirm",
        include_str!("../tests/fixtures/codex-0.166.0/decision-safety-confirm.txt"),
    ),
];

/// Every choice a Codex approval offers, worded as `approval_overlay.rs` (exec, patch,
/// permissions and MCP options), `user_verification.rs` and `mcp_server_elicitation.rs` word them
/// at rust-v0.164.0 to rust-v0.166.0, with the default key hints. The MCP form rows keep their
/// description column.
const CODEX_CHOICES: [&str; 29] = [
    "Yes, proceed (y)",
    "Yes, just this once (y)",
    "Yes, and don't ask again for commands that start with `cargo test` (p)",
    "Yes, and don't ask again for this command in this session (a)",
    "Yes, and allow these permissions for this session (a)",
    "Yes, and allow this host for this conversation (a)",
    "Yes, and allow this host in the future (p)",
    "No, and block this host in the future (d)",
    "No, continue without running it (d)",
    "No, and tell Codex what to do differently (esc)",
    "Yes, and don't ask again for these files (a)",
    "Yes, grant these permissions for this turn (y)",
    "Yes, grant for this turn with strict auto review (r)",
    "Yes, grant these permissions for this session (a)",
    "No, continue without permissions (d)",
    "Yes, provide the requested info (y)",
    "No, but continue without it (n)",
    "Cancel this request (esc)",
    "Verify and approve (y)",
    "Cancel this request (c)",
    "Allow                   Run the tool and continue",
    "Allow for this session  Run the tool and remember this choice for this session",
    "Always allow            Run the tool and remember this choice for future tool calls",
    "Cancel                  Cancel this tool call",
    "Allow                   Allow this request and continue",
    "Allow for this session  Allow this request and remember this choice for this session",
    "Always allow            Allow this request and remember this choice for future requests",
    "Deny                    Decline this request and continue",
    "Cancel                  Cancel this request",
];

/// The choices of the other Codex lists, as their fixtures draw them: those at start, and those
/// Codex opens on its own.
const CODEX_LIST_CHOICES: [&str; 22] = [
    "Update now (runs `npm install -g @openai/codex`)",
    "Skip until next version",
    "Try new model",
    "Use existing model",
    "Use session directory (/Users/example/session)",
    "Use current directory (/Users/example/current)",
    "Always use session directory",
    "Always use current directory",
    "Unarchive and fork",
    "Run without daemon this time",
    "Restart with these settings (recommended)",
    "Yes, implement this plan          Switch to Default and start coding",
    "Yes, clear context and implement  Fresh thread with this plan",
    "No, stay in Plan mode             Continue planning with the model",
    "Keep current model",
    "Keep current model (never show again)  Hide future rate limit reminders",
    "Enable Daybreak",
    "Acknowledge findings and",
    "Resume another chat",
    "Retry with a faster model",
    "Dismiss and keep waiting",
    "Stop and retry",
];

#[test]
fn every_codex_prompt_that_waits_for_a_decision_is_waiting() {
    let patterns = codex_patterns();
    let missed: Vec<&str> = CODEX_PROMPTS
        .iter()
        .chain(&CODEX_DIALOG_PROMPTS)
        .filter(|(_, fixture)| !matches_waiting(&patterns, screen(fixture)))
        .map(|(name, _)| *name)
        .collect();
    assert!(missed.is_empty(), "not waiting: {missed:?}");
}

#[test]
fn a_codex_prompt_with_newer_output_below_it_is_not_waiting() {
    let patterns = codex_patterns();
    for (name, fixture) in CODEX_PROMPTS {
        let stale = format!("{}\nworking\n1\n2\n3\n4\n5\n6", screen(fixture));
        assert!(!matches_waiting(&patterns, &stale), "{name}");
    }
}

#[test]
fn any_one_codex_choice_is_waiting_whatever_its_number_or_cursor() {
    let patterns = codex_patterns();
    // One choice under other output, without the question or the footer: a long choice or a
    // short pane can leave only one in the matched tail.
    let mut missed = Vec::new();
    for (index, choice) in CODEX_CHOICES.iter().chain(&CODEX_LIST_CHOICES).enumerate() {
        for row in [
            format!("› {}. {choice}", index % 5 + 1),
            format!("  {}. {choice}", index % 5 + 2),
            format!("  {choice}"),
        ] {
            if !matches_waiting(&patterns, &format!("• Ran ls\n  └ Cargo.toml\n\n{row}\n")) {
                missed.push(row);
            }
        }
    }
    assert!(missed.is_empty(), "not waiting: {missed:#?}");
}

#[test]
fn a_codex_approval_is_waiting_with_any_choice_count_order_or_cursor() {
    let patterns = codex_patterns();
    let network = screen(include_str!(
        "../tests/fixtures/codex-0.166.0/approval-network.txt"
    ));
    // A server may offer a fifth choice: telling Codex what to do becomes row 5.
    let five = network.replacen(
        "  4. No, and tell Codex",
        "  4. No, and block this host in the future (d)\n  5. No, and tell Codex",
        1,
    );
    // The cursor moved to the last choice.
    let moved = network
        .replacen("› 1. Yes", "  1. Yes", 1)
        .replacen("  4. No", "› 4. No", 1);
    // The choices in reverse order, and no footer.
    let choices: Vec<&str> = network
        .lines()
        .filter(|line| {
            line.trim_start_matches(['›', ' '])
                .starts_with(char::is_numeric)
        })
        .collect();
    assert_eq!(choices.len(), 4);
    let reversed = format!(
        "{}\n",
        choices.into_iter().rev().collect::<Vec<_>>().join("\n")
    );
    for (name, screen) in [("five", five), ("moved", moved), ("reversed", reversed)] {
        assert_ne!(screen, network, "{name}");
        assert!(matches_waiting(&patterns, &screen), "{name}");
    }
}

#[test]
fn a_codex_approval_with_choices_this_version_does_not_name_is_waiting_by_its_footer() {
    let patterns = codex_patterns();
    let network = screen(include_str!(
        "../tests/fixtures/codex-0.166.0/approval-network.txt"
    ));
    // A later Codex may word its choices otherwise; Enter still answers the list.
    let mut renamed = network.to_owned();
    for (old, new) in [
        ("Yes, just this once", "Allow once"),
        (
            "Yes, and allow this host for this conversation",
            "Allow for now",
        ),
        ("Yes, and allow this host in the future", "Always allow"),
        (
            "No, and tell Codex what to do differently",
            "Do something else",
        ),
    ] {
        assert!(renamed.contains(old), "{old}");
        renamed = renamed.replacen(old, new, 1);
    }
    assert!(matches_waiting(&patterns, &renamed));
    // The footer is what marks it.
    let without = renamed.replacen("Press enter to confirm", "Pressed enter", 1);
    assert!(!matches_waiting(&patterns, &without));
}

#[test]
fn any_codex_list_picker_is_waiting_by_its_footer_whatever_it_offers() {
    let patterns = codex_patterns();
    // codex-rs/tui/src/app/tests.rs (rust-v0.164.0 to rust-v0.166.0): the "Enable subagents?"
    // list drawn over the agent picker, renamed to a list Codex does not draw, so none of its
    // rows is a known one. Enter would pick its first choice, as it would in any other list.
    let enable = "      Enable subagents?\n› As  Subagents are disabled in this TUI session.\n\n  gp\n    › 1. Yes, enable  Save on the server for new threads without changing\n                      this thread\n      2. Not now      Keep subagents disabled\n\n      enter select · esc back\n";
    let mut list = enable.to_owned();
    for (old, new) in [
        ("Enable subagents?", "Pick a theme"),
        ("Yes, enable", "Dark"),
        ("Not now", "Light"),
    ] {
        assert!(list.contains(old), "{old}");
        list = list.replacen(old, new, 1);
    }
    assert!(matches_waiting(&patterns, &list));
    // The other verbs the lists of `chatwidget/usage.rs` show.
    for verb in ["choose", "confirm", "open"] {
        let footer = list.replacen("enter select", &format!("enter {verb}"), 1);
        assert!(matches_waiting(&patterns, &footer), "{verb}");
    }
    // Without the footer nothing marks this list.
    let without = list.replacen("      enter select · esc back\n", "", 1);
    assert!(!matches_waiting(&patterns, &without));
    // The list as Codex draws it is one of its dialogs, which its title and choices mark.
    let dialog = enable.replacen("      enter select · esc back\n", "", 1);
    assert!(matches_waiting(&patterns, &dialog));
}

#[test]
fn a_codex_question_footer_that_wraps_after_its_submit_tip_is_waiting() {
    let patterns = codex_patterns();
    // `request_user_input` puts " | " only between tips on one row, so a footer that wraps can
    // end a row with "enter to submit all".
    let wrapped = "  Question 2/2 (2 unanswered)\n  Which file should I change?\n\n› 1. app/tests/login.rs\n  2. app/src/session.rs\n\n  tab to add notes | enter to submit all\n  ←/→ to navigate questions | esc to interrupt\n";
    assert!(matches_waiting(&patterns, wrapped));
    let single = "  Field 1/1\n  Allow the docs server to read the page?\n\n› 1. Yes\n  2. No\n\n  enter to submit\n  esc to cancel\n";
    assert!(matches_waiting(&patterns, single));
}

#[test]
fn a_codex_feedback_note_is_waiting_whatever_it_holds() {
    let patterns = codex_patterns();
    // bottom_pane/feedback_note_view.rs: Enter sends what the note holds, a pasted send
    // included, as the reason of a feedback upload.
    let note = screen(include_str!(
        "../tests/fixtures/codex-0.166.0/feedback-note.txt"
    ));
    let pasted = note.replacen(
        "▌ (optional) Write a short description to help us further",
        "▌ owned-pane-check-instruction\n▌ second line",
        1,
    );
    assert_ne!(pasted, note);
    // A 4-row pane keeps only the disclosure and the note (feedback_disclosure_four_rows.snap).
    let short = "▌ Your data may be used to improve our models and products\n▌\n▌ A visible note.\nPress enter to submit or esc to cancel\n";
    for (name, shown) in [
        ("as drawn", note),
        ("pasted", pasted.as_str()),
        ("four rows", short),
    ] {
        assert!(matches_waiting(&patterns, shown), "{name}");
    }
    // The same words in the history are not the view.
    let working = screen(include_str!("../tests/fixtures/codex-0.164.0/working.txt"));
    for line in [
        "• Press enter to submit the form, then check the log.",
        "  Pressing enter to submit the form sends it.",
    ] {
        assert!(
            !matches_waiting(&patterns, &format!("{working}\n{line}")),
            "{line}"
        );
    }
}

#[test]
fn a_narrow_codex_approval_is_waiting() {
    let patterns = codex_patterns();
    // codex-rs/tui/src/bottom_pane/snapshots/
    // codex_tui__bottom_pane__user_verification__tests__user_verification_narrow.snap
    // (rust-v0.164.0 to rust-v0.166.0): 40 columns by 8 rows, the header folded and the footer
    // wrapped.
    let verification = "\n  Production deployment needs your\n  approval.\n  [… 7 lines] ⌃a view all\n› 1. Verify and approve (y)\n  2. Cancel this request (c)\n  Press enter to confirm or esc to\n  cancel\n";
    // approval-network.txt wrapped by hand at 40 columns: the choices and the footer wrap.
    let network = "  Do you want to approve network access\n  to \"example.com\"?\n\n  Reason: network request blocked\n\n› 1. Yes, just this once (y)\n  2. Yes, and allow this host for this\n     conversation (a)\n  3. Yes, and allow this host in the\n     future (p)\n  4. No, and tell Codex what to do\n     differently (esc)\n\n  Press enter to confirm or esc to\n  cancel\n";
    // A pane too short for the footer, with only the selected choice left.
    let short = "  [… 9 lines] ⌃a view all\n› 1. Yes, just this once (y)\n";
    for (name, screen) in [
        ("verification", verification),
        ("network", network),
        ("short", short),
    ] {
        assert!(matches_waiting(&patterns, screen), "{name}");
    }
}

#[test]
fn codex_output_that_mentions_a_prompt_is_not_waiting() {
    let patterns = codex_patterns();
    let working = screen(include_str!("../tests/fixtures/codex-0.164.0/working.txt"));
    for line in [
        // A question alone: its choices are what mark the prompt.
        "  Would you like to run the following command?",
        "  Do you want to approve network access to \"example.com\"?",
        // What Codex writes to the history once the user decided.
        "✔ You approved codex to run echo hello world this time",
        "✗ You canceled the request to run echo hello world",
        "✔ You approved codex network access to http://example.com this time",
        // A choice quoted inside a sentence.
        "• I picked \"Yes, proceed\" and the command ran.",
        // What "Yes, implement this plan" sends, as the history shows it, and list words in a
        // sentence.
        "› Implement the plan.",
        "• Open /model, then enter select · esc back to leave it.",
    ] {
        assert!(
            !matches_waiting(&patterns, &format!("{working}\n{line}")),
            "{line}"
        );
    }
}

/// Claude's folder-trust dialog. No Claude hook reports it, so the pane is the only sign of
/// this wait.
const CLAUDE_TRUST: &str = include_str!("../tests/fixtures/claude-2.1.308/trust.txt");

#[test]
fn claude_folder_trust_dialog_is_waiting() {
    let patterns = builtin_patterns("claude");
    let trust = screen(CLAUDE_TRUST);
    // The cursor starts on the first choice, which declines; the variant that goes on without
    // the folder's settings names that choice otherwise.
    let go_on = trust.replacen("❯ No, exit", "❯ No, continue without these permissions", 1);
    let moved = trust.replacen("❯ No, exit", "  No, exit", 1).replacen(
        "  Yes, I trust",
        "❯ Yes, I trust",
        1,
    );
    // Claude's accessible layout prints the choices as "y. …" and "n. …".
    let accessible = " Accessing workspace:\n\n /tmp/devbox-claude-fixture\n\n y. Yes, I trust this folder\n n. No, exit\n";
    for (name, screen) in [("go on", go_on.as_str()), ("moved", moved.as_str())] {
        assert_ne!(screen, trust, "{name}");
    }
    for (name, screen) in [
        ("as drawn", trust),
        ("go on", go_on.as_str()),
        ("moved", moved.as_str()),
        ("accessible", accessible),
    ] {
        assert!(matches_waiting(&patterns, screen), "{name}");
    }
    // Output below the dialog means it is no longer the current screen.
    assert!(!matches_waiting(
        &patterns,
        &format!("{trust}\nworking\n1\n2\n3\n4\n5\n6")
    ));
}

#[test]
fn a_normal_claude_screen_is_not_waiting() {
    let patterns = builtin_patterns("claude");
    let working = screen(include_str!("../tests/fixtures/claude-2.1.308/working.txt"));
    assert!(!matches_waiting(&patterns, working));
    // The dialog's words in Claude's output are not the dialog.
    for line in [
        "● Pick \"Yes, I trust this folder\" when Claude asks.",
        " Quick safety check: Is this a project you created or one you trust?",
        " Accessing workspace:",
    ] {
        assert!(
            !matches_waiting(&patterns, &format!("{working}\n{line}")),
            "{line}"
        );
    }
}
