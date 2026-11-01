//! Codex views where a send would not reach the prompt: the text prompts, forms, pickers,
//! settings and lists whose hint rows offer Enter for the view itself, and the views that cover
//! the screen. A send is pasted into the view, or dropped, and its Enter then acts on the view,
//! so each must read as waiting; the rows where a send still reaches the prompt must not. So must
//! the prompt box's own states where a send does not reach the task's agent as a prompt: a draft
//! in shell mode, a sub-agent's thread and a /side conversation.
use super::*;

/// Views read over the last rows, by their hint rows, as the Codex TUI source of rust-v0.164.0
/// to rust-v0.166.0 draws them (see the fixtures' headers).
const CODEX_FORMS: [(&str, &str); 26] = [
    (
        "form-custom-prompt",
        include_str!("../tests/fixtures/codex-0.166.0/form-custom-prompt.txt"),
    ),
    (
        "form-experimental-features",
        include_str!("../tests/fixtures/codex-0.166.0/form-experimental-features.txt"),
    ),
    (
        "form-hooks",
        include_str!("../tests/fixtures/codex-0.166.0/form-hooks.txt"),
    ),
    (
        "form-status-line",
        include_str!("../tests/fixtures/codex-0.166.0/form-status-line.txt"),
    ),
    (
        "form-skills",
        include_str!("../tests/fixtures/codex-0.166.0/form-skills.txt"),
    ),
    (
        "form-settings",
        include_str!("../tests/fixtures/codex-0.166.0/form-settings.txt"),
    ),
    (
        "form-keymap",
        include_str!("../tests/fixtures/codex-0.166.0/form-keymap.txt"),
    ),
    (
        "form-keymap-capture",
        include_str!("../tests/fixtures/codex-0.166.0/form-keymap-capture.txt"),
    ),
    (
        "form-plugins",
        include_str!("../tests/fixtures/codex-0.166.0/form-plugins.txt"),
    ),
    (
        "form-plugin-detail",
        include_str!("../tests/fixtures/codex-0.166.0/form-plugin-detail.txt"),
    ),
    (
        "form-reasoning",
        include_str!("../tests/fixtures/codex-0.166.0/form-reasoning.txt"),
    ),
    (
        "form-memories",
        include_str!("../tests/fixtures/codex-0.166.0/form-memories.txt"),
    ),
    (
        "form-question",
        include_str!("../tests/fixtures/codex-0.166.0/form-question.txt"),
    ),
    (
        "form-command-center",
        include_str!("../tests/fixtures/codex-0.166.0/form-command-center.txt"),
    ),
    (
        "form-history-search",
        include_str!("../tests/fixtures/codex-0.166.0/form-history-search.txt"),
    ),
    (
        "form-mention",
        include_str!("../tests/fixtures/codex-0.166.0/form-mention.txt"),
    ),
    (
        "form-warnings",
        include_str!("../tests/fixtures/codex-0.166.0/form-warnings.txt"),
    ),
    (
        "form-usage-report",
        include_str!("../tests/fixtures/codex-0.166.0/form-usage-report.txt"),
    ),
    (
        "startup-resume",
        include_str!("../tests/fixtures/codex-0.166.0/startup-resume.txt"),
    ),
    (
        "startup-import",
        include_str!("../tests/fixtures/codex-0.166.0/startup-import.txt"),
    ),
    (
        "signin-api-key",
        include_str!("../tests/fixtures/codex-0.166.0/signin-api-key.txt"),
    ),
    (
        "signin-gov-cloud",
        include_str!("../tests/fixtures/codex-0.166.0/signin-gov-cloud.txt"),
    ),
    (
        "form-shell-mode",
        include_str!("../tests/fixtures/codex-0.166.0/form-shell-mode.txt"),
    ),
    (
        "form-subagent-thread",
        include_str!("../tests/fixtures/codex-0.166.0/form-subagent-thread.txt"),
    ),
    (
        "form-side-thread",
        include_str!("../tests/fixtures/codex-0.166.0/form-side-thread.txt"),
    ),
    (
        "form-side-thread-status-line",
        include_str!("../tests/fixtures/codex-0.166.0/form-side-thread-status-line.txt"),
    ),
];

/// Views that cover the screen, read over the whole capture by a row they always draw.
const CODEX_OVERLAYS: [(&str, &str); 3] = [
    (
        "overlay-transcript-browsing",
        include_str!("../tests/fixtures/codex-0.166.0/overlay-transcript-browsing.txt"),
    ),
    (
        "overlay-patch",
        include_str!("../tests/fixtures/codex-0.166.0/overlay-patch.txt"),
    ),
    (
        "overlay-keypress-inspector",
        include_str!("../tests/fixtures/codex-0.166.0/overlay-keypress-inspector.txt"),
    ),
];

#[test]
fn every_codex_form_that_enter_would_submit_is_waiting() {
    let patterns = codex_patterns();
    let missed: Vec<&str> = CODEX_FORMS
        .iter()
        .chain(&CODEX_OVERLAYS)
        .filter(|(_, fixture)| !matches_waiting(&patterns, screen(fixture)))
        .map(|(name, _)| *name)
        .collect();
    assert!(missed.is_empty(), "not waiting: {missed:?}");
}

#[test]
fn a_codex_form_with_newer_output_below_it_is_not_waiting() {
    let patterns = codex_patterns();
    for (name, fixture) in CODEX_FORMS {
        let stale = format!("{}\nworking\n1\n2\n3\n4\n5\n6", screen(fixture));
        assert!(!matches_waiting(&patterns, &stale), "{name}");
    }
}

#[test]
fn a_codex_text_prompt_is_waiting_whatever_it_holds_or_its_escape_does() {
    let patterns = codex_patterns();
    // bottom_pane/custom_prompt_view.rs: Edit goal, the review instructions, Save conversation,
    // rename, the quick-dial prompts and Add marketplace. A send is pasted into it, and Enter
    // sets the goal, starts the review, saves the file, renames the thread, saves the preset or
    // adds the marketplace.
    let goal = screen(include_str!(
        "../tests/fixtures/codex-0.166.0/form-custom-prompt.txt"
    ));
    let pasted = goal.replacen(
        "› Check the parser and preserve 日 本\n  語 .",
        "› owned-pane-check-instruction\n  second line",
        1,
    );
    assert_ne!(pasted, goal);
    assert!(matches_waiting(&patterns, &pasted));
    // The other footers of vim_prompt_hint_tracks_escape_behavior.snap, and the labels
    // chatwidget/quick_dials.rs gives the box.
    for footer in [
        "  enter submit · esc back",
        "  enter submit · esc normal mode                                 Vim: Insert",
        "  enter submit · esc back                                        Vim: Normal",
        "  enter submit · esc normal mode                                Vim: Replace",
        "  enter choose slot · esc back",
        "  enter review changes · esc back",
    ] {
        let shown = goal.replacen("  enter submit · esc normal mode", footer, 1);
        assert!(matches_waiting(&patterns, &shown), "{footer}");
    }
}

#[test]
fn every_codex_hint_row_that_offers_enter_to_the_view_is_waiting() {
    let patterns = codex_patterns();
    let working = screen(include_str!("../tests/fixtures/codex-0.164.0/working.txt"));
    // Rows as the snapshots of rust-v0.164.0 to rust-v0.166.0 draw them (the test labels that
    // some snapshots put before a row are left out), each under the history. The remap view's
    // chord and leader rows are its other instructions in keymap_setup/capture.rs, and the
    // warnings row with "↓ scroll" is the one rust-v0.164.0 and rust-v0.165.0 draw.
    let mut missed = Vec::new();
    for row in [
        // Lists, pickers and settings: Enter saves, toggles, applies or opens.
        "  space toggle · ←/→ reorder · enter save · esc cancel",
        "  space toggle · enter save/select · esc cancel",
        "  space/enter toggle · esc close",
        "  t trust all · enter review · esc close",
        "  enter details · esc close",
        "  enter apply · s session · esc back",
        "  ⌃u upgrade · ⌃r remove · tab toggle · ←/→ tabs · enter details · esc close",
        "  left/right group · enter edit · esc close",
        "  left/right group · / search · enter open · esc clear search/close",
        "  enter toggle · esc back",
        // Startup and sign-in steps.
        "  enter continue · esc cancel",
        "  enter continue · esc skip",
        "  enter/esc confirm · ⌃c quit",
        "  ↑/↓ choose · enter select · esc LM Studio · ⌃c exit",
        "  enter confirm · esc skip",
        "  enter continue and create sandbox · esc quit",
        "  Press enter to continue",
        "  Press enter to save",
        "  Press enter to continue, ↑/↓ to move, b to go back",
        "  Use ↑/↓ to move, enter to select, c to customize",
        "  Use ↑/↓ to move, space to toggle, b to go back",
        // The prompt box's popups: Enter inserts or takes the entry back, it does not send.
        "  enter insert · esc close",
        "  reverse-i-search: e  enter accept · esc cancel",
        // The usage report, and the transcript's search and selection.
        " Dashboard · enter focus · r 7d/30d · Aug 27–Sep 2, 2026",
        " ↑/↓ row · enter details · esc back",
        " tab report · z all · ? · R refresh",
        "enter accept · ⌃p older · ⌃n newer · full transcript · esc cancel",
        "⌃c copy · enter copy & follow · esc clear",
        "y/⌃c copy · enter copy & latest · esc clear · q close",
        "arrows/hjkl move · v/space/V mark · / find · enter/q/esc close",
        // Two blanks between hints: the model's questions, the resume picker, the command center.
        "  enter submit   ⌃] skip   ⇧→ main prompt",
        "  enter submit",
        "enter resume   esc start new   ⌃c quit   tab focus sort/filter   ←/→ change option",
        "enter fork   esc new   ⌃c quit",
        "  esc back  enter rename",
        "  esc back  enter open",
        // Rows that leave Enter out, where a send is lost or Enter becomes a key.
        "  esc back",
        "  esc close",
        "  esc clear search",
        "  t trust · esc back",
        "  Managed (always on) · esc back",
        "  Press the first key, then the second. Esc cancels.",
        "  First key: ctrl-x. Press the second key. Esc cancels.",
        "  Press the key that follows the configured leader. Esc cancels.",
        "  k keep & next · esc dismiss & close · ⌃o copy · ←/→ warning · ↓ scroll",
        "  k keep & next · ⌃o copy",
    ] {
        if !matches_waiting(&patterns, &format!("{working}\n{row}")) {
            missed.push(row);
        }
    }
    assert!(missed.is_empty(), "not waiting: {missed:#?}");
}

#[test]
fn a_codex_view_that_covers_the_screen_holds_sends_while_it_shows() {
    let patterns = codex_patterns();
    // The history the pagers draw over on the alternate screen.
    let history = screen(include_str!(
        "../tests/fixtures/codex-0.166.0/fullscreen-history.txt"
    ));
    assert!(!matches_waiting(&patterns, history));
    // Every pager names itself on its top row (app/event_dispatch.rs, misalignment_policy.rs),
    // the rest of the row filled with "/ ".
    for title in [
        "T R A N S C R I P T",
        "D I F F",
        "P A T C H",
        "E X E C",
        "P E R M I S S I O N S",
        "E L I C I T A T I O N",
        "U S E R  V E R I F I C A T I O N",
        "What we detected",
    ] {
        let pager = format!(
            "/ {title} / / / / /\nline 1\nline 2\n ↑/↓ to scroll · pgup/pgdn to page · home/end to jump\n q close\n"
        );
        assert!(matches_waiting(&patterns, &pager), "{title}");
    }
    // The transcript's prompt browsing: Enter would rewind the conversation to the selected
    // prompt. Its 32-column footer is shorter.
    let browsing = screen(include_str!(
        "../tests/fixtures/codex-0.166.0/overlay-transcript-browsing.txt"
    ));
    let narrow = browsing.replacen(
        "Browsing · ↑↓/jk scroll · ←→/hl prompts · ⌃t details · ↵ rewind · esc back",
        "Browsing · ↵ rewind · esc back",
        1,
    );
    assert_ne!(narrow, browsing);
    assert!(matches_waiting(&patterns, &narrow));
    // A pager's name in the history, or the inspector's words in a sentence, are not the view.
    for line in [
        "/ D I F F of the change",
        "  / T R A N S C R I P T",
        "• Keypress Inspector shows what Codex receives.",
    ] {
        assert!(
            !matches_waiting(&patterns, &format!("{history}\n{line}")),
            "{line}"
        );
    }
}

#[test]
fn a_codex_screen_where_a_send_still_reaches_the_prompt_is_not_waiting() {
    let patterns = codex_patterns();
    let working = screen(include_str!("../tests/fixtures/codex-0.164.0/working.txt"));
    let mut held = Vec::new();
    for row in [
        // The prompt box's footers (bottom_pane/footer.rs).
        "  ? for shortcuts · Plan mode (⇧tab to cycle)                        100% context left",
        "  tab to queue message · Plan mode                                    98% context left",
        "  esc esc to edit previous message",
        "  gpt-test default · /tmp/project",
        // Paths and names that end in "enter".
        "  gpt-test default · ~/src/enter",
        "  gpt-test default · /srv/enter",
        "  gpt-test default · data-enter · feature/enter",
        // The shortcut help, which a paste closes before it reaches the prompt box.
        "  ⇧enter  New line             f8       Voice",
        // The banner above the prompt box: it takes digits and Esc only while the prompt box is
        // empty, so a paste goes to the prompt box (bottom_pane/mod.rs).
        "  Press a number to choose · esc to dismiss · type to continue",
        // The quick-dial menu, which a paste closes (chatwidget/interaction.rs).
        "  n · new   s · from settings   e · edit   d · delete   esc · close",
        // A prompt typed before the session is ready is sent once it is (startup_draft_input.rs).
        "  Waiting for startup  · esc cancel",
        // The full-screen transcript's own rows: a paste leaves the prompt browsing and reaches
        // the prompt box (app.rs), and "esc latest" only scrolls.
        "  Browsing · ↑↓/jk scroll · ←→/hl prompts · ⌃t details · ↵ rewind · esc back",
        "  New activity · esc latest",
        "  ↑ Loading earlier messages… · New activity · esc latest",
        // A working turn's status row.
        "• Working (0s • esc to interrupt) · 1 background terminal running · /ps to view · /stop to close",
        // A hidden /side conversation: its label only offers to switch to it, and a paste goes to
        // the main thread (side_context_label_shows_hidden_side.snap).
        "  GPT-5.6-Sol                                                  ctrl+/ for side",
        // The words in the history.
        "• Press enter to continue once the build finishes.",
        "• Use ↑/↓ to move, enter to select, as the picker says.",
        "  then enter submit to send the form",
        "• Press the new key now. Esc cancels. is what the remap view says.",
        "• Typing ! first switches the prompt to Shell mode",
        "  Shell mode runs the command in your shell.",
        // Instructions in an agent's reply, a code block or a command's output: Codex draws its
        // key names in lower case ("Press enter to continue", "enter to select").
        "  Press Enter to continue, or Ctrl-C to stop.",
        "  Press Enter to continue",
        "    Press Enter to continue",
        "  Press Ctrl+S to save, then reload the page.",
        "  Use ↑/↓ to move, Enter to select.",
    ] {
        if matches_waiting(&patterns, &format!("{working}\n{row}")) {
            held.push(row);
        }
    }
    assert!(held.is_empty(), "waiting: {held:#?}");
}
