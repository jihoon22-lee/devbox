mod support;
use devbox_search::{NotesQuery, Scope, SearchChunk, SearchNotes};
use std::path::Path;
use std::time::{Duration, Instant};
use support::TestDaemon;

async fn notes(client: &devbox_cli::client::Client, text: &str) -> devbox_search::NoteHits {
    support::bounded(client.call::<SearchNotes>(&NotesQuery {
        text: text.into(),
        limit: 10,
    }))
    .await
    .expect("search.notes")
}

/// Polls `search.notes` until `want` hits or 10 s.
async fn wait_hits(
    client: &devbox_cli::client::Client,
    text: &str,
    want: usize,
) -> devbox_search::NoteHits {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let hits = notes(client, text).await;
        if hits.items.len() == want || Instant::now() > deadline {
            return hits;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Every chunk of one `search.query` stream, until `done`.
async fn query(client: &devbox_cli::client::Client, params: serde_json::Value) -> Vec<SearchChunk> {
    let stream = support::bounded(client.open_stream("search.query", params))
        .await
        .expect("search.query opens");
    stream.credit(1 << 20).await.expect("credit");
    let mut bytes = Vec::new();
    let mut chunks: Vec<SearchChunk> = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !chunks.last().is_some_and(|c| c.done) && Instant::now() < deadline {
        bytes.extend(stream.read_available(Duration::from_millis(200)).await);
        while let Some(end) = bytes.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = bytes.drain(..=end).collect();
            chunks.push(serde_json::from_slice(&line).expect("JSON line chunk"));
        }
    }
    assert!(
        stream.wait_end(Duration::from_secs(2)).await,
        "the stream ends after done"
    );
    chunks
}

/// Waits until the vault watcher is live (it starts after the startup sweep):
/// rewrites a marker note until search sees it.
async fn watching(d: &TestDaemon, client: &devbox_cli::client::Client) {
    let deadline = Instant::now() + Duration::from_secs(10);
    for i in 0.. {
        write(&vault(d).join("watch-marker.md"), &format!("marker{i}\n"));
        tokio::time::sleep(Duration::from_millis(100)).await;
        if notes(client, &format!("marker{i}")).await.items.len() == 1 {
            return;
        }
        assert!(Instant::now() < deadline, "the vault watcher did not start");
    }
}

fn vault(d: &TestDaemon) -> std::path::PathBuf {
    d.home.path().join("notes")
}
fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().expect("parent")).expect("fixture dir");
    std::fs::write(path, text).expect("fixture file");
}

#[tokio::test]
async fn query_stream_sends_notes_then_code() {
    let d = TestDaemon::start();
    let client = d.client().await;
    let project = d.home.path().join("proj");
    write(&project.join("src/main.rs"), "fn main() { needle(); }\n");
    let id = d
        .add_project(&client, project.to_str().expect("utf-8"))
        .await;
    watching(&d, &client).await;
    write(&vault(&d).join("n.md"), "a needle in a note\n");
    assert_eq!(wait_hits(&client, "needle", 1).await.items.len(), 1);
    let chunks = query(
        &client,
        serde_json::json!({"text": "needle", "scopes": ["code", "notes"], "projectIds": null,
            "regex": false, "caseSensitive": false, "limit": 10}),
    )
    .await;
    let scopes: Vec<Scope> = chunks.iter().map(|c| c.scope).collect();
    assert_eq!(
        scopes,
        [Scope::Notes, Scope::Code, Scope::Code],
        "{chunks:?}"
    );
    assert_eq!(chunks[0].items[0].path, "n.md");
    assert_eq!(chunks[1].items[0].root, format!("project:{id}"));
    assert_eq!(
        (chunks[1].items[0].path.as_str(), chunks[1].items[0].line),
        ("src/main.rs", Some(1))
    );
    assert!(chunks.last().expect("chunks").done);
    assert_eq!(chunks.iter().filter(|c| c.done).count(), 1);
}

#[tokio::test]
async fn observed_time_only_changes_do_not_bump_the_rev() {
    let d = TestDaemon::start();
    let client = d.client().await;
    watching(&d, &client).await;
    let note = vault(&d).join("touch.md");
    write(&note, "steady words\n");
    assert_eq!(wait_hits(&client, "steady", 1).await.items.len(), 1);
    // The rev is read before the hits, so the first hit can still carry the
    // previous rev; take the baseline once the index has settled.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let before = notes(&client, "steady").await;
    assert_eq!(before.items.len(), 1);
    let mut sub = support::bounded(client.subscribe("search.changed"))
        .await
        .expect("subscribe");
    for i in 1..=3u64 {
        let file = std::fs::File::options()
            .write(true)
            .open(&note)
            .expect("open note");
        file.set_modified(std::time::SystemTime::now() + Duration::from_secs(i))
            .expect("touch");
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert_eq!(notes(&client, "steady").await.rev, before.rev, "touch only");
    let quiet = tokio::time::timeout(Duration::from_millis(100), sub.next_event()).await;
    assert!(quiet.is_err(), "no search.changed for a touch: {quiet:?}");
    // Control: a real edit changes the rev once.
    write(&note, "steady words, edited\n");
    let event = tokio::time::timeout(Duration::from_secs(5), sub.next_event()).await;
    assert!(
        matches!(event, Ok(Some(_))),
        "a real edit announces search.changed"
    );
    assert_ne!(wait_hits(&client, "edited", 1).await.rev, before.rev);
}

#[tokio::test]
async fn blank_title_notes_are_found_by_file_name() {
    let mut d = TestDaemon::fixture();
    for (folder, text) in [
        ("a", "---\ntitle: \"\"\n---\n"),
        ("b", "---\ntitle: \"  \"\n---\n"),
        ("c", "no frontmatter\n"),
    ] {
        write(&vault(&d).join(folder).join("idea.md"), text);
    }
    d.launch();
    let client = d.client().await;
    let hits = wait_hits(&client, "idea", 3).await;
    let titles: Vec<&str> = hits.items.iter().map(|h| h.title.as_str()).collect();
    assert_eq!(titles, ["idea", "idea", "idea"], "{hits:?}");
    let chunks = query(
        &client,
        serde_json::json!({"text": "idea", "scopes": ["files"], "projectIds": null,
            "regex": false, "caseSensitive": false, "limit": 10}),
    )
    .await;
    let mut paths: Vec<&str> = chunks
        .iter()
        .flat_map(|c| c.items.iter().map(|h| h.path.as_str()))
        .collect();
    paths.sort();
    assert_eq!(paths, ["a/idea.md", "b/idea.md", "c/idea.md"]);
}

#[tokio::test]
async fn existing_notes_are_fed_again_when_the_search_domain_is_rebuilt() {
    let mut d = TestDaemon::start();
    let client = d.client().await;
    watching(&d, &client).await;
    write(&vault(&d).join("old.md"), "persisted body\n");
    assert_eq!(wait_hits(&client, "persisted", 1).await.items.len(), 1);
    drop(client);
    d.stop();
    let index = devbox_core::IndexDb::open(&d.paths.index_db_path).expect("open owned index");
    index
        .write(|tx| {
            tx.execute("DELETE FROM search_docs", [])?;
            tx.execute("DELETE FROM search_notes", [])?;
            tx.execute(
                "UPDATE index_schema SET version = 0 WHERE domain = 'search'",
                [],
            )
        })
        .await
        .expect("make the search domain stale");
    drop(index);
    d.launch();
    let client = d.client().await;
    let hits = wait_hits(&client, "persisted", 1).await;
    assert_eq!(
        hits.items.len(),
        1,
        "unchanged notes are replayed into a rebuilt domain"
    );
}

#[tokio::test]
async fn saved_searches_survive_a_restart() {
    let mut d = TestDaemon::start();
    let client = d.client().await;
    let raw = client
        .raw_call(
            "search.save",
            serde_json::json!({"name": "할 일", "query": {"text": "TODO", "scopes": ["code"],
                "projectIds": null, "regex": false, "caseSensitive": true, "limit": 50}}),
            None,
        )
        .await
        .expect("search.save");
    let saved: devbox_search::SavedRef = serde_json::from_str(&raw).expect("saved ref");
    drop(client);
    d.restart();
    let client = d.client().await;
    let list =
        support::bounded(client.call::<devbox_search::SearchSaved>(&devbox_search::NoParams {}))
            .await
            .expect("search.saved");
    assert_eq!(list.items.len(), 1);
    assert_eq!(
        (list.items[0].id.as_str(), list.items[0].name.as_str()),
        (saved.id.as_str(), "할 일")
    );
}

#[tokio::test]
async fn closing_the_stream_stops_the_code_walk() {
    let daemon = TestDaemon::fixture();
    let services = devbox_cli::daemon::build_services(daemon.paths.clone()).expect("services");
    let listener = tokio::net::UnixListener::bind(&daemon.socket).expect("socket");
    let stop = tokio_util::sync::CancellationToken::new();
    let task = tokio::spawn(devbox_cli::daemon::server::serve(
        listener,
        services.clone(),
        stop.clone(),
    ));
    let client = support::bounded(devbox_cli::client::Client::connect(
        &daemon.socket,
        devbox_protocol::message::ClientKind::Test,
        &daemon.instance,
    ))
    .await
    .expect("client");
    // Sorted walk: `a/` gives one full chunk, then `b/` has nothing to send, so only
    // the closed stream (not a failed write) can stop the walk early.
    let project = daemon.home.path().join("proj");
    const HAY: u64 = 20_000;
    for i in 0..60 {
        write(&project.join(format!("a/f{i:02}.txt")), "needle\n");
    }
    for i in 0..HAY {
        write(
            &project.join(format!("b/{:02}/f{i:05}.txt", i / 1000)),
            "hay\n",
        );
    }
    daemon
        .add_project(&client, project.to_str().expect("utf-8"))
        .await;
    let stream = support::bounded(client.open_stream(
        "search.query",
        serde_json::json!({"text": "needle", "scopes": ["code"], "projectIds": null,
            "regex": false, "caseSensitive": false, "limit": 500}),
    ))
    .await
    .expect("search.query opens");
    stream.credit(1 << 20).await.expect("credit");
    let line = stream.read_until(b"\n", Duration::from_secs(10)).await;
    let first: SearchChunk =
        serde_json::from_slice(&line[..line.iter().position(|b| *b == b'\n').expect("a line") + 1])
            .expect("first chunk");
    assert_eq!(first.items.len(), 50);
    stream.close().await.expect("close");
    let closed = Instant::now();
    let mut last = services.search.code_files_visited();
    loop {
        tokio::time::sleep(Duration::from_millis(200)).await;
        let now = services.search.code_files_visited();
        if now == last {
            break;
        }
        last = now;
        assert!(
            closed.elapsed() < Duration::from_secs(1),
            "the walk kept going: {now}"
        );
    }
    assert!(last < 60 + HAY, "the walk stopped early: {last}");
    stop.cancel();
    let _ = support::bounded(task).await;
}

#[tokio::test]
async fn daemon_git_location_variables_do_not_redirect_file_listing() {
    let mut d = TestDaemon::fixture();
    let proj = d.git_repo("proj");
    let other = d.git_repo("other");
    for (repo, name) in [(&proj, "own.txt"), (&other, "foreign.txt")] {
        std::fs::write(Path::new(repo).join(name), "x\n").expect("fixture file");
        d.git(Path::new(repo), &["add", name]);
        d.git(Path::new(repo), &["commit", "-qm", name]);
    }
    d.extra_env
        .push(("GIT_DIR".into(), format!("{other}/.git")));
    d.launch();
    let client = d.client().await;
    let id = d.add_project(&client, &proj).await;
    let chunks = query(
        &client,
        serde_json::json!({"text": "txt", "scopes": ["files"], "projectIds": [id],
            "regex": false, "caseSensitive": false, "limit": 10}),
    )
    .await;
    let mut paths: Vec<&str> = chunks
        .iter()
        .flat_map(|c| c.items.iter().map(|h| h.path.as_str()))
        .collect();
    paths.sort();
    assert_eq!(paths, ["initial.txt", "own.txt"]);
}

#[tokio::test]
async fn editor_document_save_updates_the_real_search_sink_and_fts() {
    use devbox_documents::{
        ClaimDocument, DocKey, DocKind, DocPolicy, DocumentsClaim, DocumentsSave, Expect,
        SaveDocument,
    };
    let d = TestDaemon::start();
    let client = d.client().await;
    watching(&d, &client).await;
    let path = "saved-index.md";
    write(
        &vault(&d).join(path),
        "---\ntitle: OldIndexedTitle8739\n---\noldneedle8739",
    );
    assert_eq!(wait_hits(&client, "oldneedle8739", 1).await.items.len(), 1);
    let claim = client
        .call::<DocumentsClaim>(&ClaimDocument {
            key: DocKey {
                root: "vault".into(),
                path: path.into(),
            },
            policy: DocPolicy {
                kind: DocKind::Note,
            },
            window_id: "owned-editor".into(),
        })
        .await
        .expect("claim saved note");
    let body = "---\ntitle: NewIndexedTitle8739\ntags: [newtag8739]\n---\nfreshneedle8739";
    client
        .call::<DocumentsSave>(&SaveDocument {
            lease_id: claim.lease_id.expect("owned lease"),
            seq: 1,
            base_rev: claim.rev,
            expect: Expect::Present,
            text: body.into(),
        })
        .await
        .expect("durable editor save ACK");
    let hits = wait_hits(&client, "freshneedle8739", 1).await;
    assert_eq!(
        hits.items.len(),
        1,
        "real composed SearchSink must feed FTS after a document save"
    );
    assert_eq!(
        (hits.items[0].path.as_str(), hits.items[0].title.as_str()),
        (path, "NewIndexedTitle8739")
    );
    assert!(hits.items[0].snippet.contains("freshneedle8739"));
    assert!(notes(&client, "oldneedle8739").await.items.is_empty());
    assert_eq!(notes(&client, "NewIndexedTitle8739").await.items.len(), 1);
}
