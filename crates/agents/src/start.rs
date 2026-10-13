use crate::hooks::codex_notify_arg;
use crate::plan::sh_quote;
use crate::profile::Hooks;

pub struct StartPlan {
    pub devbox: String,
    pub instance: String,
    pub agent_id: String,
    pub workdir: String,
    pub setup: Vec<String>,
    pub tool_command: String,
    pub hooks: Hooks,
    pub claude_settings: Option<String>,
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
        ("DEVBOX_INSTANCE", p.instance.as_str()),
        ("DEVBOX_PORT", &p.port.to_string()),
        ("DEVBOX_PROMPT_FILE", p.prompt_file.as_deref().unwrap_or("")),
    ] {
        s.push_str(&format!("export {key}={}\n", sh_quote(value)));
    }
    s.push_str(&format!(
        "cd {} || {{ {dv} agent event --from setup --code 1; exec bash -l; }}\n",
        sh_quote(&p.workdir)
    ));
    if p.setup.is_empty() {
        s.push_str(&format!("{dv} agent event --from setup --code 0\n"));
    } else {
        s.push_str("( set -e\n");
        for command in &p.setup {
            s.push_str(&format!("  {command}\n"));
        }
        s.push_str(")\n");
        s.push_str(&format!(
            "setup=$?\n{dv} agent event --from setup --code \"$setup\"\n"
        ));
        s.push_str("if [ \"$setup\" -ne 0 ]; then echo \"[devbox] 준비 명령이 실패했습니다($setup).\"; exec bash -l; fi\n");
    }
    let mut tool = p.tool_command.clone();
    match p.hooks {
        Hooks::Claude => {
            if let Some(f) = &p.claude_settings {
                tool.push_str(&format!(" --settings {}", sh_quote(f)));
            }
        }
        Hooks::Codex => tool.push_str(&format!(" -c {}", sh_quote(&codex_notify_arg(&p.devbox)))),
        Hooks::None => {}
    }
    match (&p.resume_session, p.hooks) {
        (Some(id), Hooks::Claude) => tool.push_str(&format!(" --resume {}", sh_quote(id))),
        (Some(id), Hooks::Codex) => tool.push_str(&format!(" resume {}", sh_quote(id))),
        (None, Hooks::Claude | Hooks::Codex) => {
            if let Some(f) = &p.prompt_file {
                tool.push_str(&format!(" \"$(cat {})\"", sh_quote(f)));
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
    s.push_str(&format!("code=$?\n{dv} agent event --from exit --code \"$code\"\necho \"[devbox] 에이전트가 끝났습니다(종료 코드 $code). 이 셸은 그대로 쓸 수 있습니다.\"\nexec bash -l\n"));
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    fn plan() -> StartPlan {
        StartPlan {
            devbox: "/b/devbox".into(),
            instance: "prod".into(),
            agent_id: "a1".into(),
            workdir: "/w/x y".into(),
            setup: vec!["pnpm install".into()],
            tool_command: "claude".into(),
            hooks: Hooks::Claude,
            claude_settings: Some("/d/claude.json".into()),
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
            "claude --settings '/d/claude.json' \"$(cat '/d/prompt.md')\"",
            "--from exit --code \"$code\"",
        ] {
            assert!(s.contains(part), "missing {part}");
        }
        assert!(s.trim_end().ends_with("exec bash -l"));
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
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let devbox = root.join("devbox");
        let shell = root.join("bash");
        fs::write(&devbox, "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$EVENTS\"\n").unwrap();
        fs::write(&shell, "#!/bin/sh\nexit 0\n").unwrap();
        for file in [&devbox, &shell] {
            fs::set_permissions(file, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let output = root.join("output");
        let events = root.join("events");
        let mut p = plan();
        p.devbox = devbox.to_string_lossy().into_owned();
        p.workdir = root.to_string_lossy().into_owned();
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
            fs::write(&script, render_start_script(p)).unwrap();
            Command::new("/bin/bash")
                .arg(&script)
                .env("PATH", format!("{}:/usr/bin:/bin", root.display()))
                .env("EVENTS", &events)
                .output()
                .unwrap()
        };
        assert!(run(&p).status.success());
        assert_eq!(
            fs::read_to_string(&output).unwrap(),
            "a'one $HOME:5200:/d/prompt.md"
        );
        assert_eq!(
            fs::read_to_string(&events).unwrap(),
            "agent event --from setup --code 0\nagent event --from exit --code 0\n"
        );
        fs::remove_file(&output).unwrap();
        fs::remove_file(&events).unwrap();
        p.setup = vec!["false".into(), "touch should-not-exist".into()];
        assert!(run(&p).status.success());
        assert!(!output.exists());
        assert!(!root.join("should-not-exist").exists());
        assert_eq!(
            fs::read_to_string(&events).unwrap(),
            "agent event --from setup --code 1\n"
        );
        fs::remove_dir_all(root).unwrap();
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
