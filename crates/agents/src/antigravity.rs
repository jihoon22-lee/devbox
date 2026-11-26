//! The Antigravity CLI (`agy`) family: the hooks folder a run loads, what its hooks report, and
//! how its pane shows an empty prompt. Evidence: `tests/fixtures/antigravity-1.5.17/` (real
//! captures and hook payloads of agy 1.5.17).
//!
//! agy has no argument or setting for hooks, and reads them from `.agents/hooks.json` of a
//! workspace folder. devbox adds one more workspace folder to the run with `--add-dir`: the
//! sibling `<work folder>.agy-hooks`, which holds only that file. The user's agy settings are not
//! written. The folder sits beside a work folder devbox made, never beside the user's own
//! checkout, so a task needs a work folder (`AgentsError::WorkFolderRequired`).
use crate::{plan::sh_quote, state::AgentEvent};
use serde::Deserialize;
use std::path::{Component, Path, PathBuf};

/// What follows the work folder's path in the name of its hooks folder.
const HOOKS_DIR_SUFFIX: &str = ".agy-hooks";
/// The file in the hooks folder that names the task it was made for.
const OWNER_FILE: &str = ".devbox-agent";
/// How long agy waits for one hook command, in seconds. `devbox agent event` ends itself sooner.
const HOOK_TIMEOUT_SECS: u32 = 10;
/// Where agy keeps its conversations, under the home folder.
const DATA_DIR: &str = ".gemini/antigravity-cli";
/// The longest transcript path kept.
const MAX_PATH_BYTES: usize = 1024;

/// The hooks folder of the work folder `workdir`: its sibling, named after it. agy lists the
/// folders it was started in and given in order, and runs commands in the first one, so the name
/// sorts after the work folder's.
pub(crate) fn hooks_dir(workdir: &str) -> PathBuf {
    PathBuf::from(format!(
        "{}{HOOKS_DIR_SUFFIX}",
        workdir.trim_end_matches('/')
    ))
}

/// `.agents/hooks.json` of the hooks folder. Only these three events are registered: a
/// `PreToolUse` hook that prints `{}` denies every tool call. Each command reports the event's
/// payload, which agy writes to its stdin, and prints `{}` (`devbox agent event`).
pub(crate) fn hooks_json(devbox: &str) -> String {
    let command = format!("{} agent event --from antigravity", sh_quote(devbox));
    let hook = serde_json::json!([{ "type": "command", "command": command, "timeout": HOOK_TIMEOUT_SECS }]);
    serde_json::json!({ "devbox": { "SessionStart": hook, "PreInvocation": hook, "Stop": hook } })
        .to_string()
}

/// One hook call devbox acts on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Report {
    pub event: HookEvent,
    /// agy's id of the conversation, the task's session.
    pub conversation: String,
    /// The conversation's transcript file as the payload names it, not yet checked.
    pub transcript: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HookEvent {
    /// The first prompt of a new conversation. A resumed conversation never sends it.
    SessionStart,
    /// A model call begins: the first of a turn, and one after each tool call.
    PreInvocation,
    /// The agent is back at its prompt: once per turn, never at exit or after an interrupt.
    Stop,
}

/// The report in one hook payload. Only the event's name, the conversation and the transcript
/// path are read; the prompt, the tool call and the rest are skipped without being built. Another
/// event, a payload that is not the expected JSON or one without a conversation means nothing.
pub(crate) fn report(payload: &str) -> Option<Report> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Payload {
        hook_event_name: String,
        conversation_id: Option<String>,
        transcript_path: Option<String>,
    }
    let payload: Payload = serde_json::from_str(payload).ok()?;
    let event = match payload.hook_event_name.as_str() {
        "SessionStart" => HookEvent::SessionStart,
        "PreInvocation" => HookEvent::PreInvocation,
        "Stop" => HookEvent::Stop,
        _ => return None,
    };
    Some(Report {
        event,
        conversation: payload.conversation_id.filter(|id| !id.is_empty())?,
        transcript: payload.transcript_path,
    })
}

/// The task events of one hook payload. Each starts with the conversation as the task's session,
/// which changes nothing once the task has it: a resumed conversation reports it with
/// `PreInvocation` alone. The service checks the conversation against the task's before it
/// applies any of them (`AgentsService::antigravity_event`).
pub(crate) fn events(payload: &str) -> Vec<AgentEvent> {
    let Some(report) = report(payload) else {
        return vec![];
    };
    let mut events = vec![AgentEvent::SessionStarted {
        session_id: report.conversation,
    }];
    match report.event {
        HookEvent::SessionStart => {}
        HookEvent::PreInvocation => events.push(AgentEvent::PromptSubmitted),
        HookEvent::Stop => events.push(AgentEvent::TurnComplete { message: None }),
    }
    events
}

/// The transcript file a hook payload names, when it is an absolute path below agy's data folder
/// in `home` with no `..` in it. The path comes from the tool, so nothing else is ever read.
pub(crate) fn transcript_path(home: &Path, named: &str) -> Option<PathBuf> {
    let path = Path::new(named);
    let inside = path.strip_prefix(home.join(DATA_DIR)).ok()?;
    (named.len() <= MAX_PATH_BYTES
        && path.is_absolute()
        && inside.file_name().is_some()
        && path
            .components()
            .all(|part| matches!(part, Component::RootDir | Component::Normal(_))))
    .then(|| path.to_path_buf())
}

/// The last non-empty rows of `capture`, from the bottom up: the 10 a turn's spinner row is
/// looked for in. The prompt box is looked for in the first 6 of them.
fn last_rows(capture: &str) -> Vec<&str> {
    capture
        .lines()
        .rev()
        .filter(|row| !row.trim().is_empty())
        .take(10)
        .collect()
}

/// A row of a turn in progress: a Braille spinner, two blanks, then what agy is doing.
fn spinner(row: &str) -> bool {
    let mut chars = row.chars();
    chars
        .next()
        .is_some_and(|c| ('\u{2800}'..='\u{28FF}').contains(&c))
        && chars.as_str().starts_with("  ")
}

/// Whether `capture` shows a turn in progress. A tip and a queued prompt can sit between its
/// spinner row and the prompt box. Evidence: working*.txt and queued.txt of the fixtures.
pub(crate) fn shows_turn(capture: &str) -> bool {
    last_rows(capture).iter().any(|row| spinner(row))
}

/// How the hint in an empty prompt box ends, whatever permission mode it names.
const HINT_END: &str = "(shift+tab to cycle)";

/// A full row of the box-drawing rule agy draws above and below its prompt box, and, shorter,
/// above a prompt it echoes.
fn rule(row: &str) -> bool {
    row.chars().count() >= 20 && row.chars().all(|c| c == '─')
}

/// Whether `capture` shows a prompt that was already given in this pane: agy echoes a prompt as
/// `> <text>` under a rule and leaves it above the prompt box, so a rule other than the box's
/// own two with such a row under it is one. A slash command's row has no rule above it, and the
/// box's own input row is under the box's rule. A row that ends with `(shift+tab to cycle)` is
/// the hint of an empty box and never an echo, wherever a rule sits above it (a resized pane
/// can leave a piece of an older box on the screen). A long answer scrolls the echo out of the
/// rows read. Evidence: every capture after a turn among the fixtures has it (idle.txt,
/// idle-after-user-turn.txt, …), and none before a launch's first prompt (idle-after-trust.txt,
/// idle-after-panel.txt, idle-after-clearing-draft.txt, …).
pub(crate) fn shows_earlier_prompt(capture: &str) -> bool {
    let rows: Vec<&str> = capture
        .lines()
        .filter(|row| !row.trim().is_empty())
        .collect();
    let rules: Vec<usize> = (0..rows.len()).filter(|&at| rule(rows[at])).collect();
    let above_box = rules.len().saturating_sub(2);
    let echo = |row: &&str| row.starts_with("> ") && !row.trim_end().ends_with(HINT_END);
    rules[..above_box]
        .iter()
        .any(|&at| rows.get(at + 1).is_some_and(echo))
}

/// Whether `capture` shows a screen agy draws before it takes any prompt: its folder-trust
/// question or its sign-in choice, with no prompt box. Nobody can have typed a prompt into a
/// pane that shows one. Evidence: trust*.txt and login.txt of the fixtures.
pub(crate) fn shows_before_first_prompt(capture: &str) -> bool {
    let rows = last_rows(capture);
    let choice = |text: &str| {
        rows.iter().any(|row| {
            row.strip_prefix("> ")
                .or_else(|| row.strip_prefix("  "))
                .is_some_and(|rest| rest.trim_end() == text)
        })
    };
    let trust = choice("Yes, I trust this folder") && choice("No, exit");
    let sign_in = rows.iter().any(|row| {
        row.trim() == "Welcome to the Antigravity CLI. You are currently not signed in."
    });
    (trust || sign_in) && !rows.iter().any(|row| rule(row))
}

/// Whether `capture` shows agy at an empty prompt with nothing running: among the last 6 rows, the
/// prompt box, and no turn in progress. The box is a rule, an input row and a rule, and its input
/// row is empty when it is `>` alone or `>` with the hint agy draws before the first prompt of
/// each launch (of a resumed conversation too), which names the permission mode and ends with
/// `(shift+tab to cycle)`. Only the `Auto mode` hint was recorded; another mode's is taken by
/// that ending. agy draws the empty box while it works too, and under a menu, so this counts only
/// where no waiting pattern matches. Evidence: idle*.txt, working*.txt and queued.txt of the
/// fixtures.
pub(crate) fn shows_prompt(capture: &str) -> bool {
    let rows = last_rows(capture);
    rows[..rows.len().min(6)]
        .windows(3)
        .any(|box_rows| rule(box_rows[0]) && empty_input(box_rows[1]) && rule(box_rows[2]))
        && !rows.iter().any(|row| spinner(row))
}

/// What agy's prompt box shows once devbox pasted a prompt into it without Enter
/// (`AgentsService::enter_first_instruction`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pasted {
    /// The box's input row is `> ` and the start of the prompt's first line, enough of it to tell
    /// ([`SHOWN_AT_LEAST`]), or the placeholder agy draws for this prompt when it is too large to
    /// show: nothing was typed into the box before the paste, and the box is not in shell mode.
    Leads,
    /// The box is empty, or its input row shows less of the first line than that: agy may not
    /// have drawn all of the paste yet.
    Drawing,
    /// No prompt box: the screen's last two rules are missing, or not of one length.
    NoBox,
    /// The box's input row starts otherwise: with text typed before the paste, in shell mode
    /// (`!`) or with a `/`, or with more than the first line.
    Other,
}

/// How much of the prompt's first line the box's input row must show, in characters. A shorter
/// line shows whole.
const SHOWN_AT_LEAST: usize = 16;

/// `text` with each run of blanks as one space and none at its end: the box's input row and the
/// prompt's first line are compared so.
fn collapsed(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut blank = false;
    for c in text.chars() {
        if c.is_whitespace() {
            blank = true;
            continue;
        }
        if std::mem::take(&mut blank) {
            out.push(' ');
        }
        out.push(c);
    }
    out
}

/// Whether `row`, the prompt box's input row, is empty: `>` alone, or the hint agy draws before
/// a launch's first prompt, which ends with `(shift+tab to cycle)`.
fn empty_input(row: &str) -> bool {
    let row = row.trim_end();
    row == ">" || (row.starts_with("> ") && row.ends_with(HINT_END))
}

/// Whether `shown`, the box's input row after its `> ` and without the blanks at its end, is all
/// of the placeholder agy draws instead of a paste too large to show, with the count of `prompt`,
/// the text pasted: `[Pasted text #<n> +<lines> lines]` or `[Pasted text #<n> <characters>
/// chars]`, where `<n>` numbers the pastes of agy's session. The count is all it shows of the
/// paste, and a number or count with a leading zero is not one agy draws. Evidence (agy 1.5.17,
/// a real run): pasted-lines.txt and pasted-chars.txt. Its probes showed 15 lines, a line of
/// 1000 characters and three of 600 whole, 16 to 20 lines (and the run's 45) as `+<lines>
/// lines`, and one line of 1500 to 2999 characters as `<characters> chars`, each the exact count
/// of what was pasted. The probes counted ASCII text: a count agy makes otherwise is not taken,
/// and the paste gets no Enter.
fn placeholder(shown: &str, prompt: &str) -> bool {
    let Some((paste, count)) = shown
        .strip_prefix("[Pasted text #")
        .and_then(|rest| rest.strip_suffix(']'))
        .and_then(|rest| rest.split_once(' '))
    else {
        return false;
    };
    // agy numbers the pastes of its session from 1, with no leading zero.
    let numbered = paste
        .parse::<u64>()
        .is_ok_and(|n| n > 0 && n.to_string() == paste);
    let lines = format!("+{} lines", prompt.split('\n').count());
    let chars = format!("{} chars", prompt.chars().count());
    numbered && (count == lines || count == chars)
}

/// What the prompt box in `capture` shows after devbox pasted `prompt` into it. The box is the
/// screen's last two rules, which agy draws across the pane and so are of one length, and the row
/// under the upper one is its input row. Only how that row starts counts. A key typed before the
/// paste landed is in front of the prompt there, and `!` or `/` as the first key of an empty box
/// also changes the row's marker or first character; a key typed after the paste follows the
/// prompt. How agy wraps a long line, or fits a prompt taller than the box, does not change how
/// the row starts, and a paste it shows as a placeholder fills the row alone. Evidence:
/// draft.txt, shell-mode-draft.txt, slash-draft.txt, pasted-lines.txt and pasted-chars.txt.
pub(crate) fn pasted(capture: &str, prompt: &str) -> Pasted {
    let rows: Vec<&str> = capture.lines().map(str::trim_end).collect();
    let mut rules = (0..rows.len()).rev().filter(|&at| rule(rows[at]));
    let (Some(bottom), Some(top)) = (rules.next(), rules.next()) else {
        return Pasted::NoBox;
    };
    if rows[top].chars().count() != rows[bottom].chars().count() || bottom == top + 1 {
        return Pasted::NoBox;
    }
    let input = rows[top + 1];
    if empty_input(input) {
        return Pasted::Drawing;
    }
    let Some(shown) = input.strip_prefix("> ") else {
        return Pasted::Other;
    };
    if placeholder(shown, prompt) {
        return Pasted::Leads;
    }
    let (line, shown) = (
        collapsed(prompt.lines().next().unwrap_or_default()),
        collapsed(shown),
    );
    if line.is_empty() || !line.starts_with(&shown) {
        return Pasted::Other;
    }
    if shown.chars().count() >= line.chars().count().min(SHOWN_AT_LEAST) {
        Pasted::Leads
    } else {
        Pasted::Drawing
    }
}

/// What is at `path`, a symlink never followed: `Some(true)` a real folder, `Some(false)` anything
/// else (a file, a symlink), `None` nothing.
fn found(path: &Path) -> std::io::Result<Option<bool>> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) => Ok(Some(meta.is_dir())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn real_dir(path: &Path) -> std::io::Result<bool> {
    match found(path)? {
        Some(true) => Ok(true),
        Some(false) => Err(std::io::Error::other("not a directory")),
        None => Ok(false),
    }
}

/// Writes the hooks folder `dir` for task `agent`: private, with the task's name in it and
/// `.agents/hooks.json`. A first launch (`fresh`) makes the folder and refuses one that is
/// already there. A resume takes the folder the task's first launch made, or makes it again when
/// it is gone, and writes the file anew: the devbox path may have changed. No symlink is followed.
pub(crate) fn write_hooks_dir(
    dir: &Path,
    devbox: &str,
    agent: &str,
    fresh: bool,
) -> std::io::Result<()> {
    use crate::prepare::{no_symlink_ancestors, private_dir, private_file};
    let parent = dir
        .parent()
        .ok_or_else(|| std::io::Error::other("hooks folder without a parent"))?;
    no_symlink_ancestors(parent)?;
    let owner = dir.join(OWNER_FILE);
    let taken = || std::io::Error::new(std::io::ErrorKind::AlreadyExists, "hooks folder exists");
    match found(dir)? {
        // Another task's folder, one devbox did not make, or a file or a symlink of that name,
        // is never taken over: the user removes or renames it.
        Some(false) => return Err(taken()),
        Some(true) if fresh || !owned_by(dir, agent) => return Err(taken()),
        Some(true) => {}
        None => {
            std::fs::create_dir(dir)?;
            private_dir(dir)?;
            private_file(&owner, agent, false)?;
        }
    }
    let agents = dir.join(".agents");
    if !real_dir(&agents)? {
        std::fs::create_dir(&agents)?;
    }
    private_dir(&agents)?;
    let file = agents.join("hooks.json");
    match std::fs::symlink_metadata(&file) {
        // Removing a name never follows it.
        Ok(_) => std::fs::remove_file(&file)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    private_file(&file, &hooks_json(devbox), false)
}

/// Whether the hooks folder `dir` is a real folder that names `agent` as its task.
fn owned_by(dir: &Path, agent: &str) -> bool {
    let owner = dir.join(OWNER_FILE);
    real_dir(dir).is_ok_and(|exists| exists)
        && std::fs::symlink_metadata(&owner).is_ok_and(|meta| meta.is_file() && meta.len() <= 256)
        && std::fs::read_to_string(&owner).is_ok_and(|name| name == agent)
}

/// What [`remove_hooks_dir`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Removal {
    /// The task's folder held nothing but what devbox writes there, and is gone.
    Removed,
    /// Nothing of that name is there, or it is not the task's folder: left as it is.
    NotOwned,
    /// The task's folder holds something devbox did not write (the agent can write in it, as in
    /// any folder of agy's workspace): kept whole.
    Kept,
}

/// Whether `dir` holds only real files named in `files` and real folders named in `dirs`, each
/// of which passes `inside`. Nothing is followed: a symlink is never one of them.
fn holds_only(
    dir: &Path,
    files: &[&str],
    dirs: &[&str],
    inside: impl Fn(&Path) -> std::io::Result<bool>,
) -> std::io::Result<bool> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let name = entry.file_name();
        let named = |names: &[&str]| names.iter().any(|known| name == **known);
        let devbox = (kind.is_file() && named(files))
            || (kind.is_dir() && named(dirs) && inside(&entry.path())?);
        if !devbox {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Removes the hooks folder of the work folder `workdir` when it names `agent` as its task and
/// holds nothing but what devbox writes there: the task's name and `.agents/hooks.json` (or less,
/// after a write that stopped midway). A folder with anything else in it, a symlink included,
/// stays whole, and any other folder of that name stays. Each name is removed alone, never a
/// tree, so what appears meanwhile stops the removal instead of going with it.
pub(crate) fn remove_hooks_dir(workdir: &str, agent: &str) -> std::io::Result<Removal> {
    let dir = hooks_dir(workdir);
    if !owned_by(&dir, agent) {
        return Ok(Removal::NotOwned);
    }
    let agents = dir.join(".agents");
    let hooks_only = |agents: &Path| holds_only(agents, &["hooks.json"], &[], |_| Ok(false));
    if !holds_only(&dir, &[OWNER_FILE], &[".agents"], hooks_only)? {
        return Ok(Removal::Kept);
    }
    if real_dir(&agents)? {
        let file = agents.join("hooks.json");
        if found(&file)?.is_some() {
            std::fs::remove_file(&file)?;
        }
        std::fs::remove_dir(&agents)?;
    }
    std::fs::remove_file(dir.join(OWNER_FILE))?;
    std::fs::remove_dir(&dir)?;
    Ok(Removal::Removed)
}

#[cfg(test)]
#[path = "antigravity_box_tests.rs"]
pub(crate) mod box_tests;
#[cfg(test)]
#[path = "antigravity_tests.rs"]
mod tests;
