//! Waiting patterns that come with a tool family, the `hooks` of a profile: every profile of
//! the family gets them (see `profile::compiled`).
use crate::profile::Hooks;

/// The waiting patterns of one family, before they are compiled.
#[derive(Debug, Default)]
pub(crate) struct FamilyPatterns {
    /// Read over the whole capture (see `watch::matches_waiting`).
    pub screen: Vec<String>,
}

pub(crate) fn family_patterns(hooks: Hooks) -> FamilyPatterns {
    match hooks {
        Hooks::Codex => FamilyPatterns {
            screen: CODEX_DIALOGS
                .iter()
                .map(|(title, choices)| dialog(title, choices))
                .collect(),
        },
        Hooks::Claude | Hooks::Opencode | Hooks::Pi | Hooks::None => FamilyPatterns::default(),
    }
}

/// The dialogs Codex opens with `SelectionViewParams::confirmation()`, by title and choices.
/// Evidence: crates/agents/tests/fixtures/codex-0.166.0/dialog-*.txt (소스 기반: drawn by the TUI
/// source of rust-v0.164.0 to rust-v0.166.0, the same in all three; not real captures). In the
/// default full-screen layout (alternate screen `Auto`, `tui.fullscreen_transcript`) Codex draws
/// them in the middle of the screen, over the history and well above the rows the tail patterns
/// read, while the composer stays at the bottom. Enter picks the selected choice.
const CODEX_DIALOGS: [(&str, &str); 15] = [
    // chatwidget/goal_menu.rs: opened on its own when `codex resume` finds a paused goal.
    (r"Resume paused goal\?", "Resume goal|Leave paused"),
    // app/thread_goal_actions.rs
    (r"Replace goal\?", "Replace current goal|Cancel"),
    // chatwidget/misalignment_policy.rs
    (
        "Chat paused as a precaution",
        "Acknowledge findings and continue|Back",
    ),
    // chatwidget/safety_buffering.rs
    (
        r"Stop this attempt and retry\?",
        "Keep waiting|Stop and retry",
    ),
    // chatwidget.rs: the agent picker or the memories menu while that feature is off.
    (r"Enable (?:subagents|memories)\?", "Yes, enable|Not now"),
    // chatwidget/usage.rs
    (r"Use this reset\?", "Yes, use reset|No, go back"),
    // chatwidget/permission_popups.rs
    (r"Enable full access\?", "Yes, continue anyway|Cancel"),
    // chatwidget/worktree_picker.rs
    (r"Delete this worktree\?", "Cancel|Delete worktree"),
    // chatwidget/slash_dispatch.rs: /archive and /delete.
    (r"Archive this session\?", "No, don't archive|Yes, archive"),
    (
        r"Delete this session\?",
        "No, keep this session|Yes, delete and (?:exit|return to command center)",
    ),
    // chatwidget/plugin_catalog.rs: "Remove {name} marketplace?", which a long name wraps.
    (r"Remove [^\n]*", "Remove marketplace|Back to plugins"),
    // bottom_pane/feedback_view.rs
    (r"Upload logs\?", "Yes|No"),
    // app/daemon_menu.rs
    (r"Restart background server\?", "Cancel|Restart"),
    (r"Update daemon and exit Codex\?", "Cancel|Update and exit"),
    // app/agents_overview_actions.rs: the task's name, which can wrap, is quoted.
    (
        r"(?:Archive|Permanently delete) “[^\n]*",
        "Cancel|(?:Archive|Permanently delete) task and child agents",
    ),
];

/// A dialog drawn anywhere on the screen: its title alone on its row (the dialog's margin before
/// it, its blank width or the end of the row after it), then within 40 rows one of its numbered
/// choices, after the cursor or the dialog's margin and before its description column or the end
/// of the row. The words in a sentence are spaced singly and do not match.
fn dialog(title: &str, choices: &str) -> String {
    format!(
        r"(?m)[ \t]{{2}}(?:{title})(?:[ \t]{{2}}[^\n]*|[ \t]*)\n(?:[^\n]*\n){{0,40}}?[^\n]*?(?:›|[ \t])[ \t]\d+\.[ \t](?:{choices})(?:[ \t]{{2}}|[ \t]*$)"
    )
}
