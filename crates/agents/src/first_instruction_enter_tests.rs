//! The first instruction is pasted without the blank lines before it and the blanks after it, and
//! without Enter, and Enter goes in only once agy's box starts with it, or shows it as the
//! placeholder agy draws for a paste too large to show. It follows the user's answer to agy's
//! trust question in the same pane, where a key typed before the paste landed (`!`) would turn
//! the box into a shell command line. Left without Enter, the instruction is never sent again,
//! and the launch waits for an instruction as before the paste: idle at agy's prompt, told
//! whether the box showed the instruction, or as the output watcher judges the pane.
use super::antigravity_pane::fail_after_paste;
use super::*;
use crate::antigravity::box_tests::{PLACEHOLDER_ROW, long_line, placeholder_with, tall_prompt};
use crate::first_instruction::{LEFT_IN_BOX, NOT_SEEN};

/// The calls of a first instruction pasted and then left without Enter.
const NO_ENTER: [&str; 5] = [
    "list-sessions",
    "capture-pane",
    "load-buffer",
    "paste-buffer",
    "capture-pane",
];
/// The one-line prompt pasted for shell-mode-draft.txt and slash-draft.txt.
const ONE_LINE: &str = "Reply with OK and stop.";
/// The rows of draft.txt's box, between its two rules.
const DRAFT_ROWS: &str = "> Reply with OK and stop.\n  This is line two of the same pasted prompt.\n  Line three: also say how many lines this prompt had.\n";

/// draft.txt with the rows of its box replaced by `rows`. NOT a real capture: the recorded
/// screen changed in that one place.
fn draft_with(rows: &str) -> String {
    let draft = pane(capture!("draft"));
    assert!(draft.contains(DRAFT_ROWS), "the recorded box");
    draft.replacen(DRAFT_ROWS, rows, 1)
}

/// What devbox loaded into tmux's buffer for its last paste.
fn buffer() -> String {
    let live = std::env::var("DEVBOX_RESUME_READY_LIVE").expect("fake");
    fs::read_to_string(format!("{live}.buffer")).expect("the paste's buffer")
}

/// A queued first instruction `prompt`, sent at agy's empty box, whose paste leaves the pane
/// showing `after`: the calls it made, the task right after, and the task after a few more passes
/// over `then`.
async fn sent_with(
    prompt: &str,
    after: &str,
    then: &'static str,
) -> (Vec<String>, AgentTask, AgentTask) {
    let f = launched(Some(InstructionDelivery::Queued)).await;
    let prompt = prompt.to_owned();
    f.svc
        .update("a", move |task| {
            task.prompt = prompt;
            true
        })
        .await
        .expect("owned prompt");
    after_paste(after);
    let mut watcher = crate::watch::Watcher::new(&f.svc).expect("patterns");
    let start = Instant::now();
    settle(&f, &mut watcher, capture!("idle-after-trust"), start).await;
    let made = calls();
    let sent = f.task().await;
    // Never sent again, whatever the pane shows.
    show(then);
    for step in 2..6 {
        pass(&f, &mut watcher, then, start + QUIET * step).await;
    }
    assert_eq!(calls(), Vec::<String>::new());
    let later = f.task().await;
    f.close().await;
    (made, sent, later)
}

/// The task left at agy's prompt without Enter: waiting for the user, with `notice`, which says
/// whether the last read of the pane showed the box starting with the instruction.
fn left_at_prompt(task: &AgentTask, notice: &str) {
    assert_eq!(delivery(task), Some(InstructionDelivery::Unknown));
    assert_eq!(task.state, AgentState::Idle);
    let shown = task.attention.as_ref().map(|a| a.message.as_str());
    assert_eq!(shown, Some(notice));
    // The task's own record of it is all the user is shown: no follow-up was added.
    assert_eq!(task.follow_ups, []);
}

/// The task left without Enter where agy was not at rest at its prompt: the launch waits as
/// before the paste, with no notice, and the output watcher judges its pane.
fn waits_as_before(task: &AgentTask) {
    assert_eq!(delivery(task), Some(InstructionDelivery::Unknown));
    assert_eq!(task.state, AgentState::Running);
    assert_eq!(task.attention, None);
}

#[test]
fn the_box_starting_with_the_pasted_prompt_is_entered() {
    with_fake_tmux(
        "first_instruction::entering::the_box_starting_with_the_pasted_prompt_is_entered",
        || async {
            // NOT real captures, but the first: draft.txt's box with a one-line prompt, a first
            // line of Korean wider than the pane that agy wrapped, sixty lines in a box as tall
            // as the rows read allow, and a key typed after the paste below its first line.
            let one = draft_with("> Reply with OK and stop.\n");
            let korean = "가".repeat(150);
            let part = |from: usize, to: usize| -> String {
                korean.chars().skip(from).take(to - from).collect()
            };
            let wide = draft_with(&format!("> {}\n  {}\n", part(0, 99), part(99, 150)));
            let tall: Vec<String> = (1..=60)
                .map(|n| format!("Step {n} of a long task description."))
                .collect();
            let tall_rows: String = tall
                .iter()
                .enumerate()
                .map(|(at, line)| format!("{}{line}\n", if at == 0 { "> " } else { "  " }))
                .collect();
            let typed_after = draft_with(&DRAFT_ROWS.replacen("had.\n", "had.x\n", 1));
            for (prompt, after) in [
                (DRAFT.to_owned(), pane(capture!("draft")).to_owned()),
                (ONE_LINE.to_owned(), one),
                (korean.clone(), wide),
                (tall.join("\n"), draft_with(&tall_rows)),
                (DRAFT.to_owned(), typed_after),
            ] {
                let idle = capture!("idle-after-trust");
                let (made, task, _) = sent_with(&prompt, &after, idle).await;
                assert_eq!(made, FIRST_SEND, "{prompt:?}");
                assert_eq!(delivery(&task), Some(InstructionDelivery::Delivered));
                assert_eq!(task.state, AgentState::Running);
                assert_eq!(buffer(), prompt);
            }
        },
    );
}

#[test]
fn the_paste_keeps_the_first_lines_indentation_and_leaves_out_blank_lines_and_the_end() {
    use crate::first_instruction::to_paste;
    // Blank lines before the first line go, whatever blanks they hold, and the blanks and line
    // breaks at the end. The first line's own indentation stays, and a byte-order mark in front
    // of it goes.
    for (prompt, paste) in [
        ("fix it", "fix it"),
        ("\n\n  \t\nfix it\n  more", "fix it\n  more"),
        ("    key: value\n  child: 1", "    key: value\n  child: 1"),
        ("\n \n\t- item\n\t- item", "\t- item\n\t- item"),
        ("\r\n  \r\n  code()", "  code()"),
        ("\r  code()", "  code()"),
        ("\u{feff}\n  code()", "  code()"),
        ("\u{feff}  code()", "  code()"),
        ("fix it \n\n\t", "fix it"),
    ] {
        assert_eq!(to_paste(prompt), paste, "{prompt:?}");
    }
    // A paste that would end with a backslash gets a space after it: agy is never handed `\` and
    // then Enter, which some tools take for a line break.
    for (prompt, paste) in [
        ("fix it\\", "fix it\\ "),
        ("fix it\\ \n", "fix it\\ "),
        ("C:\\work\\", "C:\\work\\ "),
        ("\\\\", "\\\\ "),
        ("a \\ b", "a \\ b"),
    ] {
        assert_eq!(to_paste(prompt), paste, "{prompt:?}");
    }
}

#[test]
fn blank_lines_before_the_prompt_and_blanks_after_it_are_left_out_of_the_paste() {
    with_fake_tmux(
        "first_instruction::entering::blank_lines_before_the_prompt_and_blanks_after_it_are_left_out_of_the_paste",
        || async {
            // The box then starts with the prompt's first line, as draft.txt recorded it, and a
            // count agy shows is that of the text pasted.
            let prompt = format!("\n \n\t\n{DRAFT}\n \n\t");
            let idle = capture!("idle-after-trust");
            let (made, task, _) = sent_with(&prompt, pane(capture!("draft")), idle).await;
            assert_eq!(made, FIRST_SEND);
            assert_eq!(delivery(&task), Some(InstructionDelivery::Delivered));
            assert_eq!(buffer(), DRAFT);
            // The task keeps its prompt as it was written.
            assert_eq!(task.prompt, prompt);
            // A first line's own indentation is pasted, and the box shows it after its `> ` (NOT
            // a real capture: draft.txt with its rows changed).
            let yaml = "  steps:\n    - run: cargo test";
            let drawn = draft_with(">   steps:\n      - run: cargo test\n");
            let (made, task, _) = sent_with(&format!("\n\n{yaml}\n"), &drawn, idle).await;
            assert_eq!(made, FIRST_SEND);
            assert_eq!(delivery(&task), Some(InstructionDelivery::Delivered));
            assert_eq!(buffer(), yaml);
        },
    );
}

#[test]
fn a_paste_that_would_end_with_a_backslash_gets_a_space_after_it() {
    with_fake_tmux(
        "first_instruction::entering::a_paste_that_would_end_with_a_backslash_gets_a_space_after_it",
        || async {
            let idle = capture!("idle-after-trust");
            // The box starts with the prompt as before (NOT a real capture: draft.txt with its
            // rows changed), and Enter goes in after the space.
            let path = "Use the files under C:\\work\\";
            let drawn = draft_with(&format!("> {path} \n"));
            let (made, task, _) = sent_with(path, &drawn, idle).await;
            assert_eq!(made, FIRST_SEND);
            assert_eq!(delivery(&task), Some(InstructionDelivery::Delivered));
            assert_eq!(buffer(), format!("{path} "));
            // A placeholder's count is that of the text pasted, the space too (NOT real captures:
            // pasted-chars.txt with its count changed). The count of the prompt without it is
            // not the paste's.
            let line = format!("{}\\", &long_line()[..1499]);
            let chars = pane(capture!("pasted-chars"));
            let counted = placeholder_with(chars, "> [Pasted text #10 1501 chars]");
            let (made, task, _) = sent_with(&line, &counted, idle).await;
            assert_eq!(made, FIRST_SEND);
            assert_eq!(delivery(&task), Some(InstructionDelivery::Delivered));
            assert_eq!(buffer(), format!("{line} "));
            let (made, task, _) = sent_with(&line, chars, idle).await;
            assert_eq!(made, NO_ENTER);
            left_at_prompt(&task, NOT_SEEN);
        },
    );
}

#[test]
fn text_typed_in_front_of_the_paste_gets_no_enter_and_the_task_waits_for_the_user() {
    with_fake_tmux(
        "first_instruction::entering::text_typed_in_front_of_the_paste_gets_no_enter_and_the_task_waits_for_the_user",
        || async {
            // NOT real captures: draft.txt with keys typed before the paste landed, and with one
            // typed after it on the row of a one-line prompt.
            for (prompt, rows) in [
                (DRAFT, DRAFT_ROWS.replacen("> Reply", "> xReply", 1)),
                (DRAFT, DRAFT_ROWS.replacen("> Reply", "> x Reply", 1)),
                (ONE_LINE, "> Reply with OK and stop.x\n".to_owned()),
            ] {
                let idle = capture!("idle-after-trust");
                let (made, task, later) = sent_with(prompt, &draft_with(&rows), idle).await;
                assert_eq!(made, NO_ENTER, "{rows}");
                left_at_prompt(&task, NOT_SEEN);
                assert_eq!(later, task);
            }
        },
    );
}

#[test]
fn a_first_row_showing_too_little_of_the_line_gets_no_enter() {
    with_fake_tmux(
        "first_instruction::entering::a_first_row_showing_too_little_of_the_line_gets_no_enter",
        || async {
            // 15 characters of a longer first line (NOT a real capture): read again for a while,
            // and never more.
            let short = draft_with("> Reply with OK a\n");
            let idle = capture!("idle-after-trust");
            let (made, task, _) = sent_with(ONE_LINE, &short, idle).await;
            assert_eq!(made[..5], NO_ENTER);
            assert!(made.len() > NO_ENTER.len(), "{made:?}");
            assert!(
                made[5..].iter().all(|call| call == "capture-pane"),
                "{made:?}"
            );
            left_at_prompt(&task, NOT_SEEN);
        },
    );
}

#[test]
fn a_paste_agy_never_draws_gets_no_enter() {
    with_fake_tmux(
        "first_instruction::entering::a_paste_agy_never_draws_gets_no_enter",
        || async {
            // The box stays empty: the pane is read again for a while, and Enter never goes in.
            let idle = capture!("idle-after-trust");
            let (made, task, _) = sent_with(DRAFT, pane(idle), idle).await;
            assert_eq!(made[..5], NO_ENTER);
            assert!(made.len() > NO_ENTER.len(), "{made:?}");
            assert!(
                made[5..].iter().all(|call| call == "capture-pane"),
                "{made:?}"
            );
            left_at_prompt(&task, NOT_SEEN);
        },
    );
}

#[test]
fn a_shell_mode_or_slash_draft_after_the_paste_gets_no_enter() {
    with_fake_tmux(
        "first_instruction::entering::a_shell_mode_or_slash_draft_after_the_paste_gets_no_enter",
        || async {
            // The recorded pastes into a box in shell mode and after `/`: Enter would run the
            // prompt as a shell command, or as a command with the prompt as its argument.
            let bang = draft_with(&DRAFT_ROWS.replacen("> ", "! ", 1));
            let slash = draft_with(&DRAFT_ROWS.replacen("> ", "> /", 1));
            for (prompt, after) in [
                (ONE_LINE, pane(capture!("shell-mode-draft"))),
                (ONE_LINE, pane(capture!("slash-draft"))),
                (DRAFT, bang.as_str()),
                (DRAFT, slash.as_str()),
            ] {
                let idle = capture!("idle-after-trust");
                let (made, task, later) = sent_with(prompt, after, idle).await;
                assert_eq!(made, NO_ENTER, "{after}");
                // A draft agy's patterns name: the launch waits as before the paste, and once the
                // box shows empty the watcher makes it idle.
                waits_as_before(&task);
                assert_eq!(later.state, AgentState::Idle);
                assert_eq!(delivery(&later), Some(InstructionDelivery::Unknown));
            }
            // While the draft stays, the watcher shows the task waiting for the user.
            let draft = capture!("shell-mode-draft");
            let (_, _, later) = sent_with(ONE_LINE, pane(draft), draft).await;
            assert_eq!(later.state, AgentState::Waiting);
        },
    );
}

#[test]
fn a_prompt_or_turn_that_comes_up_after_the_paste_gets_no_enter() {
    with_fake_tmux(
        "first_instruction::entering::a_prompt_or_turn_that_comes_up_after_the_paste_gets_no_enter",
        || async {
            // At these the paste is dropped and Enter would answer "Yes", or agy works on
            // something typed first. The launch waits as before the paste, with no notice, and the
            // watcher judges the pane.
            for after in [
                capture!("trust"),
                capture!("approval"),
                capture!("login"),
                capture!("selection-model"),
                capture!("working"),
            ] {
                let idle = capture!("idle-after-trust");
                let (made, task, later) = sent_with(DRAFT, pane(after), idle).await;
                assert_eq!(made, NO_ENTER, "{after}");
                waits_as_before(&task);
                assert_eq!(later.state, AgentState::Idle);
            }
        },
    );
}

#[test]
fn a_pane_that_cannot_be_read_after_the_paste_gets_no_enter() {
    with_fake_tmux(
        "first_instruction::entering::a_pane_that_cannot_be_read_after_the_paste_gets_no_enter",
        || async {
            fail_after_paste("capture-pane");
            let f = launched(Some(InstructionDelivery::Queued)).await;
            let mut watcher = crate::watch::Watcher::new(&f.svc).expect("patterns");
            let start = Instant::now();
            settle(&f, &mut watcher, capture!("idle-after-trust"), start).await;
            assert_eq!(calls(), NO_ENTER);
            // What the pane shows is not known: the launch waits as before the paste.
            waits_as_before(&f.task().await);
            assert!(f.binding().await.resumed_session);
            fail_tmux(None);
            show(capture!("idle-after-trust"));
            for step in 2..6 {
                let at = start + QUIET * step;
                pass(&f, &mut watcher, capture!("idle-after-trust"), at).await;
            }
            assert_eq!(calls(), Vec::<String>::new());
            assert_eq!(f.task().await.state, AgentState::Idle);
            f.close().await;
        },
    );
}

#[test]
fn a_placeholder_of_the_pasted_prompt_is_entered() {
    with_fake_tmux(
        "first_instruction::entering::a_placeholder_of_the_pasted_prompt_is_entered",
        || async {
            // Recorded: agy shows a prompt of 45 lines, and a line of 1500 characters, by its
            // count alone. pasted-chars.txt has a turn above the box whose prompt row is out of
            // the rows read, as after a long answer.
            let idle = capture!("idle-after-trust");
            for (prompt, after) in [
                (tall_prompt(), pane(capture!("pasted-lines"))),
                (long_line(), pane(capture!("pasted-chars"))),
            ] {
                let (made, task, _) = sent_with(&prompt, after, idle).await;
                assert_eq!(made, FIRST_SEND);
                assert_eq!(delivery(&task), Some(InstructionDelivery::Delivered));
                assert_eq!(task.state, AgentState::Running);
                assert_eq!(buffer(), prompt);
            }
        },
    );
}

#[test]
fn a_placeholder_of_another_count_or_after_a_key_gets_no_enter() {
    with_fake_tmux(
        "first_instruction::entering::a_placeholder_of_another_count_or_after_a_key_gets_no_enter",
        || async {
            let idle = capture!("idle-after-trust");
            let lines = pane(capture!("pasted-lines"));
            // The recorded placeholders after a prompt of another count, a line fewer or a
            // character more, and (NOT a real capture) pasted-lines.txt with a key typed in front
            // of the paste.
            let tall = tall_prompt();
            let fewer = tall.rsplit_once('\n').expect("lines").0;
            let more = format!("{}x", long_line());
            let typed = placeholder_with(lines, "> x[Pasted text #1 +45 lines]");
            for (prompt, after) in [
                (fewer, lines),
                (more.as_str(), pane(capture!("pasted-chars"))),
                (tall.as_str(), typed.as_str()),
            ] {
                let (made, task, later) = sent_with(prompt, after, idle).await;
                assert_eq!(made, NO_ENTER, "{after}");
                left_at_prompt(&task, NOT_SEEN);
                assert_eq!(later, task);
            }
            // NOT a real capture: `!` typed before the paste puts the box in shell mode, which
            // agy's patterns name.
            let bang = placeholder_with(lines, "! [Pasted text #1 +45 lines]");
            let (made, task, later) = sent_with(&tall, &bang, idle).await;
            assert_eq!(made, NO_ENTER);
            waits_as_before(&task);
            assert_eq!(later.state, AgentState::Idle);
        },
    );
}

#[test]
fn a_paste_under_an_earlier_prompt_gets_no_enter_and_the_notice_says_whether_the_box_has_it() {
    with_fake_tmux(
        "first_instruction::entering::a_paste_under_an_earlier_prompt_gets_no_enter_and_the_notice_says_whether_the_box_has_it",
        || async {
            // idle-after-user-turn.txt: a prompt agy took earlier above its empty box. Enter would
            // send the paste after a prompt devbox never saw.
            let turn = pane(capture!("idle-after-user-turn"));
            assert_eq!(turn.matches("\n>\n").count(), 1, "the recorded empty box");
            let idle = capture!("idle-after-trust");
            let (made, task, later) = sent_with(DRAFT, turn, idle).await;
            assert_eq!(made, NO_ENTER);
            left_at_prompt(&task, NOT_SEEN);
            assert_eq!(later, task);
            // NOT real captures: that screen with the paste in its box, as text and as a
            // placeholder. The notice says it is there.
            let with = |row: &str| turn.replacen("\n>\n", &format!("\n{row}\n"), 1);
            for (prompt, after) in [
                (ONE_LINE.to_owned(), with("> Reply with OK and stop.")),
                (tall_prompt(), with(PLACEHOLDER_ROW)),
            ] {
                let (made, task, later) = sent_with(&prompt, &after, idle).await;
                assert_eq!(made, NO_ENTER, "{prompt:?}");
                left_at_prompt(&task, LEFT_IN_BOX);
                assert_eq!(later, task);
            }
        },
    );
}
