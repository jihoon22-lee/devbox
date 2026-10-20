mod support;
use devbox_cli::{client::Client, daemon};
use devbox_core::run_log::attempt_marker;
use devbox_observe::*;
use devbox_protocol::message::ClientKind;
use devbox_runtime::exit_record::ExitStore;
use std::{sync::Arc, time::Duration};
use support::bounded;

struct Fixture {
    daemon: support::TestDaemon,
    services: Arc<daemon::state::Services>,
    client: Client,
    cancel: tokio_util::sync::CancellationToken,
    task: tokio::task::JoinHandle<std::io::Result<()>>,
    project: String,
}
impl Fixture {
    async fn start() -> Self {
        let daemon = support::TestDaemon::fixture();
        let services = daemon::build_services(daemon.paths.clone()).unwrap();
        let listener = tokio::net::UnixListener::bind(&daemon.socket).unwrap();
        let cancel = tokio_util::sync::CancellationToken::new();
        let task = tokio::spawn(daemon::server::serve(
            listener,
            services.clone(),
            cancel.clone(),
        ));
        let client = bounded(Client::connect(
            &daemon.socket,
            ClientKind::Test,
            &daemon.instance,
        ))
        .await
        .unwrap();
        let root = daemon.home.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let project = client
            .call::<devbox_projects::ProjectsAdd>(&devbox_projects::AddProject {
                path: root.display().to_string(),
            })
            .await
            .unwrap()
            .id;
        Self {
            daemon,
            services,
            client,
            cancel,
            task,
            project,
        }
    }
    async fn seed(&self, id: &str, attempt: &str, lines: &str) -> ExitStore {
        let store = ExitStore::new(&self.daemon.paths.data_dir.join("runs"), id).unwrap();
        store.begin(attempt).unwrap();
        std::fs::write(
            store.log_path(),
            format!("{}{}", record("s", &attempt_marker(attempt)), lines),
        )
        .unwrap();
        let row=serde_json::json!({"run":{"id":id,"projectId":self.project,"defId":"fixture","kind":"task","state":"stopped","exit":null,"startedMs":1,"endedMs":2,"restarts":0,"unit":"devbox-test-problem.service","ready":false,"pid":null,"port":null,"attemptId":attempt},"restart":"no","start_limit":null,"worktree":null,"start_limit_notified":false}).to_string();
        let id = id.to_string();
        let project = self.project.clone();
        let (_, rev) = self
            .services
            .db
            .write(move |tx| {
                tx.execute(
                    "INSERT INTO runtime_runs(id,project_id,started_ms,body) VALUES(?1,?2,1,?3)",
                    (&id, &project, &row),
                )?;
                Ok(())
            })
            .await
            .unwrap();
        self.services.hub.publish::<devbox_runtime::RuntimeChanged>(
            rev,
            &devbox_runtime::RuntimeChange {
                rev: rev.to_string(),
                project_id: self.project.clone(),
            },
        );
        store
    }
    async fn query(&self) -> ProblemsResult {
        bounded(self.client.call::<ObserveProblems>(&ProblemsQuery {
            project_id: self.project.clone(),
            limit: Some(100),
            cursor: None,
        }))
        .await
        .unwrap()
    }
    async fn stop(self) {
        self.cancel.cancel();
        bounded(self.task).await.unwrap().unwrap();
    }
}
fn record(stream: &str, text: &str) -> String {
    format!("2026-10-20T01:02:03.004Z\t{stream}\t{text}\n")
}
fn append(path: &std::path::Path, text: &str) {
    use std::io::Write;
    std::fs::OpenOptions::new()
        .append(true)
        .open(path)
        .unwrap()
        .write_all(text.as_bytes())
        .unwrap();
}

#[tokio::test]
async fn problems_first_query_ingests_then_inotify_events_and_filtered_logs_preserve_diagnostics() {
    let f = Fixture::start().await;
    let store = f
        .seed(
            "one",
            "a",
            &record("o", "src/x.ts(10,5): error TS1005: expected"),
        )
        .await;
    let mut events = f
        .client
        .subscribe("observe.problems.changed")
        .await
        .unwrap();
    let before = f.services.db.rev();
    let first = f.query().await;
    assert_eq!(first.items.len(), 1);
    assert_eq!(first.items[0].line, 10);
    assert_eq!(first.items[0].source, "run:one");
    let event = bounded(events.next_event()).await.unwrap();
    assert_eq!(event.rev, first.rev);
    assert_eq!(
        f.services.db.rev(),
        before,
        "memory observation must not write DB rev"
    );
    let stream=f.client.open_stream("logs.open",serde_json::json!({"sources":[{"kind":"run","runId":"one"}],"from":{"offset":0},"filter":{"text":"never matches"}})).await.unwrap();
    let search = f
        .client
        .call::<LogsSearch>(&SearchLogs {
            sources: vec![LogSource::Run {
                run_id: "one".into(),
            }],
            query: "never matches".into(),
            regex: false,
            before: None,
            limit: 1,
        })
        .await
        .unwrap();
    assert!(search.items.is_empty());
    append(
        &store.log_path(),
        &record("e", "src/x.ts(11,1): warning second"),
    );
    let event = bounded(events.next_event()).await.unwrap();
    let next = f.query().await;
    assert_eq!(next.items.len(), 2);
    assert_eq!(event.rev, next.rev);
    append(
        &store.log_path(),
        &record("e", "src/x.ts(11,1): warning second"),
    );
    assert_eq!(f.query().await.rev, next.rev);
    assert!(
        tokio::time::timeout(Duration::from_millis(100), events.next_event())
            .await
            .is_err()
    );
    stream.close().await.unwrap();
    f.stop().await;
}
#[tokio::test]
async fn problems_restart_clears_only_its_source_and_old_search_cannot_restore_it() {
    let f = Fixture::start().await;
    let a = f
        .seed("one", "a", &record("o", "x.ts(1,1): error old"))
        .await;
    f.seed("two", "a", &record("o", "x.ts(2,1): error preserved"))
        .await;
    assert_eq!(f.query().await.items.len(), 2);
    let mut events = f
        .client
        .subscribe("observe.problems.changed")
        .await
        .unwrap();
    a.begin("b").unwrap();
    // Identical timestamps: only system marker identity partitions the attempts.
    append(
        &a.log_path(),
        &(record("o", &attempt_marker("b"))
            + &record("o", "x.ts(3,1): error fake marker")
            + &record("s", &attempt_marker("b"))
            + &record("o", "x.ts(4,1): error new")),
    );
    let page = f.query().await;
    assert_eq!(
        page.items.iter().map(|p| p.line).collect::<Vec<_>>(),
        vec![2, 4]
    );
    bounded(events.next_event()).await.unwrap();
    let rev = page.rev;
    let old = f
        .client
        .call::<LogsSearch>(&SearchLogs {
            sources: vec![LogSource::Run {
                run_id: "one".into(),
            }],
            query: "old".into(),
            regex: false,
            before: None,
            limit: 10,
        })
        .await
        .unwrap();
    assert_eq!(old.items.len(), 1);
    let page = f.query().await;
    assert_eq!(page.rev, rev);
    assert_eq!(page.items.len(), 2);
    // Clear is published even before the new invocation writes its marker/output.
    a.begin("c").unwrap();
    let page = f.query().await;
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].source, "run:two");
    f.stop().await;
}
#[tokio::test]
async fn problems_query_validation_is_typed() {
    let f = Fixture::start().await;
    for (project, limit, code) in [
        ("missing".to_string(), 1, "observe.project_not_found"),
        (f.project.clone(), 101, "observe.invalid_input"),
    ] {
        let error = f
            .client
            .call::<ObserveProblems>(&ProblemsQuery {
                project_id: project,
                limit: Some(limit),
                cursor: None,
            })
            .await
            .unwrap_err();
        assert!(format!("{error:?}").contains(code));
    }
    f.stop().await;
}

#[tokio::test]
async fn problems_metadata_keeps_live_history_and_manual_starting_has_no_previous_attempt() {
    let daemon = support::TestDaemon::fixture();
    let services = daemon::build_services(daemon.paths.clone()).unwrap();
    let root = daemon.home.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let project = services
        .projects
        .add(devbox_projects::AddProject {
            path: root.display().to_string(),
        })
        .await
        .unwrap()
        .id;
    let p = project.clone();
    services.db.write(move|tx| {
        for i in 0..300 {
            let id=format!("history-{i}");
            let row=serde_json::json!({"run":{"id":id,"projectId":p,"defId":"fixture","kind":"task","state":if i==0 {"running"} else {"stopped"},"exit":null,"startedMs":i,"endedMs":null,"restarts":0,"unit":"devbox-test-problems.service","ready":false,"pid":null,"port":null,"attemptId":"old"},"restart":"no","start_limit":null,"worktree":null,"start_limit_notified":false}).to_string();
            tx.execute("INSERT INTO runtime_runs(id,project_id,started_ms,body) VALUES(?1,?2,?3,?4)",(&id,&p,i,&row))?;
        } Ok(())
    }).await.unwrap();
    let store = ExitStore::new(&daemon.paths.data_dir.join("runs"), "history-0").unwrap();
    store.begin("old").unwrap();
    std::fs::write(
        store.log_path(),
        record("s", &attempt_marker("old")) + &record("o", "x.ts(1,1): error retired"),
    )
    .unwrap();
    let page = services.runs.problem_runs(&project).await.unwrap();
    assert!(page.truncated);
    assert_eq!(page.items.len(), 129);
    assert!(page.items.iter().any(|r| r.run_id == "history-0"));
    services.db.write(|tx|{tx.execute("UPDATE runtime_runs SET started_ms=999,body=json_set(body,'$.run.startedMs',999,'$.run.state','starting','$.run.attemptId',NULL) WHERE id='history-0'",[])?;Ok(())}).await.unwrap();
    let meta = services.runs.run_log_metadata("history-0").await.unwrap();
    assert_eq!(meta.attempt_id, None);
    let run = ProblemRun {
        run_id: meta.run_id,
        project_id: meta.project_id,
        root: root.clone(),
        path: meta.path,
        attempt_id: meta.attempt_id,
        started_ms: meta.started_ms,
    };
    assert!(
        services
            .observe
            .read_problem_batch(&run)
            .await
            .unwrap()
            .problems
            .is_empty()
    );
    store.begin("new").unwrap();
    append(
        &store.log_path(),
        &(record("s", &attempt_marker("new")) + &record("o", "x.ts(2,1): error current")),
    );
    services.db.write(|tx|{tx.execute("UPDATE runtime_runs SET body=json_set(body,'$.run.state','running','$.run.attemptId','new') WHERE id='history-0'",[])?;Ok(())}).await.unwrap();
    let meta = services.runs.run_log_metadata("history-0").await.unwrap();
    let mut new = run;
    new.attempt_id = meta.attempt_id;
    let rows = services
        .observe
        .read_problem_batch(&new)
        .await
        .unwrap()
        .problems;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].line, 2);
    services.observe.shutdown().await;
}

#[tokio::test]
async fn problems_watch_a_later_run_directory_and_explicit_old_runs_outside_history() {
    let f = Fixture::start().await;
    assert!(f.query().await.items.is_empty());
    let mut events = f
        .client
        .subscribe("observe.problems.changed")
        .await
        .unwrap();
    let old = f
        .seed(
            "explicit-old",
            "old",
            &record("o", "x.ts(7,1): error oldest"),
        )
        .await;
    bounded(events.next_event()).await.unwrap();
    append(&old.log_path(), &record("e", "x.ts(8,1): error append"));
    bounded(events.next_event()).await.unwrap();
    let project = f.project.clone();
    f.services.db.write(move|tx|{
        for i in 0..130 {
            let id=format!("new-{i}");
            let row=serde_json::json!({"run":{"id":id,"projectId":project,"defId":"fixture","kind":"task","state":"stopped","exit":null,"startedMs":i+10,"endedMs":null,"restarts":0,"unit":"devbox-test-problems.service","ready":false,"pid":null,"port":null,"attemptId":null},"restart":"no","start_limit":null,"worktree":null,"start_limit_notified":false}).to_string();
            tx.execute("INSERT INTO runtime_runs(id,project_id,started_ms,body) VALUES(?1,?2,?3,?4)",(&id,&project,i+10,&row))?;
        }Ok(())
    }).await.unwrap();
    // Explicit log lookup still follows this run outside automatic history.
    let page = f.services.runs.problem_runs(&f.project).await.unwrap();
    assert!(page.truncated);
    assert!(!page.items.iter().any(|r| r.run_id == "explicit-old"));
    let search = f
        .client
        .call::<LogsSearch>(&SearchLogs {
            sources: vec![LogSource::Run {
                run_id: "explicit-old".into(),
            }],
            query: "never matches".into(),
            regex: false,
            before: None,
            limit: 1,
        })
        .await
        .unwrap();
    assert!(search.items.is_empty());
    old.begin("new").unwrap();
    append(
        &old.log_path(),
        &(record("s", &attempt_marker("new")) + &record("o", "x.ts(9,1): error explicit current")),
    );
    let page = f.query().await;
    assert!(page.truncated);
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].line, 9);
    f.stop().await;
}

#[tokio::test]
async fn problems_new_reader_after_rotation_keeps_only_current_attempt_diagnostics() {
    use devbox_runtime::logfile::{Stream, Writer};
    let f = Fixture::start().await;
    for (id, cut_in_new) in [("rotate-old", false), ("rotate-current", true)] {
        let store = f.seed(id, "old", "").await;
        let mut writer = Writer::open(&store.log_path()).unwrap();
        for _ in 0..20 {
            writer
                .write_line(
                    Stream::Out,
                    format!("x.ts(1,1): error {}", "o".repeat(100)).as_bytes(),
                )
                .unwrap();
        }
        store.begin("new").unwrap();
        writer
            .write_line(Stream::System, attempt_marker("new").as_bytes())
            .unwrap();
        if cut_in_new {
            for _ in 0..20 {
                writer
                    .write_line(
                        Stream::Out,
                        format!("current padding {}", "x".repeat(100)).as_bytes(),
                    )
                    .unwrap();
            }
        }
        writer
            .write_line(Stream::Err, b"x.ts(9,1): error current")
            .unwrap();
        drop(writer);
        Writer::with_limit(&store.log_path(), 2048)
            .unwrap()
            .write_line(Stream::Out, b"rotate now")
            .unwrap();
    }
    // No earlier query or log request registered a collector for these files.
    let page: ProblemsResult = serde_json::from_str(
        &f.client
            .raw_call(
                "observe.problems",
                serde_json::json!({"projectId":f.project}),
                None,
            )
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(page.items.len(), 2);
    assert!(page.items.iter().all(|p| p.line == 9));
    f.stop().await;
}
