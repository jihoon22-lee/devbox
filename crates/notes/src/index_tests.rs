use crate::fixture::*;
use crate::*;
use devbox_core::HubMessage;
use std::time::Duration;

#[tokio::test]
async fn blank_title_falls_back_to_the_file_name() -> T {
    for text in [
        "---\ntitle: \"\"\n---\nbody\n",
        "---\ntitle: \"  \"\n---\nbody\n",
        "---\ntags: [a]\n---\nbody\n",
    ] {
        let n = fixture(&[("idea.md", text)]).await;
        assert_eq!(n.index_title("idea.md").await?, "idea");
        assert_eq!(n.resolve("idea").await?.paths, ["idea.md"]);
        assert_eq!(std::fs::read_to_string(n.vault().join("idea.md"))?, text);
    }
    Ok(())
}

#[tokio::test]
async fn wiki_links_resolve_with_alias_heading_and_nearest_folder() -> T {
    let n = fixture(&[
        ("a/x.md", "ax\n"),
        ("b/x.md", "bx\n"),
        ("a/sub/note.md", "[[x]] [[b/x|별칭]] [[x#요약]]\n"),
        ("root.md", "[[x]]\n"),
    ])
    .await;
    assert_eq!(
        n.resolve_from("x", Some("a/sub/note.md")).await?.paths,
        ["a/x.md"]
    );
    assert_eq!(
        n.resolve_from("b/x|별칭", Some("a/sub/note.md"))
            .await?
            .paths,
        ["b/x.md"]
    );
    assert_eq!(
        n.resolve_from("x#요약", Some("a/sub/note.md")).await?.paths,
        ["a/x.md"]
    );
    let parsed = links::parse_wikilinks("[[x#요약]]");
    assert_eq!(parsed[0].heading.as_deref(), Some("요약"));
    assert_eq!(parsed[0].target_key.as_deref(), Some("x"));
    let root = n.resolve_from("x", Some("root.md")).await?;
    assert!(root.ambiguous);
    assert_eq!(root.paths, ["a/x.md", "b/x.md"]);
    // Backlinks follow the same resolution: the root link is ambiguous, so it counts for neither.
    let back = |path: &str| NotePath { path: path.into() };
    let a = n
        .svc
        .backlinks(back("a/x.md"))
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        a.items.iter().map(|b| b.src.as_str()).collect::<Vec<_>>(),
        ["a/sub/note.md", "a/sub/note.md"]
    );
    assert_eq!((a.items[0].line, a.items[0].from_utf16), (1, 0));
    let b = n
        .svc
        .backlinks(back("b/x.md"))
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(b.items.len(), 1);
    let md = n
        .svc
        .resolve(ResolveLink {
            target: "../x.md".into(),
            kind: LinkKind::Md,
            from: Some("a/sub/note.md".into()),
        })
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(md.paths, ["a/x.md"]);
    Ok(())
}

#[tokio::test]
async fn backlinks_update_after_external_edit() -> T {
    let n = fixture(&[("a.md", "a\n"), ("b.md", "b\n")]).await;
    let mut changed = n.hub.subscribe("notes.changed");
    n.svc.start().await?;
    n.docs
        .start()
        .await
        .map_err(|e| format!("documents start: {e}"))?;
    let back = || NotePath {
        path: "a.md".into(),
    };
    assert!(
        n.svc
            .backlinks(back())
            .await
            .map_err(|e| format!("{e:?}"))?
            .items
            .is_empty()
    );
    n.write("b.md", "b [[a]]\n");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
    let found = loop {
        let items = n
            .svc
            .backlinks(back())
            .await
            .map_err(|e| format!("{e:?}"))?
            .items;
        if !items.is_empty() || tokio::time::Instant::now() > deadline {
            break items;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].src, "b.md");
    let mut events = 0;
    while let Ok(Some(HubMessage::Event { payload, .. })) =
        tokio::time::timeout(Duration::from_millis(500), changed.recv()).await
    {
        let payload: NotesChanged = serde_json::from_str(&payload)?;
        assert_eq!(payload.paths, ["b.md"]);
        events += 1;
    }
    assert_eq!(events, 1, "one notes.changed for one external edit");
    Ok(())
}

#[tokio::test]
async fn tree_pages_are_bounded_and_share_one_rev() -> T {
    let notes: Vec<(String, String)> = (0..600)
        .map(|i| (format!("n{i:03}.md"), format!("# {i}\n")))
        .collect();
    let refs: Vec<(&str, &str)> = notes
        .iter()
        .map(|(p, t)| (p.as_str(), t.as_str()))
        .collect();
    let n = fixture(&refs).await;
    let first = n
        .svc
        .tree(TreeParams { cursor: None })
        .await
        .map_err(|e| format!("{e:?}"))?;
    n.write("zz-new.md", "new\n");
    n.svc.rescan().await.map_err(|e| format!("{e:?}"))?;
    let (mut sizes, mut cursor, mut total) = (
        vec![first.items.len()],
        first.next_cursor.clone(),
        first.items.len(),
    );
    assert!(serde_json::to_string(&first)?.len() <= 768 * 1024);
    while let Some(c) = cursor {
        let page = n
            .svc
            .tree(TreeParams { cursor: Some(c) })
            .await
            .map_err(|e| format!("{e:?}"))?;
        assert_eq!(
            page.rev, first.rev,
            "the walk stays on the first page's rev"
        );
        assert!(serde_json::to_string(&page)?.len() <= 768 * 1024);
        sizes.push(page.items.len());
        total += page.items.len();
        assert!(!page.truncated);
        cursor = page.next_cursor;
    }
    assert_eq!(sizes, [256, 256, 88]);
    assert_eq!(total, 600);
    let again = n
        .svc
        .tree(TreeParams { cursor: None })
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_ne!(again.rev, first.rev);
    n.advance(61_000);
    let stale = n
        .svc
        .tree(TreeParams {
            cursor: again.next_cursor,
        })
        .await;
    assert_eq!(domain(stale), NotesError::StaleCursor);
    Ok(())
}

#[tokio::test]
async fn trash_and_temp_files_are_not_indexed_or_linked() -> T {
    let n = fixture(&[
        ("a.md", "---\ntags: [live]\n---\na\n"),
        (
            ".trash/2026-10-01/old.md",
            "---\ntags: [gone]\n---\n[[a]]\n",
        ),
        (".devbox-tmp-1", "[[a]]"),
        ("x/.devbox-tmp-2/y.md", "[[a]]"),
        (".git/z.md", "[[a]]"),
    ])
    .await;
    let tree = n
        .svc
        .tree(TreeParams { cursor: None })
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        tree.items
            .iter()
            .map(|i| i.path.as_str())
            .collect::<Vec<_>>(),
        ["a.md"]
    );
    let tags = n
        .svc
        .tags(NoParams {})
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        tags.items,
        [TagCount {
            tag: "live".into(),
            count: 1
        }]
    );
    let back = n
        .svc
        .backlinks(NotePath {
            path: "a.md".into(),
        })
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert!(back.items.is_empty());
    assert!(n.resolve("old").await?.paths.is_empty());
    assert!(n.resolve("y").await?.paths.is_empty());
    Ok(())
}

/// Nothing reached through a symlink (file or folder) is indexed, by the scan or by
/// a change event, so outside notes never show up anywhere.
async fn assert_only_a(n: &Nx) -> T {
    let tree = n
        .svc
        .tree(TreeParams { cursor: None })
        .await
        .map_err(|e| format!("{e:?}"))?;
    let paths: Vec<&str> = tree.items.iter().map(|i| i.path.as_str()).collect();
    assert_eq!(paths, ["a.md"]);
    let tags = n
        .svc
        .tags(NoParams {})
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert!(tags.items.is_empty(), "{:?}", tags.items);
    let back = n
        .svc
        .backlinks(NotePath {
            path: "a.md".into(),
        })
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert!(back.items.is_empty(), "{:?}", back.items);
    for target in ["ln", "inner", "lnd/inner", "leak"] {
        assert!(n.resolve(target).await?.paths.is_empty(), "{target}");
    }
    Ok(())
}

#[tokio::test]
async fn symlinked_files_and_folders_are_not_indexed() -> T {
    let n = fixture(&[("a.md", "a\n")]).await;
    let outside = n.home.join("outside");
    std::fs::create_dir_all(&outside)?;
    let leak = "---\ntitle: leak\ntags: [leak]\n---\n[[a]]\n";
    std::fs::write(outside.join("target.md"), leak)?;
    std::fs::write(outside.join("inner.md"), leak)?;
    std::os::unix::fs::symlink(outside.join("target.md"), n.vault().join("ln.md"))?;
    std::os::unix::fs::symlink(&outside, n.vault().join("lnd"))?;
    n.svc.rescan().await.map_err(|e| format!("{e:?}"))?;
    assert_only_a(&n).await?;
    n.svc
        .refresh(vec!["ln.md".into(), "lnd/inner.md".into()])
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_only_a(&n).await
}
