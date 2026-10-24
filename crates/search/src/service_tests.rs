use crate::fixture::*;
use crate::*;

#[tokio::test]
async fn query_chunks_arrive_notes_then_files_then_code_and_end_done() -> T {
    let s = fixture();
    let p = s.project("p");
    write(&p.join("src/needle.rs"), "needle\n");
    s.index_note("needle.md", "t", "a needle").await?;
    let chunks = s
        .run(q("needle", &[Scope::Code, Scope::Files, Scope::Notes]))
        .await
        .map_err(|e| e.to_string())?;
    let scopes: Vec<Scope> = chunks.iter().map(|c| c.scope).collect();
    // Code chunks are sent while walking; the scope ends with an empty chunk.
    assert_eq!(
        scopes,
        [Scope::Notes, Scope::Files, Scope::Code, Scope::Code],
        "{chunks:?}"
    );
    let done: Vec<bool> = chunks.iter().map(|c| c.done).collect();
    assert_eq!(done, [false, false, false, true]);
    assert!(chunks[3].items.is_empty() && !chunks[3].truncated);
    assert_eq!(
        found(&chunks, Scope::Notes),
        [("vault".into(), "needle.md".into())]
    );
    assert_eq!(
        found(&chunks, Scope::Files),
        [
            ("project:p".into(), "src/needle.rs".into()),
            ("vault".into(), "needle.md".into())
        ]
    );
    assert_eq!(
        found(&chunks, Scope::Code),
        [("project:p".into(), "src/needle.rs".into())]
    );
    assert!(chunks.iter().all(|c| c.rev == "1"));
    // A scope with nothing found still reports once, so the screen knows it finished.
    let chunks = s
        .run(q("absent", &[Scope::Notes]))
        .await
        .map_err(|e| e.to_string())?;
    assert_eq!(chunks.len(), 1);
    assert!(chunks[0].done && chunks[0].items.is_empty());
    Ok(())
}

#[tokio::test]
async fn invalid_regex_is_rejected_with_position() {
    let s = fixture();
    let mut query = q("(a", &[Scope::Code]);
    query.regex = true;
    assert_eq!(
        s.run(query.clone()).await,
        Err(SearchError::InvalidRegex { position: 0 })
    );
    query.text = "가나(a".into();
    assert_eq!(
        s.run(query.clone()).await,
        Err(SearchError::InvalidRegex { position: 2 }),
        "UTF-16 offset"
    );
    query.text = "ab[".into();
    assert_eq!(
        s.run(query.clone()).await,
        Err(SearchError::InvalidRegex { position: 2 })
    );
    query.text = "(a".into();
    query.regex = false;
    assert!(s.run(query).await.is_ok(), "literal text is not a regex");
}

#[tokio::test]
async fn oversized_regex_programs_are_refused() {
    let s = fixture();
    let mut query = q("(\\w{50}){20}", &[Scope::Code]);
    query.regex = true;
    assert!(matches!(
        s.run(query).await,
        Err(SearchError::InvalidRegex { .. })
    ));
}

#[tokio::test]
async fn query_inputs_are_checked_before_any_work() {
    let s = fixture();
    s.project("p");
    let long = "a".repeat(MAX_TEXT_BYTES + 1);
    let ids = (0..=MAX_PROJECT_IDS)
        .map(|i| format!("p{i}"))
        .collect::<Vec<_>>();
    let cases: Vec<(SearchQuery, SearchError)> = vec![
        (
            SearchQuery {
                limit: 0,
                ..q("x", &[Scope::Notes])
            },
            SearchError::InvalidInput,
        ),
        (
            SearchQuery {
                limit: MAX_QUERY_LIMIT + 1,
                ..q("x", &[Scope::Notes])
            },
            SearchError::InvalidInput,
        ),
        (q("x", &[]), SearchError::InvalidInput),
        (q("  ", &[Scope::Code]), SearchError::InvalidInput),
        (q("a\0", &[Scope::Code]), SearchError::InvalidInput),
        (q(&long, &[Scope::Code]), SearchError::InvalidInput),
        (
            SearchQuery {
                project_ids: Some(ids),
                ..q("x", &[Scope::Code])
            },
            SearchError::InvalidInput,
        ),
        (
            SearchQuery {
                project_ids: Some(vec!["nope".into()]),
                ..q("x", &[Scope::Code])
            },
            SearchError::ProjectNotFound,
        ),
    ];
    for (query, want) in cases {
        assert_eq!(
            s.run(query.clone()).await,
            Err(want),
            "{:?}",
            query.text.get(..8)
        );
    }
    let only = SearchQuery {
        project_ids: Some(vec!["p".into()]),
        ..q("x", &[Scope::Code])
    };
    assert!(s.run(only).await.is_ok());
}

#[tokio::test]
async fn project_ids_limit_code_and_file_names() -> T {
    let s = fixture();
    for id in ["a", "b"] {
        write(&s.project(id).join("needle.txt"), "needle\n");
    }
    let chunks = s
        .run(SearchQuery {
            project_ids: Some(vec!["b".into()]),
            ..q("needle", &[Scope::Code, Scope::Files])
        })
        .await
        .map_err(|e| e.to_string())?;
    let want = [("project:b".to_string(), "needle.txt".to_string())];
    assert_eq!(found(&chunks, Scope::Code), want);
    assert_eq!(found(&chunks, Scope::Files), want);
    Ok(())
}

#[tokio::test]
async fn regex_and_case_sensitive_note_search_scan_stored_text() -> T {
    let s = fixture();
    s.index_note("upper.md", "u", "Foo bar").await?;
    s.index_note("lower.md", "l", "foo bar").await?;
    let mut query = q("Foo", &[Scope::Notes]);
    query.case_sensitive = true;
    let chunks = s.run(query).await.map_err(|e| e.to_string())?;
    assert_eq!(
        found(&chunks, Scope::Notes),
        [("vault".into(), "upper.md".into())]
    );
    let hit = &chunks[0].items[0];
    assert_eq!(hl(&hit.snippet, hit.ranges_utf16[0]), "Foo");
    let mut query = q("f[o]+ b", &[Scope::Notes]);
    query.regex = true;
    let chunks = s.run(query).await.map_err(|e| e.to_string())?;
    assert_eq!(found(&chunks, Scope::Notes).len(), 2);
    // Punctuation-only text has no FTS term; it is matched literally.
    s.index_note("arrow.md", "a", "x -> y").await?;
    let chunks = s
        .run(q("->", &[Scope::Notes]))
        .await
        .map_err(|e| e.to_string())?;
    assert_eq!(
        found(&chunks, Scope::Notes),
        [("vault".into(), "arrow.md".into())]
    );
    Ok(())
}

#[tokio::test]
async fn trash_and_temp_files_are_not_searched() -> T {
    let s = fixture();
    for path in [".trash/2026-10-23/n.md", "a/.devbox-tmp-1/n.md"] {
        s.index_note(path, "needle", "needle").await?;
    }
    s.index_note("needle.md", "needle", "needle").await?;
    let p = s.project("p");
    for rel in [
        ".trash/needle.rs",
        "a/.devbox-tmp-1/needle.rs",
        "src/needle.rs",
    ] {
        write(&p.join(rel), "needle\n");
    }
    let chunks = s
        .run(q("needle", &[Scope::Notes, Scope::Files, Scope::Code]))
        .await
        .map_err(|e| e.to_string())?;
    assert_eq!(
        found(&chunks, Scope::Notes),
        [("vault".into(), "needle.md".into())]
    );
    assert_eq!(
        found(&chunks, Scope::Files),
        [
            ("project:p".into(), "src/needle.rs".into()),
            ("vault".into(), "needle.md".into())
        ]
    );
    assert_eq!(
        found(&chunks, Scope::Code),
        [("project:p".into(), "src/needle.rs".into())]
    );
    Ok(())
}

#[tokio::test]
async fn private_folders_inside_a_project_are_not_searched() -> T {
    let s = fixture_with(
        SearchSettings::default(),
        files::GitConfig::default(),
        |base| vec![base.join("p/data")],
    );
    let p = s.project("p");
    write(&p.join("data/needle.json"), "needle\n");
    write(&p.join("src/needle.rs"), "needle\n");
    let chunks = s
        .run(q("needle", &[Scope::Files, Scope::Code]))
        .await
        .map_err(|e| e.to_string())?;
    let want = [("project:p".to_string(), "src/needle.rs".to_string())];
    assert_eq!(found(&chunks, Scope::Files), want);
    assert_eq!(found(&chunks, Scope::Code), want);
    Ok(())
}

#[tokio::test]
async fn saved_searches_round_trip_and_are_bounded() -> T {
    let s = fixture();
    let saved = s
        .svc
        .save(SaveSearch {
            name: "내 검색".into(),
            query: q("needle", &[Scope::Notes]),
        })
        .await
        .map_err(fail)?;
    let list = s.svc.saved(NoParams {}).await.map_err(fail)?;
    assert_eq!(list.rev, saved.rev);
    assert_eq!(list.items.len(), 1);
    assert_eq!(
        (list.items[0].id.as_str(), list.items[0].name.as_str()),
        (saved.id.as_str(), "내 검색")
    );
    assert_eq!(list.items[0].query, q("needle", &[Scope::Notes]));
    s.svc
        .unsave(SavedId {
            id: saved.id.clone(),
        })
        .await
        .map_err(fail)?;
    assert!(
        s.svc
            .saved(NoParams {})
            .await
            .map_err(fail)?
            .items
            .is_empty()
    );
    assert_eq!(
        s.svc.unsave(SavedId { id: saved.id }).await.map_err(fail),
        Err(SearchError::NotFound)
    );
    for (name, query) in [
        ("", q("x", &[Scope::Notes])),
        ("a\nb", q("x", &[Scope::Notes])),
        (&"가".repeat(MAX_NAME_CHARS + 1), q("x", &[Scope::Notes])),
        (
            "ok",
            SearchQuery {
                limit: 0,
                ..q("x", &[Scope::Notes])
            },
        ),
        ("ok", q("", &[Scope::Notes])),
    ] {
        let r = s
            .svc
            .save(SaveSearch {
                name: name.into(),
                query,
            })
            .await
            .map_err(fail);
        assert_eq!(r.map(|_| ()), Err(SearchError::InvalidInput), "{name:?}");
    }
    for i in 0..MAX_SAVED {
        s.svc
            .save(SaveSearch {
                name: format!("s{i}"),
                query: q("x", &[Scope::Notes]),
            })
            .await
            .map_err(fail)?;
    }
    let over = s
        .svc
        .save(SaveSearch {
            name: "over".into(),
            query: q("x", &[Scope::Notes]),
        })
        .await
        .map_err(fail);
    assert_eq!(over.map(|_| ()), Err(SearchError::InvalidInput));
    assert_eq!(
        s.svc.saved(NoParams {}).await.map_err(fail)?.items.len(),
        MAX_SAVED
    );
    Ok(())
}

#[tokio::test]
async fn file_listing_stops_when_the_query_is_cancelled() {
    let t = tempfile::tempdir().expect("fixture");
    let git = files::GitConfig {
        program: script(&t.path().join("git"), "sleep 1"),
        ..files::GitConfig::default()
    };
    let s = fixture_with(SearchSettings::default(), git, |_| Vec::new());
    for id in ["a", "b", "c"] {
        s.project(id);
    }
    let cancel = tokio_util::sync::CancellationToken::new();
    let (tx, _rx) = tokio::sync::mpsc::channel(16);
    let stop = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        stop.cancel();
    });
    let started = std::time::Instant::now();
    s.svc
        .run(q("x", &[Scope::Files]), cancel, tx)
        .await
        .map_err(fail)
        .expect("run");
    assert!(
        started.elapsed() < std::time::Duration::from_millis(1800),
        "{:?}",
        started.elapsed()
    );
}
