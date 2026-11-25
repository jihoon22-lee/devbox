//! A profile's waiting patterns come with its tool family, whatever its id.
use super::*;

#[test]
fn every_profile_of_a_tool_family_keeps_its_patterns_and_adds_its_own() {
    let profile = |id: &str, hooks, patterns: &[&str]| profile::ToolProfile {
        id: id.into(),
        label: id.into(),
        command: id.into(),
        hooks,
        waiting_patterns: patterns.iter().map(|&p| p.to_owned()).collect(),
        prompt_arg: None,
    };
    let compiled = profile::compiled(&crate::AgentProfiles {
        items: vec![
            // A configured `codex` replaces the builtin one; the others are added.
            profile("codex", profile::Hooks::Codex, &[]),
            profile("codex-fast", profile::Hooks::Codex, &["(?m)^ready$"]),
            profile("claude-work", profile::Hooks::Claude, &[]),
            profile("pi-alt", profile::Hooks::Pi, &[]),
            profile("opencode-alt", profile::Hooks::Opencode, &[]),
            profile("mine", profile::Hooks::None, &["(?m)^ready$"]),
        ],
    })
    .expect("owned profiles");
    let network = screen(include_str!(
        "../tests/fixtures/codex-0.166.0/approval-network.txt"
    ));
    let dialog = screen(include_str!(
        "../tests/fixtures/codex-0.166.0/dialog-resume-goal-200x50.txt"
    ));
    let claude = screen(include_str!("../tests/fixtures/claude-2.1.308/trust.txt"));
    let claude_shell = screen(include_str!(
        "../tests/fixtures/claude-2.1.308/shell-mode.txt"
    ));
    let pi = screen(include_str!("../tests/fixtures/pi-1.3.0/trust.txt"));
    let opencode = screen(include_str!(
        "../tests/fixtures/opencode-1.19.3/shell-mode.txt"
    ));
    let waits = |id: &str, shown: &str| matches_waiting(&compiled[id], shown);
    for id in ["codex", "codex-fast"] {
        assert!(waits(id, network), "{id}");
        assert!(waits(id, dialog), "{id}");
        assert!(!waits(id, opencode), "{id}");
        assert!(!waits(id, claude_shell), "{id}");
    }
    assert!(waits("claude-work", claude));
    // Claude's shell mode comes with its family, whatever the profile's id, and with no other.
    assert!(waits("claude-work", claude_shell));
    assert!(!waits("pi-alt", claude_shell));
    assert!(waits("pi-alt", pi));
    assert!(waits("opencode-alt", opencode));
    // A profile's own patterns add to its family's.
    assert!(waits("codex-fast", "ready\n"));
    assert!(!waits("codex", "ready\n"));
    assert!(waits("mine", "ready\n"));
    assert!(!waits("mine", opencode));
    // opencode's plugin reports its prompts, so only its shell mode is read from the screen, and
    // `none` has only its own patterns.
    for id in ["opencode-alt", "mine"] {
        for shown in [network, dialog, claude, pi, claude_shell] {
            assert!(!waits(id, shown), "{id}");
        }
    }
}
