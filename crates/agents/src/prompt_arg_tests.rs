use super::*;
use crate::start::{StartPlan, render_start_script};

fn profile(hooks: Hooks, prompt_arg: Option<&str>) -> ToolProfile {
    ToolProfile {
        id: "mine".into(),
        label: "Mine".into(),
        command: "mine".into(),
        hooks,
        waiting_patterns: vec![],
        prompt_arg: prompt_arg.map(str::to_owned),
    }
}

fn plan(prompt_arg: Option<&str>) -> StartPlan {
    StartPlan {
        devbox: "/b/devbox".into(),
        instance: "prod".into(),
        agent_id: "a1".into(),
        launch_id: "launch-1".into(),
        phase_file: "/d/phase".into(),
        workdir: "/w".into(),
        setup: vec![],
        tool_command: "mine --verbose".into(),
        tool_dir: None,
        hooks: Hooks::None,
        hook_file: None,
        prompt_file: Some("/d/it's prompt.md".into()),
        prompt_arg: prompt_arg.map(str::to_owned),
        resume_session: None,
        session_id: None,
        use_scope: false,
        port: 5200,
    }
}

#[test]
fn the_flag_is_one_plain_option_or_the_empty_string_or_two_dashes() {
    for flag in [
        "",
        "--",
        "-p",
        "-i",
        "--prompt",
        "--prompt=",
        "--prompt-interactive",
        "--initial_prompt",
        "-2",
    ] {
        assert!(check(&profile(Hooks::None, Some(flag))).is_ok(), "{flag:?}");
    }
    let long = format!("--{}", "x".repeat(MAX_FLAG_BYTES));
    for flag in [
        "prompt",
        "-",
        "---",
        "---prompt",
        "--prompt ",
        " --prompt",
        "--prompt --yes",
        "--prompt=x",
        "--prompt==",
        "=",
        "--=",
        "--prompt;rm",
        "--prompt$(id)",
        "--prompt`id`",
        "--pro'mpt",
        "--pro\"mpt",
        "--prompt\n",
        "--지시문",
        "-- ",
        long.as_str(),
    ] {
        let refused = check(&profile(Hooks::None, Some(flag)));
        assert!(
            matches!(&refused, Err(ProfileError::PromptArg { profile, .. }) if profile == "mine"),
            "{flag:?}: {refused:?}"
        );
    }
    // Without the key nothing is checked, whatever the family.
    for hooks in [Hooks::None, Hooks::Claude, Hooks::Antigravity] {
        assert!(check(&profile(hooks, None)).is_ok());
    }
}

#[test]
fn only_a_none_profile_takes_the_key() {
    for hooks in [
        Hooks::Claude,
        Hooks::Codex,
        Hooks::Opencode,
        Hooks::Pi,
        Hooks::Antigravity,
    ] {
        let refused = check(&profile(hooks, Some("--prompt")));
        assert!(
            matches!(refused, Err(ProfileError::PromptArg { .. })),
            "{hooks:?}"
        );
    }
}

#[test]
fn the_key_is_read_from_the_settings_file_in_either_spelling_and_checked() {
    let config = |key: &str, value: &str, hooks: &str| {
        let text = format!(
            "[[agents.profiles]]\nid='x'\nlabel='X'\ncommand='x'\nhooks='{hooks}'\n{key}={value}\n"
        );
        toml::from_str::<devbox_core::Config>(&text)
            .expect("owned config")
            .agents
    };
    for key in ["promptArg", "prompt_arg"] {
        let profiles = crate::profile::configured(&config(key, "'--prompt='", "none"))
            .unwrap_or_else(|error| panic!("{key}: {error}"));
        assert_eq!(profiles[0].prompt_arg.as_deref(), Some("--prompt="));
    }
    let bare = crate::profile::configured(&config("promptArg", "''", "none")).expect("bare");
    assert_eq!(bare[0].prompt_arg.as_deref(), Some(""));
    // A profile without the key is as before.
    let text = "[[agents.profiles]]\nid='x'\nlabel='X'\ncommand='x'\nhooks='none'\n";
    let plain = toml::from_str::<devbox_core::Config>(text).expect("owned config");
    let plain = crate::profile::configured(&plain.agents).expect("plain");
    assert_eq!(plain[0].prompt_arg, None);
    // A bad value or a tool family's profile is reported, and only that entry is skipped.
    for (value, hooks) in [
        ("'--prompt x'", "none"),
        ("'--prompt'", "claude"),
        ("7", "none"),
    ] {
        let config = config("promptArg", value, hooks);
        assert!(
            crate::profile::configured(&config).is_err(),
            "{value} {hooks}"
        );
        let (profiles, problems) = crate::profile::configured_lenient(&config);
        assert!(profiles.is_empty(), "{value} {hooks}");
        assert_eq!(problems.len(), 1, "{value} {hooks}: {problems:?}");
    }
}

#[test]
fn an_empty_prompt_or_a_tool_family_gets_no_prompt_argument() {
    let none = profile(Hooks::None, Some("--prompt"));
    assert_eq!(for_launch(&none, "fix it").as_deref(), Some("--prompt"));
    assert_eq!(for_launch(&none, "-x").as_deref(), Some("--prompt"));
    for empty in ["", " ", "\n\t "] {
        assert_eq!(for_launch(&none, empty), None, "{empty:?}");
    }
    // The bare form is a value too.
    let bare = profile(Hooks::None, Some(""));
    assert_eq!(for_launch(&bare, "fix it").as_deref(), Some(""));
    assert_eq!(for_launch(&profile(Hooks::None, None), "fix it"), None);
    // A programmatic profile of a tool family never gets one, even with the field set.
    assert_eq!(
        for_launch(&profile(Hooks::Claude, Some("--prompt")), "fix it"),
        None
    );
}

#[test]
fn a_prompt_the_tool_could_read_as_an_option_is_refused_where_it_is_an_argument_of_its_own() {
    use crate::model::AgentsError;
    let refused = |flag: &str, prompt: &str| {
        matches!(
            admit(&profile(Hooks::None, Some(flag)), prompt),
            Err(AgentsError::PromptOption { tool }) if tool == "Mine"
        )
    };
    // The prompt alone, or the value of a flag in the next argument: a leading `-`, whatever
    // blanks and line breaks come before it, may be read as an option.
    for flag in ["", "--prompt", "-p"] {
        for prompt in ["-x", "--model=unrequested", "--", "-", "  -x", "\n\t -x"] {
            assert!(refused(flag, prompt), "{flag:?} {prompt:?}");
        }
        for prompt in ["fix it", "x -y", "\u{2010}x", "", " ", "\n"] {
            assert!(
                admit(&profile(Hooks::None, Some(flag)), prompt).is_ok(),
                "{flag:?} {prompt:?}"
            );
        }
    }
    // The joined form and `--` keep any prompt an argument.
    for flag in ["--prompt=", "-p=", "--"] {
        assert!(
            admit(&profile(Hooks::None, Some(flag)), "-x").is_ok(),
            "{flag:?}"
        );
    }
    // Without the key the prompt is not an argument, and the families pass it their own way.
    assert!(admit(&profile(Hooks::None, None), "-x").is_ok());
    assert!(admit(&profile(Hooks::Claude, Some("--prompt")), "-x").is_ok());
    assert_eq!(
        devbox_protocol::to_wire(&AgentsError::PromptOption { tool: "x".into() }).code,
        "agents.prompt_option"
    );
}

#[test]
fn the_start_script_appends_the_prompt_after_the_command_in_each_form() {
    let tool = |prompt_arg: Option<&str>| {
        let script = render_start_script(&plan(prompt_arg));
        let line = script
            .lines()
            .find(|line| line.starts_with("mine --verbose"))
            .unwrap_or_else(|| panic!("tool line: {script}"));
        line.to_owned()
    };
    let prompt = r#""$(cat '/d/it'\''s prompt.md')""#;
    assert_eq!(
        tool(Some("--prompt")),
        format!("mine --verbose --prompt {prompt}")
    );
    assert_eq!(tool(Some("-p")), format!("mine --verbose -p {prompt}"));
    assert_eq!(
        tool(Some("--prompt=")),
        format!("mine --verbose --prompt={prompt}")
    );
    assert_eq!(tool(Some("--")), format!("mine --verbose -- {prompt}"));
    assert_eq!(tool(Some("")), format!("mine --verbose {prompt}"));
    // Without the key the command is as it was: a `none` profile gets no prompt argument.
    assert_eq!(tool(None), "mine --verbose");
    let mut unchanged = plan(None);
    unchanged.prompt_file = Some("/d/prompt.md".into());
    assert!(!render_start_script(&unchanged).contains("$(cat"));
    // The key belongs to `none`: a tool family's own form is never doubled.
    let mut claude = plan(Some("--prompt"));
    claude.hooks = Hooks::Claude;
    let script = render_start_script(&claude);
    assert!(
        !script.contains("--prompt") && script.contains(" -- \"$(cat "),
        "{script}"
    );
}

#[cfg(unix)]
#[test]
fn the_prompt_reaches_the_tool_as_one_argument_whatever_it_holds() {
    use crate::test_executable::write_executable;
    use std::{fs, process::Command};
    let root = tempfile::tempdir().expect("owned argv fixtures");
    let tool = root.path().join("tool");
    write_executable(
        &tool,
        "#!/usr/bin/env python3\nimport json,os,sys\njson.dump(sys.argv[1:],open(os.environ['DEVBOX_ARGV_RECEIPT'],'w'))\n",
    );
    let devbox = root.path().join("devbox");
    write_executable(&devbox, "#!/bin/sh\nexit 0\n");
    let prompt_file = root.path().join("it's a prompt.md");
    let receipt = root.path().join("argv.json");
    let prompts = [
        "fix the login bug",
        "quotes ' \" ` $HOME $(touch injected) \\ end",
        "first line\nsecond line\n\nfourth",
        "-x",
        "--model=unrequested",
        "--",
        "한국어 지시문 --option",
    ];
    for (flag, before) in [
        ("--prompt", vec!["--prompt"]),
        ("-p", vec!["-p"]),
        ("--", vec!["--"]),
        ("", vec![]),
    ] {
        for prompt in prompts {
            fs::write(&prompt_file, prompt).expect("owned prompt");
            let mut p = plan(Some(flag));
            p.devbox = devbox.to_string_lossy().into_owned();
            p.workdir = root.path().to_string_lossy().into_owned();
            p.phase_file = root.path().join("phase").to_string_lossy().into_owned();
            p.tool_command = sh_quote(tool.to_str().expect("tool"));
            p.prompt_file = Some(prompt_file.to_string_lossy().into_owned());
            let script = root.path().join("start.sh");
            fs::write(&script, render_start_script(&p)).expect("script");
            let output = Command::new("/bin/bash")
                .arg(&script)
                .current_dir(root.path())
                .env("PATH", "/usr/bin:/bin")
                .env("DEVBOX_ARGV_RECEIPT", &receipt)
                .output()
                .expect("owned rendered script");
            assert!(output.status.success());
            let args: Vec<String> =
                serde_json::from_str(&fs::read_to_string(&receipt).expect("argv receipt"))
                    .expect("argv");
            let mut expected: Vec<&str> = before.clone();
            expected.push(prompt);
            assert_eq!(args, expected, "{flag:?} {prompt:?}");
        }
    }
    // The joined form is one argument: the flag, `=` and the prompt.
    fs::write(&prompt_file, "-x 'quoted'\nnext").expect("owned prompt");
    let mut p = plan(Some("--prompt="));
    p.devbox = devbox.to_string_lossy().into_owned();
    p.workdir = root.path().to_string_lossy().into_owned();
    p.phase_file = root.path().join("phase").to_string_lossy().into_owned();
    p.tool_command = sh_quote(tool.to_str().expect("tool"));
    p.prompt_file = Some(prompt_file.to_string_lossy().into_owned());
    let script = root.path().join("start.sh");
    fs::write(&script, render_start_script(&p)).expect("script");
    let output = Command::new("/bin/bash")
        .arg(&script)
        .env("PATH", "/usr/bin:/bin")
        .env("DEVBOX_ARGV_RECEIPT", &receipt)
        .output()
        .expect("owned rendered script");
    assert!(output.status.success());
    let args: Vec<String> =
        serde_json::from_str(&fs::read_to_string(&receipt).expect("argv receipt")).expect("argv");
    assert_eq!(args, ["--prompt=-x 'quoted'\nnext"]);
    assert!(!root.path().join("injected").exists());
}
