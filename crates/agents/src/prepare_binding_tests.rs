use super::*;

#[test]
fn initial_preparation_compensates_both_terminal_create_and_binding_write_failures() {
    cases(&["bind", "create"]);
}

#[test]
fn initial_preparation_preserves_newer_launch_and_reports_incomplete_cleanup_honestly() {
    cases(&["newer", "stop-fails"]);
}

async fn case(mode: &str) {
    let mut f = Fixture::new().await;
    Arc::get_mut(&mut f.svc).expect("owned service").use_scope = true;
    fs::write(
        std::env::var_os("DEVBOX_PREP_DB").expect("receipt"),
        f.svc.paths.db_path.to_str().expect("DB path"),
    )
    .expect("owned receipt");
    f.svc.db.write(|tx| {
        tx.execute_batch("CREATE TRIGGER owned_prepare_binding_fail BEFORE UPDATE ON agents_tasks WHEN json_extract(NEW.body,'$.terminalId') IS NOT NULL BEGIN SELECT RAISE(FAIL,'owned binding failure'); END;")?;
        tx.execute("INSERT INTO terminal_sessions(id,project_id,title,cwd,kind,command,created_ms) VALUES('thost-owned',NULL,'Host fixture','/owned','shell','owned',0)", [])?;
        Ok(())
    }).await.expect("owned trigger and unrelated terminal");
    let mut request = f.request();
    request.worktree = false;
    request.prompt = "accepted user instructions".into();
    let reserved = f.svc.create(request).await.expect("reserve");
    f.drain().await;
    let task = f.svc.get(&reserved.id).await.expect("task");
    let binding = f.svc.launch_binding(&reserved.id).await.expect("binding");
    assert_eq!(task.prompt, "accepted user instructions");
    assert!(task.follow_ups.is_empty());
    assert!(task.branch.is_none());
    assert!(task.worktree_path.is_none());
    assert!(matches!(
        binding.resources,
        crate::tracker::Resources::Planned
    ));
    assert!(
        f.repo.exists(),
        "preparation compensation may not remove Git resources"
    );
    let rows = f
        .svc
        .db
        .read(|tx| {
            tx.query_row(
                "SELECT COUNT(*) FROM terminal_sessions WHERE id!='thost-owned'",
                [],
                |row| row.get::<_, i64>(0),
            )
        })
        .await
        .expect("terminal rows");
    assert_eq!(rows, 0, "unbound exact terminal metadata must be removed");
    let hosts = f
        .svc
        .db
        .read(|tx| {
            tx.query_row(
                "SELECT COUNT(*) FROM terminal_sessions WHERE id='thost-owned'",
                [],
                |row| row.get::<_, i64>(0),
            )
        })
        .await
        .expect("host row");
    assert_eq!(hosts, 1);
    assert!(
        !Path::new(&std::env::var_os("DEVBOX_PREP_LIVE").expect("live")).exists(),
        "unbound exact terminal must be gone"
    );
    let stopped = Path::new(&std::env::var_os("DEVBOX_PREP_STOP").expect("stop receipt")).exists();
    if mode == "newer" {
        assert_eq!(binding.launch_id.as_deref(), Some("newer-generation"));
        assert_eq!(task.state, AgentState::Running);
        assert!(!task.tool_exited);
        assert!(
            !stopped,
            "old attempt must not stop a newer generation's scope"
        );
    } else if mode == "stop-fails" {
        assert!(stopped, "must attempt to stop the owned scope");
        assert_eq!(
            task.state,
            AgentState::Preparing,
            "failed scope cleanup must not claim ordinary setup failure completion"
        );
        assert!(
            !task.tool_exited,
            "possibly live tool must not be marked exited"
        );
    } else {
        assert!(
            stopped,
            "terminal close alone does not stop a HUP-ignoring scoped process"
        );
        assert_eq!(task.state, AgentState::Failed);
        assert!(task.tool_exited);
        assert_eq!(task.reason.as_deref(), Some("preparation_failed"));
    }
}

struct OwnedProcess(std::process::Child);
impl Drop for OwnedProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn cases(modes: &[&str]) {
    if let Ok(mode) = std::env::var("DEVBOX_PREP_MODE") {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime")
            .block_on(case(&mode));
        return;
    }
    let name = if modes[0] == "bind" {
        "initial_preparation_compensates_both_terminal_create_and_binding_write_failures"
    } else {
        "initial_preparation_preserves_newer_launch_and_reports_incomplete_cleanup_honestly"
    };
    for mode in modes {
        let root = tempfile::tempdir().expect("owned executable fixtures");
        let tmux = root.path().join("tmux");
        fs::write(&tmux, r#"#!/usr/bin/env python3
import sys,os,pathlib,sqlite3,json
a=sys.argv[1:]; live=pathlib.Path(os.environ['DEVBOX_PREP_LIVE']); mode=os.environ['DEVBOX_PREP_MODE']
if 'new-session' in a:
    sid=a[a.index('-s')+1]; live.write_text(sid)
    if mode=='newer':
        db=sqlite3.connect(pathlib.Path(os.environ['DEVBOX_PREP_DB']).read_text())
        key,body=db.execute('SELECT id,body FROM agents_tasks').fetchone(); task=json.loads(body)
        task['state']='running';task['toolExited']=False
        db.execute('UPDATE agents_tasks SET body=? WHERE id=?',(json.dumps(task),key))
        body=db.execute('SELECT body FROM agents_runtime WHERE id=?',(key,)).fetchone()[0]; binding=json.loads(body);binding['launch_id']='newer-generation'
        db.execute('UPDATE agents_runtime SET body=? WHERE id=?',(json.dumps(binding),key));db.commit()
    if mode=='create': sys.exit(1)
elif 'has-session' in a:
    target=a[a.index('-t')+1].lstrip('=').rstrip(':')
    if not live.exists() or live.read_text()!=target:
        print("can't find session",file=sys.stderr);sys.exit(1)
elif 'list-sessions' in a:
    print('thost-owned\t0\tbash\t/owned')
    if live.exists(): print(live.read_text()+'\t0\tbash\t/owned')
elif 'kill-session' in a:
    target=a[a.index('-t')+1].lstrip('=').rstrip(':')
    if live.exists() and live.read_text()==target: live.unlink()
    else: sys.exit(2)
"#).expect("fake tmux");
        let systemctl = root.path().join("systemctl");
        fs::write(&systemctl, r#"#!/usr/bin/env python3
import sys,os,pathlib,signal
a=sys.argv[1:]
if 'stop' in a:
    assert a[-1].startswith('devbox-test-resource-') and '-agent-' in a[-1] and a[-1].endswith('.scope')
    pathlib.Path(os.environ['DEVBOX_PREP_STOP']).write_text(a[-1])
    if os.environ['DEVBOX_PREP_MODE']=='stop-fails': sys.exit(1)
    os.kill(int(os.environ['DEVBOX_PREP_PID']),signal.SIGTERM)
else: print('loaded')
"#).expect("fake systemctl");
        for file in [&tmux, &systemctl] {
            fs::set_permissions(file, fs::Permissions::from_mode(0o700)).expect("mode");
        }
        let ready = root.path().join("ready");
        let mut owned = OwnedProcess(Command::new("/usr/bin/python3").args(["-c", "import signal,time,sys,pathlib; signal.signal(signal.SIGHUP,signal.SIG_IGN); pathlib.Path(sys.argv[1]).write_text('ready'); time.sleep(120)"]).arg(&ready).spawn().expect("owned HUP ignoring process"));
        for _ in 0..100 {
            if ready.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(ready.exists(), "owned process ready");
        let output = Command::new(std::env::current_exe().expect("test binary"))
            .args([
                "--exact",
                &format!("service::resource_tests::prepare_binding_tests::{name}"),
                "--nocapture",
            ])
            .env("PATH", format!("{}:/usr/bin:/bin", root.path().display()))
            .env("DEVBOX_PREP_MODE", mode)
            .env("DEVBOX_PREP_DB", root.path().join("db"))
            .env("DEVBOX_PREP_LIVE", root.path().join("live"))
            .env("DEVBOX_PREP_STOP", root.path().join("stop"))
            .env("DEVBOX_PREP_PID", owned.0.id().to_string())
            .output()
            .expect("owned child");
        let exited = owned.0.try_wait().expect("owned status").is_some();
        let _ = owned.0.kill();
        let _ = owned.0.wait();
        assert!(
            output.status.success(),
            "mode={mode}\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            exited,
            *mode == "bind" || *mode == "create",
            "scope process lifetime mode={mode}"
        );
    }
}
