//! The Antigravity family through the built `devbox`: the hook command answers agy whatever
//! happens, and a task with a fixture tool in agy's place follows the hooks as agy sends them.
//! The payloads are real ones of agy 1.5.17, redacted
//! (`crates/agents/tests/fixtures/antigravity-1.5.17/README.md`).
mod support;
use serde_json::{Value, json};
use std::{io::Write, path::Path, process::Stdio, time::Duration};
use support::{ChildGuard, TestDaemon};

/// The fixture's content: what follows its `#` provenance lines and the `---` line.
fn fixture(text: &'static str) -> &'static str {
    text.split_once("\n---\n")
        .expect("fixture header ends with a --- line")
        .1
}
fn session_start() -> &'static str {
    fixture(include_str!(
        "../../agents/tests/fixtures/antigravity-1.5.17/hook-session-start.json"
    ))
}
fn pre_invocation() -> &'static str {
    fixture(include_str!(
        "../../agents/tests/fixtures/antigravity-1.5.17/hook-pre-invocation.json"
    ))
}
fn stop() -> &'static str {
    fixture(include_str!(
        "../../agents/tests/fixtures/antigravity-1.5.17/hook-stop.json"
    ))
}
const CONVERSATION: &str = "00000000-0000-4000-8000-000000000001";

/// Runs `command` with `payload` on its stdin, or with its stdin left open when there is none,
/// and returns what it printed. The hook must end by itself.
fn answered(mut command: std::process::Command, payload: Option<&str>) -> String {
    command.stdin(Stdio::piped()).stdout(Stdio::piped());
    let mut child = ChildGuard(command.spawn().expect("owned hook"));
    let stdin = child.0.stdin.take().expect("stdin");
    let held = match payload {
        Some(payload) => {
            let mut stdin = stdin;
            stdin.write_all(payload.as_bytes()).expect("hook payload");
            None
        }
        // agy never closes it: the hook's own deadline ends the wait.
        None => Some(stdin),
    };
    let mut printed = String::new();
    let mut stdout = child.0.stdout.take().expect("stdout");
    std::io::Read::read_to_string(&mut stdout, &mut printed).expect("hook output");
    assert!(support::exits(child).success());
    drop(held);
    printed
}

#[test]
fn an_antigravity_hook_answers_with_an_empty_object_whatever_happens() {
    let d = TestDaemon::fixture();
    let hook = || {
        let mut command = d.environment(env!("CARGO_BIN_EXE_devbox"));
        command
            .args(["agent", "event", "--from", "antigravity"])
            .env_remove("DEVBOX_AGENT_ID")
            .env_remove("DEVBOX_AGENT_LAUNCH");
        command
    };
    // Outside a devbox task: nothing is reported, and agy still gets its answer.
    assert_eq!(answered(hook(), Some(stop())), "{}\n");
    // In a task whose daemon is not there.
    let mut lost = hook();
    lost.env("DEVBOX_AGENT_ID", "owned-agent")
        .env("DEVBOX_AGENT_LAUNCH", "owned-launch");
    assert_eq!(answered(lost, Some(stop())), "{}\n");
    // A payload that is not agy's, and none at all with stdin left open.
    let mut odd = hook();
    odd.env("DEVBOX_AGENT_ID", "owned-agent");
    assert_eq!(answered(odd, Some("not json")), "{}\n");
    let mut silent = hook();
    silent.env("DEVBOX_AGENT_ID", "owned-agent");
    assert_eq!(answered(silent, None), "{}\n");
    // The other sources print nothing, as before.
    let mut claude = d.environment(env!("CARGO_BIN_EXE_devbox"));
    claude
        .args(["agent", "event", "--from", "claude"])
        .env_remove("DEVBOX_AGENT_ID");
    assert_eq!(answered(claude, Some("{}")), "");
}

/// What the fixture tool noted in `file`, once it holds `lines` whole lines: the file exists
/// before its text does, and the watcher sends the prompt only after the pane has sat still over
/// several of its passes. Bounded like `TestDaemon::wait_for`.
async fn noted(home: &Path, file: &str, lines: usize) -> String {
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        let text = std::fs::read_to_string(home.join(file)).unwrap_or_default();
        if text.ends_with('\n') && text.lines().count() >= lines {
            return text;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "{file} still holds {text:?}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn call(c: &devbox_cli::client::Client, method: &str, params: Value) -> Value {
    serde_json::from_str(&c.raw_call(method, params, None).await.expect(method)).expect("JSON")
}

#[tokio::test]
async fn an_antigravity_task_starts_beside_its_hooks_folder_and_follows_its_hooks() {
    let mut d = TestDaemon::fixture();
    d.paths.ensure_dirs().expect("owned paths");
    std::fs::write(
        d.paths.instance_config_dir.join("tmux.conf"),
        devbox_terminal::TMUX_CONF,
    )
    .expect("owned tmux config");
    // In agy's place: it notes how it was started and draws agy's prompt box (a rule, `>` and
    // what was typed since the last Enter, a rule) over a status row, drawn again once its
    // terminal has sent nothing for 50 ms, as agy draws a paste in its box. Each line ended with
    // Enter is noted.
    let tool = d.home.path().join("tool.sh");
    std::fs::write(
        &tool,
        r#"#!/usr/bin/env bash
printf '%s\n' "$PWD" > "$HOME/tool-cwd"
printf '%s\n' "$@" > "$HOME/tool-args"
rule=$(printf '─%.0s' $(seq 1 60))
draw() { printf '\033[H%s\033[K\n> %s\033[K\n%s\033[K\nfixture status\033[K\n' "$rule" "$1" "$rule"; }
stty -echo -icanon
line=
draw ""
while IFS= read -r -s -N 1 char; do
  while :; do
    if [ "$char" = $'\n' ]; then printf '%s\n' "$line" >> "$HOME/tool-input"; line=; else line+=$char; fi
    IFS= read -r -s -N 1 -t 0.05 char || break
  done
  draw "$line"
done
"#,
    )
    .expect("owned tool");
    let command = format!(
        "bash {}",
        devbox_core::sh_quote(tool.to_str().expect("path"))
    );
    d.write_config(&format!(
        "[[agents.profiles]]\nid='agy-fixture'\nlabel='Antigravity fixture'\nhooks='antigravity'\ncommand={}\n",
        serde_json::to_string(&command).expect("command")
    ));
    let repo = d.git_repo("projects/agy");
    d.launch();
    let c = d.client().await;
    let project = d.add_project(&c, &repo).await;
    d.approve_project(&c, &project).await;
    // Without a work folder the start is refused before anything exists.
    let refused = c
        .raw_call(
            "agents.create",
            json!({"projectId":project,"title":"No folder","profile":"agy-fixture","worktree":false,"prompt":"fix it"}),
            None,
        )
        .await
        .expect_err("a work folder is required");
    assert!(
        format!("{refused:?}").contains("agents.work_folder_required"),
        "{refused:?}"
    );
    assert_eq!(call(&c, "agents.list", json!({})).await["items"], json!([]));
    assert!(!Path::new(&format!("{repo}.agy-hooks")).exists());

    let id = d.create_agent(&c, &project, "agy-fixture", true).await;
    let running = d
        .wait_for(&c, &id, "running", Duration::from_secs(10))
        .await;
    let worktree = running["worktreePath"].as_str().expect("work folder");
    let hooks = format!("{worktree}.agy-hooks");
    // The tool started in the work folder with the hooks folder added, and no prompt argument.
    let home = d.home.path();
    assert_eq!(
        noted(home, "tool-args", 2).await,
        format!("--add-dir\n{hooks}\n")
    );
    assert_eq!(noted(home, "tool-cwd", 1).await, format!("{worktree}\n"));
    assert!(!Path::new(worktree).join(".agents").exists());
    let registered: Value = serde_json::from_str(
        &std::fs::read_to_string(Path::new(&hooks).join(".agents/hooks.json")).expect("hooks"),
    )
    .expect("hooks JSON");
    let events = registered["devbox"].as_object().expect("events");
    assert_eq!(events.len(), 3, "{registered}");

    // The prompt was not an argument: it is the task's first instruction, queued until the tool
    // shows its prompt box. The daemon's watcher then pastes it, once, through the task's pane.
    assert!(matches!(
        running["firstInstruction"]["delivery"].as_str(),
        Some("queued" | "pending" | "delivered")
    ));
    assert_eq!(noted(home, "tool-input", 1).await, "fix it\n");
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let sent = loop {
        let task = call(&c, "agents.get", json!({"id":id})).await;
        if task["firstInstruction"]["delivery"] == "delivered" {
            break task;
        }
        assert!(std::time::Instant::now() < deadline, "{task}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    assert_eq!(sent["state"], "running");
    assert_eq!(sent["followUps"], json!([]));

    // Each hook runs as agy runs it: the registered command through a shell, in the folder of
    // `hooks.json`, with the payload on stdin and the environment of the tool.
    let launch = d.agent_launch(&id).await;
    let hook = |event: &str, payload: &str| {
        let line = events[event][0]["command"].as_str().expect("command");
        let mut command = d.environment("sh");
        command
            .args(["-c", line])
            .current_dir(Path::new(&hooks).join(".agents"))
            .env("DEVBOX_AGENT_ID", &id)
            .env("DEVBOX_AGENT_LAUNCH", &launch);
        assert_eq!(answered(command, Some(payload)), "{}\n", "{event}");
    };
    hook("SessionStart", session_start());
    let task = call(&c, "agents.get", json!({"id":id})).await;
    assert_eq!(task["sessionId"], CONVERSATION);
    assert_eq!(task["state"], "running");
    hook("PreInvocation", pre_invocation());
    assert_eq!(
        call(&c, "agents.get", json!({"id":id})).await["state"],
        "running"
    );
    hook("Stop", stop());
    let idle = call(&c, "agents.get", json!({"id":id})).await;
    assert_eq!(idle["state"], "idle");
    assert_eq!(idle["attention"]["message"], "다음 지시를 기다립니다");
    assert_eq!(idle["sessionId"], CONVERSATION);
    assert_eq!(idle["firstInstruction"]["delivery"], "delivered");
    // Another conversation's report that does not start one changes nothing.
    let other = "11111111-1111-4111-8111-111111111111";
    hook(
        "PreInvocation",
        &pre_invocation().replace(CONVERSATION, other),
    );
    let kept = call(&c, "agents.get", json!({"id":id})).await;
    for field in ["state", "sessionId", "attention", "firstInstruction"] {
        assert_eq!(kept[field], idle[field], "{field}");
    }
    // The user starts a new conversation inside the tool: the task follows it.
    hook(
        "SessionStart",
        &session_start().replace(CONVERSATION, other),
    );
    hook(
        "PreInvocation",
        &pre_invocation().replace(CONVERSATION, other),
    );
    let moved = call(&c, "agents.get", json!({"id":id})).await;
    assert_eq!(moved["state"], "running");
    assert_eq!(moved["sessionId"], other);
    hook("Stop", &stop().replace(CONVERSATION, other));
    assert_eq!(
        call(&c, "agents.get", json!({"id":id})).await["state"],
        "idle"
    );
    // The watcher passes every two seconds: over several more passes the prompt is not pasted
    // again.
    let until = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < until {
        assert_eq!(noted(home, "tool-input", 1).await, "fix it\n");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    // Discarding the task removes the hooks folder with the work folder.
    let check = call(&c, "agents.discard_check", json!({"id":id})).await;
    let closed = call(
        &c,
        "agents.discard",
        json!({"id":id,"expect":{"headOid":check["headOid"],"unmergedCommits":check["unmergedCommits"],"discardFingerprint":check["fingerprint"]}}),
    )
    .await;
    assert_eq!(closed["closedHow"], "discarded");
    assert!(!Path::new(worktree).exists());
    assert!(!Path::new(&hooks).exists(), "left as an orphan");
}

#[tokio::test]
async fn a_daemon_that_restarts_does_not_send_a_queued_prompt_it_cannot_vouch_for() {
    let mut d = TestDaemon::fixture();
    d.paths.ensure_dirs().expect("owned paths");
    std::fs::write(
        d.paths.instance_config_dir.join("tmux.conf"),
        devbox_terminal::TMUX_CONF,
    )
    .expect("owned tmux config");
    // In agy's place: it draws nothing a daemon knows for a while, then agy's empty prompt box,
    // and notes every line its terminal gets.
    let tool = d.home.path().join("tool.sh");
    std::fs::write(
        &tool,
        r#"#!/usr/bin/env bash
printf 'starting\n' > "$HOME/tool-started"
sleep 6
rule=$(printf '─%.0s' $(seq 1 60))
printf '%s\n> \n%s\nfixture status\n' "$rule" "$rule"
printf 'box\n' > "$HOME/tool-box"
while IFS= read -r line; do printf '%s\n' "$line" >> "$HOME/tool-input"; done
"#,
    )
    .expect("owned tool");
    let command = format!(
        "bash {}",
        devbox_core::sh_quote(tool.to_str().expect("path"))
    );
    d.write_config(&format!(
        "[[agents.profiles]]\nid='agy-fixture'\nlabel='Antigravity fixture'\nhooks='antigravity'\ncommand={}\n",
        serde_json::to_string(&command).expect("command")
    ));
    let repo = d.git_repo("projects/agy");
    d.launch();
    let c = d.client().await;
    let project = d.add_project(&c, &repo).await;
    let id = d.create_agent(&c, &project, "agy-fixture", true).await;
    let running = d
        .wait_for(&c, &id, "running", Duration::from_secs(10))
        .await;
    assert_eq!(running["firstInstruction"]["delivery"], "queued");
    let home = d.home.path().to_owned();
    noted(&home, "tool-started", 1).await;
    // The daemon goes away and comes back while the tool is still starting. What happened in
    // the pane meanwhile left it nothing to judge by: its first sight of the pane is not a
    // screen that comes before any prompt.
    drop(c);
    d.restart();
    let c = d.client().await;
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    let cancelled = loop {
        let task = call(&c, "agents.get", json!({"id":id})).await;
        if task["firstInstruction"]["delivery"] == "notSent" {
            break task;
        }
        assert!(std::time::Instant::now() < deadline, "{task}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    assert_eq!(cancelled["firstInstruction"]["reason"], "unobserved");
    // The prompt box comes up and stays: over several passes of the watcher nothing is pasted.
    noted(&home, "tool-box", 1).await;
    let until = std::time::Instant::now() + Duration::from_secs(7);
    while std::time::Instant::now() < until {
        assert!(!home.join("tool-input").exists());
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let after = call(&c, "agents.get", json!({"id":id})).await;
    assert_eq!(after["firstInstruction"]["delivery"], "notSent");
    assert_eq!(after["prompt"], "fix it");
}

#[tokio::test]
async fn a_hook_whose_payload_is_too_long_still_cancels_the_queued_first_instruction() {
    let mut d = TestDaemon::fixture();
    d.paths.ensure_dirs().expect("owned paths");
    std::fs::write(
        d.paths.instance_config_dir.join("tmux.conf"),
        devbox_terminal::TMUX_CONF,
    )
    .expect("owned tmux config");
    // In agy's place: it never shows a prompt box, so the first instruction stays queued.
    let tool = d.home.path().join("tool.sh");
    std::fs::write(&tool, "#!/usr/bin/env bash\nexec sleep 600\n").expect("owned tool");
    let command = format!(
        "bash {}",
        devbox_core::sh_quote(tool.to_str().expect("path"))
    );
    d.write_config(&format!(
        "[[agents.profiles]]\nid='agy-fixture'\nlabel='Antigravity fixture'\nhooks='antigravity'\ncommand={}\n",
        serde_json::to_string(&command).expect("command")
    ));
    let repo = d.git_repo("projects/agy");
    d.launch();
    let c = d.client().await;
    let project = d.add_project(&c, &repo).await;
    let id = d.create_agent(&c, &project, "agy-fixture", true).await;
    let running = d
        .wait_for(&c, &id, "running", Duration::from_secs(10))
        .await;
    assert_eq!(running["firstInstruction"]["delivery"], "queued");
    let hooks = format!(
        "{}.agy-hooks",
        running["worktreePath"].as_str().expect("work folder")
    );
    let registered: Value = serde_json::from_str(
        &std::fs::read_to_string(Path::new(&hooks).join(".agents/hooks.json")).expect("hooks"),
    )
    .expect("hooks JSON");
    // The user typed a long prompt into agy: its hook's payload holds it in `lastUserInput`, over
    // the hook's 1 MiB limit, and the limit falls inside a character.
    let limit = 1024 * 1024;
    let mut payload = format!(
        "{{\"hookEventName\":\"SessionStart\",\"conversationId\":\"{CONVERSATION}\",\"lastUserInput\":\""
    );
    while (limit + 1 - payload.len()) % 3 != 1 {
        payload.push('x');
    }
    payload.push_str(&"가".repeat(limit / 3 + 1));
    payload.push_str("\"}");
    assert!(std::str::from_utf8(&payload.as_bytes()[..limit + 1]).is_err());
    let launch = d.agent_launch(&id).await;
    let line = registered["devbox"]["SessionStart"][0]["command"]
        .as_str()
        .expect("command");
    let mut command = d.environment("sh");
    command
        .args(["-c", line])
        .current_dir(Path::new(&hooks).join(".agents"))
        .env("DEVBOX_AGENT_ID", &id)
        .env("DEVBOX_AGENT_LAUNCH", &launch)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    let mut child = ChildGuard(command.spawn().expect("owned hook"));
    let mut stdin = child.0.stdin.take().expect("stdin");
    // The hook reads only as far as its limit, and may end before the rest is written.
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(payload.as_bytes());
    });
    let mut printed = String::new();
    let mut stdout = child.0.stdout.take().expect("stdout");
    std::io::Read::read_to_string(&mut stdout, &mut printed).expect("hook output");
    assert!(support::exits(child).success());
    writer.join().expect("payload writer");
    assert_eq!(printed, "{}\n");
    // The queued instruction will never be sent, and nothing else of the payload was taken.
    let after = call(&c, "agents.get", json!({"id":id})).await;
    assert_eq!(after["firstInstruction"]["delivery"], "notSent");
    assert_eq!(after["sessionId"], Value::Null);
    assert_eq!(after["state"], "running");
}
