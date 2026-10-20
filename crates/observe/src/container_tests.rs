use crate::stream_tests::{Open, until};
use crate::*;
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};
fn docker(root: &Path) -> PathBuf {
    let script = root.join("docker");
    std::fs::write(&script, r#"#!/usr/bin/python3
import os, sys, signal
from pathlib import Path
root=Path(__file__).parent
(root/'pid').write_text(str(os.getpid()))
assert sys.argv[1:3] == ['logs', '--timestamps'], sys.argv
mode=sys.argv[-1]
assert mode in ['history','live','idle','eof','failure','tabs']
if mode in ['idle','live']:
    if mode == 'live': os.write(1,b'2026-01-01T00:00:01Z '+b'x'*1000000+b'\n')
    signal.pause()
elif mode == 'failure': sys.exit(1)
elif mode == 'tabs':
    os.write(1,b'2026-01-01T00:00:01Z hello\tworld\n2026-01-01T00:00:02Z hello\twide\tworld\n')
    os.write(2,b'2026-01-01T00:00:03Z final\n')
else:
    out=b'2026-01-01T00:00:01Z a\n2026-01-01T00:00:02Z b\n2026-01-01T00:00:03Z c\n2026-01-01T00:00:03Z d\n'
    err=b'2026-01-01T00:00:03Z e\n2026-01-01T00:00:04Z f\n'
    toggle=root/'toggle'
    if toggle.exists():
        os.write(2,err); os.write(1,out); toggle.unlink()
    else:
        os.write(1,out); os.write(2,err); toggle.touch()
"#).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    script
}
fn source(id: &str) -> ResolvedSource {
    ResolvedSource {
        source: LogSource::Container { id: id.into() },
        path: None,
    }
}
fn ctx() -> devbox_protocol::Ctx {
    devbox_protocol::Ctx {
        channel: 0,
        cancel: tokio_util::sync::CancellationToken::new(),
        streams: None,
    }
}
fn alive(root: &Path) -> bool {
    root.join("pid").exists()
        && std::fs::read_to_string(root.join("pid"))
            .is_ok_and(|pid| Path::new("/proc").join(pid).exists())
}
#[tokio::test]
async fn container_snapshot_search_has_stable_pipe_cursors_and_equal_timestamps() {
    let temp = tempfile::tempdir().unwrap();
    let service = ObserveService::new(docker(temp.path()));
    let mut cursor = None;
    let mut rows = vec![];
    for _ in 0..7 {
        let source = source("history");
        let page = service
            .search(
                ctx(),
                SearchLogs {
                    sources: vec![source.source.clone()],
                    query: "".into(),
                    regex: false,
                    before: cursor,
                    limit: 1,
                },
                vec![source],
                vec![],
            )
            .await
            .unwrap();
        rows.extend(page.items.into_iter().map(|l| (l.text, l.stream)));
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    assert!(cursor.is_none());
    assert_eq!(
        rows.iter().map(|r| r.0.as_str()).collect::<Vec<_>>(),
        ["f", "d", "c", "e", "b", "a"]
    );
    assert_eq!(rows[0].1, LogStream::Stderr);
    assert!(!alive(temp.path()));
    service.shutdown().await;
}
#[tokio::test]
async fn container_only_eof_delivers_backlog_then_fin_and_reaps_child() {
    for id in ["eof", "failure"] {
        let temp = tempfile::tempdir().unwrap();
        let service = ObserveService::new(docker(temp.path()));
        let open = Open::new(256 * 1024);
        let source = source(id);
        service
            .open(
                open.ctx(),
                OpenLogs {
                    sources: vec![source.source.clone()],
                    from: LogFrom::Offset { offset: 0 },
                    filter: None,
                },
                vec![source],
                vec![],
            )
            .await
            .unwrap();
        until(|| open.finished()).await;
        if id == "eof" {
            let rows: Vec<LogLine> = open
                .data()
                .split(|b| *b == b'\n')
                .filter(|s| !s.is_empty())
                .map(|s| serde_json::from_slice(s).unwrap())
                .collect();
            assert_eq!(
                rows.iter().map(|r| r.text.as_str()).collect::<Vec<_>>(),
                ["a", "b", "c", "d", "e", "f"]
            );
        }
        assert!(!alive(temp.path()));
        service.shutdown().await;
    }
}
#[tokio::test]
async fn container_close_reaps_owned_child_during_idle_and_credit_wait() {
    for id in ["idle", "live"] {
        let temp = tempfile::tempdir().unwrap();
        let service = ObserveService::new(docker(temp.path()));
        let open = Open::new(0);
        let source = source(id);
        service
            .open(
                open.ctx(),
                OpenLogs {
                    sources: vec![source.source.clone()],
                    from: LogFrom::Tail(Tail::Tail),
                    filter: None,
                },
                vec![source],
                vec![],
            )
            .await
            .unwrap();
        until(|| alive(temp.path())).await;
        open.closed.cancel();
        until(|| !alive(temp.path())).await;
        service.shutdown().await;
    }
}
#[tokio::test]
async fn container_search_cancellation_reaps_owned_child() {
    let temp = tempfile::tempdir().unwrap();
    let service = std::sync::Arc::new(ObserveService::new(docker(temp.path())));
    let ctx = ctx();
    let cancel = ctx.cancel.clone();
    let work = service.clone();
    let task = tokio::spawn(async move {
        let source = source("idle");
        work.search(
            ctx,
            SearchLogs {
                sources: vec![source.source.clone()],
                query: "".into(),
                regex: false,
                before: None,
                limit: 1,
            },
            vec![source],
            vec![],
        )
        .await
    });
    until(|| alive(temp.path())).await;
    cancel.cancel();
    assert!(task.await.unwrap().is_err());
    assert!(!alive(temp.path()));
    service.shutdown().await;
}
#[tokio::test]
async fn container_offset_skips_raw_bytes_in_each_pipe() {
    let temp = tempfile::tempdir().unwrap();
    let service = ObserveService::new(docker(temp.path()));
    let open = Open::new(256 * 1024);
    let source = source("eof");
    service
        .open(
            open.ctx(),
            OpenLogs {
                sources: vec![source.source.clone()],
                from: LogFrom::Offset {
                    offset: b"2026-01-01T00:00:01Z a\n".len() as u64,
                },
                filter: None,
            },
            vec![source],
            vec![],
        )
        .await
        .unwrap();
    until(|| open.finished()).await;
    let rows: Vec<LogLine> = open
        .data()
        .split(|b| *b == b'\n')
        .filter(|s| !s.is_empty())
        .map(|s| serde_json::from_slice(s).unwrap())
        .collect();
    assert_eq!(
        rows.iter().map(|r| r.text.as_str()).collect::<Vec<_>>(),
        ["b", "c", "d", "f"]
    );
    service.shutdown().await;
}

#[tokio::test]
async fn docker_tabs_survive_live_stream_and_snapshot_search() {
    let temp = tempfile::tempdir().unwrap();
    let service = ObserveService::new(docker(temp.path()));
    let open = Open::new(256 * 1024);
    let target = source("tabs");
    service
        .open(
            open.ctx(),
            OpenLogs {
                sources: vec![target.source.clone()],
                from: LogFrom::Offset { offset: 0 },
                filter: None,
            },
            vec![target.clone()],
            vec![],
        )
        .await
        .unwrap();
    until(|| open.finished()).await;
    let rows: Vec<LogLine> = open
        .data()
        .split(|b| *b == b'\n')
        .filter(|s| !s.is_empty())
        .map(|s| serde_json::from_slice(s).unwrap())
        .collect();
    assert_eq!(
        rows.iter()
            .map(|r| (r.ts.as_str(), r.text.as_str()))
            .collect::<Vec<_>>(),
        [
            ("2026-01-01T00:00:01Z", "hello\tworld"),
            ("2026-01-01T00:00:02Z", "hello\twide\tworld"),
            ("2026-01-01T00:00:03Z", "final")
        ]
    );
    let page = service
        .search(
            ctx(),
            SearchLogs {
                sources: vec![target.source.clone()],
                query: "".into(),
                regex: false,
                before: None,
                limit: 256,
            },
            vec![target],
            vec![],
        )
        .await
        .unwrap();
    assert_eq!(
        page.items
            .iter()
            .map(|r| (r.ts.as_str(), r.text.as_str()))
            .collect::<Vec<_>>(),
        [
            ("2026-01-01T00:00:03Z", "final"),
            ("2026-01-01T00:00:02Z", "hello\twide\tworld"),
            ("2026-01-01T00:00:01Z", "hello\tworld")
        ]
    );
    service.shutdown().await;
}
