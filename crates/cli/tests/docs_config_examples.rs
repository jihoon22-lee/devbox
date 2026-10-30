//! Every TOML block in docs/config.md loads with the readers devbox itself uses, so the
//! documented examples cannot drift from the code. A block's first line names its file:
//! `# config.toml` and `# config.local.toml` go through the daemon's start-up reads
//! (`crates/cli/src/daemon/mod.rs`) and the `[windows] root` check a Windows run makes;
//! `# .devbox/devbox.toml` goes through the project loader, the run-definition checks and the
//! `[launch]` terminal and agent preflight (`crates/cli/src/daemon/launch.rs`). `[launch]`
//! service ids are checked only inside `RuntimeService::prepare_launch_snapshot`, which needs a
//! running service, so this test does not check them.
use devbox_core::config::{Disabled, load_lenient};
use devbox_core::{Env, Instance, Paths};
use std::fmt::{Debug, Display};
use std::path::{Path, PathBuf};

const MARKERS: [&str; 3] = [
    "# config.toml",
    "# config.local.toml",
    "# .devbox/devbox.toml",
];

/// An owned home folder; nothing is read from the real environment.
struct Home(PathBuf);

impl Env for Home {
    fn var(&self, key: &str) -> Option<String> {
        matches!(key, "HOME" | "XDG_RUNTIME_DIR").then(|| self.0.display().to_string())
    }
    fn uid(&self) -> u32 {
        1000
    }
}

fn text(error: impl Display) -> String {
    error.to_string()
}

fn debug(error: impl Debug) -> String {
    format!("{error:?}")
}

/// Each TOML block: the line its fence opens on, and its body. Backtick and tilde fences of any
/// length count, and the info string's first word is compared without case, so ```` ```TOML ````
/// and `~~~toml` blocks are checked too.
fn toml_blocks(doc: &str) -> Result<Vec<(usize, String)>, String> {
    // The open fence: its character, its length, and the TOML block it holds, if it is one.
    let (mut blocks, mut open) = (Vec::new(), None::<(char, usize, Option<(usize, String)>)>);
    for (index, line) in doc.lines().enumerate() {
        let trimmed = line.trim_start();
        let mark = trimmed.chars().next().filter(|c| matches!(c, '`' | '~'));
        let run = mark.map_or(0, |mark| trimmed.chars().take_while(|c| *c == mark).count());
        let rest = &trimmed[run..];
        if let Some((fence, length, block)) = open.as_mut() {
            if mark == Some(*fence) && run >= *length && rest.trim().is_empty() {
                blocks.extend(block.take());
                open = None;
            } else if let Some((_, body)) = block {
                body.push_str(line);
                body.push('\n');
            }
        } else if let Some(mark) = mark.filter(|_| run >= 3) {
            let info = rest.split_whitespace().next().unwrap_or_default();
            let toml = info.eq_ignore_ascii_case("toml");
            open = Some((mark, run, toml.then(|| (index + 1, String::new()))));
        }
    }
    match open {
        None => Ok(blocks),
        Some(_) => Err("a code fence is not closed".into()),
    }
}

/// What the daemon reads at start, in the same order, must load with no problem.
fn daemon_config(file: &str, body: &str) -> Result<(), String> {
    let home = tempfile::tempdir().map_err(text)?;
    let instance = Instance::parse("test-docs-config").map_err(text)?;
    let paths = Paths::resolve(instance, &Home(home.path().to_owned())).map_err(text)?;
    std::fs::create_dir_all(&paths.config_dir).map_err(text)?;
    std::fs::write(paths.config_dir.join(file), body).map_err(text)?;
    let mut load = load_lenient(&paths);
    let notes = load.section("notes", |table| {
        devbox_notes::NotesSettings::from_table(table, &paths.home)
    });
    let search = load.section("search", devbox_search::SearchSettings::from_table);
    let activity = load.section(
        "activity",
        devbox_activity::ActivitySettings::try_from_table,
    );
    let (profiles, invalid) = devbox_agents::profile::configured_lenient(&load.config.agents);
    let problems: Vec<String> = load
        .problems
        .iter()
        .chain(&invalid)
        .map(|problem| format!("{problem:?}"))
        .collect();
    if !problems.is_empty() {
        return Err(problems.join("; "));
    }
    if load.disabled != Disabled::default() || notes.is_none() || search.is_none() {
        return Err(format!("a section was turned off: {:?}", load.disabled));
    }
    let activity = activity.ok_or("[activity] did not load")?;
    devbox_activity::model::privacy::CompiledRules::compile(&activity.rules)
        .map_err(|invalid| format!("[activity] rules do not compile: {invalid:?}"))?;
    if profiles.len() != load.config.agents.profiles.len() {
        return Err("an [[agents.profiles]] entry was skipped".into());
    }
    // Checked only when a Windows run starts, so a bad root does not show at load.
    if let Some(root) = &load.config.windows.root {
        devbox_runtime::windows::windows_to_wsl(root)
            .map_err(|error| format!("[windows] root {root:?}: {error}"))?;
    }
    Ok(())
}

/// A project's file as the daemon loads it, then the checks every run definition gets and the
/// launch preflight's terminal and agent checks.
fn project_config(body: &str) -> Result<(), String> {
    let project = tempfile::tempdir().map_err(text)?;
    std::fs::create_dir(project.path().join(".devbox")).map_err(text)?;
    std::fs::write(project.path().join(".devbox/devbox.toml"), body).map_err(text)?;
    let config = devbox_projects::config::load(project.path()).map_err(text)?;
    devbox_runtime::merge(config.run_config().definitions(), Vec::new()).map_err(debug)?;
    for terminal in &config.launch.terminals {
        devbox_terminal::validate_command(terminal.command.as_deref().unwrap_or("bash -l"))
            .map_err(debug)?;
        devbox_terminal::validate_title(&terminal.name).map_err(debug)?;
    }
    // The profile lookup agent creation makes (`crates/agents/src/create.rs`). An example names
    // a built-in tool, which works without `[[agents.profiles]]`.
    for agent in &config.launch.agents {
        devbox_agents::profile::resolve(&[], &agent.tool)
            .ok_or_else(|| format!("[launch] tool {:?} is not a built-in profile", agent.tool))?;
        devbox_agents::validate_input(agent.prompt.as_deref().unwrap_or_default())
            .map_err(debug)?;
    }
    Ok(())
}

#[test]
fn every_toml_example_in_the_config_doc_loads_without_problems() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/config.md");
    let doc = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let blocks = toml_blocks(&doc).unwrap_or_else(|error| panic!("docs/config.md: {error}"));
    let mut seen = [0usize; MARKERS.len()];
    let mut failures = Vec::new();
    for (line, body) in &blocks {
        let marker = body.lines().next().unwrap_or_default().trim();
        let result = match MARKERS.iter().position(|known| *known == marker) {
            Some(index) => {
                seen[index] += 1;
                match index {
                    2 => project_config(body),
                    _ => daemon_config(&marker[2..], body),
                }
            }
            None => Err(format!("the first line must be one of {MARKERS:?}")),
        };
        if let Err(message) = result {
            failures.push(format!("docs/config.md:{line}: {message}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(
        seen.iter().all(|count| *count > 0),
        "each of {MARKERS:?} needs an example; found {seen:?}"
    );
}
