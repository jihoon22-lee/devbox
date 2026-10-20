use crate::{LogLine, LogSource, LogStream, logs::oldest_head};
fn at(ts: &str, text: &str) -> LogLine {
    LogLine {
        src: LogSource::File {
            path: "/fixture".into(),
        },
        ts: ts.into(),
        stream: LogStream::Stdout,
        text: text.into(),
    }
}
#[test]
fn merges_two_sources_in_timestamp_order() {
    let mut sources = [
        std::collections::VecDeque::from([
            at("2026-01-01T00:00:01Z", "a"),
            at("2026-01-01T00:00:03Z", "c"),
        ]),
        std::collections::VecDeque::from([at("2026-01-01T00:00:02Z", "b")]),
    ];
    let mut merged = vec![];
    while let Some(index) = oldest_head(
        &sources
            .iter()
            .map(|s| s.front().cloned())
            .collect::<Vec<_>>(),
    ) {
        merged.push(sources[index].pop_front().expect("fixture value").text);
    }
    assert_eq!(merged, ["a", "b", "c"]);
}

fn ctx() -> devbox_protocol::Ctx {
    devbox_protocol::Ctx {
        channel: 0,
        cancel: tokio_util::sync::CancellationToken::new(),
        streams: None,
    }
}
fn resolved(path: &std::path::Path, run: bool) -> crate::ResolvedSource {
    crate::ResolvedSource {
        source: if run {
            LogSource::Run {
                run_id: path
                    .file_name()
                    .expect("fixture value")
                    .to_string_lossy()
                    .into_owned(),
            }
        } else {
            LogSource::File {
                path: path.display().to_string(),
            }
        },
        path: Some(path.into()),
    }
}
async fn search(
    service: &crate::ObserveService,
    sources: Vec<crate::ResolvedSource>,
    roots: Vec<std::path::PathBuf>,
    before: Option<crate::SearchCursor>,
    limit: u32,
) -> crate::SearchResult {
    service
        .search(
            ctx(),
            crate::SearchLogs {
                sources: sources.iter().map(|s| s.source.clone()).collect(),
                query: "".into(),
                regex: false,
                before,
                limit,
            },
            sources,
            roots,
        )
        .await
        .expect("fixture result")
}
#[tokio::test]
async fn follows_appended_lines_and_survives_truncation() {
    use tokio::io::AsyncWriteExt;
    let temp = tempfile::tempdir().expect("fixture result");
    let path = temp.path().join("log");
    std::fs::write(&path, b"old-long-line\n").expect("fixture result");
    let source = resolved(&path, false);
    let mut reader = crate::sources::FileSource::open(
        &source,
        &[temp.path().into()],
        &crate::LogFrom::Tail(crate::Tail::Tail),
    )
    .await
    .expect("fixture result");
    assert!(reader.next().await.expect("fixture result").is_none());
    let mut file = tokio::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .await
        .expect("fixture result");
    file.write_all(b"new\n").await.expect("fixture result");
    // Tokio queues the write; the independent reader must wait for its completion.
    file.flush().await.expect("fixture append completed");
    assert_eq!(
        reader
            .next()
            .await
            .expect("fixture result")
            .expect("fixture value")
            .text,
        "new"
    );
    std::fs::write(&path, b"short\n").expect("fixture result");
    reader.refresh().await.expect("fixture result");
    assert_eq!(
        reader
            .next()
            .await
            .expect("fixture result")
            .expect("fixture value")
            .text,
        "short"
    );
    let next = temp.path().join("next");
    std::fs::write(&next, b"rotated\n").expect("fixture result");
    std::fs::rename(next, &path).expect("fixture result");
    reader.refresh().await.expect("fixture result");
    assert_eq!(
        reader
            .next()
            .await
            .expect("fixture result")
            .expect("fixture value")
            .text,
        "rotated"
    );
    // EOF fragments are emitted once; the cursor consumes their exact bytes.
    std::fs::write(&path, b"partial").expect("fixture result");
    reader.refresh().await.expect("fixture result");
    assert_eq!(
        reader
            .next()
            .await
            .expect("fixture result")
            .expect("fixture value")
            .text,
        "partial"
    );
    assert!(reader.next().await.expect("fixture result").is_none());
}
#[test]
fn filter_by_stream_and_regex() {
    let filter = crate::logs::Filter::new(crate::LogFilter {
        text: Some("error".into()),
        regex: Some("E[0-9]+".into()),
        streams: Some(vec![LogStream::Stderr]),
    })
    .expect("fixture result");
    let mut row = at("2026-01-01T00:00:01Z", "error E42");
    assert!(!filter.accepts(&row));
    row.stream = LogStream::Stderr;
    assert!(filter.accepts(&row));
    row.text = "error hello".into();
    assert!(!filter.accepts(&row));
    assert!(matches!(
        crate::logs::Filter::new(crate::LogFilter {
            regex: Some("[".into()),
            ..Default::default()
        }),
        Err(devbox_protocol::Fail::Domain(
            crate::ObserveError::InvalidRegex
        ))
    ));
}
#[tokio::test]
async fn file_source_outside_allowed_roots_is_rejected() {
    let root = tempfile::tempdir().expect("fixture result");
    let outside = tempfile::tempdir().expect("fixture result");
    let private = outside.path().join("private");
    std::fs::write(&private, b"private\n").expect("fixture result");
    let link = root.path().join("link");
    std::os::unix::fs::symlink(&private, &link).expect("fixture result");
    assert!(matches!(
        crate::sources::FileSource::open(
            &resolved(&link, false),
            &[root.path().into()],
            &crate::LogFrom::Tail(crate::Tail::Tail)
        )
        .await,
        Err(devbox_protocol::Fail::Domain(
            crate::ObserveError::FileOutsideRoots
        ))
    ));
    std::fs::remove_file(&link).expect("fixture result");
    std::fs::write(&link, b"safe\n").expect("fixture result");
    let mut reader = crate::sources::FileSource::open(
        &resolved(&link, false),
        &[root.path().into()],
        &crate::LogFrom::Tail(crate::Tail::Tail),
    )
    .await
    .expect("fixture result");
    std::fs::remove_file(&link).expect("fixture result");
    std::os::unix::fs::symlink(private, &link).expect("fixture result");
    assert!(matches!(
        reader.refresh().await,
        Err(devbox_protocol::Fail::Domain(
            crate::ObserveError::FileOutsideRoots
        ))
    ));
}
#[tokio::test]
async fn reverse_search_equal_times_partial_lines_and_byte_budget_progress() {
    let root = tempfile::tempdir().expect("fixture result");
    let path = root.path().join("run");
    let ts = "2026-01-01T00:00:01Z";
    std::fs::write(
        &path,
        format!(
            "{ts}\to\tfirst\n{ts}\te\tsecond\n{ts}\to\t{}\n{ts}\to\tpartial",
            "\u{0001}".repeat(500_000)
        ),
    )
    .expect("fixture result");
    let service = crate::ObserveService::default();
    let mut cursor = None;
    let mut all = vec![];
    for _ in 0..5 {
        let page = search(
            &service,
            vec![resolved(&path, true)],
            vec![root.path().into()],
            cursor,
            1,
        )
        .await;
        assert!(serde_json::to_vec(&page).expect("fixture result").len() <= 768 * 1024);
        all.extend(page.items.into_iter().map(|l| l.text));
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    assert!(cursor.is_none());
    assert_eq!(all.len(), 4);
    assert_eq!(all[0], "partial");
    assert!(all[1].ends_with("[log line truncated]"));
    assert_eq!(&all[2..], ["second", "first"]);
    service.shutdown().await;
}
#[tokio::test]
async fn search_merges_reverse_source_heads_and_rejects_stale_cursor() {
    let root = tempfile::tempdir().expect("fixture result");
    let a = root.path().join("a");
    let b = root.path().join("b");
    std::fs::write(
        &a,
        b"2026-01-01T00:00:01Z\to\ta\n2026-01-01T00:00:03Z\to\tc\n",
    )
    .expect("fixture result");
    std::fs::write(&b, b"2026-01-01T00:00:02Z\te\tb\n").expect("fixture result");
    let service = crate::ObserveService::default();
    let page = search(
        &service,
        vec![resolved(&a, true), resolved(&b, true)],
        vec![],
        None,
        256,
    )
    .await;
    assert_eq!(
        page.items
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>(),
        ["c", "b", "a"]
    );
    let page = search(&service, vec![resolved(&a, true)], vec![], None, 1).await;
    std::fs::write(&a, b"replacement\n").expect("fixture result");
    let sources = vec![resolved(&a, true)];
    assert!(matches!(
        service
            .search(
                ctx(),
                crate::SearchLogs {
                    sources: sources.iter().map(|s| s.source.clone()).collect(),
                    query: "".into(),
                    regex: false,
                    before: page.next_cursor,
                    limit: 1
                },
                sources,
                vec![]
            )
            .await,
        Err(devbox_protocol::Fail::Domain(
            crate::ObserveError::InvalidCursor
        ))
    ));
    service.shutdown().await;
}

#[tokio::test]
async fn appended_previously_empty_source_does_not_invalidate_previous_page() {
    let root = tempfile::tempdir().expect("fixture result");
    let a = root.path().join("a");
    let b = root.path().join("b");
    std::fs::write(
        &a,
        b"2026-01-01T00:00:01Z\to\ta\n2026-01-01T00:00:02Z\to\tb\n",
    )
    .expect("fixture result");
    std::fs::write(&b, b"").expect("fixture result");
    let service = crate::ObserveService::default();
    let first = search(
        &service,
        vec![resolved(&a, true), resolved(&b, true)],
        vec![],
        None,
        1,
    )
    .await;
    std::fs::write(&b, b"2026-01-01T00:00:03Z\to\tnew\n").expect("fixture result");
    let second = search(
        &service,
        vec![resolved(&a, true), resolved(&b, true)],
        vec![],
        first.next_cursor,
        1,
    )
    .await;
    assert_eq!(
        second
            .items
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>(),
        ["a"]
    );
    assert!(second.next_cursor.is_none());
    service.shutdown().await;
}

#[test]
fn docker_tabs_are_text_while_run_tabs_are_record_delimiters() {
    let source = LogSource::Container {
        id: "fixture".into(),
    };
    for text in ["hello\tworld", "hello\twide\tworld"] {
        let row = crate::logs::parse(
            &source,
            format!("2026-10-20T01:00:00Z {text}\n").as_bytes(),
            LogStream::Stderr,
        );
        assert_eq!(row.ts, "2026-10-20T01:00:00Z");
        assert_eq!(row.text, text);
        assert_eq!(row.stream, LogStream::Stderr);
    }
    let row = crate::logs::parse(
        &LogSource::Run {
            run_id: "fixture".into(),
        },
        b"2026-10-20T01:00:00Z\te\thello\tworld\n",
        LogStream::Stdout,
    );
    assert_eq!(row.ts, "2026-10-20T01:00:00Z");
    assert_eq!(row.text, "hello\tworld");
    assert_eq!(row.stream, LogStream::Stderr);
}
