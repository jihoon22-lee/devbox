use crate::fixture::*;
use crate::*;
use devbox_protocol::router::BoxFuture;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq, Eq)]
enum Seen {
    Indexed(String, String, String),
    Removed(Vec<String>),
    Retain(Vec<String>),
}
#[derive(Default)]
struct Recorder(Mutex<Vec<Seen>>);
impl NoteSink for Recorder {
    fn indexed(&self, notes: Vec<SinkNote>) -> BoxFuture<()> {
        let mut seen = self.0.lock().expect("recorder");
        seen.extend(
            notes
                .into_iter()
                .map(|n| Seen::Indexed(n.path, n.title, n.body)),
        );
        Box::pin(async {})
    }
    fn removed(&self, paths: Vec<String>) -> BoxFuture<()> {
        self.0.lock().expect("recorder").push(Seen::Removed(paths));
        Box::pin(async {})
    }
    fn retain(&self, paths: Vec<String>) -> BoxFuture<()> {
        self.0.lock().expect("recorder").push(Seen::Retain(paths));
        Box::pin(async {})
    }
}
impl Recorder {
    fn take(&self) -> Vec<Seen> {
        std::mem::take(&mut *self.0.lock().expect("recorder"))
    }
}
fn indexed(path: &str, title: &str, body: &str) -> Seen {
    Seen::Indexed(path.into(), title.into(), body.into())
}

#[tokio::test]
async fn changed_and_removed_notes_reach_the_sink_once() -> T {
    let mut n = fixture(&[]).await;
    let sink = Arc::new(Recorder::default());
    n.restart_with(sink.clone(), false);
    n.write("a.md", "---\ntitle: \"\"\n---\nbody");
    n.write(".trash/x.md", "hidden");
    n.svc.rescan().await.map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        sink.take(),
        [indexed("a.md", "a", "---\ntitle: \"\"\n---\nbody")]
    );
    n.svc.rescan().await.map_err(|e| format!("{e:?}"))?;
    assert_eq!(sink.take(), [], "unchanged notes are not sent again");
    std::fs::remove_file(n.vault().join("a.md"))?;
    n.svc.rescan().await.map_err(|e| format!("{e:?}"))?;
    assert_eq!(sink.take(), [Seen::Removed(vec!["a.md".into()])]);
    Ok(())
}

#[tokio::test]
async fn replay_feeds_every_note_after_the_first_scan() -> T {
    let mut n = fixture(&[
        ("a.md", "alpha"),
        ("f/b.md", "beta"),
        (".trash/c.md", "gone"),
    ])
    .await;
    let sink = Arc::new(Recorder::default());
    n.restart_with(sink.clone(), false);
    n.svc.start().await?;
    assert_eq!(sink.take(), [], "control: an unchanged vault sends nothing");
    n.restart_with(sink.clone(), true);
    n.svc.start().await?;
    let seen = sink.take();
    let (mut fed, last) = (
        seen[..seen.len().saturating_sub(1)].to_vec(),
        seen.last().cloned(),
    );
    fed.sort_by_key(|s| format!("{s:?}"));
    assert_eq!(
        fed,
        [
            indexed("a.md", "a", "alpha"),
            indexed("f/b.md", "b", "beta")
        ]
    );
    assert_eq!(
        last,
        Some(Seen::Retain(vec!["a.md".into(), "f/b.md".into()])),
        "the feed ends with the full list"
    );
    Ok(())
}

#[tokio::test]
async fn a_bom_note_is_indexed_without_the_bom() -> T {
    let mut n = fixture(&[("b.md", "target")]).await;
    let sink = Arc::new(Recorder::default());
    n.restart_with(sink.clone(), false);
    let text = "---\ntitle: Alpha\n---\nsee [[b]]";
    n.write("a.md", format!("\u{feff}{text}"));
    n.svc.rescan().await.map_err(|e| format!("{e:?}"))?;
    assert_eq!(n.index_title("a.md").await?, "Alpha");
    assert_eq!(sink.take(), [indexed("a.md", "Alpha", text)]);
    let back = n
        .svc
        .backlinks(NotePath {
            path: "b.md".into(),
        })
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(back.items.len(), 1);
    let at = "---\ntitle: Alpha\n---\nsee ".encode_utf16().count() as u32;
    assert_eq!(back.items[0].from_utf16, at, "editor offsets have no BOM");
    Ok(())
}

#[tokio::test]
async fn bom_notes_indexed_by_schema_2_are_reindexed_and_other_domains_kept() -> T {
    let mut n = Nx::empty();
    std::fs::create_dir_all(n.vault())?;
    n.write("a.md", "\u{feff}---\ntitle: Alpha\n---\nbody");
    // The index as schema 2 left it: the BOM note under its file name title.
    let index = devbox_core::IndexDb::open(&n.data.join("index.db"))?;
    index.ensure_domain("notes", 2, NOTES_SCHEMA)?;
    index.ensure_domain("api", 1, "CREATE TABLE api_y(v TEXT);")?;
    index.ensure_domain("search", 1, "CREATE TABLE search_x(v TEXT);")?;
    let (hash, _) = crate::vault::read_note(&n.vault(), "a.md").ok_or("fixture note")?;
    index
        .write(move |tx| {
            tx.execute(
                "INSERT INTO notes_files(path, title, hash) VALUES ('a.md', 'a', ?1)",
                [hash],
            )?;
            tx.execute("INSERT INTO api_y VALUES ('kept')", [])?;
            tx.execute("INSERT INTO search_x VALUES ('kept')", [])
        })
        .await?;
    n.restart();
    n.svc.rescan().await.map_err(|e| format!("{e:?}"))?;
    assert_eq!(n.index_title("a.md").await?, "Alpha");
    let count = |table: &'static str| {
        let index = index.clone();
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
    assert_eq!((count("api_y").await?, count("search_x").await?), (1, 1));
    Ok(())
}

async fn editor_save(n: &Nx, path: &str, seq: u64, text: &str) -> T {
    let c = n.open_note(path, "owned-editor", None).await;
    n.docs
        .save(devbox_documents::SaveDocument {
            lease_id: c.lease_id.expect("editor lease"),
            seq,
            base_rev: c.rev,
            expect: devbox_documents::Expect::Present,
            text: text.into(),
        })
        .await
        .map_err(|e| format!("{e:?}"))?;
    Ok(())
}
async fn wait_title(n: &Nx, path: &str, title: &str) -> T {
    tokio::time::timeout(std::time::Duration::from_secs(4), async {
        loop {
            if n.index_title(path).await.ok().as_deref() == Some(title) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await?;
    Ok(())
}

#[tokio::test]
async fn editor_save_indexes_title_tags_backlinks_and_sink_without_external_echo() -> T {
    use std::time::Duration;
    let mut n = fixture(&[
        (
            "source.md",
            "---\ntitle: Old\ntags: [oldtag]\n---\nold body",
        ),
        ("target.md", "target body"),
        ("control.md", "control"),
    ])
    .await;
    let sink = Arc::new(Recorder::default());
    n.restart_with(sink.clone(), false);
    n.svc.start().await?;
    n.docs.start().await?;
    n.write("control.md", "---\ntitle: ExternalControl\n---\ncontrol");
    wait_title(&n, "control.md", "ExternalControl").await?;
    tokio::time::sleep(Duration::from_millis(400)).await;
    sink.take();
    let mut external = n.hub.subscribe("documents.changed");
    let saved = "---\ntitle: NewSavedTitle\ntags: [newtag]\n---\nnew saved body [[target]]";
    editor_save(&n, "source.md", 1, saved).await?;
    assert_eq!(n.read("source.md"), saved);
    wait_title(&n, "source.md", "NewSavedTitle").await?;
    assert!(
        sink.take()
            .contains(&indexed("source.md", "NewSavedTitle", saved))
    );
    let tags = n
        .svc
        .tags(NoParams {})
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert!(tags.items.iter().any(|t| t.tag == "newtag"));
    assert!(!tags.items.iter().any(|t| t.tag == "oldtag"));
    let back = n
        .svc
        .backlinks(NotePath {
            path: "target.md".into(),
        })
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(back.items.len(), 1);
    assert!(
        tokio::time::timeout(Duration::from_millis(900), external.recv())
            .await
            .is_err(),
        "a save must not look like an external edit to its renderer"
    );
    Ok(())
}

#[tokio::test]
async fn acknowledged_editor_save_is_indexed_before_immediate_rename_planning() -> T {
    // No background consumer is started: rename must establish its own ordering barrier.
    let n = fixture(&[("source.md", "no link yet"), ("target.md", "target")]).await;
    editor_save(&n, "source.md", 1, "a newly saved [[target]]").await?;
    let renamed = n.renamed("target.md", "renamed.md").await?;
    assert_eq!(renamed.state, RenameState::Done);
    assert!(renamed.failed.is_empty());
    assert_eq!(n.read("source.md"), "a newly saved [[renamed]]");
    Ok(())
}

struct PausedSink {
    seen: Arc<Recorder>,
    once: std::sync::atomic::AtomicBool,
    reached: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
}
impl PausedSink {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            seen: Arc::default(),
            once: std::sync::atomic::AtomicBool::new(true),
            reached: Arc::default(),
            release: Arc::default(),
        })
    }
}
impl NoteSink for PausedSink {
    fn indexed(&self, notes: Vec<SinkNote>) -> BoxFuture<()> {
        let pause = self.once.swap(false, std::sync::atomic::Ordering::SeqCst);
        let (seen, reached, release) = (
            self.seen.clone(),
            self.reached.clone(),
            self.release.clone(),
        );
        Box::pin(async move {
            if pause {
                reached.notify_one();
                release.notified().await;
            }
            seen.indexed(notes).await;
        })
    }
    fn removed(&self, paths: Vec<String>) -> BoxFuture<()> {
        self.seen.removed(paths)
    }
}

#[tokio::test]
async fn a_save_during_startup_scan_remains_pending_after_its_snapshot() -> T {
    use std::time::Duration;
    let mut n = fixture(&[]).await;
    n.write("source.md", "---\ntitle: BeforeScan\n---\nold");
    let sink = PausedSink::new();
    n.restart_with(sink.clone(), false);
    let startup = n.svc.start();
    tokio::time::timeout(Duration::from_secs(4), sink.reached.notified()).await?;
    // Scan has already read old bytes, but has not written them into the index.
    tokio::time::timeout(
        Duration::from_secs(4),
        editor_save(&n, "source.md", 1, "---\ntitle: DuringScan\n---\nnew"),
    )
    .await??;
    sink.release.notify_one();
    startup.await?;
    wait_title(&n, "source.md", "DuringScan").await?;
    assert_eq!(
        n.svc.s.committed.load(std::sync::atomic::Ordering::SeqCst),
        n.docs.commit_revision()
    );
    Ok(())
}

#[tokio::test]
async fn bounded_commit_history_lag_rescans_and_keeps_saves_during_that_scan_pending() -> T {
    use std::time::Duration;
    let mut n = fixture(&[
        ("evicted.md", "---\ntitle: Old\n---\nold"),
        ("churn.md", "churn"),
    ])
    .await;
    let sink = PausedSink::new();
    n.restart_with(sink.clone(), false);
    n.svc.start().await?; // All hashes unchanged: sink is not called yet.
    let held = n.svc.s.update.lock().await;
    tokio::time::timeout(Duration::from_secs(10), async {
        editor_save(&n, "evicted.md", 1, "---\ntitle: EvictedSave\n---\nnew").await?;
        for seq in 1..=devbox_documents::COMMIT_HISTORY as u64 + 1 {
            editor_save(&n, "churn.md", seq, &format!("churn {seq}")).await?;
        }
        Ok::<_, Box<dyn std::error::Error>>(())
    })
    .await??;
    assert!(
        n.docs.committed_since(0).paths.is_none(),
        "the first saved path is no longer in bounded history"
    );
    drop(held);
    tokio::time::timeout(Duration::from_secs(4), sink.reached.notified()).await?;
    let snapshot = n.docs.commit_revision();
    // The lag rescan read EvictedSave already; this later commit must not be marked applied by it.
    tokio::time::timeout(
        Duration::from_secs(4),
        editor_save(
            &n,
            "evicted.md",
            2,
            "---\ntitle: AfterLagSnapshot\n---\nlatest",
        ),
    )
    .await??;
    assert!(n.docs.commit_revision() > snapshot);
    sink.release.notify_one();
    wait_title(&n, "evicted.md", "AfterLagSnapshot").await?;
    let seen = sink.seen.take();
    assert!(
        seen.contains(&indexed(
            "evicted.md",
            "EvictedSave",
            "---\ntitle: EvictedSave\n---\nnew"
        )),
        "rescan covered the evicted path"
    );
    assert!(seen.contains(&indexed(
        "evicted.md",
        "AfterLagSnapshot",
        "---\ntitle: AfterLagSnapshot\n---\nlatest"
    )));
    Ok(())
}
