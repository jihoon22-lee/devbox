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
            params: serde_json::value::to_raw_value(&ingest()).expect("params"),
            client_request_id: Some("activity-owned-run-2".into()),
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
