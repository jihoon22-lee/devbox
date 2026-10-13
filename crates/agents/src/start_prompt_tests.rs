use super::tests::plan;
use super::*;
use std::{fs, os::unix::fs::PermissionsExt, process::Command};

#[test]
fn generated_initial_prompts_are_literal_argv_after_tool_options_and_before_scope_exit() {
    let root = tempfile::tempdir().expect("owned argv fixtures");
    let tool = root.path().join("tool");
    fs::write(&tool, "#!/usr/bin/env python3\nimport json,os,sys\njson.dump(sys.argv[1:],open(os.environ['DEVBOX_ARGV_RECEIPT'],'w'))\n").expect("owned argv receiver");
    let devbox = root.path().join("devbox");
    fs::write(&devbox, "#!/bin/sh\nexit 0\n").expect("fake hook");
    let scope = root.path().join("systemd-run");
    fs::write(
        &scope,
        "#!/bin/sh\nwhile [ \"$1\" != -- ]; do shift; done\nshift\nexec \"$@\"\n",
    )
    .expect("fake owned scope");
    for file in [&tool, &devbox, &scope] {
        fs::set_permissions(file, fs::Permissions::from_mode(0o700)).expect("mode");
    }
    let prompt_file = root.path().join("prompt.md");
    let receipt = root.path().join("argv.json");
    for hooks in [Hooks::Claude, Hooks::Codex] {
        for use_scope in [false, true] {
            for prompt in [
                "--dangerously-skip-permissions",
                "--dangerously-bypass-approvals-and-sandbox",
                "--model=unrequested",
                "quotes '\" $HOME\n한국어 --option",
                "",
                "--",
            ] {
                fs::write(&prompt_file, prompt).expect("owned prompt");
                let mut p = plan();
                p.devbox = devbox.to_string_lossy().into_owned();
                p.workdir = root.path().to_string_lossy().into_owned();
                p.phase_file = root.path().join("phase").to_string_lossy().into_owned();
                p.setup = vec![];
                p.use_scope = use_scope;
                p.hooks = hooks;
                p.tool_command = sh_quote(tool.to_str().expect("tool"));
                p.prompt_file = Some(prompt_file.to_string_lossy().into_owned());
                let script = root.path().join("start.sh");
                fs::write(&script, render_start_script(&p)).expect("script");
                let output = Command::new("/bin/bash")
                    .arg(&script)
                    .env("PATH", format!("{}:/usr/bin:/bin", root.path().display()))
                    .env("DEVBOX_ARGV_RECEIPT", &receipt)
                    .output()
                    .expect("owned rendered script");
                assert!(
                    output.status.success(),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
                let args: Vec<String> = serde_json::from_str(
                    &fs::read_to_string(&receipt).expect("actual argv receipt"),
                )
                .expect("argv");
                assert_eq!(args.last().map(String::as_str), Some(prompt));
                assert_eq!(
                    args.get(args.len() - 2).map(String::as_str),
                    Some("--"),
                    "hook={hooks:?},scope={use_scope},prompt={prompt:?}: {args:?}"
                );
                assert_eq!(
                    args.len(),
                    4,
                    "tool-owned settings/notify pair then delimiter and one prompt"
                );
            }
        }
    }
}
