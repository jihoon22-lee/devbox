//! pi's tool line: devbox's extension, the session devbox chose, and the prompt after `--`.
use super::tests::plan;
use super::*;

const EXTENSION: &str = "/d/agent-hooks/pi-extension-0123456789abcdef.ts";
const SESSION: &str = "6f1c2d3e-4b5a-4c6d-8e7f-001122334455";

fn pi_plan() -> StartPlan {
    let mut p = plan();
    p.hooks = Hooks::Pi;
    p.tool_command = "pi".into();
    p.hook_file = Some(EXTENSION.into());
    p.session_id = Some(SESSION.into());
    p
}

#[test]
fn pi_launch_loads_the_extension_with_a_session_id_and_prompt_after_double_dash() {
    let s = render_start_script(&pi_plan());
    let tool =
        format!(" -- pi -e '{EXTENSION}' --session-id '{SESSION}' -- \"$(cat '/d/prompt.md')\"\n");
    let (before, _) = s.split_once(&tool).expect("pi tool line");
    assert!(before.contains("printf 'tool\\n'"), "after setup: {s}");
    // The extension is pi's only connection to devbox.
    for other in ["--settings", "OPENCODE_CONFIG_CONTENT", "--prompt", " -c "] {
        assert!(!s.contains(other), "{other}: {s}");
    }
    // A path or id a shell would split or expand stays one argument.
    let mut odd = pi_plan();
    odd.use_scope = false;
    odd.hook_file = Some("/d/it's a $HOME/pi-extension-x.ts".into());
    assert!(render_start_script(&odd).contains(&format!(
        "\npi -e '/d/it'\\''s a $HOME/pi-extension-x.ts' --session-id '{SESSION}' -- "
    )));
    // Without an extension file or a session, those options are left out rather than empty.
    odd.hook_file = None;
    odd.session_id = None;
    assert!(
        render_start_script(&odd).contains("\npi -- \"$(cat '/d/prompt.md')\"\n"),
        "{}",
        render_start_script(&odd)
    );
}

#[test]
fn pi_resume_reuses_the_session_id_without_a_prompt() {
    let mut p = pi_plan();
    p.session_id = None;
    p.resume_session = Some(SESSION.into());
    let s = render_start_script(&p);
    assert!(
        s.contains(&format!(
            " -- pi -e '{EXTENSION}' --session-id '{SESSION}'\n"
        )),
        "{s}"
    );
    assert!(!s.contains("$(cat") && !s.contains(" -- \""), "{s}");
    // The session a resume names wins over one a new launch would start.
    p.session_id = Some("unused-new-session".into());
    assert!(!render_start_script(&p).contains("unused-new-session"));
}
