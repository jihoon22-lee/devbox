//! Dialogs Codex draws in the middle of its default full-screen layout (alternate screen `Auto`,
//! `tui.fullscreen_transcript = true`). The composer stays at the bottom, so the dialog is 15 or
//! more rows above the pane's last rows: only a signature read over the whole screen sees it.
use super::*;

/// A simulated 200×50 Codex screen: the history above, the composer at the bottom.
const HISTORY: &str = include_str!("../tests/fixtures/codex-0.166.0/fullscreen-history.txt");
const RESUME_GOAL: &str = include_str!("../tests/fixtures/codex-0.166.0/dialog-resume-goal.txt");

/// Every `SelectionViewParams::confirmation()` dialog of rust-v0.164.0 to rust-v0.166.0, by the
/// file that opens it. The bodies are cut from Codex's own snapshots of them (see their headers).
const CODEX_DIALOGS: [(&str, &str); 15] = [
    ("chatwidget/goal_menu.rs", RESUME_GOAL),
    (
        "app/thread_goal_actions.rs",
        include_str!("../tests/fixtures/codex-0.166.0/dialog-replace-goal.txt"),
    ),
    (
        "chatwidget/misalignment_policy.rs",
        include_str!("../tests/fixtures/codex-0.166.0/dialog-precaution-review.txt"),
    ),
    (
        "chatwidget/safety_buffering.rs",
        include_str!("../tests/fixtures/codex-0.166.0/dialog-retry.txt"),
    ),
    (
        "chatwidget.rs",
        include_str!("../tests/fixtures/codex-0.166.0/dialog-enable-feature.txt"),
    ),
    (
        "chatwidget/usage.rs",
        include_str!("../tests/fixtures/codex-0.166.0/dialog-usage-reset.txt"),
    ),
    (
        "chatwidget/permission_popups.rs",
        include_str!("../tests/fixtures/codex-0.166.0/dialog-full-access.txt"),
    ),
    (
        "chatwidget/worktree_picker.rs",
        include_str!("../tests/fixtures/codex-0.166.0/dialog-delete-worktree.txt"),
    ),
    (
        "chatwidget/slash_dispatch.rs: /archive",
        include_str!("../tests/fixtures/codex-0.166.0/dialog-archive-session.txt"),
    ),
    (
        "chatwidget/slash_dispatch.rs: /delete",
        include_str!("../tests/fixtures/codex-0.166.0/dialog-delete-session.txt"),
    ),
    (
        "chatwidget/plugin_catalog.rs",
        include_str!("../tests/fixtures/codex-0.166.0/dialog-remove-marketplace.txt"),
    ),
    (
        "bottom_pane/feedback_view.rs",
        include_str!("../tests/fixtures/codex-0.166.0/dialog-upload-logs.txt"),
    ),
    (
        "app/daemon_menu.rs: restart",
        include_str!("../tests/fixtures/codex-0.166.0/dialog-restart-server.txt"),
    ),
    (
        "app/daemon_menu.rs: update",
        include_str!("../tests/fixtures/codex-0.166.0/dialog-update-daemon.txt"),
    ),
    (
        "app/agents_overview_actions.rs",
        include_str!("../tests/fixtures/codex-0.166.0/dialog-task-action.txt"),
    ),
];

/// The screen with `dialog` open over `history`, the way `bottom_pane/view_stack.rs` places a
/// centered dialog on a 200×50 screen: 72 columns wide, centered both ways, its area cleared and
/// the rest of each row left as drawn.
fn centered(history: &str, dialog: &str) -> String {
    const WIDTH: usize = 200;
    const HEIGHT: usize = 50;
    let mut rows: Vec<String> = history.lines().map(str::to_owned).collect();
    rows.resize(HEIGHT, String::new());
    let body: Vec<&str> = dialog.lines().collect();
    let width = (WIDTH - 4).min(72);
    let (x, y) = ((WIDTH - width) / 2, (HEIGHT - body.len()) / 2);
    for (index, line) in body.iter().enumerate() {
        let padded = |text: &str, columns: usize| -> Vec<char> {
            text.chars()
                .chain(std::iter::repeat(' '))
                .take(columns)
                .collect()
        };
        let row = padded(&rows[y + index], WIDTH);
        let joined: String = row[..x]
            .iter()
            .chain(&padded(line, width))
            .chain(&row[x + width..])
            .collect();
        rows[y + index] = joined.trim_end().to_owned();
    }
    rows.join("\n") + "\n"
}

/// The pane's last 6 non-empty rows, which the bottom prompts are matched on.
fn last_rows(shown: &str) -> Vec<&str> {
    shown
        .lines()
        .rev()
        .filter(|row| !row.trim().is_empty())
        .take(6)
        .collect()
}

#[test]
fn every_codex_dialog_drawn_in_the_middle_of_the_screen_is_waiting() {
    let patterns = codex_patterns();
    let history = screen(HISTORY);
    let mut missed = Vec::new();
    for (name, dialog) in CODEX_DIALOGS {
        let shown = centered(history, screen(dialog));
        // The last rows are the composer and the history, as without the dialog.
        assert!(
            last_rows(&shown).iter().all(|row| history.contains(row)),
            "{name}"
        );
        if !matches_waiting(&patterns, &shown) {
            missed.push(name);
        }
    }
    assert!(missed.is_empty(), "not waiting: {missed:#?}");
}

#[test]
fn the_other_wordings_of_these_dialogs_are_waiting() {
    let patterns = codex_patterns();
    let history = screen(HISTORY);
    let task = include_str!("../tests/fixtures/codex-0.166.0/dialog-task-action.txt");
    let feature = include_str!("../tests/fixtures/codex-0.166.0/dialog-enable-feature.txt");
    let session = include_str!("../tests/fixtures/codex-0.166.0/dialog-delete-session.txt");
    for (name, dialog, changes) in [
        (
            "archive task",
            task,
            &[
                (
                    "Permanently delete “Current task”?",
                    "Archive “Current task”?",
                ),
                (
                    "Permanently delete task and child agents",
                    "Archive task and child agents",
                ),
            ][..],
        ),
        (
            "memories",
            feature,
            &[
                ("Enable subagents?", "Enable memories?"),
                ("Subagents are", "Memories are"),
            ][..],
        ),
        (
            "delete without the command center",
            session,
            &[(
                "Yes, delete and return to command center",
                "Yes, delete and exit",
            )][..],
        ),
    ] {
        let mut body = screen(dialog).to_owned();
        for (old, new) in changes {
            assert!(body.contains(old), "{name}: {old}");
            body = body.replacen(old, new, 1);
        }
        assert!(
            matches_waiting(&patterns, &centered(history, &body)),
            "{name}"
        );
    }
}

#[test]
fn the_resume_goal_fixture_is_that_dialog_drawn_over_the_history() {
    let drawn = centered(screen(HISTORY), screen(RESUME_GOAL));
    let fixture = include_str!("../tests/fixtures/codex-0.166.0/dialog-resume-goal-200x50.txt");
    assert_eq!(drawn, screen(fixture));
}

#[test]
fn the_screen_once_a_dialog_closed_is_not_waiting() {
    let patterns = codex_patterns();
    let history = screen(HISTORY);
    assert!(!matches_waiting(&patterns, history));
    // Codex resumed the goal and went on: the dialog is gone, the history grew.
    let resumed = history.replacen(
        "\n› Ask Codex to do anything",
        "\n• Goal resumed: Keep improving the bare goal command until it feels calm and useful.\n\n› Ask Codex to do anything",
        1,
    );
    assert_ne!(resumed, history);
    assert!(!matches_waiting(&patterns, &resumed));
}

#[test]
fn codex_history_that_quotes_a_dialog_is_not_waiting() {
    let patterns = codex_patterns();
    let history = screen(HISTORY);
    // The titles and choices in prose, in the middle of the screen where a dialog would be.
    for prose in [
        "• Codex asked Resume paused goal? and I picked 1. Resume goal for you.",
        "• If it asks \"Enable full access?\", answer 2. Cancel unless you trust the repo.",
        "• Upload logs? 1. Yes shares them. Restart background server? 2. Restart does it.",
        // A title alone, then its choices in a sentence or at the end of one.
        "  Resume paused goal?\n  1. Resume goal marks it active, 2. Leave paused keeps it.",
        "  Delete this session?\n  To stay, pick 1. No, keep this session",
        // A title in a sentence or ending one, then a list.
        "  Upload logs? Only if you need to:\n  1. Yes",
        "• Then it asks Upload logs?\n  1. Yes",
    ] {
        let quoted = history.replacen(
            "\n• Edited app/tests/login.rs",
            &format!("\n{prose}\n\n• Edited app/tests/login.rs"),
            1,
        );
        assert_ne!(quoted, history);
        assert!(!matches_waiting(&patterns, &quoted), "{prose}");
    }
}
