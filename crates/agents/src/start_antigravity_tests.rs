use super::tests::plan;
use super::*;

fn agy_plan() -> StartPlan {
    let mut p = plan();
    p.hooks = Hooks::Antigravity;
    p.tool_command = "agy".into();
    p.workdir = "/w/devbox-agents/fix".into();
    p.hook_file = Some("/w/devbox-agents/fix.agy-hooks".into());
    p
}

/// The row of the script that runs the tool.
fn tool_row(script: &str) -> &str {
    script
        .lines()
        .find(|row| row.contains("agy"))
        .unwrap_or_else(|| panic!("tool row: {script}"))
}

#[test]
fn a_first_launch_adds_the_hooks_folder_and_passes_no_prompt() {
    let s = render_start_script(&agy_plan());
    assert!(s.contains("cd '/w/devbox-agents/fix' || "), "{s}");
    assert!(
        tool_row(&s).ends_with(" -- agy --add-dir '/w/devbox-agents/fix.agy-hooks'"),
        "{s}"
    );
    // No prompt argument: with `-i` in a folder not trusted yet, agy 1.5.17 runs the prompt's turn
    // with the hooks folder as its only workspace. And no session trust: the user decides that.
    for absent in [
        "$(cat",
        " -i ",
        "--prompt",
        "--settings",
        "trustedWorkspaces",
        "--conversation",
    ] {
        assert!(!s.contains(absent), "{absent}: {s}");
    }
    // The prompt stays readable from its file, and the hooks get the task from the environment.
    for export in [
        "export DEVBOX_PROMPT_FILE='/d/prompt.md'",
        "export DEVBOX_AGENT_ID='a1'",
        "export DEVBOX_AGENT_LAUNCH='launch-1'",
        "export DEVBOX_INSTANCE='prod'",
    ] {
        assert!(s.contains(export), "{export}: {s}");
    }
}

#[test]
fn a_resume_names_the_conversation_after_the_hooks_folder() {
    let mut p = agy_plan();
    p.use_scope = false;
    p.resume_session = Some("00000000-0000-4000-8000-000000000001".into());
    let s = render_start_script(&p);
    assert_eq!(
        tool_row(&s),
        "agy --add-dir '/w/devbox-agents/fix.agy-hooks' --conversation '00000000-0000-4000-8000-000000000001'"
    );
    assert!(!s.contains("$(cat"), "{s}");
    // A task that never reported a conversation starts agy again, without one.
    p.resume_session = None;
    assert_eq!(
        tool_row(&render_start_script(&p)),
        "agy --add-dir '/w/devbox-agents/fix.agy-hooks'"
    );
}

#[test]
fn paths_and_ids_are_quoted_and_the_profiles_own_arguments_stay() {
    let mut p = agy_plan();
    p.use_scope = false;
    p.tool_command = "agy --model 'x y'".into();
    p.hook_file = Some("/w/it's a \"dir\" $HOME/fix.agy-hooks".into());
    p.resume_session = Some("id'; touch injected #".into());
    assert_eq!(
        tool_row(&render_start_script(&p)),
        r#"agy --model 'x y' --add-dir '/w/it'\''s a "dir" $HOME/fix.agy-hooks' --conversation 'id'\''; touch injected #'"#
    );
    // A CLI found in a shared folder runs by its path, like the other families'.
    let mut p = agy_plan();
    p.use_scope = false;
    p.tool_dir = Some("/usr/local/bin".into());
    assert_eq!(
        tool_row(&render_start_script(&p)),
        "'/usr/local/bin/agy' --add-dir '/w/devbox-agents/fix.agy-hooks'"
    );
    // Without a hooks folder nothing is added: the service always gives one.
    let mut p = agy_plan();
    p.use_scope = false;
    p.hook_file = None;
    assert_eq!(tool_row(&render_start_script(&p)), "agy");
}

#[cfg(unix)]
#[test]
fn the_tool_gets_exactly_the_folder_and_conversation_arguments() {
    use crate::test_executable::write_executable;
    use std::{fs, process::Command};
    let root = tempfile::tempdir().expect("owned argv fixtures");
    let tool = root.path().join("agy");
    write_executable(
        &tool,
        "#!/usr/bin/env python3\nimport json,os,sys\njson.dump({'argv':sys.argv[1:],'cwd':os.getcwd(),'agent':os.environ.get('DEVBOX_AGENT_ID'),'launch':os.environ.get('DEVBOX_AGENT_LAUNCH')},open(os.environ['DEVBOX_ARGV_RECEIPT'],'w'))\n",
    );
    let devbox = root.path().join("devbox");
    write_executable(&devbox, "#!/bin/sh\nexit 0\n");
    let workdir = root.path().join("it's a fix");
    fs::create_dir(&workdir).expect("owned work folder");
    let hooks = crate::antigravity::hooks_dir(workdir.to_str().expect("UTF-8"));
    let prompt = root.path().join("prompt.md");
    fs::write(&prompt, "-i --dangerous 'prompt'").expect("owned prompt");
    let receipt = root.path().join("argv.json");
    let mut p = agy_plan();
    p.devbox = devbox.to_string_lossy().into_owned();
    p.workdir = workdir.to_string_lossy().into_owned();
    p.phase_file = root.path().join("phase").to_string_lossy().into_owned();
    p.setup = vec![];
    p.use_scope = false;
    p.tool_dir = Some(root.path().to_string_lossy().into_owned());
    p.hook_file = Some(hooks.to_string_lossy().into_owned());
    p.prompt_file = Some(prompt.to_string_lossy().into_owned());
    for (resume, expected) in [
        (None, vec!["--add-dir", hooks.to_str().expect("UTF-8")]),
        (
            Some("conversation-1"),
            vec![
                "--add-dir",
                hooks.to_str().expect("UTF-8"),
                "--conversation",
                "conversation-1",
            ],
        ),
    ] {
        p.resume_session = resume.map(str::to_owned);
        let script = root.path().join("start.sh");
        fs::write(&script, render_start_script(&p)).expect("script");
        let output = Command::new("/bin/bash")
            .arg(&script)
            .env("PATH", "/usr/bin:/bin")
            .env("DEVBOX_ARGV_RECEIPT", &receipt)
            .output()
            .expect("owned rendered script");
        assert!(output.status.success());
        let ran: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&receipt).expect("argv receipt"))
                .expect("receipt");
        assert_eq!(ran["argv"], serde_json::json!(expected), "{resume:?}");
        // agy starts in the work folder, never in the hooks folder.
        assert_eq!(ran["cwd"], serde_json::json!(workdir.to_str()));
        assert_eq!(ran["agent"], "a1");
        assert_eq!(ran["launch"], "launch-1");
    }
}
