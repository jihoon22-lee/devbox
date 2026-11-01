//! Waiting patterns that come with a tool family, the `hooks` of a profile. They mark the
//! prompts that stop the tool until the user decides, so every profile of the family gets them,
//! whatever its id, and a profile's own `waitingPatterns` add to them (see `profile::compiled`).
//!
//! Codex 승인·결정 화면, 확인 상자, 입력 상자·목록·설정의 힌트 줄, 입력창의 셸 모드·하위 에이전트·/side
//! 표시와 화면을 덮는 보기는 0.164.0~0.166.0 공개 소스가 그리는 화면(소스 기반, 실제 화면 아님),
//! 폴더 신뢰 화면은 0.164.0 실제 녹화와 0.166.0 소스, hook 검토 화면은 0.164.0 실행 파일 문자열(실제 화면
//! 아님)이다. Claude 폴더 신뢰 대화는 설치본 2.1.308의 코드에서, opencode 셸 모드 힌트는 설치본 1.19.3의
//! 코드에서 읽은 문자열 기반이다. 다른 UI 유형·버전은 실제 fixture로 보강한다.
use crate::profile::Hooks;

/// The waiting patterns of one family, before they are compiled.
#[derive(Debug, Default)]
pub(crate) struct FamilyPatterns {
    /// Read over the last 6 non-empty rows (see `watch::matches_waiting`).
    pub tail: &'static [&'static str],
    /// Read over the whole capture.
    pub screen: Vec<String>,
}

pub(crate) fn family_patterns(hooks: Hooks) -> FamilyPatterns {
    match hooks {
        Hooks::Claude => FamilyPatterns {
            tail: CLAUDE,
            screen: Vec::new(),
        },
        Hooks::Codex => FamilyPatterns {
            tail: CODEX,
            screen: CODEX_DIALOGS
                .iter()
                .map(|(title, choices)| dialog(title, choices))
                .chain(CODEX_SCREENS.iter().map(|&pattern| pattern.to_owned()))
                .collect(),
        },
        Hooks::Pi => FamilyPatterns {
            tail: PI,
            screen: Vec::new(),
        },
        Hooks::Opencode => FamilyPatterns {
            tail: OPENCODE,
            screen: Vec::new(),
        },
        Hooks::None => FamilyPatterns::default(),
    }
}

/// Claude Code. Its hooks report its waits, but none reports its folder-trust dialog, which
/// comes before any of them.
const CLAUDE: &[&str] = &[
    // Folder trust. Evidence: crates/agents/tests/fixtures/claude-2.1.308/trust.txt (문자열 기반:
    // the dialog's code in the installed 2.1.308, not a real capture). The declining choice comes
    // first and holds the cursor, the choices have no number, and the accessible layout prints
    // them as "y. …" and "n. …".
    r"(?m)^\s*(?:[❯›>]\s*)?(?:(?:\d+|[yn])[.)]\s+)?(?:Yes, I trust this folder|No, continue without these permissions)",
];

/// Codex. Its `notify` reports only finished turns, so every prompt is matched on screen.
const CODEX: &[&str] = &[
    // Prompts that stop Codex until the user decides, and the views where a send would not
    // reach Codex as a prompt: it is pasted into the view or dropped, and Enter then acts on the
    // view. Evidence: crates/agents/tests/fixtures/codex-0.166.0/ (소스 기반: drawn by the TUI
    // source of rust-v0.164.0 to rust-v0.166.0, the same in all three; not real captures). The
    // question and long choices can leave the matched tail, so each pattern reads rows only such
    // a prompt draws, whatever their number, order or cursor, and none needs the footer.
    // Matching too much only holds sends back while it shows.
    // 1. The footer of a choice list or a form Enter would answer: the approvals of commands,
    //    network hosts, permissions, edits and terminal input, MCP requests and user
    //    verification, whatever their choices are called. "to submit" is the feedback note's,
    //    where Enter uploads the feedback with what the note holds, a pasted send included.
    //    "to save" and "to continue" are the sign-in steps' (Enter saves a pasted send as the
    //    API key, or goes on with the selected way to sign in) and the setup import's. Codex
    //    names the key in lower case ("enter", "f3"), so "Press Enter to continue" in an agent's
    //    reply or a command's output is not it.
    r"(?m)^\s*Press [^A-Z\s\[]\S* to (?:confirm|submit|save|continue)\b",
    // 2. Their choices.
    r"(?m)^\s*(?:[›>]\s*)?(?:\d+[.)]\s+)?(?:Yes, proceed|Yes, just this once|Yes, and (?:don't ask again|allow this host|allow these permissions)|Yes, grant (?:these permissions|for this turn)|Yes, provide the requested info|Verify and approve|No, and tell Codex|No, and block this host|No, continue without (?:running it|permissions)|No, but continue without it|Cancel this request)",
    // 3. MCP forms and the model's questions: tool-call choices, and the footers. The tips share
    //    a row with " | " between them; a footer that wraps can end a row with one.
    r"(?m)^\s*(?:[›>]\s*)?(?:\d+[.)]\s+)?(?:Allow|Allow for this session|Always allow|Deny|Cancel)\s+(?:Run the tool and|Allow this request and|Decline this request and continue|Cancel this (?:tool call|request))",
    r"(?m)\bto submit(?: all| answer)?(?: \||[ \t]*$)",
    // 4. An MCP URL request: "tab / ↑ ↓ move · enter select · esc close".
    r"(?m)\bmove · \S+ select\b",
    // 5. Choices at start: an update (Enter runs the installer), a new model, the folder to
    //    resume in, an archived conversation, a background server that does not fit.
    r"(?m)^\s*(?:[›>]\s*)?(?:\d+[.)]\s+)?(?:Update now \(runs |Skip until next version|Try new model|Use existing model|(?:Use|Always use) (?:session|current) directory|Unarchive and |Run without daemon this time|Restart with these settings)",
    // 6. Lists Codex opens on its own when a turn ends or stalls: implement the plan (Enter
    //    starts coding), switch model near a rate limit, turn on Daybreak after a refusal, a chat
    //    paused as a precaution, retry with a faster model.
    r"(?m)^\s*(?:[›>]\s*)?(?:\d+[.)]\s+)?(?:Yes, implement this plan|Yes, clear context and implement|No, stay in Plan mode|Keep current model|Enable Daybreak|Acknowledge findings|Resume another chat|Retry with a faster model|Dismiss and keep waiting|Stop and retry)",
    // 7. The footer of any other list ("enter select · esc back"): Enter answers the list,
    //    whoever opened it.
    r"(?m)^\s*\S+ (?:select|choose|confirm|open) · \S+ back\s*$",
    // 8. Any other hint row that offers Enter next to another hint after " · ", as Codex draws
    //    them under its text prompts, lists, pickers, settings and forms, the prompt box's
    //    popups, the usage report and the transcript's search ("enter submit · esc back",
    //    "space toggle · enter save · esc cancel", "reverse-i-search: git  enter accept · esc
    //    cancel"; form-*.txt). Only the key's own name counts, alone or joined by "/" to other
    //    key names, so a path or a word that ends in "enter" does not. "Browsing · ↵ rewind" is
    //    left out: in the full-screen transcript a paste leaves the browsing for the prompt box,
    //    and the pager's browsing is matched by its top row (`CODEX_SCREENS`).
    r"(?m)(?:^[ \t]*|[ \t]{2}|[ \t]·[ \t])(?:[a-z]+/)*enter(?:/[a-z]+)*[ \t][^·\n]*?[ \t]·(?:[ \t]|$)|[ \t]·[ \t](?:[a-z]+/)*enter(?:/[a-z]+)*[ \t][^\s·][^·\n]*$",
    // 9. The rows that space their hints by two blanks: the model's questions ("enter submit
    //    ⌃] skip", Enter answers the question), the resume picker ("enter resume  esc start
    //    new") and the agent command center ("esc back  ↑/↓ move  enter open").
    r"(?m)(?:^[ \t]*|[ \t]{2})enter[ \t](?:to[ \t])?(?:submit|resume|fork|open|rename)(?:[ \t]{2}|[ \t]*$)",
    // 10. Views whose hint rows leave Enter out, where a send is dropped and Enter acts or does
    //     nothing: a row that only leaves ("esc close" under a plugin, whose Enter installs it;
    //     "esc back"), the hooks of an event that need review or are managed, a shortcut being
    //     remapped (Enter becomes its key), the setup import from another agent (its key names
    //     in lower case, as in item 1), the startup warnings and the usage report.
    r"(?m)^[ \t]*esc (?:back|close|clear search)[ \t]*$",
    r"(?m)^[ \t]*(?:t trust|Managed \(always on\)) · esc back[ \t]*$",
    r"(?m)^[ \t]*(?:Press the |First key: )[^\n]*\. Esc cancels\.[ \t]*$",
    r"(?m)^\s*Use \S+ to move, [^A-Z\s\[]\S* to (?:select|toggle)\b",
    r"(?m)(?:^|[ \t])k keep & next\b|\bdismiss & close\b",
    r"(?m)\btab(?:/\S+)? report · z \S",
    // 11. States of the prompt box itself, where Enter would not hand a send to the task's agent
    //     as a prompt (lines of rust-v0.166.0). A draft that starts with "!" is in shell mode:
    //     the send is pasted after it, and Enter runs both as a local shell command
    //     (chatwidget/input_submission.rs:256-266). The footer then ends with "Shell mode"
    //     (bottom_pane/chat_composer.rs:3639-3647). In a sub-agent's thread the empty prompt box
    //     reads "Viewing sub-agent — direct input is disabled", and Enter keeps the send unsent
    //     (chat_composer.rs:1713-1716, 3344-3361). A /side conversation's footer reads "Side
    //     from main thread" or "Side from parent thread (…)", alone or at the right end after the
    //     left hints: the send goes to a side thread that is told not to go on with the task and
    //     is discarded when closed (app/side.rs).
    r"(?m)[ \t]{2}Shell mode[ \t]*$",
    r"(?m)Viewing sub-agent — direct input is disabled",
    r"(?m)(?:^[ \t]*|[ \t]{2})Side from (?:main thread|parent thread \()",
    // Folder trust. Evidence: crates/agents/tests/fixtures/codex-0.164.0/trust.txt (real
    // capture), and for a folder saved as untrusted or a task started while it was trusted,
    // codex-0.166.0/trust-restricted.txt and trust-existing-task.txt (소스 기반:
    // onboarding/trust_directory.rs of rust-v0.164.0 to rust-v0.166.0). Codex wraps the
    // explanation to the terminal width, which can push it out of the matched tail, so this
    // anchors on the first choice row, always among the last rows.
    r"(?im)^\s*(?:[›>]\s*)?(?:\d+[.)]\s+)?(?:Trust and continue|Open restricted|Open existing task)\s*$",
    // Hooks review. Evidence: crates/agents/tests/fixtures/codex-0.164.0/hooks-review.txt
    // (문자열 기반: sentences from the 0.164.0 binary, not a real capture). Either choice row marks
    // the prompt; "N hooks need review before they can run." is only a notice.
    r"(?im)^\s*(?:[›>]\s*)?(?:\d+[.)]\s+)?(?:Trust all and continue|Continue without trusting \(hooks won['’]t run\))",
];

/// pi. Its extension reports the folder trust question and extension dialogs. The trust
/// question is matched on screen as well: a send taken without the extension's ready report,
/// 30 s after the launch, must not answer it.
const PI: &[&str] = &[
    // Folder trust. Evidence: crates/agents/tests/fixtures/pi-1.3.0/trust.txt (real capture).
    // The question, the folder rows and the explanation can leave the matched tail, and `→` marks
    // the selected choice, so this anchors on the last two choice rows, with or without it.
    r"(?im)^\s*(?:→\s*)?Do not trust\s*\n\s*(?:→\s*)?Do not trust \(this session only\)\s*$",
];

/// opencode. Its plugin reports its permission prompts and questions, but not a draft the user
/// left in shell mode, where Enter would run the draft and a send pasted after it as a shell
/// command.
const OPENCODE: &[&str] = &[
    // Shell mode. Evidence: crates/agents/tests/fixtures/opencode-1.19.3/shell-mode.txt (문자열
    // 기반: the TUI code in the installed 1.19.3, not a real capture). While the prompt box is in
    // shell mode its hint row reads "esc exit shell mode", which the TUI draws nowhere else. Where
    // that row sits is not known, so only the words are matched.
    r"\besc exit shell mode\b",
];

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

/// Views that cover the screen and read no prompt, by rows they always draw. The pagers Codex
/// opens over the history (the transcript, a diff, an approval's whole patch, command,
/// permissions, request or verification, a paused chat's findings) name themselves on the top
/// row: a send is dropped there, and while the transcript browses prompts Enter rewinds the
/// conversation to the selected one. The keypress inspector only shows the keys it gets.
/// Evidence: crates/agents/tests/fixtures/codex-0.166.0/overlay-*.txt (소스 기반, see their
/// headers).
const CODEX_SCREENS: &[&str] = &[
    r"(?m)^/ (?:T R A N S C R I P T|D I F F|P A T C H|E X E C|P E R M I S S I O N S|E L I C I T A T I O N|U S E R  V E R I F I C A T I O N|What we detected)[ /]*$",
    r"(?m)^[ \t]*Keypress Inspector[ \t]*\n[ \t]*Press any key to see what Codex receives\b",
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
