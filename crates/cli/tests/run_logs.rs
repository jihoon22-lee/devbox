mod support;
use devbox_cli::client::Client;
use devbox_observe::{LogLine, LogSource, SearchLogs, SearchResult};
use devbox_protocol::{
    message::ClientKind,
    system::{Ping, PingParams},
};
use std::{io::Write, sync::Arc, time::Duration};
use support::bounded;

struct Fixture {
    daemon: support::TestDaemon,
    services: Arc<devbox_cli::daemon::state::Services>,
    cancel: tokio_util::sync::CancellationToken,
    task: tokio::task::JoinHandle<std::io::Result<()>>,
    client: Client,
}
impl Fixture {
    async fn start() -> Self {
        let daemon = support::TestDaemon::fixture();
        let services = devbox_cli::daemon::build_services(daemon.paths.clone()).expect("services");
        let listener = tokio::net::UnixListener::bind(&daemon.socket).expect("socket");
        let cancel = tokio_util::sync::CancellationToken::new();
        let task = tokio::spawn(devbox_cli::daemon::server::serve(
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
        .expect("client");
        Self {
            daemon,
            services,
            cancel,
            task,
            client,
        }
    }
    fn file(&self, name: &str, data: &[u8]) -> std::path::PathBuf {
        let vault = self.daemon.home.path().join("notes");
        std::fs::create_dir_all(&vault).unwrap();
        let file = vault.join(name);
        std::fs::write(&file, data).unwrap();
        file
    }
    async fn stop(self) {
        self.cancel.cancel();
        bounded(self.task).await.unwrap().unwrap();
    }
}
#[tokio::test]
async fn slow_client_pauses_reading_not_the_daemon() {
    let fixture = Fixture::start().await;
    let path = fixture.file(
        "large.log",
        &("x".repeat(100 * 1024) + "\n").repeat(100).into_bytes(),
    );
    let stream = bounded(fixture.client.open_stream(
        "logs.open",
        serde_json::json!({"sources":[{"kind":"file","path":path}],"from":{"offset":0}}),
    ))
    .await
    .expect("open must return without credit");
    bounded(fixture.client.call::<Ping>(&PingParams {}))
        .await
        .expect("ping while credit is zero");
    tokio::time::sleep(Duration::from_millis(40)).await;
    let first = fixture.services.observe.file_bytes_read();
    assert!(
        first > 256 * 1024 && first <= (256 + 108) * 1024,
        "prefetch includes the initial 256KiB credit window plus one head and 8KiB chunk: {first}"
    );
    tokio::time::sleep(Duration::from_millis(40)).await;
    assert_eq!(
        fixture.services.observe.file_bytes_read(),
        first,
        "zero credit stops file consumption"
    );
    stream.credit(256 * 1024).await.unwrap();
    let line = stream.read_until(b"\n", Duration::from_secs(2)).await;
    let row: LogLine = serde_json::from_slice(line.split(|b| *b == b'\n').next().unwrap())
        .expect("JSONL across 64KiB transport chunks");
    assert_eq!(row.text.len(), 100 * 1024);
    stream.close().await.unwrap();
    bounded(fixture.client.call::<Ping>(&PingParams {}))
        .await
        .unwrap();
    fixture.stop().await;
}
#[tokio::test]
async fn file_tail_append_search_and_close_use_actual_rpc() {
    let fixture = Fixture::start().await;
    let path = fixture.file("tail.log", b"old\n");
    let source = LogSource::File {
        path: path.display().to_string(),
    };
    let stream = bounded(fixture.client.open_stream(
        "logs.open",
        serde_json::json!({"sources":[source],"from":"tail"}),
    ))
    .await
    .unwrap();
    stream.credit(256 * 1024).await.unwrap();
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b"new\n")
        .unwrap();
    let bytes = stream.read_until(b"\n", Duration::from_secs(2)).await;
    let row: LogLine = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(row.text, "new");
    let page: SearchResult = bounded(fixture.client.call::<devbox_observe::LogsSearch>(
        &SearchLogs {
            sources: vec![source],
            query: "".into(),
            regex: false,
            before: None,
            limit: 1,
        },
    ))
    .await
    .unwrap();
    assert_eq!(page.items[0].text, "new");
    assert!(page.next_cursor.is_some());
    stream.close().await.unwrap();
    fixture.stop().await;
}
#[tokio::test]
async fn rpc_rejects_outside_symlink_invalid_regex_and_unknown_run() {
    let fixture = Fixture::start().await;
    let inside = fixture.file("inside.log", b"ok\n");
    let outside = fixture.daemon.home.path().join("outside.log");
    std::fs::write(&outside, "private").unwrap();
    let link = inside.with_file_name("outside-link");
    std::os::unix::fs::symlink(outside, &link).unwrap();
    for (source, regex, query, code) in [
        (
            serde_json::json!({"kind":"file","path":link}),
            false,
            "",
            "observe.file_outside_roots",
        ),
        (
            serde_json::json!({"kind":"file","path":inside}),
            true,
            "[",
            "observe.invalid_regex",
        ),
        (
            serde_json::json!({"kind":"run","runId":"../../outside"}),
            false,
            "",
            "observe.run_not_found",
        ),
    ] {
        let error = fixture
            .client
            .raw_call(
                "logs.search",
                serde_json::json!({"sources":[source],"query":query,"regex":regex,"limit":1}),
                None,
            )
            .await
            .unwrap_err();
        assert!(format!("{error:?}").contains(code), "{error:?}");
    }
    fixture.stop().await;
}

#[tokio::test]
#[ignore = "needs systemd user"]
async fn recorded_run_stdout_stderr_are_streamed_and_searched() {
    use devbox_runtime::{RunState, SaveTo};
    use support::runtime::{RuntimeFixture, task};
    let fixture = RuntimeFixture::new();
    let client = fixture.daemon.client().await;
    let project = fixture.project(&client).await;
    fixture
        .define(
            &client,
            &project,
            task("logs", "printf 'task7-out\\n'; printf 'task7-err\\n' >&2"),
            SaveTo::ThisPc,
        )
        .await;
    let started = fixture.start(&client, &project, "logs").await;
    let run = fixture
        .wait(
            &client,
            &project,
            &started.id,
            RunState::Exited,
            Duration::from_secs(5),
        )
        .await;
    let source = LogSource::Run { run_id: run.id };
    let page = bounded(client.call::<devbox_observe::LogsSearch>(&SearchLogs {
        sources: vec![source.clone()],
        query: "task7-".into(),
        regex: false,
        before: None,
        limit: 256,
    }))
    .await
    .unwrap();
    assert_eq!(page.items.len(), 2);
    assert!(
        page.items
            .iter()
            .any(|l| l.text == "task7-out" && l.stream == devbox_observe::LogStream::Stdout)
    );
    assert!(
        page.items
            .iter()
            .any(|l| l.text == "task7-err" && l.stream == devbox_observe::LogStream::Stderr)
    );
    let stream = bounded(client.open_stream(
        "logs.open",
        serde_json::json!({"sources":[source],"from":{"offset":0}}),
    ))
    .await
    .unwrap();
    let bytes = stream.read_available(Duration::from_millis(150)).await;
    let rows: Vec<LogLine> = bytes
        .split(|b| *b == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice(line).unwrap())
        .collect();
    assert_eq!(rows.len(), 3);
    assert!(
        rows.iter()
            .any(|l| l.stream == devbox_observe::LogStream::System
                && devbox_core::run_log::parse_attempt_marker(&l.text).is_some())
    );
    assert!(
        rows.iter()
            .any(|l| l.text == "task7-out" && l.stream == devbox_observe::LogStream::Stdout)
    );
    assert!(
        rows.iter()
            .any(|l| l.text == "task7-err" && l.stream == devbox_observe::LogStream::Stderr)
    );
    stream.close().await.unwrap();
}
