use super::*;
use std::os::unix::fs::PermissionsExt;

#[test]
fn failed_codex_resume_restores_correlation_and_can_retry() {
    cases(&["create", "persist", "resources"]);
}
#[test]
fn failed_resume_does_not_overwrite_a_later_launch_or_claim_failed_cleanup_completed() {
    cases(&["newer", "cleanup", "create-cleanup"]);
}
async fn case(mode: &str) {
    let mut f = Fixture::new().await;
    Arc::get_mut(&mut f.svc).expect("owned service").profiles = vec![crate::profile::ToolProfile {
        id: "codex".into(),
        label: "Owned fake Codex".into(),
        command: "owned-fake-codex".into(),
        hooks: crate::profile::Hooks::Codex,
        waiting_patterns: vec![],
    }];
    fs::create_dir(&f.workdir).expect("owned work directory");
    let project = devbox_projects::Project {
        id: "p".into(),
        name: "Owned".into(),
        path: f.workdir.clone(),
        favorite: false,
        added_ms: 0,
    };
    f.svc.db.write(move |tx| tx.execute("INSERT INTO projects_projects(id,name,path,favorite,added_ms) VALUES(?1,?2,?3,?4,?5)",rusqlite::params![project.id,project.name,project.path,project.favorite,project.added_ms])).await.expect("owned project");
    f.rollout(
        "primary",
        "user",
        "2026-10-13T00:00:01Z",
        "one",
        "2026-10-13T00:00:02Z",
    );
    let task = f.svc.get("a").await.expect("task");
    f.svc
        .refresh_binding(&task, false)
        .await
        .expect("previous completed launch metadata");
    f.svc
        .db
        .write(|tx| {
            let mut binding = store::binding(tx, "a")?.expect("binding");
            binding.launch_id = Some("old-generation".into());
            binding.resources = tracker::Resources::Owned;
            binding.owned_rebase = true;
            store::put_binding(tx, "a", &binding)?;
            let mut task = store::get(tx, "a")?.expect("task");
            apply_event(&mut task, &AgentEvent::Exited { code: 0 }, now_ms());
            task.prompt = "accepted prompt".into();
            task.follow_ups.push(FollowUp {
                at_ms: 1,
                text: "accepted follow-up".into(),
                instruction_id: None,
                delivery: None,
            });
            store::put(tx, &task)
        })
        .await
        .expect("ended previous launch");
    let ended = f.svc.get("a").await.expect("ended task");
    let phase = f.svc.phase_path(&ended, "old-generation");
    crate::prepare::private_dir(phase.parent().expect("data directory")).expect("old data");
    crate::prepare::private_file(&phase, "exit:0\n", false)
        .expect("all cases completed the previous tool launch");
    let old = f.svc.launch_binding("a").await.expect("old binding");
    let before = f.svc.get("a").await.expect("old task");
    fs::write(
        std::env::var_os("DEVBOX_RESUME_DB_RECEIPT").expect("receipt"),
        f.svc.paths.db_path.to_str().expect("db path"),
    )
    .expect("owned DB receipt");
    if mode == "create" {
        // The old ended holder is removed before the failed replacement begins.
        let old_terminal = "t11111111111111111111111111111111";
        let cwd = f.workdir.clone();
        f.svc.db.write(move |tx| {
            tx.execute("INSERT INTO terminal_sessions(id,project_id,title,cwd,kind,command,created_ms) VALUES(?1,'p','Old holder',?2,'agent','owned',0)", rusqlite::params![old_terminal,cwd])?;
            let mut task = store::get(tx, "a")?.expect("task");
            task.terminal_id = Some(old_terminal.into());
            store::put(tx, &task)
        }).await.expect("ended owned holder row");
        fs::write(
            std::env::var_os("DEVBOX_RESUME_LIVE").expect("live"),
            old_terminal,
        )
        .expect("fake old holder");
    }
    if mode == "persist" || mode == "cleanup" {
        f.svc.db.write(|tx| tx.execute_batch("CREATE TRIGGER owned_resume_terminal_fail BEFORE UPDATE ON agents_tasks WHEN json_extract(NEW.body, '$.terminalId') IS NOT NULL BEGIN SELECT RAISE(FAIL, 'owned terminal-id failure'); END;")).await.expect("owned terminal-id failure");
    }
    let error = f
        .svc
        .resume("a")
        .await
        .expect_err("deliberate owned resume failure");
    assert!(matches!(error, Fail::Internal(_)), "{error:?}");
    let current = f
        .svc
        .launch_binding("a")
        .await
        .expect("binding after failure");
    let task = f.svc.get("a").await.expect("task after failure");
    assert_eq!(task.prompt, before.prompt);
    assert_eq!(task.follow_ups, before.follow_ups);
    let observed: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(std::env::var_os("DEVBOX_RESUME_LAUNCH_RECEIPT").expect("receipt"))
            .expect("provisional launch receipt"),
    )
    .expect("provisional binding");
    assert_ne!(
        observed["launch_id"], "old-generation",
        "new correlation must be published before tmux launch"
    );
    assert!(observed["turn_id"].is_null());
    if mode == "newer" {
        assert_eq!(current.launch_id.as_deref(), Some("later-generation"));
        assert_eq!(
            task.state,
            AgentState::Waiting,
            "failed old attempt may not clobber a newer launch"
        );
        return;
    }
    if mode == "cleanup" || mode == "create-cleanup" {
        assert_ne!(
            current.launch_id, old.launch_id,
            "failed stop keeps possibly live generation ownership"
        );
        assert!(Path::new(&std::env::var_os("DEVBOX_RESUME_LIVE").expect("live")).exists());
        return;
    }
    assert_eq!(current.launch_ms, old.launch_ms);
    assert_eq!(current.launch_id, old.launch_id);
    assert_eq!(current.thread_id, old.thread_id);
    assert_eq!(current.turn_id, old.turn_id);
    assert_eq!(current.turn_started_ms, old.turn_started_ms);
    assert_eq!(current.rollout, old.rollout);
    assert_eq!(task.state, AgentState::Failed);
    assert_eq!(task.reason.as_deref(), Some("resume_failed"));
    assert!(task.tool_exited);
    assert!(
        task.terminal_id.is_none(),
        "do not restore a removed old terminal record"
    );
    assert!(!Path::new(&std::env::var_os("DEVBOX_RESUME_LIVE").expect("live")).exists());
    if mode == "resources" {
        assert!(matches!(
            current.resources,
            tracker::Resources::Partial { .. }
        ));
        assert!(!current.owned_rebase);
    } else {
        assert_eq!(current.resources, old.resources);
        assert_eq!(current.owned_rebase, old.owned_rebase);
    }
    fs::remove_file(std::env::var_os("DEVBOX_RESUME_FAIL").expect("failure flag"))
        .expect("remove owned launch failure");
    if mode == "persist" {
        f.svc
            .db
            .write(|tx| tx.execute_batch("DROP TRIGGER owned_resume_terminal_fail"))
            .await
            .expect("remove owned DB failure");
    }
    let retried = f
        .svc
        .resume("a")
        .await
        .expect("unpoisoned old rollout allows a new resume");
    assert_eq!(retried.state, AgentState::Running);
    assert_ne!(
        f.svc
            .launch_binding("a")
            .await
            .expect("replacement binding")
            .launch_id,
        old.launch_id
    );
    f.svc
        .terminal
        .close_internal(retried.terminal_id.as_deref().expect("owned terminal"))
        .await
        .expect("remove owned fake terminal");
}
fn cases(modes: &[&str]) {
    const CHILD: &str = "DEVBOX_RESUME_CASE";
    if let Ok(mode) = std::env::var(CHILD) {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("owned runtime")
            .block_on(case(&mode));
        return;
    }
    let root = tempfile::tempdir().expect("owned fake tmux");
    fs::write(root.path().join("tmux"), r#"#!/bin/sh
case "$5" in
 has-session)
   target=${7#=}
   if [ -f "$DEVBOX_RESUME_LIVE" ] && [ "$(cat "$DEVBOX_RESUME_LIVE")" = "$target" ]; then exit 0; fi
   printf '%s\n' "can't find session" >&2; exit 1;;
 new-session)
   previous=; id=
   for argument do
     if [ "$previous" = -s ]; then id=$argument; fi
     previous=$argument
   done
   printf '%s' "$id" > "$DEVBOX_RESUME_LIVE"
   python3 - "$(cat "$DEVBOX_RESUME_DB_RECEIPT")" <<'PYHOOK'
import sqlite3,json,os
c=sqlite3.connect(__import__('sys').argv[1]);c.execute('PRAGMA busy_timeout=5000')
b=json.loads(c.execute("SELECT body FROM agents_runtime WHERE id='a'").fetchone()[0])
open(os.environ['DEVBOX_RESUME_LAUNCH_RECEIPT'],'w').write(json.dumps(b))
if os.path.exists(os.environ['DEVBOX_RESUME_FAIL']):
 if os.environ['DEVBOX_RESUME_CASE']=='newer':
  b['launch_id']='later-generation';b['launch_ms']+=1
  t=json.loads(c.execute("SELECT body FROM agents_tasks WHERE id='a'").fetchone()[0]);t['state']='waiting'
  c.execute("UPDATE agents_tasks SET state='waiting',body=? WHERE id='a'",(json.dumps(t),))
 if os.environ['DEVBOX_RESUME_CASE']=='resources':
  b['resources']={'Partial':{'path_exists':False,'branch_exists':False,'worktree_registered':False}};b['owned_rebase']=False
 c.execute("UPDATE agents_runtime SET body=? WHERE id='a'",(json.dumps(b),));c.commit()
PYHOOK
   if [ -f "$DEVBOX_RESUME_FAIL" ]; then
     case "$DEVBOX_RESUME_CASE" in create|newer|resources|create-cleanup) exit 43;; esac
   fi;;
 list-sessions) if [ -f "$DEVBOX_RESUME_LIVE" ]; then printf '%s\t0\tbash\t/owned\n' "$(cat "$DEVBOX_RESUME_LIVE")"; fi;;
 kill-session) if [ -f "$DEVBOX_RESUME_FAIL" ]; then case "$DEVBOX_RESUME_CASE" in *cleanup) exit 44;; esac; fi
   rm -f "$DEVBOX_RESUME_LIVE";;
esac
exit 0
"#).expect("owned fake executable");
    fs::set_permissions(root.path().join("tmux"), fs::Permissions::from_mode(0o700)).expect("mode");
    let path = std::env::join_paths(std::iter::once(root.path().to_owned()).chain(
        std::env::split_paths(&std::env::var_os("PATH").expect("PATH")),
    ))
    .expect("owned PATH");
    let test = if modes[0] == "create" {
        "failed_codex_resume_restores_correlation_and_can_retry"
    } else {
        "failed_resume_does_not_overwrite_a_later_launch_or_claim_failed_cleanup_completed"
    };
    let mut failures = vec![];
    for mode in modes {
        let flag = root.path().join(format!("fail-{mode}"));
        fs::write(&flag, "owned failure").expect("flag");
        let output = std::process::Command::new(std::env::current_exe().expect("owned executable"))
            .args([
                "--exact",
                &format!("service::correlation_tests::resume_rollback_tests::{test}"),
                "--nocapture",
            ])
            .env(CHILD, mode)
            .env("PATH", &path)
            .env("DEVBOX_RESUME_FAIL", flag)
            .env(
                "DEVBOX_RESUME_LIVE",
                root.path().join(format!("live-{mode}")),
            )
            .env(
                "DEVBOX_RESUME_DB_RECEIPT",
                root.path().join(format!("db-{mode}")),
            )
            .env(
                "DEVBOX_RESUME_LAUNCH_RECEIPT",
                root.path().join(format!("launch-{mode}")),
            )
            .output()
            .expect("owned child");
        if !output.status.success() {
            failures.push(format!(
                "{mode}: {}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
