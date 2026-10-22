use crate::fixture::*;
use crate::*;
use data_encoding::BASE64;
use std::os::unix::fs::MetadataExt;

fn png(width: u32, height: u32, size: usize) -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    bytes.extend(width.to_be_bytes());
    bytes.extend(height.to_be_bytes());
    bytes.extend([8, 6, 0, 0, 0]);
    bytes.resize(size.max(bytes.len()), 0);
    bytes
}
fn files_below(dir: &std::path::Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                stack.push(e.path());
            } else {
                out.push(
                    e.path()
                        .strip_prefix(dir)
                        .expect("below")
                        .display()
                        .to_string(),
                );
            }
        }
    }
    out.sort();
    out
}

#[tokio::test]
async fn attachments_are_content_addressed_and_stay_inside_the_vault() -> T {
    let n = fixture(&[("d/note.md", "n\n")]).await;
    let outside_before = files_below(&n.home);
    let attach = |name: &str| AttachImage {
        note_path: "d/note.md".into(),
        name: name.into(),
        data: BASE64.encode(&png(2, 2, 64)),
    };
    let one = n
        .svc
        .attach(attach("../../x.png"))
        .await
        .map_err(|e| format!("{e:?}"))?;
    let two = n
        .svc
        .attach(attach("../../x.png"))
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(one, two, "the same image gives the same attachment");
    let hash = attachments::content_hash_hex(&png(2, 2, 64));
    assert_eq!(one.path, format!("attachments/{}-x.png", &hash[..12]));
    assert_eq!(one.markdown, format!("![image](../{})", one.path));
    assert_eq!(std::fs::read(n.vault().join(&one.path))?, png(2, 2, 64));
    let mut after = files_below(&n.home);
    after.retain(|p| !outside_before.contains(p));
    assert_eq!(
        after,
        [format!("notes/{}", one.path)],
        "nothing else was created"
    );
    let bad = AttachImage {
        data: BASE64.encode(b"<svg/>"),
        ..attach("x.svg")
    };
    assert!(matches!(
        domain(n.svc.attach(bad).await),
        NotesError::InvalidImage {
            problem: ImageProblem::Unsupported
        }
    ));
    let escape = AttachImage {
        note_path: "../out.md".into(),
        ..attach("x.png")
    };
    assert_eq!(domain(n.svc.attach(escape).await), NotesError::InvalidInput);
    Ok(())
}

#[tokio::test]
async fn delete_moves_to_trash_and_undo_restores() -> T {
    let n = fixture(&[("a/n.md", "keep me\n"), ("b.md", "[[n]]\n")]).await;
    let trashed = n
        .svc
        .delete(NotePath {
            path: "a/n.md".into(),
        })
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(trashed.trash_path, ".trash/2026-10-22/a/n.md");
    assert!(!n.vault().join("a/n.md").exists());
    assert_eq!(n.read(".trash/2026-10-22/a/n.md"), "keep me\n");
    assert!(
        n.resolve("n").await?.paths.is_empty(),
        "trashed notes leave the index"
    );
    n.write("a/n.md", "second\n");
    let second = n
        .svc
        .delete(NotePath {
            path: "a/n.md".into(),
        })
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(second.trash_path, ".trash/2026-10-22/a/n 2.md");
    n.svc
        .restore(RestoreNote {
            trash_path: trashed.trash_path.clone(),
            path: "a/n.md".into(),
        })
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(n.read("a/n.md"), "keep me\n");
    assert_eq!(n.resolve("n").await?.paths, ["a/n.md"]);
    let twice = n
        .svc
        .restore(RestoreNote {
            trash_path: second.trash_path,
            path: "a/n.md".into(),
        })
        .await;
    assert_eq!(domain(twice), NotesError::Exists, "restore never replaces");
    Ok(())
}

#[tokio::test]
async fn first_vault_creation_adds_trash_to_gitignore() -> T {
    let mut n = Nx::empty();
    assert!(!n.vault().exists());
    n.svc.start().await?;
    assert_eq!(std::fs::read(n.vault().join(".gitignore"))?, b".trash/\n");
    n.restart();
    n.svc.start().await?;
    assert_eq!(std::fs::read(n.vault().join(".gitignore"))?, b".trash/\n");
    Ok(())
}

#[tokio::test]
async fn existing_vault_gitignore_is_never_modified() -> T {
    for existing in [Some("node_modules/\n"), None] {
        let n = Nx::empty();
        std::fs::create_dir_all(n.vault())?;
        n.write("a.md", "a\n");
        let gitignore = n.vault().join(".gitignore");
        if let Some(text) = existing {
            std::fs::write(&gitignore, text)?;
        }
        let before = std::fs::metadata(&gitignore)
            .ok()
            .map(|m| (m.mtime(), m.mtime_nsec()));
        n.svc.start().await?;
        n.svc
            .delete(NotePath {
                path: "a.md".into(),
            })
            .await
            .map_err(|e| format!("{e:?}"))?;
        assert!(n.vault().join(".trash").is_dir());
        match existing {
            Some(text) => {
                assert_eq!(std::fs::read_to_string(&gitignore)?, text);
                let after = std::fs::metadata(&gitignore).map(|m| (m.mtime(), m.mtime_nsec()))?;
                assert_eq!(Some(after), before);
            }
            None => assert!(!gitignore.exists()),
        }
    }
    Ok(())
}

#[tokio::test]
async fn create_names_from_the_title_renders_templates_and_never_replaces() -> T {
    let n = fixture(&[(
        "templates/daily.md",
        "# {{title}}\n{{date}} {{time}} {{pc}} {{vault-relative-path}}\n",
    )])
    .await;
    let create = |template: Option<&str>| CreateNote {
        folder: "work".into(),
        title: "  Plan: a/b?  ".into(),
        template: template.map(Into::into),
    };
    let one = n
        .svc
        .create(create(Some("daily")))
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(one.path, "work/Plan ab.md");
    assert_eq!(
        n.read("work/Plan ab.md"),
        "# Plan: a/b?\n2026-10-22 09:30 desk work/Plan ab.md\n"
    );
    let two = n
        .svc
        .create(create(None))
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(two.path, "work/Plan ab 2.md");
    assert_eq!(
        n.read("work/Plan ab.md").lines().count(),
        2,
        "first note untouched"
    );
    let templates = n
        .svc
        .templates(NoParams {})
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(templates.items[0].name, "daily");
    let missing = n.svc.create(create(Some("nope"))).await;
    assert!(matches!(
        domain(missing),
        NotesError::InvalidTemplate {
            problem: TemplateProblem::NotFound
        }
    ));
    let blank = CreateNote {
        folder: "".into(),
        title: "???".into(),
        template: None,
    };
    assert_eq!(domain(n.svc.create(blank).await), NotesError::InvalidName);
    Ok(())
}

#[tokio::test]
async fn restore_takes_only_a_trashed_note_file() -> T {
    let n = fixture(&[]).await;
    n.write(".trash/2026-10-22/a.md", "a\n");
    n.write(".trash/2026-10-22/b.txt", "b\n");
    std::fs::create_dir_all(n.vault().join(".trash/2026-10-22/dir.md"))?;
    for trash in [
        ".trash/2026-10-22",
        ".trash/2026-10-22/b.txt",
        ".trash/2026-10-22/dir.md",
    ] {
        let r = n
            .svc
            .restore(RestoreNote {
                trash_path: trash.into(),
                path: "back.md".into(),
            })
            .await;
        assert_eq!(domain(r), NotesError::InvalidInput, "{trash}");
    }
    assert!(!n.vault().join("back.md").exists());
    n.svc
        .restore(RestoreNote {
            trash_path: ".trash/2026-10-22/a.md".into(),
            path: "back.md".into(),
        })
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(n.read("back.md"), "a\n");
    Ok(())
}
