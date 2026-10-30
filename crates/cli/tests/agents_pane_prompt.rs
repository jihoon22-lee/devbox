//! A send must not answer a Codex or pi prompt the output watcher has not reported yet. The
//! watcher needs 2 to about 6 s of unchanged output, and Enter on a trust prompt saves trust for
//! good.
mod support;
use devbox_cli::client::{Client, ClientError};
use serde_json::{Value, json};
use std::{os::unix::fs::PermissionsExt, time::Duration};
use support::TestDaemon;

const TRUST: &str = include_str!("../../agents/tests/fixtures/codex-0.164.0/trust.txt");
const HOOKS: &str = include_str!("../../agents/tests/fixtures/codex-0.164.0/hooks-review.txt");
const WORKING: &str = include_str!("../../agents/tests/fixtures/codex-0.164.0/working.txt");
const NETWORK: &str =
    include_str!("../../agents/tests/fixtures/codex-0.166.0/approval-network.txt");
const PI_TRUST: &str = include_str!("../../agents/tests/fixtures/pi-1.3.0/trust.txt");
const TEXT: &str = "owned-pane-check-instruction";

/// Leave a start marker, paint the screen once, then record every line that is sent.
const FAKE_CODEX: &str = r#"#!/usr/bin/env bash
touch "$HOME/fake-codex-started"
cat "$HOME/screen.txt"
while IFS= read -r line; do printf '%s\n' "$line" >> "$HOME/received"; done
"#;

/// The same for pi, which first says it is ready, as devbox's extension does once pi has started
/// its session. A send is then held back only by what the pane shows.
const FAKE_PI: &str = r#"#!/usr/bin/env bash
touch "$HOME/fake-pi-started"
"$DEVBOX_BIN" agent event --from pi '{"v":1,"event":"ready"}'
cat "$HOME/screen.txt"
while IFS= read -r line; do printf '%s\n' "$line" >> "$HOME/received"; done
"#;

/// The pane text of a fixture: what follows its `#` header and the `---` line.
fn screen(fixture: &str) -> &str {
    fixture
        .split_once("\n---\n")
        .expect("fixture header ends with a --- line")
        .1
}

/// A running task whose tool paints `shown` and records what it is sent; returns once `last_row`
/// is in the pane. By default the profile is the builtin `codex`, run as a fake `codex` first on
/// PATH. `custom` is a config entry `(id, extra profile lines)` that runs the same fake by path.
/// `tmux` is wrapped to count pastes and, when `fail-tmux` exists, to fail `fail`.
async fn showing(
    shown: &str,
    last_row: &str,
    fail: &str,
    custom: Option<(&str, &str)>,
) -> (TestDaemon, Client, String) {
    showing_as("codex", FAKE_CODEX, shown, last_row, fail, custom).await
}

/// `showing` for the builtin profile `tool`, played by `script` as a fake `tool` first on PATH.
async fn showing_as(
    tool: &str,
    script: &str,
    shown: &str,
    last_row: &str,
    fail: &str,
    custom: Option<(&str, &str)>,
) -> (TestDaemon, Client, String) {
    let mut d = TestDaemon::fixture();
    d.paths.ensure_dirs().expect("owned dirs");
    std::fs::write(
        d.paths.instance_config_dir.join("tmux.conf"),
        devbox_terminal::TMUX_CONF,
    )
    .expect("tmux config");
    d.tmux_failure_wrapper(fail);
    let fake = d.home.path().join("bin").join(tool);
    std::fs::write(&fake, script).expect("owned fake tool");
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755))
        .expect("fake tool permissions");
    std::fs::write(d.home.path().join("screen.txt"), screen(shown)).expect("owned screen");
    let profile = match custom {
        Some((profile, lines)) => {
            let command = format!(
                "bash {}",
                devbox_core::sh_quote(fake.to_str().expect("fake path"))
            );
            d.write_config(&format!(
                "[[agents.profiles]]\nid='{profile}'\nlabel='Custom'\ncommand={}\nhooks='none'\n{lines}\n",
                serde_json::to_string(&command).expect("TOML quoted string")
            ));
            profile
        }
        None => tool,
    };
    let repo = d.git_repo("projects/app");
    d.launch();
    let client = d.client().await;
    let project = d.add_project(&client, &repo).await;
    let id = d.create_agent(&client, &project, profile, false).await;
    let running = d
        .wait_for(&client, &id, "running", Duration::from_secs(10))
        .await;
    let pane = format!("={}:", running["terminalId"].as_str().expect("terminal"));
    support::bounded(async {
        while !d
            .tmux(&["capture-pane", "-p", "-J", "-t", &pane])
            .contains(last_row)
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    // The fake tool ran, never a real Codex or pi.
    assert!(d.home.path().join(format!("fake-{tool}-started")).exists());
    (d, client, id)
}

async fn send(client: &Client, id: &str) -> Result<String, ClientError> {
    client
        .raw_call("agents.send", json!({"id":id,"text":TEXT}), None)
        .await
}

/// The send is refused, nothing was pasted or recorded, and the task gained no instruction.
async fn assert_nothing_sent(d: &TestDaemon, client: &Client, id: &str) {
    assert_eq!(d.paste_count(), 0);
    assert!(!d.home.path().join("received").exists());
    let task: Value = serde_json::from_str(
        &client
            .raw_call("agents.get", json!({"id":id}), None)
            .await
            .expect("task"),
    )
    .expect("task JSON");
    assert_eq!(task["followUps"], json!([]));
}

async fn prompt_refuses_the_send(shown: &str, last_row: &str, custom: Option<(&str, &str)>) {
    let (d, client, id) = showing(shown, last_row, "load-buffer", custom).await;
    let refused = send(&client, &id).await.expect_err("send onto a prompt");
    assert!(
        matches!(&refused, ClientError::Rpc { code, .. } if code == "agents.awaiting_input"),
        "{refused:?}"
    );
    assert_nothing_sent(&d, &client, &id).await;
}

#[tokio::test]
async fn a_send_cannot_answer_the_codex_trust_prompt_before_the_watcher_reports_it() {
    prompt_refuses_the_send(TRUST, "esc back", None).await;
}

#[tokio::test]
async fn a_send_cannot_answer_the_codex_hooks_review_prompt_before_the_watcher_reports_it() {
    prompt_refuses_the_send(HOOKS, "hooks won't run", None).await;
}

#[tokio::test]
async fn a_send_cannot_answer_a_codex_network_approval_before_the_watcher_reports_it() {
    // Enter would pick "Yes, just this once" and let the command reach the host.
    prompt_refuses_the_send(NETWORK, "esc to cancel", None).await;
}

#[tokio::test]
async fn a_send_cannot_answer_the_pi_trust_question_before_the_watcher_reports_it() {
    // pi said it is ready, so only the pane holds the send back. The selection is still on
    // "Trust", which Enter would save for good.
    let (d, client, id) = showing_as(
        "pi",
        FAKE_PI,
        PI_TRUST,
        "escape/ctrl+c cancel",
        "load-buffer",
        None,
    )
    .await;
    let refused = send(&client, &id)
        .await
        .expect_err("send onto pi's trust question");
    assert!(
        matches!(&refused, ClientError::Rpc { code, .. } if code == "agents.awaiting_input"),
        "{refused:?}"
    );
    assert_nothing_sent(&d, &client, &id).await;
}

#[tokio::test]
async fn a_working_screen_does_not_hold_a_send_back() {
    let (d, client, id) = showing(WORKING, "context left", "load-buffer", None).await;
    let sent: Value = serde_json::from_str(&send(&client, &id).await.expect("send while working"))
        .expect("task JSON");
    assert_eq!(sent["followUps"].as_array().expect("history").len(), 1);
    assert_eq!(d.paste_count(), 1);
    let received = d.home.path().join("received");
    support::bounded(async {
        while std::fs::read_to_string(&received).unwrap_or_default() != format!("{TEXT}\n") {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
}

#[tokio::test]
async fn a_pane_that_cannot_be_read_refuses_the_send_instead_of_skipping_the_check() {
    // This screen would be delivered; a failing read must not be taken for "no prompt".
    let (d, client, id) = showing(WORKING, "context left", "capture-pane", None).await;
    std::fs::write(d.home.path().join("fail-tmux"), "owned failure").expect("marker");
    let refused = send(&client, &id)
        .await
        .expect_err("send onto an unreadable pane");
    assert!(
        matches!(&refused, ClientError::Rpc { code, .. } if code == "internal"),
        "{refused:?}"
    );
    assert_nothing_sent(&d, &client, &id).await;
}

#[tokio::test]
async fn a_custom_profile_is_checked_with_its_own_patterns() {
    // The send reads the patterns the watcher compiled from the configured profiles.
    let patterns = r"waitingPatterns=['(?m)^\s*2\. Back to Agent Command Center\s*$']";
    prompt_refuses_the_send(TRUST, "esc back", Some(("mine", patterns))).await;
}

#[tokio::test]
async fn a_custom_codex_profile_without_patterns_replaces_the_builtin_check() {
    // A configured `codex` replaces the builtin profile, patterns included, as for the watcher.
    let (d, client, id) = showing(TRUST, "esc back", "load-buffer", Some(("codex", ""))).await;
    send(&client, &id).await.expect("send under the override");
    assert_eq!(d.paste_count(), 1);
}
