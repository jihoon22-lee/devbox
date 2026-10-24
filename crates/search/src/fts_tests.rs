use crate::fixture::*;
use crate::fts::{NoteIndex, build_fts_query, grams};
use crate::*;
use devbox_core::HubMessage;
use std::time::Duration;

#[tokio::test]
async fn korean_substring_matches_in_note_fts() -> T {
    let s = fixture();
    s.index_note("a.md", "제목", "어제 데몬재시작 했다").await?;
    s.index_note("b.md", "다른", "재시 시작은 따로 쓴 두 낱말")
        .await?;
    let hits = s.notes("재시작", 10).await?;
    assert_eq!(hits.items.len(), 1, "{hits:?}");
    assert_eq!(hits.items[0].path, "a.md");
    assert_eq!(hits.items[0].title, "제목");
    assert_eq!(hits.items[0].ranges_utf16.len(), 1);
    assert_eq!(
        hl(&hits.items[0].snippet, hits.items[0].ranges_utf16[0]),
        "재시작"
    );
    Ok(())
}

#[test]
fn grams_are_run_bigrams_ending_with_the_last_syllable() {
    assert_eq!(
        grams("어제 데몬재시작 했다"),
        "어제 제 데몬 몬재 재시 시작 작 했다 다"
    );
    assert_eq!(grams("a 가 b"), "가");
    assert_eq!(grams("plain ascii"), "");
}

#[tokio::test]
async fn single_syllable_ascii_prefix_and_title_queries_match() -> T {
    let s = fixture();
    s.index_note("a.md", "Kubernetes 메모", "daemon restarted 재시작")
        .await?;
    for (text, want) in [("작", "작"), ("daem", "daem"), ("RESTART", "restart")] {
        let hits = s.notes(text, 10).await?;
        assert_eq!(hits.items.len(), 1, "{text}: {hits:?}");
        let ranges = &hits.items[0].ranges_utf16;
        assert_eq!(ranges.len(), 1, "{text}: {hits:?}");
        assert_eq!(hl(&hits.items[0].snippet, ranges[0]), want, "{text}");
    }
    let hits = s.notes("kube", 10).await?;
    assert_eq!(hits.items.len(), 1, "title-only match: {hits:?}");
    assert!(
        s.notes("aemon", 10).await?.items.is_empty(),
        "ASCII terms are word prefixes"
    );
    Ok(())
}

#[tokio::test]
async fn fts_syntax_in_the_query_is_quoted_not_interpreted() -> T {
    let s = fixture();
    s.index_note("one.md", "alpha", "one").await?;
    s.index_note("two.md", "two", "beta").await?;
    assert_eq!(
        s.notes("alpha", 10).await?.items.len(),
        1,
        "positive control"
    );
    for text in [
        "alpha OR beta",
        "title:alpha",
        "{title}:alpha",
        "alpha NOT one",
    ] {
        let hits = s.notes(text, 10).await?;
        assert!(hits.items.is_empty(), "{text} must be literal: {hits:?}");
    }
    for text in [
        "\"",
        "a\" OR \"b",
        "NEAR(alpha beta)",
        "^alpha",
        "alpha*",
        "(",
        "-alpha",
        "*",
        "\"\"",
    ] {
        s.notes(text, 10)
            .await
            .map_err(|e| format!("{text}: {e}"))?;
    }
    Ok(())
}

// v0.9.0 `crates/search/src/lib.rs` tests, names kept.
#[test]
fn tokenizes_on_whitespace() {
    assert_eq!(build_fts_query("foo bar"), "\"foo\"* \"bar\"*");
}
#[test]
fn collapses_multiple_spaces() {
    assert_eq!(build_fts_query("foo   bar"), "\"foo\"* \"bar\"*");
}
#[test]
fn escapes_double_quotes() {
    assert_eq!(build_fts_query("my \"file\""), "\"my\"* \"\"\"file\"\"\"*");
}
#[test]
fn handles_empty_input() {
    assert_eq!(build_fts_query(""), "");
    assert_eq!(build_fts_query("   "), "");
}
#[test]
fn prefixes_every_token() {
    assert_eq!(build_fts_query("abc"), "\"abc\"*");
}
#[test]
fn fts5_special_chars_are_quoted_away() {
    let q = build_fts_query("a-b");
    assert!(q.starts_with('"'), "{q}");
    let q2 = build_fts_query("*x");
    assert!(q2.starts_with('"'), "{q2}");
    let q3 = build_fts_query("foo:bar");
    assert!(q3.contains("\"foo:bar\"*"), "{q3}");
}

#[tokio::test]
async fn observed_time_only_changes_do_not_bump_the_rev() -> T {
    let s = fixture();
    let mut events = s.hub.subscribe("search.changed");
    s.index_note("a.md", "a", "first text").await?;
    assert_eq!(s.svc.rev(), 1, "a real change bumps the rev");
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(1), events.recv()).await,
        Ok(Some(HubMessage::Event { rev: 1, .. }))
    ));
    for _ in 0..3 {
        s.index_note("a.md", "a", "first text").await?;
    }
    assert_eq!(s.svc.rev(), 1, "same content three times");
    assert_eq!(s.notes("first", 10).await?.rev, "1");
    assert!(
        tokio::time::timeout(Duration::from_millis(100), events.recv())
            .await
            .is_err(),
        "no search.changed for same content"
    );
    s.index_note("a.md", "a", "second text").await?;
    assert_eq!(s.svc.rev(), 2);
    Ok(())
}

#[tokio::test]
async fn search_index_rebuilds_only_its_domain_when_version_changes() -> T {
    let s = fixture();
    s.index
        .ensure_domain("notes", 1, "CREATE TABLE notes_x(v TEXT);")?;
    s.index
        .ensure_domain("api", 1, "CREATE TABLE api_y(v TEXT);")?;
    s.index
        .write(|tx| {
            tx.execute("INSERT INTO notes_x VALUES ('n')", [])?;
            tx.execute("INSERT INTO api_y VALUES ('a')", [])
        })
        .await?;
    s.index_note("a.md", "a", "kept until the rebuild").await?;
    let count = |table: &'static str| {
        let index = s.index.clone();
        async move {
            index
                .read(move |tx| {
                    tx.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| {
                        r.get::<_, i64>(0)
                    })
                })
                .await
        }
    };
    assert_eq!(
        (count("search_docs").await?, count("search_notes").await?),
        (1, 1)
    );
    let (_, rebuilt) = NoteIndex::open(s.index.clone(), 2)?;
    assert!(rebuilt);
    assert_eq!(
        (count("search_docs").await?, count("search_notes").await?),
        (0, 0)
    );
    assert_eq!((count("notes_x").await?, count("api_y").await?), (1, 1));
    let (_, again) = NoteIndex::open(s.index.clone(), 2)?;
    assert!(!again, "the same version keeps the domain");
    Ok(())
}

#[tokio::test]
async fn trash_temp_and_hidden_notes_are_never_indexed() -> T {
    let s = fixture();
    for path in [
        ".trash/2026-10-23/a.md",
        "x/.devbox-tmp-1/b.md",
        ".hidden/c.md",
        "d/.e.md",
    ] {
        s.index_note(path, "t", "needle").await?;
    }
    s.index_note("visible.md", "t", "needle").await?;
    let hits = s.notes("needle", 10).await?;
    let paths: Vec<&str> = hits.items.iter().map(|h| h.path.as_str()).collect();
    assert_eq!(paths, ["visible.md"]);
    Ok(())
}

#[tokio::test]
async fn removed_notes_and_folders_leave_the_index() -> T {
    let s = fixture();
    for path in ["f/a.md", "f/g/b.md", "fx.md", "keep.md"] {
        s.index_note(path, "t", "needle").await?;
    }
    let rev = s.svc.rev();
    s.svc.remove_notes(vec!["f".into()]).await;
    assert_eq!(s.svc.rev(), rev + 1);
    s.svc.remove_notes(vec!["missing.md".into()]).await;
    assert_eq!(s.svc.rev(), rev + 1, "removing nothing changes nothing");
    let mut paths: Vec<String> = s
        .notes("needle", 10)
        .await?
        .items
        .into_iter()
        .map(|h| h.path)
        .collect();
    paths.sort();
    assert_eq!(paths, ["fx.md", "keep.md"]);
    Ok(())
}

#[tokio::test]
async fn code_block_and_comment_text_is_searchable_like_prose() -> T {
    let s = fixture();
    let body =
        "intro\n\n```sh\nsystemctl fencedword\n```\n\n<!-- commentword -->\n    indentedword\n";
    s.index_note("a.md", "a", body).await?;
    for word in ["fencedword", "commentword", "indentedword"] {
        assert_eq!(s.notes(word, 10).await?.items.len(), 1, "{word}");
    }
    Ok(())
}

#[tokio::test]
async fn a_bom_is_not_part_of_the_indexed_text() -> T {
    let s = fixture();
    s.index_note("a.md", "a", "\u{feff}hello world").await?;
    let hits = s.notes("hello", 10).await?;
    assert_eq!(hits.items.len(), 1);
    assert_eq!(hits.items[0].ranges_utf16, [(0, 5)], "{:?}", hits.items[0]);
    assert!(!hits.items[0].snippet.contains('\u{feff}'));
    Ok(())
}

#[tokio::test]
async fn long_notes_give_bounded_snippets_and_titles() -> T {
    let s = fixture();
    let body = format!(
        "{}\n중간에 needle 하나\n{}",
        "가".repeat(500_000),
        "b ".repeat(200_000)
    );
    s.index_note("big.md", &"제".repeat(1000), &body).await?;
    let hits = s.notes("needle", 10).await?;
    assert_eq!(hits.items.len(), 1);
    let hit = &hits.items[0];
    assert!(
        hit.snippet.chars().count() <= 160,
        "{}",
        hit.snippet.chars().count()
    );
    assert!(hit.title.chars().count() <= 256);
    assert!(!hit.snippet.contains('\n'));
    assert_eq!(hl(&hit.snippet, hit.ranges_utf16[0]), "needle");
    Ok(())
}

#[tokio::test]
async fn note_results_are_bounded_and_inputs_checked() -> T {
    let s = fixture();
    let notes = (0..60)
        .map(|i| IndexedNote {
            path: format!("n{i:02}.md"),
            title: "t".into(),
            body: "needle".into(),
        })
        .collect();
    s.svc.index_notes(notes).await;
    let hits = s.notes("needle", 50).await?;
    assert_eq!((hits.items.len(), hits.truncated), (50, true));
    let hits = s.notes("needle", 10).await?;
    assert_eq!((hits.items.len(), hits.truncated), (10, true));
    let long = "a".repeat(MAX_TEXT_BYTES + 1);
    for (text, limit) in [
        ("needle", 0),
        ("needle", 51),
        ("", 5),
        ("   ", 5),
        ("a\0b", 5),
        (long.as_str(), 5),
    ] {
        let r = s
            .svc
            .notes(NotesQuery {
                text: text.into(),
                limit,
            })
            .await;
        assert!(
            matches!(
                r,
                Err(devbox_protocol::Fail::Domain(SearchError::InvalidInput))
            ),
            "{limit} {:?}",
            text.get(..10)
        );
    }
    Ok(())
}

#[tokio::test]
async fn failed_index_writes_are_replayed_on_the_next_start() -> T {
    let mut s = fixture();
    s.index_note("gone.md", "g", "ghost words").await?;
    s.svc
        .s
        .fail_writes
        .store(true, std::sync::atomic::Ordering::SeqCst);
    s.index_note("new.md", "n", "fresh words").await?;
    s.svc.remove_notes(vec!["gone.md".into()]).await;
    s.svc
        .s
        .fail_writes
        .store(false, std::sync::atomic::Ordering::SeqCst);
    assert!(
        s.notes("fresh", 10).await?.items.is_empty(),
        "the failed write is missing"
    );
    assert!(
        s.base.join("search-reindex").exists(),
        "a failure leaves the replay marker"
    );
    s.reopen();
    assert!(s.svc.rebuilt(), "the next start feeds every note again");
    // The notes replay: every note, then the full list.
    s.index_note("new.md", "n", "fresh words").await?;
    s.svc.retain(vec!["new.md".into()]).await;
    assert_eq!(s.notes("fresh", 10).await?.items.len(), 1);
    assert!(
        s.notes("ghost", 10).await?.items.is_empty(),
        "a failed removal is applied"
    );
    assert!(
        !s.base.join("search-reindex").exists(),
        "a clean replay clears the marker"
    );
    s.reopen();
    assert!(!s.svc.rebuilt());
    Ok(())
}
