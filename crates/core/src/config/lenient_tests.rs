use super::super::tests::{paths_in, write};
use super::super::*;
use devbox_protocol::system::StartupProblem;

fn file(paths: &Paths, name: &str) -> String {
    paths.config_dir.join(name).display().to_string()
}

/// `(file name, section)` of every section problem, in report order.
fn sections(load: &ConfigLoad) -> Vec<(String, String)> {
    load.problems
        .iter()
        .map(|problem| match problem {
            StartupProblem::ConfigSection { file, section, .. } => (
                file.rsplit('/').next().expect("file name").to_owned(),
                section.clone(),
            ),
            other => panic!("unexpected problem {other:?}"),
        })
        .collect()
}

/// What a file that does not load leaves: every default, except that nothing is cleaned up
/// after a task closes (an unset key would read as on).
fn defaults_without_cleanup() -> Config {
    Config {
        agents: AgentsConfig {
            cleanup_after_close: Some(false),
            ..AgentsConfig::default()
        },
        ..Config::default()
    }
}

#[test]
fn syntax_error_yields_defaults_disables_user_paths_and_reports_the_file() {
    let temp = tempfile::tempdir().expect("owned config");
    let paths = paths_in(temp.path());
    write(
        &paths,
        "config.toml",
        "[notes]\nvault = '~/n'\n[mcp]\nwrite = true\n[[agents.profiles]]\nid = 'x'\n[projects\n",
    );
    write(
        &paths,
        "config.local.toml",
        "[windows]\nroot = 'C:\\dev'\n[projects]\nroots = ['~/a']\n",
    );
    let mut load = load_lenient(&paths);
    assert_eq!(load.config, defaults_without_cleanup());
    assert_eq!(
        load.disabled,
        Disabled {
            notes: true,
            search: true,
            windows: true
        }
    );
    assert!(
        matches!(&load.problems[..], [StartupProblem::ConfigFile { file: path, message }]
            if *path == file(&paths, "config.toml") && !message.is_empty()),
        "{:?}",
        load.problems
    );
    // Domain-owned sections fail too (activity then records nothing), without a second report.
    assert!(load.section("activity", |_| Ok::<_, String>(())).is_none());
    assert_eq!(load.problems.len(), 1);
}

#[test]
fn unreadable_or_unparsable_files_are_one_line_file_problems() {
    let temp = tempfile::tempdir().expect("owned config");
    let paths = paths_in(temp.path());
    write(&paths, "config.toml", "[pc]\nname = 'desk'\n");
    std::fs::create_dir(paths.config_dir.join("config.local.toml")).expect("owned folder");
    let load = load_lenient(&paths);
    assert_eq!(load.config, defaults_without_cleanup());
    assert!(load.disabled.notes && load.disabled.search && load.disabled.windows);
    assert!(
        matches!(&load.problems[..], [StartupProblem::ConfigFile { file: path, .. }]
            if *path == file(&paths, "config.local.toml")),
        "{:?}",
        load.problems
    );
    std::fs::remove_dir(paths.config_dir.join("config.local.toml")).expect("owned folder");
    write(&paths, "config.toml", "[pc]\nname = 'desk'\n[notes\n");
    let load = load_lenient(&paths);
    let [StartupProblem::ConfigFile { message, .. }] = &load.problems[..] else {
        panic!("{:?}", load.problems)
    };
    assert!(message.starts_with("line 3, column 7: "), "{message}");
    assert!(!message.contains('\n'), "{message}");
}

#[test]
fn invalid_section_only_disables_that_section() {
    let temp = tempfile::tempdir().expect("owned config");
    let paths = paths_in(temp.path());
    write(
        &paths,
        "config.toml",
        "[projects]\nroots = ['~/a']\n[mcp]\nwrite = 'yes'\n",
    );
    // A valid override cannot bring write access back while one copy of [mcp] is broken.
    write(&paths, "config.local.toml", "[mcp]\nwrite = true\n");
    let mut load = load_lenient(&paths);
    assert!(!load.config.mcp.write);
    assert_eq!(load.config.projects.roots, ["~/a"]);
    assert_eq!(load.disabled, Disabled::default());
    assert_eq!(sections(&load), [("config.toml".into(), "mcp".into())]);
    assert!(load.section("activity", |_| Ok::<_, String>(())).is_some());
}

#[test]
fn local_file_problem_names_the_local_file() {
    let temp = tempfile::tempdir().expect("owned config");
    let paths = paths_in(temp.path());
    write(&paths, "config.toml", "[notes]\nvault = '~/n'\n");
    write(
        &paths,
        "config.local.toml",
        "[windows]\nroot = 1\n[notes]\nvalt = '~/x'\n",
    );
    let mut load = load_lenient(&paths);
    assert_eq!(
        load.disabled,
        Disabled {
            notes: false,
            search: false,
            windows: true
        }
    );
    assert_eq!(load.config.windows.root, None);
    // A domain check runs on each file's own copy, so the typo names the local file.
    let notes = load.section("notes", |table| match table.contains_key("valt") {
        true => Err("unknown field `valt`"),
        false => Ok(()),
    });
    assert!(notes.is_none());
    assert!(load.disabled.notes);
    assert_eq!(
        sections(&load),
        [
            ("config.local.toml".into(), "windows".into()),
            ("config.local.toml".into(), "notes".into())
        ]
    );
    let StartupProblem::ConfigSection { file: path, .. } = &load.problems[0] else {
        unreachable!()
    };
    assert_eq!(*path, file(&paths, "config.local.toml"));
}

#[test]
fn a_section_failing_without_any_copy_names_the_absolute_shared_file() {
    let temp = tempfile::tempdir().expect("owned config");
    let paths = paths_in(temp.path());
    let mut load = load_lenient(&paths);
    assert!(
        load.section("search", |_| Err::<(), _>("defaults rejected"))
            .is_none()
    );
    assert!(load.disabled.search);
    assert!(
        matches!(&load.problems[..], [StartupProblem::ConfigSection { file: path, section, .. }]
            if *path == file(&paths, "config.toml") && section == "search"),
        "{:?}",
        load.problems
    );
}

#[test]
fn unknown_and_misplaced_sections_are_reported_and_ignored() {
    let temp = tempfile::tempdir().expect("owned config");
    let paths = paths_in(temp.path());
    write(
        &paths,
        "config.toml",
        "unexpected = 1\n[windows]\nroot = 'C:\\shared'\n[pc]\nname = 'desk'\n",
    );
    write(
        &paths,
        "config.local.toml",
        "[windows]\nroot = 'C:\\local'\n",
    );
    let load = load_lenient(&paths);
    assert_eq!(load.config.windows.root.as_deref(), Some("C:\\local"));
    assert_eq!(load.config.pc.name.as_deref(), Some("desk"));
    assert_eq!(load.disabled, Disabled::default());
    assert_eq!(
        sections(&load),
        [
            ("config.toml".into(), "unexpected".into()),
            ("config.toml".into(), "windows".into())
        ]
    );
}

#[test]
fn each_section_problem_says_whether_it_turned_something_off() {
    use devbox_protocol::system::SectionEffect::{Disabled, Ignored};
    let temp = tempfile::tempdir().expect("owned config");
    let paths = paths_in(temp.path());
    let shared = "x = 1\n[windows]\nroot = 'C:\\s'\n[mcp]\nwrite = 'yes'\n[agents]\nprofiles = 1\n";
    write(&paths, "config.toml", shared);
    write(&paths, "config.local.toml", "[windows]\nroot = 1\n");
    let mut load = load_lenient(&paths);
    assert!(load.section("notes", |_| Err::<(), _>("no")).is_none());
    let mut effects: Vec<_> = (load.problems.iter())
        .map(|problem| match problem {
            StartupProblem::ConfigSection {
                file,
                section,
                effect,
                ..
            } => (
                file.rsplit('/').next().map(str::to_owned),
                section.as_str(),
                *effect,
            ),
            other => panic!("unexpected problem {other:?}"),
        })
        .collect();
    effects.sort_by_key(|(file, section, _)| (file.clone(), *section));
    let shared = Some("config.toml".to_owned());
    assert_eq!(
        effects,
        [
            (Some("config.local.toml".into()), "windows", Disabled),
            (shared.clone(), "agents.profiles", Disabled),
            (shared.clone(), "mcp", Disabled),
            (shared.clone(), "notes", Disabled),
            // Left out, while every other section and Windows copies still load.
            (shared.clone(), "windows", Ignored),
            (shared, "x", Ignored),
        ]
    );
}

#[test]
fn agent_profiles_keep_their_file_and_outlive_other_agents_errors() {
    let temp = tempfile::tempdir().expect("owned config");
    let paths = paths_in(temp.path());
    write(
        &paths,
        "config.toml",
        "[agents]\ncleanup_after_close = 'yes'\n[[agents.profiles]]\nid = 'one'\n",
    );
    let load = load_lenient(&paths);
    assert_eq!(load.config.agents.cleanup_after_close, Some(false));
    assert_eq!(load.config.agents.profiles.len(), 1);
    assert_eq!(
        load.config.agents.profiles_file.as_deref(),
        Some(paths.config_dir.join("config.toml").as_path())
    );
    assert_eq!(sections(&load), [("config.toml".into(), "agents".into())]);
    // Lists replace: the local list wins and names its own file.
    write(
        &paths,
        "config.local.toml",
        "[[agents.profiles]]\nid = 'two'\n",
    );
    let load = load_lenient(&paths);
    assert_eq!(
        load.config.agents.profiles[0]
            .get("id")
            .and_then(toml::Value::as_str),
        Some("two")
    );
    assert_eq!(
        load.config.agents.profiles_file.as_deref(),
        Some(paths.config_dir.join("config.local.toml").as_path())
    );
    // The shared file's [agents] is still broken, and the local file does not set the key.
    assert_eq!(load.config.agents.cleanup_after_close, Some(false));
}

// Cleanup after close deletes the work folder and branch of a task that was merged or whose
// PR was opened, and the daemon reads an unset key as on. So a config that did not load must
// set the key off instead of leaving it unset.

#[test]
fn a_misread_cleanup_value_turns_cleanup_after_close_off() {
    let temp = tempfile::tempdir().expect("owned config");
    let paths = paths_in(temp.path());
    for value in ["'false'", "'true'", "1"] {
        let text = format!("[agents]\ncleanup_after_close = {value}\n");
        write(&paths, "config.toml", &text);
        let load = load_lenient(&paths);
        assert_eq!(
            load.config.agents.cleanup_after_close,
            Some(false),
            "{value}"
        );
        let agents = [("config.toml".into(), "agents".into())];
        assert_eq!(sections(&load), agents, "{value}");
    }
}

#[test]
fn an_unknown_agents_key_turns_cleanup_after_close_off_and_keeps_the_profiles() {
    let temp = tempfile::tempdir().expect("owned config");
    let paths = paths_in(temp.path());
    // The section does not load, so the valid value beside the typo does not count either way.
    for value in ["true", "false"] {
        let text = format!(
            "[agents]\ncleanup_after_close = {value}\ncleanup_after_clos = false\n[[agents.profiles]]\nid = 'one'\n"
        );
        write(&paths, "config.toml", &text);
        let load = load_lenient(&paths);
        assert_eq!(
            load.config.agents.cleanup_after_close,
            Some(false),
            "{value}"
        );
        assert_eq!(load.config.agents.profiles.len(), 1, "{value}");
        let agents = [("config.toml".into(), "agents".into())];
        assert_eq!(sections(&load), agents, "{value}");
    }
}

#[test]
fn a_broken_agents_section_does_not_borrow_the_other_files_cleanup_setting() {
    let temp = tempfile::tempdir().expect("owned config");
    let paths = paths_in(temp.path());
    write(
        &paths,
        "config.toml",
        "[agents]\ncleanup_after_close = true\n",
    );
    write(&paths, "config.local.toml", "[agents]\nunknown_key = 1\n");
    let load = load_lenient(&paths);
    assert_eq!(load.config.agents.cleanup_after_close, Some(false));
    assert_eq!(
        sections(&load),
        [("config.local.toml".into(), "agents".into())]
    );
}

#[test]
fn a_syntax_error_turns_cleanup_after_close_off() {
    let temp = tempfile::tempdir().expect("owned config");
    let paths = paths_in(temp.path());
    write(
        &paths,
        "config.toml",
        "[agents]\ncleanup_after_close = false\n[notes\n",
    );
    let load = load_lenient(&paths);
    assert_eq!(load.config.agents.cleanup_after_close, Some(false));
    assert!(
        matches!(&load.problems[..], [StartupProblem::ConfigFile { .. }]),
        "{:?}",
        load.problems
    );
}

#[test]
fn a_syntax_error_in_the_local_file_turns_off_the_cleanup_the_shared_file_sets() {
    let temp = tempfile::tempdir().expect("owned config");
    let paths = paths_in(temp.path());
    write(
        &paths,
        "config.toml",
        "[agents]\ncleanup_after_close = true\n",
    );
    write(&paths, "config.local.toml", "[pc\n");
    let load = load_lenient(&paths);
    assert_eq!(load.config.agents.cleanup_after_close, Some(false));
    assert!(
        matches!(&load.problems[..], [StartupProblem::ConfigFile { file: path, .. }]
            if *path == file(&paths, "config.local.toml")),
        "{:?}",
        load.problems
    );
}

#[test]
fn a_broken_section_other_than_agents_leaves_cleanup_after_close_as_configured() {
    let temp = tempfile::tempdir().expect("owned config");
    let paths = paths_in(temp.path());
    for (broken, section) in [
        ("search = 1\n", "search"),
        ("[mcp]\nwrite = 'yes'\n", "mcp"),
        ("[projects]\nroots = 1\n", "projects"),
    ] {
        let text = format!("{broken}[agents]\ncleanup_after_close = true\n");
        write(&paths, "config.toml", &text);
        let load = load_lenient(&paths);
        assert_eq!(
            load.config.agents.cleanup_after_close,
            Some(true),
            "{section}"
        );
        assert_eq!(
            sections(&load),
            [("config.toml".into(), section.into())],
            "{section}"
        );
    }
}

#[test]
fn a_profiles_value_that_does_not_load_leaves_cleanup_after_close_as_configured() {
    let temp = tempfile::tempdir().expect("owned config");
    let paths = paths_in(temp.path());
    write(
        &paths,
        "config.toml",
        "[agents]\ncleanup_after_close = true\nprofiles = 1\n",
    );
    let load = load_lenient(&paths);
    assert_eq!(load.config.agents.cleanup_after_close, Some(true));
    assert_eq!(
        sections(&load),
        [("config.toml".into(), "agents.profiles".into())]
    );
    // An entry that is not a table is skipped later, one entry at a time, not as `[agents]`.
    write(
        &paths,
        "config.toml",
        "[agents]\ncleanup_after_close = true\nprofiles = [1]\n",
    );
    let load = load_lenient(&paths);
    assert_eq!(load.config.agents.cleanup_after_close, Some(true));
    assert!(load.problems.is_empty(), "{:?}", load.problems);
}

#[test]
fn valid_files_load_like_the_strict_loader_without_problems() {
    let temp = tempfile::tempdir().expect("owned config");
    let paths = paths_in(temp.path());
    write(
        &paths,
        "config.toml",
        "[projects]\nroots = ['~/a']\n[mcp]\nwrite = true\n[notes]\nvault = '~/n'\n[[terminal.profiles]]\nname = 'Nu'\ncommand = 'nu'\n",
    );
    write(
        &paths,
        "config.local.toml",
        "[pc]\nname = 'desk'\n[windows]\nroot = 'C:\\dev'\n[notes]\ninbox = 'in.md'\n",
    );
    let mut load = load_lenient(&paths);
    assert!(load.problems.is_empty(), "{:?}", load.problems);
    assert_eq!(load.disabled, Disabled::default());
    assert_eq!(load.config, Config::load(&paths).expect("strict load"));
    // No `[agents]` key, no problem: the key stays unset, which the daemon reads as on.
    assert_eq!(load.config.agents.cleanup_after_close, None);
    let merged = load.section("notes", |table| Ok::<_, String>(table.clone()));
    assert_eq!(merged.expect("merged notes").len(), 2);
}
