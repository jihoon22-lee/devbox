use crate::{CheckRef, InstallPlan, ToolsInstallCommand, WindowsPackage, install::install_command};
use devbox_protocol::{Confirm, Method, MethodKind};
fn plan(id: &str) -> InstallPlan {
    install_command(CheckRef { id: id.into() }).expect("static plan")
}
#[test]
fn install_plans_use_only_the_fixed_allowlist() {
    for (id, expected) in [
        ("tool.tmux", "sudo apt install tmux"),
        ("tool.git", "sudo apt install git"),
        ("tool.gh", "sudo apt install gh"),
        ("tool.pnpm", "npm i -g pnpm@9"),
        ("tool.claude", "npm i -g @anthropic-ai/claude-code"),
        ("tool.codex", "npm i -g @openai/codex"),
    ] {
        let InstallPlan::Wsl {
            command,
            title,
            shell_command,
        } = plan(id)
        else {
            panic!("WSL plan")
        };
        assert_eq!(command, expected);
        assert!(!title.is_empty());
        assert_eq!(
            shell_command,
            format!("bash -lc '{expected}; exec bash -l'")
        );
    }
    for id in ["tool.node", "tool.rust"] {
        assert!(matches!(plan(id), InstallPlan::Manual { .. }));
    }
    assert_eq!(
        plan("tool.docker"),
        InstallPlan::Windows {
            package: WindowsPackage::DockerDesktop
        }
    );
    for id in [
        "unknown",
        "tool.tmux; touch /tmp/not-allowed",
        "sudo",
        "hook.binary",
    ] {
        assert_eq!(plan(id), InstallPlan::None);
    }
    assert!(
        install_command(CheckRef {
            id: "x".repeat(129)
        })
        .is_err()
    );
}
#[test]
fn install_suggestions_are_queries_and_windows_packages_are_typed() {
    assert_eq!(ToolsInstallCommand::NAME, "tools.install_command");
    assert_eq!(ToolsInstallCommand::KIND, MethodKind::Query);
    assert_eq!(ToolsInstallCommand::CONFIRM, Confirm::None);
    assert!(serde_json::from_str::<WindowsPackage>("\"arbitrary.package\"").is_err());
    let wire = serde_json::to_value(plan("tool.tmux")).expect("wire DTO");
    assert!(wire.get("shellCommand").is_some());
}
#[test]
fn wsl_install_runs_in_a_shell_that_stays_open_and_quotes_single_quotes() {
    assert_eq!(
        crate::install::shell_command("printf 'fixture'"),
        "bash -lc 'printf '\\''fixture'\\''; exec bash -l'"
    );
}
