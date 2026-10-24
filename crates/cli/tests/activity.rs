mod support;
use devbox_activity::model::event::{Observation, ObservationKind};
use devbox_activity::{DayQuery, Ingest, IngestBatch, Ingested};
use devbox_cli::client::{Client, ClientError};
use devbox_protocol::frame::FrameCodec;
use devbox_protocol::message::{ClientKind, ClientMessage, ServerMessage, json_frame, parse_json};
use futures::{SinkExt, StreamExt};
use std::time::{Duration, Instant};
use support::{TestDaemon, bounded};

const TITLE: &str = "owned-activity-title-7f3e";

fn local_ms(h: u32) -> i64 {
    use chrono::TimeZone;
    let date = chrono::NaiveDate::from_ymd_opt(2026, 3, 10).expect("date");
    chrono::Local
        .from_local_datetime(&date.and_hms_opt(h, 0, 0).expect("time"))
        .earliest()
        .expect("local time")
        .timestamp_millis()
}

fn ingest() -> Ingest {
    let obs = |seq, at, kind, app: Option<&str>, title: Option<&str>| Observation {
        seq,
        at_ms: at,
        kind,
        app: app.map(Into::into),
        title: title.map(Into::into),
        last_input_ms: at,
    };
    let (start, end) = (local_ms(10), local_ms(11));
    Ingest {
        batch: Some(IngestBatch {
            run_id: "owned-run".into(),
            upto: 2,
        }),
        events: vec![
            obs(
                1,
                start,
                ObservationKind::Foreground,
                Some("code.exe"),
                Some(TITLE),
            ),
            obs(2, end, ObservationKind::Pause, None, None),
        ],
        dropped: 0,
    }
}

async fn client(d: &TestDaemon, kind: ClientKind) -> Client {
    bounded(Client::connect(&d.socket, kind, &d.instance))
        .await
        .expect("fixture client")
}

/// The renderer: an App hello and requests on channel 1 (channel 0 is native).
async fn renderer_ingest(d: &TestDaemon) -> ServerMessage {
    renderer_send(d, &ingest(), "activity-owned-run-2").await
}

async fn renderer_send(d: &TestDaemon, params: &Ingest, request_id: &str) -> ServerMessage {
    let stream = tokio::net::UnixStream::connect(&d.socket)
        .await
        .expect("renderer socket");
    let mut wire = tokio_util::codec::Framed::new(stream, FrameCodec::default());
    wire.send(json_frame(
        1,
        &ClientMessage::Hello {
            version: devbox_cli::VERSION.into(),
            protocol: devbox_protocol::PROTOCOL_VERSION,
            client: ClientKind::App,
            instance: d.instance.clone(),
        },
    ))
    .await
    .expect("renderer hello");
    let _ = bounded(wire.next()).await.expect("welcome").expect("frame");
    wire.send(json_frame(
        1,
        &ClientMessage::Request {
            id: 1,
            method: "activity.ingest".into(),
            params: serde_json::value::to_raw_value(params).expect("params"),
            client_request_id: Some(request_id.into()),
        },
    ))
    .await
    .expect("renderer request");
    let frame = bounded(wire.next())
        .await
        .expect("response")
        .expect("frame");
    parse_json::<ServerMessage>(&frame).expect("reply")
}

#[tokio::test]
async fn ingest_is_app_only() {
    let d = TestDaemon::start();
    for kind in [
        ClientKind::Cli,
        ClientKind::Mcp,
        ClientKind::Dev,
        ClientKind::Test,
        ClientKind::App, // channel 0 belongs to native secrets and Windows runs
    ] {
        let other = client(&d, kind).await;
        let result = other.raw_call("activity.ingest", ingest(), None).await;
        assert!(
            matches!(&result, Err(ClientError::Rpc { code, .. }) if code == "method_not_found"),
            "{kind:?}: {result:?}"
        );
    }
    let ServerMessage::Response {
        result: Some(result),
        error: None,
        ..
    } = renderer_ingest(&d).await
    else {
        panic!("the renderer may ingest");
    };
    let ingested: Ingested = serde_json::from_str(result.get()).expect("ingest result");
    assert_eq!(ingested.accepted, 2);
    // The daemon's 1 s writer stores the session without being asked.
    let test = d.client().await;
    let deadline = Instant::now() + Duration::from_secs(10);
    let day = loop {
        let day = bounded(test.call::<devbox_activity::ActivityDay>(&DayQuery {
            date: "2026-03-10".into(),
        }))
        .await
        .expect("activity.day");
        if day.total_ms > 0 || Instant::now() > deadline {
            break day;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    assert_eq!(day.total_ms, 3_600_000);
    assert_eq!(day.titles[0].title, TITLE);
    let logs = std::fs::read_to_string(d.home.path().join("stderr.log")).expect("daemon logs");
    let file_logs = std::fs::read_dir(&d.paths.log_dir)
        .map(|dir| {
            dir.flatten()
                .filter_map(|e| std::fs::read_to_string(e.path()).ok())
                .collect::<String>()
        })
        .unwrap_or_default();
    assert!(
        !logs.contains(TITLE) && !file_logs.contains(TITLE),
        "titles never reach logs"
    );
}

fn accepted(reply: ServerMessage) -> u32 {
    let ServerMessage::Response {
        result: Some(result),
        error: None,
        ..
    } = reply
    else {
        panic!("ingest failed: {reply:?}");
    };
    serde_json::from_str::<Ingested>(result.get())
        .expect("ingest result")
        .accepted
}

/// Waits for the daemon's writer, then reads the stored rows (ids are not on the wire).
async fn stored(d: &TestDaemon) -> Vec<devbox_activity::Session> {
    let test = d.client().await;
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let status =
            bounded(test.call::<devbox_activity::ActivityGetStatus>(&devbox_activity::NoParams {}))
                .await
                .expect("activity.status");
        if status.pending == 0 {
            break;
        }
        assert!(Instant::now() < deadline, "writer deadline: {status:?}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let migrations = [
        devbox_projects::MIGRATIONS,
        devbox_terminal::MIGRATIONS,
        devbox_agents::MIGRATIONS,
        devbox_runtime::MIGRATIONS,
        devbox_secrets::MIGRATIONS,
        devbox_search::MIGRATIONS,
        devbox_activity::MIGRATIONS,
    ]
    .concat();
    let db = devbox_core::Db::open(
        &d.paths.db_path,
        &d.paths.backups_dir,
        devbox_cli::VERSION,
        &migrations,
    )
    .expect("owned DB");
    devbox_activity::store::overlapping(&db, local_ms(0), local_ms(23))
        .await
        .expect("stored sessions")
}

#[tokio::test]
async fn replayed_batch_is_ignored() {
    let mut d = TestDaemon::start();
    assert_eq!(
        accepted(renderer_send(&d, &ingest(), "activity-replay-a").await),
        2
    );
    // Same `{runId, upto}` under a new request id: past the dedupe window.
    assert_eq!(
        accepted(renderer_send(&d, &ingest(), "activity-replay-b").await),
        0
    );
    let before = stored(&d).await;
    assert_eq!(before.len(), 1, "{before:?}");
    // A restart forgets the run's `upto`; the deterministic id keeps one row.
    d.restart();
    assert_eq!(
        accepted(renderer_send(&d, &ingest(), "activity-replay-c").await),
        2
    );
    let after = stored(&d).await;
    let ids =
        |rows: &[devbox_activity::Session]| rows.iter().map(|s| s.id.clone()).collect::<Vec<_>>();
    assert_eq!(ids(&after), ids(&before));
    assert_eq!(
        (after[0].start_ms, after[0].end_ms),
        (local_ms(10), local_ms(11))
    );
}

#[tokio::test]
async fn graceful_owned_daemon_shutdown_finalizes_the_open_activity_run() {
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;
    let mut d = TestDaemon::start();
    let mut batch = ingest();
    batch.events[1].kind = ObservationKind::Input;
    batch.events[1].app = Some("code.exe".into());
    assert_eq!(
        accepted(renderer_send(&d, &batch, "activity-open-shutdown").await),
        2
    );
    assert!(
        stored(&d).await.is_empty(),
        "ordinary writer flush cannot see the open session"
    );
    let child = d.child.take().expect("owned foreground fixture child");
    kill(Pid::from_raw(child.0.id() as i32), Signal::SIGTERM).expect("term only owned fixture PID");
    assert!(support::exits(child).success(), "graceful fixture shutdown");
    d.launch();
    let rows = stored(&d).await;
    assert_eq!(
        rows.len(),
        1,
        "daemon shutdown finalized the interval before restart"
    );
    assert_eq!(
        (rows[0].start_ms, rows[0].end_ms),
        (local_ms(10), local_ms(11))
    );
    assert_eq!(rows[0].title, TITLE);
}
