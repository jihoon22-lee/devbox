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

#[test]
fn prompt_with_a_leading_dash_quotes_and_dollar_is_passed_verbatim() {
    let root = tempfile::tempdir().expect("owned argv fixtures");
    // Python's own URL parser reads the plugin path back, independently of devbox's encoder.
    let tool = root.path().join("opencode");
    fs::write(&tool, "#!/usr/bin/env python3\nimport json,os,sys,urllib.parse,urllib.request\nconfig=json.loads(os.environ['OPENCODE_CONFIG_CONTENT'])\nurl=urllib.parse.urlparse(config['plugin'][0])\njson.dump({'argv':sys.argv[1:],'scheme':url.scheme,'path':urllib.request.url2pathname(url.path)},open(os.environ['DEVBOX_ARGV_RECEIPT'],'w'))\n").expect("owned argv receiver");
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
    let plugin = "/owned/it's a #1 100% \"폴더\"?/opencode-plugin-0123456789abcdef.ts";
    let prompt_file = root.path().join("prompt.md");
    let receipt = root.path().join("argv.json");
    for use_scope in [false, true] {
        for prompt in [
            "-x",
            "--model=unrequested",
            "--",
            "-s ses_other",
            "quotes '\" $HOME $(touch injected) `touch ticked` \\ end",
            "한국어 --option\n두 번째 줄",
            "",
        ] {
            fs::write(&prompt_file, prompt).expect("owned prompt");
            let mut p = plan();
            p.devbox = devbox.to_string_lossy().into_owned();
            p.workdir = root.path().to_string_lossy().into_owned();
            p.phase_file = root.path().join("phase").to_string_lossy().into_owned();
            p.setup = vec![];
            p.use_scope = use_scope;
            p.hooks = Hooks::Opencode;
            p.tool_command = sh_quote(tool.to_str().expect("tool"));
            p.hook_file = Some(plugin.into());
            p.prompt_file = Some(prompt_file.to_string_lossy().into_owned());
            let script = root.path().join("start.sh");
            fs::write(&script, render_start_script(&p)).expect("script");
            let output = Command::new("/bin/bash")
                .arg(&script)
                .current_dir(root.path())
                .env("PATH", format!("{}:/usr/bin:/bin", root.path().display()))
                .env("DEVBOX_ARGV_RECEIPT", &receipt)
                .output()
                .expect("owned rendered script");
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let seen: serde_json::Value =
                serde_json::from_str(&fs::read_to_string(&receipt).expect("argv receipt"))
                    .expect("receipt");
            assert_eq!(
                seen["argv"],
                serde_json::json!([format!("--prompt={prompt}")]),
                "scope={use_scope}, prompt={prompt:?}"
            );
            assert_eq!(seen["scheme"], "file");
            assert_eq!(seen["path"], plugin);
        }
    }
    for marker in ["injected", "ticked"] {
        assert!(!root.path().join(marker).exists(), "{marker}");
    }
}

#[test]
fn pi_prompts_reach_pi_as_one_message_after_double_dash() {
    let root = tempfile::tempdir().expect("owned argv fixtures");
    let tool = root.path().join("pi");
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
    let extension = "/owned/it's a \"dir\"/pi-extension-0123456789abcdef.ts";
    let session = "6f1c2d3e-4b5a-4c6d-8e7f-001122334455";
    let prompt_file = root.path().join("prompt.md");
    let receipt = root.path().join("argv.json");
    for use_scope in [false, true] {
        for (prompt, message) in [
            ("-x", "-x"),
            ("--", "--"),
            ("--session-id other", "--session-id other"),
            ("--model=unrequested", "--model=unrequested"),
            (
                "quotes '\" $HOME $(touch injected) `touch ticked` \\ end",
                "quotes '\" $HOME $(touch injected) `touch ticked` \\ end",
            ),
            ("한국어 --option\n두 번째 줄", "한국어 --option\n두 번째 줄"),
            ("", ""),
            // pi attaches an argument starting with `@` as a file, even after `--`.
            ("@notes.md fix it", " @notes.md fix it"),
            ("@", " @"),
            ("see @notes.md", "see @notes.md"),
        ] {
            let text = crate::prepare::prompt_text(Hooks::Pi, prompt);
            fs::write(&prompt_file, text.as_bytes()).expect("owned prompt");
            let mut p = plan();
            p.devbox = devbox.to_string_lossy().into_owned();
            p.workdir = root.path().to_string_lossy().into_owned();
            p.phase_file = root.path().join("phase").to_string_lossy().into_owned();
            p.setup = vec![];
            p.use_scope = use_scope;
            p.hooks = Hooks::Pi;
            p.tool_command = sh_quote(tool.to_str().expect("tool"));
            p.hook_file = Some(extension.into());
            p.session_id = Some(session.into());
            p.prompt_file = Some(prompt_file.to_string_lossy().into_owned());
            let script = root.path().join("start.sh");
            fs::write(&script, render_start_script(&p)).expect("script");
            let output = Command::new("/bin/bash")
                .arg(&script)
                .current_dir(root.path())
                .env("PATH", format!("{}:/usr/bin:/bin", root.path().display()))
                .env("DEVBOX_ARGV_RECEIPT", &receipt)
                .output()
                .expect("owned rendered script");
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let args: Vec<String> =
                serde_json::from_str(&fs::read_to_string(&receipt).expect("argv receipt"))
                    .expect("argv");
            assert_eq!(
                args,
                ["-e", extension, "--session-id", session, "--", message],
                "scope={use_scope}, prompt={prompt:?}"
            );
        }
    }
    for marker in ["injected", "ticked"] {
        assert!(!root.path().join(marker).exists(), "{marker}");
    }
}
