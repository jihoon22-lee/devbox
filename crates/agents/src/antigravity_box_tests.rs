//! What agy's prompt box shows once devbox pasted its first instruction there without Enter: the
//! box's input row has to start with the prompt. The screens are real ones of agy 1.5.17,
//! redacted (`tests/fixtures/antigravity-1.5.17/README.md`); a screen changed in one place says so
//! where it is made.
use super::*;

fn screen(fixture: &'static str) -> &'static str {
    fixture
        .split_once("\n---\n")
        .expect("fixture header ends with a --- line")
        .1
}
macro_rules! capture {
    ($name:literal) => {
        screen(include_str!(concat!(
            "../tests/fixtures/antigravity-1.5.17/",
            $name,
            ".txt"
        )))
    };
}
/// The three-line prompt pasted for draft.txt, as its hooks reported it (`lastUserInput`).
const DRAFT: &str = "Reply with OK and stop.\nThis is line two of the same pasted prompt.\nLine three: also say how many lines this prompt had.";
/// The one-line prompt pasted for shell-mode-draft.txt and slash-draft.txt.
const ONE_LINE: &str = "Reply with OK and stop.";
/// The rows of draft.txt's box, between its two rules.
const DRAFT_ROWS: &str = "> Reply with OK and stop.\n  This is line two of the same pasted prompt.\n  Line three: also say how many lines this prompt had.\n";

/// draft.txt with the rows of its box replaced by `rows` (each ending with a line break). NOT a
/// real capture: the recorded screen changed in that one place.
fn draft_with(rows: &str) -> String {
    let draft = capture!("draft");
    assert!(draft.contains(DRAFT_ROWS), "the recorded box");
    draft.replacen(DRAFT_ROWS, rows, 1)
}

/// `screen` in a pane `width` columns wide, as far as the box goes: its rules that long. NOT a
/// real capture.
fn narrowed(screen: &str, width: usize) -> String {
    screen.replace(&"─".repeat(200), &"─".repeat(width))
}

/// The 45-line prompt devbox pasted for pasted-lines.txt.
pub(crate) fn tall_prompt() -> String {
    let first = "Tall prompt test: this prompt has 45 short lines. Reply with one word: OK";
    let lines: Vec<String> = std::iter::once(first.to_owned())
        .chain((2..45).map(|n| format!("line {n:02} of 45")))
        .chain(["line 45 of 45 (last line)".to_owned()])
        .collect();
    lines.join("\n")
}

/// The line of 1500 characters pasted for pasted-chars.txt.
pub(crate) fn long_line() -> String {
    format!("{}x", &"probe words ".repeat(125)[..1499])
}

/// The input row of pasted-lines.txt, without the blanks agy pads it with to the pane's width.
pub(crate) const PLACEHOLDER_ROW: &str = "> [Pasted text #1 +45 lines]";

/// `screen`, pasted-lines.txt or pasted-chars.txt, with its input row, the placeholder, replaced
/// by `row`. NOT a real capture: the recorded screen changed in that one place.
pub(crate) fn placeholder_with(screen: &str, row: &str) -> String {
    let rows: Vec<&str> = screen.split('\n').collect();
    let at = rows
        .iter()
        .position(|shown| shown.starts_with("> [Pasted text #"))
        .expect("the recorded input row");
    let mut rows: Vec<String> = rows.into_iter().map(str::to_owned).collect();
    rows[at] = row.to_owned();
    rows.join("\n")
}

#[test]
fn the_input_row_starting_with_the_prompt_is_taken_for_it() {
    // Recorded: `> ` and the first line, then each further line after two blanks.
    assert_eq!(pasted(capture!("draft"), DRAFT), Pasted::Leads);
    // Only the input row counts, whatever the box shows under it and however the prompt goes on
    // after its first line: joined into one line, the row shows its start.
    for same_start in [
        ONE_LINE.to_owned(),
        format!("{DRAFT}\n"),
        format!("{DRAFT}\nLine four."),
        DRAFT.replace('\n', " "),
    ] {
        assert_eq!(
            pasted(capture!("draft"), &same_start),
            Pasted::Leads,
            "{same_start:?}"
        );
    }
    // Another first line is not: another word, another case, or a shorter line than the row.
    for other in [
        "Reply with NO and stop.",
        "reply with OK and stop.",
        "Reply with OK",
    ] {
        assert_eq!(pasted(capture!("draft"), other), Pasted::Other, "{other}");
    }
    // Runs of blanks count as one, on either side, and blanks at the end not at all (NOT real
    // captures).
    let spaced = draft_with("> Reply  with OK   and stop.   \n");
    assert_eq!(pasted(&spaced, ONE_LINE), Pasted::Leads);
    let one = draft_with("> Reply with OK and stop.\n");
    assert_eq!(pasted(&one, "Reply with\tOK  and stop.  "), Pasted::Leads);
}

#[test]
fn a_long_first_line_or_a_tall_prompt_is_told_by_the_start_of_the_input_row() {
    // NOT real captures: no recording shows how agy wraps a line or fits a prompt taller than
    // its box. Only how the input row starts counts, however the rest goes.
    let korean: String = "가".repeat(150);
    let prompt = format!("{korean}\n둘째 줄");
    let part =
        |from: usize, to: usize| -> String { korean.chars().skip(from).take(to - from).collect() };
    // Wider than the fixture's 200 columns, wrapped there, and in a pane of 120 columns.
    let wide = draft_with(&format!(
        "> {}\n  {}\n  둘째 줄\n",
        part(0, 99),
        part(99, 150)
    ));
    assert_eq!(pasted(&wide, &prompt), Pasted::Leads);
    let rows = format!(
        "> {}\n  {}\n  {}\n  둘째 줄\n",
        part(0, 59),
        part(59, 118),
        part(118, 150)
    );
    let narrow = narrowed(&draft_with(&rows), 120);
    assert_eq!(pasted(&narrow, &prompt), Pasted::Leads);
    // Sixty lines, the box as tall as the rows read allow.
    let tall: Vec<String> = (1..=60)
        .map(|n| format!("Step {n} of a long task description."))
        .collect();
    let rows: String = tall
        .iter()
        .enumerate()
        .map(|(at, line)| format!("{}{line}\n", if at == 0 { "> " } else { "  " }))
        .collect();
    assert_eq!(pasted(&draft_with(&rows), &tall.join("\n")), Pasted::Leads);
    // A box agy scrolled to its end shows no first row of the prompt: not taken.
    let scrolled: String = tall[40..]
        .iter()
        .map(|line| format!("  {line}\n"))
        .collect();
    assert_eq!(
        pasted(&draft_with(&scrolled), &tall.join("\n")),
        Pasted::Other
    );
    // A box whose top rule is out of the rows read is no box.
    let cut = capture!("draft").replacen(&format!("{}\n> Reply", "─".repeat(200)), "> Reply", 1);
    assert_ne!(cut, capture!("draft"));
    assert_eq!(pasted(&cut, DRAFT), Pasted::NoBox);
}

#[test]
fn a_draft_in_shell_mode_or_after_a_slash_is_never_the_prompt() {
    // The recorded pastes into a box that was in shell mode (`!`) and after a `/`: Enter would
    // run the prompt as a shell command, or as a command with the prompt as its argument.
    assert_eq!(
        pasted(capture!("shell-mode-draft"), ONE_LINE),
        Pasted::Other
    );
    assert_eq!(pasted(capture!("slash-draft"), ONE_LINE), Pasted::Other);
    // The three-line prompt after `!` or `/` (NOT real captures).
    let bang = DRAFT_ROWS.replacen("> ", "! ", 1);
    assert_eq!(pasted(&draft_with(&bang), DRAFT), Pasted::Other);
    let slash = DRAFT_ROWS.replacen("> ", "> /", 1);
    assert_eq!(pasted(&draft_with(&slash), DRAFT), Pasted::Other);
}

#[test]
fn text_typed_in_front_of_the_prompt_is_never_taken_for_it() {
    // NOT real captures: draft.txt with what the user typed before the paste landed, a blank
    // included, or a `!` after another key.
    for first in ["> xReply", "> x Reply", ">  Reply", "> !Reply"] {
        let rows = DRAFT_ROWS.replacen("> Reply", first, 1);
        assert_eq!(pasted(&draft_with(&rows), DRAFT), Pasted::Other, "{first}");
    }
    // Typed after it on the input row, the row is more than the first line: not taken either.
    let after = draft_with("> Reply with OK and stop.x\n");
    assert_eq!(pasted(&after, ONE_LINE), Pasted::Other);
    // Typed after it below the first line, the input row still starts with the prompt.
    for rows in [
        DRAFT_ROWS.replacen("had.\n", "had.x\n", 1),
        format!("{DRAFT_ROWS}  more\n"),
    ] {
        assert_eq!(pasted(&draft_with(&rows), DRAFT), Pasted::Leads, "{rows:?}");
    }
}

#[test]
fn an_empty_box_or_too_little_of_the_line_is_a_paste_still_being_drawn() {
    // The empty box, with the launch's hint or `>` alone (NOT a real capture: the hint removed).
    let bare = capture!("idle-after-trust").replace(
        "> Auto mode: edits & safe commands auto-approved (shift+tab to cycle)",
        ">",
    );
    for empty in [
        capture!("idle-after-trust"),
        capture!("idle-default-statusline"),
        bare.as_str(),
    ] {
        assert_eq!(pasted(empty, DRAFT), Pasted::Drawing);
    }
    // Less than 16 characters of a longer first line, and then 16 (NOT real captures).
    let shown = |row: &str| draft_with(&format!("> {row}\n"));
    assert_eq!(pasted(&shown("Reply with OK a"), ONE_LINE), Pasted::Drawing);
    assert_eq!(pasted(&shown("Reply with OK an"), ONE_LINE), Pasted::Leads);
    // A shorter first line shows whole.
    assert_eq!(pasted(&shown("fix it"), "fix it\nmore"), Pasted::Leads);
    assert_eq!(pasted(&shown("fix"), "fix it"), Pasted::Drawing);
    // No box at all: the service's own checks (the waiting patterns) refuse these screens.
    for none in [capture!("trust"), capture!("login"), ""] {
        assert_eq!(pasted(none, DRAFT), Pasted::NoBox);
    }
    // The last two rules are of different lengths: not one box (approval.txt draws a full rule
    // in the command's section, under the shorter rule of an earlier prompt).
    assert_eq!(pasted(capture!("approval"), DRAFT), Pasted::NoBox);
}

#[test]
fn a_prompt_without_a_first_line_is_never_taken_for_one() {
    // devbox pastes a prompt without the blank lines before it: one that still starts with one
    // has an empty first line, and one with blanks in front of its first line needs a row that
    // shows them, which draft.txt's does not.
    for prompt in [
        "",
        "   ",
        "\nReply with OK and stop.",
        " Reply with OK and stop.",
    ] {
        assert_eq!(
            pasted(capture!("draft"), prompt),
            Pasted::Other,
            "{prompt:?}"
        );
    }
}

#[test]
fn a_paste_agy_shows_as_a_placeholder_is_taken_for_the_prompt_of_its_count() {
    // Recorded: a prompt of 45 lines shown as `+45 lines`, and a line of 1500 characters as
    // `1500 chars`, each input row padded with blanks to the pane's width.
    let (lines, chars) = (capture!("pasted-lines"), capture!("pasted-chars"));
    assert!(lines.contains(&format!("{PLACEHOLDER_ROW}  ")), "padded");
    assert_eq!(pasted(lines, &tall_prompt()), Pasted::Leads);
    assert_eq!(pasted(chars, &long_line()), Pasted::Leads);
    // Another count is another paste: a line fewer or more, a character fewer or more, or the
    // count of the other form.
    let tall = tall_prompt();
    let fewer = tall.rsplit_once('\n').expect("lines").0.to_owned();
    let line = long_line();
    for other in [fewer, format!("{tall}\nline 46"), line.clone()] {
        assert_eq!(pasted(lines, &other), Pasted::Other, "{other:?}");
    }
    for other in [line[..1499].to_owned(), format!("{line}x"), tall] {
        assert_eq!(pasted(chars, &other), Pasted::Other, "{other:?}");
    }
    // NOT real captures: pasted-lines.txt with its input row changed. Another paste's number,
    // and no blanks at the end, are the same placeholder.
    for same in ["> [Pasted text #12 +45 lines]", PLACEHOLDER_ROW] {
        let screen = placeholder_with(lines, same);
        assert_eq!(pasted(&screen, &tall_prompt()), Pasted::Leads, "{same}");
    }
}

#[test]
fn a_placeholder_with_anything_else_on_its_row_is_never_the_prompt() {
    // NOT real captures: pasted-lines.txt with its input row changed. A key typed in front of the
    // paste, after it, or `!` or `/` before it; a placeholder drawn otherwise than agy draws it;
    // and two pastes.
    for row in [
        "! [Pasted text #1 +45 lines]",
        "> x[Pasted text #1 +45 lines]",
        "> x [Pasted text #1 +45 lines]",
        ">  [Pasted text #1 +45 lines]",
        "> [Pasted text #1 +45 lines]x",
        "> [Pasted text #1 +45 lines] x",
        "> [Pasted text #1 +45 lines",
        "> [Pasted text #1 +45 line]",
        "> [Pasted text #1 45 lines]",
        "> [Pasted text #1 +045 lines]",
        "> [Pasted text #1  +45 lines]",
        "> [Pasted text # +45 lines]",
        "> [Pasted text #x +45 lines]",
        "> [Pasted text #01 +45 lines]",
        "> [Pasted text #0 +45 lines]",
        "> [pasted text #1 +45 lines]",
        "> [Pasted text #1 +45 lines][Pasted text #2 +45 lines]",
        "> /[Pasted text #1 +45 lines]",
    ] {
        let screen = placeholder_with(capture!("pasted-lines"), row);
        assert_eq!(pasted(&screen, &tall_prompt()), Pasted::Other, "{row}");
    }
}

#[test]
fn a_first_line_pasted_with_its_indentation_is_told_by_a_row_that_shows_it() {
    // NOT real captures: draft.txt with its box showing a prompt whose first line is indented,
    // drawn as agy draws a paste: `> ` and then the text, each further line after two blanks.
    let yaml = "  steps:\n    - run: cargo test";
    let drawn = draft_with(">   steps:\n      - run: cargo test\n");
    assert_eq!(pasted(&drawn, yaml), Pasted::Leads);
    // A run of blanks counts as one: an indentation of tabs, or blanks typed in front of it,
    // which change nothing of the box's mode.
    assert_eq!(
        pasted(&drawn, "\tsteps:\n\t- run: cargo test"),
        Pasted::Leads
    );
    let wider = draft_with(">     steps:\n      - run: cargo test\n");
    assert_eq!(pasted(&wider, yaml), Pasted::Leads);
    // Text typed in front of it is still in front, and a row without the indentation does not
    // start with the first line (agy was not seen to drop it).
    for row in [">  x steps:\n", "> x  steps:\n", "> steps:\n"] {
        assert_eq!(pasted(&draft_with(row), yaml), Pasted::Other, "{row}");
    }
}
