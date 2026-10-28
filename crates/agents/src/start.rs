use crate::command_path::shared_folder;
use crate::hooks::codex_notify_arg;
use crate::plan::sh_quote;
use crate::profile::Hooks;

pub struct StartPlan {
    pub devbox: String,
    pub instance: String,
    pub agent_id: String,
    pub launch_id: String,
    pub phase_file: String,
    pub workdir: String,
    pub setup: Vec<String>,
    pub tool_command: String,
    /// The folder of the CLI found for `tool_command`, put first on `PATH` before the CLI runs by
    /// name. A shared folder such as `/usr/bin` goes last instead, and the CLI runs by its path.
    pub tool_dir: Option<String>,
    pub hooks: Hooks,
    pub hook_file: Option<String>,
    pub prompt_file: Option<String>,
    pub resume_session: Option<String>,
    pub use_scope: bool,
    pub port: u16,
}

pub fn render_start_script(p: &StartPlan) -> String {
    let dv = sh_quote(&p.devbox);
    let mut s = String::from("#!/usr/bin/env bash\n# devbox 에이전트 시작 스크립트(자동 생성)\n");
    for (key, value) in [
        ("DEVBOX_BIN", p.devbox.as_str()),
        ("DEVBOX_AGENT_ID", p.agent_id.as_str()),
        ("DEVBOX_AGENT_LAUNCH", p.launch_id.as_str()),
        ("DEVBOX_INSTANCE", p.instance.as_str()),
        ("DEVBOX_PORT", &p.port.to_string()),
        ("DEVBOX_PROMPT_FILE", p.prompt_file.as_deref().unwrap_or("")),
    ] {
        s.push_str(&format!("export {key}={}\n", sh_quote(value)));
    }
    let phase = sh_quote(&p.phase_file);
    s.push_str("hold_terminal() { echo '[devbox] 작업이 끝났습니다. 재개 버튼으로 다시 시작하세요.'; while IFS= read -r discarded; do :; done; exit 0; }\n");
    s.push_str(&format!(
        "cd {} || {{ printf 'setup:1\\n' > {phase}; {dv} agent event --from setup --code 1; hold_terminal; }}\n",
        sh_quote(&p.workdir)
    ));
    if p.setup.is_empty() {
        s.push_str(&format!(
            "printf 'tool\\n' > {phase}\n{dv} agent event --from setup --code 0\n"
        ));
    } else {
        s.push_str("( set -e\n");
        for command in &p.setup {
            s.push_str(&format!("  {command}\n"));
        }
        s.push_str(")\n");
        s.push_str(&format!(
            "setup=$?\nif [ \"$setup\" -ne 0 ]; then printf 'setup:%s\\n' \"$setup\" > {phase}; else printf 'tool\\n' > {phase}; fi\n{dv} agent event --from setup --code \"$setup\"\n"
        ));
        s.push_str("if [ \"$setup\" -ne 0 ]; then echo \"[devbox] 준비 명령이 실패했습니다($setup).\"; hold_terminal; fi\n");
    }
    let mut tool = p.tool_command.clone();
    match p.tool_dir.as_deref() {
        // Exactly the CLI that was found. Its folder goes last, where it shadows nothing and
        // still has the interpreter a `#!/usr/bin/env` line names when nothing before it does.
        Some(dir) if shared_folder(dir) => {
            tool = by_path(dir, &p.tool_command);
            s.push_str(&format!("export PATH=\"$PATH\":{}\n", sh_quote(dir)));
        }
        // The CLI found for this task, and the interpreter its `#!/usr/bin/env` line names.
        Some(dir) => s.push_str(&format!("export PATH={}:\"$PATH\"\n", sh_quote(dir))),
        None => {}
    }
    if let (Hooks::Opencode, Some(plugin)) = (p.hooks, &p.hook_file) {
        // opencode merges this into the user's own config, so their plugins still load.
        let config = serde_json::json!({ "plugin": [file_url(plugin)] });
        s.push_str(&format!(
            "export OPENCODE_CONFIG_CONTENT={}\n",
            sh_quote(&config.to_string())
        ));
    }
    match p.hooks {
        Hooks::Claude => {
            if let Some(f) = &p.hook_file {
                tool.push_str(&format!(" --settings {}", sh_quote(f)));
            }
        }
        Hooks::Codex => tool.push_str(&format!(" -c {}", sh_quote(&codex_notify_arg(&p.devbox)))),
        Hooks::Opencode | Hooks::Pi | Hooks::None => {}
    }
    match (&p.resume_session, p.hooks) {
        (Some(id), Hooks::Claude) => tool.push_str(&format!(" --resume {}", sh_quote(id))),
        (Some(id), Hooks::Codex) => tool.push_str(&format!(" resume {}", sh_quote(id))),
        (Some(id), Hooks::Opencode) => tool.push_str(&format!(" -s {}", sh_quote(id))),
        (None, Hooks::Claude | Hooks::Codex) => {
            if let Some(f) = &p.prompt_file {
                tool.push_str(&format!(" -- \"$(cat {})\"", sh_quote(f)));
            }
        }
        // With `=` a prompt that starts with `-` is still the value, never an option.
        (None, Hooks::Opencode) => {
            if let Some(f) = &p.prompt_file {
                tool.push_str(&format!(" --prompt=\"$(cat {})\"", sh_quote(f)));
            }
        }
        _ => {}
    }
    if p.use_scope {
        let unit = format!("devbox-{}-agent-{}", p.instance, p.agent_id);
        s.push_str(&format!(
            "systemd-run --user --scope --quiet --expand-environment=no --unit {} -- {tool}\n",
            sh_quote(&unit)
        ));
    } else {
        s.push_str(&format!("{tool}\n"));
    }
    s.push_str(&format!("code=$?\nprintf 'exit:%s\\n' \"$code\" > {phase}\n{dv} agent event --from exit --code \"$code\"\necho \"[devbox] 에이전트가 끝났습니다(종료 코드 $code).\"\nhold_terminal\n"));
    s
}

/// `command` with its first word, the program the lookup found in `dir`, replaced by that
/// program's path. The rest of the command is kept as written.
fn by_path(dir: &str, command: &str) -> String {
    let command = command.trim_start_matches(|c: char| c.is_ascii_whitespace());
    let end = command
        .find(|c: char| c.is_ascii_whitespace())
        .unwrap_or(command.len());
    let (program, rest) = command.split_at(end);
    let name = program.rsplit('/').next().unwrap_or(program);
    format!("{}{rest}", sh_quote(&format!("{dir}/{name}")))
}

/// The `file://` URL of an absolute path. Every byte other than `A-Z a-z 0-9 - . _ ~ /` is
/// percent-encoded, so the URL needs no escaping in JSON or between shell single quotes.
fn file_url(path: &str) -> String {
    let mut url = String::from("file://");
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/".contains(&byte) {
            url.push(char::from(byte));
        } else {
            url.push_str(&format!("%{byte:02X}"));
        }
    }
    url
}

#[cfg(test)]
mod tests {
    use super::*;
    pub(super) fn plan() -> StartPlan {
        StartPlan {
            devbox: "/b/devbox".into(),
            instance: "prod".into(),
            agent_id: "a1".into(),
            launch_id: "launch-1".into(),
            phase_file: "/d/phase".into(),
            workdir: "/w/x y".into(),
            setup: vec!["pnpm install".into()],
            tool_command: "claude".into(),
            tool_dir: None,
            hooks: Hooks::Claude,
            hook_file: Some("/d/claude.json".into()),
            prompt_file: Some("/d/prompt.md".into()),
            resume_session: None,
            use_scope: true,
            port: 5200,
        }
    }
    #[test]
    fn injects_environment_and_runs_scoped_tool_after_setup() {
        let s = render_start_script(&plan());
        for part in [
            "export DEVBOX_BIN='/b/devbox'",
            "export DEVBOX_AGENT_ID='a1'",
            "export DEVBOX_INSTANCE='prod'",
            "export DEVBOX_PORT='5200'",
            "export DEVBOX_PROMPT_FILE='/d/prompt.md'",
            "cd '/w/x y'",
            "pnpm install",
            "--expand-environment=no",
            "claude --settings '/d/claude.json' -- \"$(cat '/d/prompt.md')\"",
            "--from exit --code \"$code\"",
        ] {
            assert!(s.contains(part), "missing {part}");
        }
        assert!(s.trim_end().ends_with("hold_terminal"));
    }
    #[cfg(unix)]
    #[test]
    fn generated_script_exports_values_and_stops_after_setup_failure() {
        use std::{
            fs,
            os::unix::fs::PermissionsExt,
            process::Command,
            time::{SystemTime, UNIX_EPOCH},
        };
        let root = std::env::temp_dir().join(format!(
            "devbox-start-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("owned fixture")
                .as_nanos()
        ));
        fs::create_dir(&root).expect("owned fixture");
        let devbox = root.join("devbox");
        let shell = root.join("bash");
        fs::write(&devbox, "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$EVENTS\"\n")
            .expect("owned fixture");
        fs::write(&shell, "#!/bin/sh\nexit 0\n").expect("owned fixture");
        for file in [&devbox, &shell] {
            fs::set_permissions(file, fs::Permissions::from_mode(0o700)).expect("owned fixture");
        }
        let output = root.join("output");
        let events = root.join("events");
        let mut p = plan();
        p.devbox = devbox.to_string_lossy().into_owned();
        p.workdir = root.to_string_lossy().into_owned();
        p.phase_file = root.join("phase").to_string_lossy().into_owned();
        p.agent_id = "a'one $HOME".into();
        p.hooks = Hooks::None;
        p.use_scope = false;
        p.setup = vec!["true".into()];
        p.tool_command = format!(
            "printf '%s' \"$DEVBOX_AGENT_ID:$DEVBOX_PORT:$DEVBOX_PROMPT_FILE\" > {}",
            sh_quote(&output.to_string_lossy())
        );
        let script = root.join("start.sh");
        let run = |p: &StartPlan| {
            fs::write(&script, render_start_script(p)).expect("owned fixture");
            Command::new("/bin/bash")
                .arg(&script)
                .env("PATH", format!("{}:/usr/bin:/bin", root.display()))
                .env("EVENTS", &events)
                .output()
                .expect("owned fixture")
        };
        assert!(run(&p).status.success());
        assert_eq!(
            fs::read_to_string(&output).expect("owned fixture"),
            "a'one $HOME:5200:/d/prompt.md"
        );
        assert_eq!(
            fs::read_to_string(&events).expect("owned fixture"),
            "agent event --from setup --code 0\nagent event --from exit --code 0\n"
        );
        fs::remove_file(&output).expect("owned fixture");
        fs::remove_file(&events).expect("owned fixture");
        p.setup = vec!["false".into(), "touch should-not-exist".into()];
        assert!(run(&p).status.success());
        assert!(!output.exists());
        assert!(!root.join("should-not-exist").exists());
        assert_eq!(
            fs::read_to_string(&events).expect("owned fixture"),
            "agent event --from setup --code 1\n"
        );
        fs::remove_dir_all(root).expect("owned fixture");
    }
    #[cfg(unix)]
    #[test]
    fn resolved_tool_directory_is_prepended_to_path() {
        use std::{fs, os::unix::fs::PermissionsExt, process::Command};
        let root = tempfile::tempdir().expect("owned fixture");
        // Only the user's interactive shell knows this folder. Its CLI is a `#!/usr/bin/env`
        // script whose interpreter sits next to it, the way nvm installs Node CLIs.
        let tools = root.path().join("it's a \"tool\" dir");
        fs::create_dir(&tools).expect("owned fixture");
        let devbox = root.path().join("devbox");
        for (file, body) in [
            (
                tools.join("fixture-node"),
                "#!/bin/sh\nexec /bin/sh \"$@\"\n",
            ),
            (
                tools.join("fixture-agent"),
                "#!/usr/bin/env fixture-node\nprintf '%s' \"$PATH\" > \"$DEVBOX_PATH_RECEIPT\"\n",
            ),
            (devbox.clone(), "#!/bin/sh\nexit 0\n"),
        ] {
            fs::write(&file, body).expect("owned fixture");
            fs::set_permissions(&file, fs::Permissions::from_mode(0o700)).expect("owned fixture");
        }
        let tools_text = tools.to_str().expect("fixture UTF-8");
        let receipt = root.path().join("path-receipt");
        let mut p = plan();
        p.devbox = devbox.to_string_lossy().into_owned();
        p.workdir = root.path().to_string_lossy().into_owned();
        p.phase_file = root.path().join("phase").to_string_lossy().into_owned();
        p.setup = vec![];
        p.hooks = Hooks::None;
        p.use_scope = false;
        p.tool_command = "fixture-agent".into();
        p.tool_dir = Some(tools_text.into());
        let script = render_start_script(&p);
        let line = format!("export PATH={}:\"$PATH\"\n", sh_quote(tools_text));
        let (before, after) = script.split_once(&line).expect("PATH line");
        assert!(after.starts_with("fixture-agent\n"), "{script}");
        assert!(before.contains("printf 'tool\\n'"), "after setup: {script}");
        let file = root.path().join("start.sh");
        fs::write(&file, &script).expect("owned fixture");
        let output = Command::new("/bin/bash")
            .arg(&file)
            .env("PATH", "/usr/bin:/bin")
            .env("DEVBOX_PATH_RECEIPT", &receipt)
            .output()
            .expect("owned fixture");
        assert!(output.status.success());
        assert_eq!(
            fs::read_to_string(&receipt).expect("the tool and its interpreter ran"),
            format!("{tools_text}:/usr/bin:/bin")
        );
        p.tool_dir = None;
        assert!(!render_start_script(&p).contains("export PATH="));
    }
    #[test]
    fn a_cli_in_a_shared_folder_runs_by_its_path_with_the_folder_last() {
        // Putting one of these first would change which systemd-run, node or other program
        // the script finds next. Last, it shadows nothing and still gives an env-shebang CLI
        // the interpreter next to it; the CLI itself runs by its absolute path.
        for dir in [
            "/usr/local/sbin",
            "/usr/local/bin",
            "/usr/sbin",
            "/usr/bin",
            "/sbin",
            "/bin",
            "/snap/bin",
            "/home/linuxbrew/.linuxbrew/bin",
        ] {
            let mut p = plan();
            p.tool_dir = Some(dir.into());
            p.tool_command = "claude --verbose".into();
            let s = render_start_script(&p);
            let last = format!("\nexport PATH=\"$PATH\":'{dir}'\n");
            assert!(s.contains(&last), "{dir}: {s}");
            assert_eq!(s.matches("export PATH=").count(), 1, "{dir}: {s}");
            assert!(!s.contains(&format!("export PATH='{dir}'")), "{dir}: {s}");
            let tool = format!(
                " -- '{dir}/claude' --verbose --settings '/d/claude.json' -- \"$(cat '/d/prompt.md')\"\n"
            );
            assert!(s.contains(&tool), "{dir}: {s}");
            assert!(s.find(&last) < s.find(&tool), "{dir}: {s}");
        }
        // Also when the command names the CLI by its path or starts with blanks.
        let mut p = plan();
        p.use_scope = false;
        p.tool_dir = Some("/usr/bin".into());
        p.tool_command = "  /usr/bin/claude".into();
        let s = render_start_script(&p);
        assert!(s.contains("\nexport PATH=\"$PATH\":'/usr/bin'\n"), "{s}");
        assert!(
            s.contains("\n'/usr/bin/claude' --settings '/d/claude.json'"),
            "{s}"
        );
        // Any other folder, a version manager's for one, still goes first and the CLI runs
        // by name, as before.
        for own in ["/home/u/.nvm/versions/node/v24.1.0/bin", "/usr/bin-extra"] {
            p.tool_dir = Some(own.into());
            p.tool_command = "claude --verbose".into();
            let s = render_start_script(&p);
            assert!(
                s.contains(&format!("export PATH='{own}':\"$PATH\"\n")),
                "{s}"
            );
            assert_eq!(s.matches("export PATH=").count(), 1, "{s}");
            assert!(
                s.contains("\nclaude --verbose --settings '/d/claude.json'"),
                "{s}"
            );
        }
    }
    fn opencode_plan() -> StartPlan {
        let mut p = plan();
        p.hooks = Hooks::Opencode;
        p.tool_command = "opencode".into();
        p.hook_file = Some("/d/agent-hooks/opencode-plugin-0123456789abcdef.ts".into());
        p
    }
    #[test]
    fn opencode_launch_exports_plugin_config_and_uses_prompt_equals() {
        let s = render_start_script(&opencode_plan());
        let config = r#"export OPENCODE_CONFIG_CONTENT='{"plugin":["file:///d/agent-hooks/opencode-plugin-0123456789abcdef.ts"]}'"#;
        let tool = r#"opencode --prompt="$(cat '/d/prompt.md')""#;
        let (before, after) = s.split_once(config).expect("plugin config line");
        assert!(before.contains("printf 'tool\\n'"), "after setup: {s}");
        assert!(after.contains(tool), "{s}");
        assert!(
            !s.contains("--settings") && !s.contains("-- \"$(cat"),
            "{s}"
        );
        // Without a plugin file there is nothing to load.
        let mut p = opencode_plan();
        p.hook_file = None;
        assert!(!render_start_script(&p).contains("OPENCODE_CONFIG_CONTENT"));
        // Characters a URL or JSON would misread are percent-encoded.
        p.hook_file = Some("/d/it's a #1 100% \"폴더\"?/opencode-plugin-x.ts".into());
        assert!(render_start_script(&p).contains(
            r#"{"plugin":["file:///d/it%27s%20a%20%231%20100%25%20%22%ED%8F%B4%EB%8D%94%22%3F/opencode-plugin-x.ts"]}"#
        ));
    }
    #[test]
    fn opencode_resume_uses_the_session_flag_without_a_prompt() {
        let mut p = opencode_plan();
        p.resume_session = Some("ses_9".into());
        let s = render_start_script(&p);
        assert!(s.contains("-- opencode -s 'ses_9'\n"), "{s}");
        assert!(s.contains("export OPENCODE_CONFIG_CONTENT="), "{s}");
        assert!(!s.contains("--prompt") && !s.contains("$(cat"), "{s}");
        // A task that never reported a session starts again from its prompt.
        p.resume_session = None;
        assert!(render_start_script(&p).contains(r#"opencode --prompt="$(cat '/d/prompt.md')""#));
    }
    #[test]
    fn resume_and_custom_profiles_have_no_prompt_argument() {
        let mut p = plan();
        p.hooks = Hooks::Codex;
        p.tool_command = "codex".into();
        p.resume_session = Some("s-9".into());
        let s = render_start_script(&p);
        assert!(s.contains("resume 's-9'"));
        assert!(!s.contains("$(cat"));
        p.hooks = Hooks::None;
        p.use_scope = false;
        p.resume_session = None;
        let s = render_start_script(&p);
        assert!(!s.contains("systemd-run"));
        assert!(!s.contains("$(cat"));
    }
}

#[cfg(all(test, unix))]
#[path = "start_prompt_tests.rs"]
mod prompt_tests;
