//! A project's start configuration with an Antigravity agent: what its tool family needs is
//! checked before the first terminal or agent is started.
mod support;
use devbox_cli::client::Client;
use serde_json::{Value, json};
use std::path::Path;
use support::TestDaemon;

async fn call(c: &Client, method: &str, params: Value) -> Value {
    serde_json::from_str(
        &c.raw_call(method, params, None)
            .await
            .expect("owned fixture"),
    )
    .expect("owned fixture")
}

/// A project whose `.devbox/devbox.toml` is `config`, on a daemon with a profile of the
/// Antigravity family that runs a fixture command.
fn fixture(config: &str) -> (TestDaemon, String) {
    let d = TestDaemon::fixture();
    d.write_config(
        "[[agents.profiles]]\nid='fake'\nlabel='Fake'\ncommand='sleep 600'\nhooks='none'\n\
         [[agents.profiles]]\nid='agy-fixture'\nlabel='Antigravity fixture'\ncommand='sleep 600'\nhooks='antigravity'\n",
    );
    let repo = d.git_repo("projects/web");
    std::fs::create_dir_all(Path::new(&repo).join(".devbox")).expect("owned fixture");
    std::fs::write(Path::new(&repo).join(".devbox/devbox.toml"), config).expect("owned fixture");
    (d, repo)
}

/// The launch of `repo`'s start configuration is refused: its code and detail. Nothing was
/// started: no terminal, no task, no work folder, and nothing beside the user's checkout.
async fn refused(d: &mut TestDaemon, repo: &str) -> (String, Value) {
    d.launch();
    let c = d.client().await;
    let p = d.add_project(&c, repo).await;
    d.approve_project(&c, &p).await;
    let before = call(&c, "terminal.list", json!({})).await;
    let error = c
        .raw_call("projects.launch", json!({"id":p}), None)
        .await
        .expect_err("the launch is refused");
    let devbox_cli::client::ClientError::Rpc {
        code,
        detail: Some(detail),
        ..
    } = error
    else {
        panic!("expected a typed launch error")
    };
    assert_eq!(call(&c, "terminal.list", json!({})).await, before);
    assert_eq!(call(&c, "agents.list", json!({})).await["items"], json!([]));
    assert!(!Path::new(&format!("{repo}-agents")).exists());
    assert!(!Path::new(&format!("{repo}.agy-hooks")).exists());
    (
        code,
        serde_json::from_str(detail.get()).expect("launch detail"),
    )
}

#[tokio::test]
async fn launch_with_an_antigravity_agent_without_a_work_folder_starts_nothing_and_names_the_tool()
{
    // `worktree` is false unless the entry says otherwise.
    let (mut d, repo) = fixture(
        "[launch]\nterminals=[{name='must not start',command='sleep 600'}]\nagents=[{tool='fake',worktree=true},{tool='agy-fixture'}]\n",
    );
    let (code, detail) = refused(&mut d, &repo).await;
    assert_eq!(code, "projects.launch_work_folder_required");
    assert_eq!(detail["tool"], "Antigravity fixture");
}

#[tokio::test]
async fn launch_with_an_antigravity_prompt_devbox_would_not_paste_starts_nothing() {
    // devbox sends an Antigravity agent's prompt through its terminal: a control character would
    // act as a key there, and a prompt that starts with `!` or `/` would run as a command.
    for (prompt, reason) in [
        ("esc \\u001b[201~", "agents.control_characters"),
        ("/model", "agents.first_instruction_command"),
        ("  !rm -rf build", "agents.first_instruction_command"),
    ] {
        let (mut d, repo) = fixture(&format!(
            "[launch]\nterminals=[{{name='must not start',command='sleep 600'}}]\nagents=[{{tool='fake',worktree=true}},{{tool='agy-fixture',worktree=true,prompt=\"{prompt}\"}}]\n"
        ));
        let (code, detail) = refused(&mut d, &repo).await;
        assert_eq!(code, "projects.launch_failed", "{prompt}");
        assert_eq!(detail["message"], format!("agent 2: {reason}"), "{prompt}");
        assert_eq!(
            (detail["terminals"].clone(), detail["agents"].clone()),
            (json!(0), json!(0)),
            "{prompt}"
        );
    }
}

#[tokio::test]
async fn launch_passes_the_same_prompt_to_a_tool_that_takes_it_as_an_argument() {
    // The rule is the pasting family's alone: the other tools read `/…` as an instruction.
    let (mut d, repo) = fixture("[launch]\nagents=[{tool='fake',prompt=\"/model\"}]\n");
    d.launch();
    let c = d.client().await;
    let p = d.add_project(&c, &repo).await;
    d.approve_project(&c, &p).await;
    let result = call(&c, "projects.launch", json!({"id":p})).await;
    assert_eq!(result["agents"], 1);
}
