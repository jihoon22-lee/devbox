use crate::fixture::*;
use crate::*;
use devbox_protocol::Fail;

fn refused<T: std::fmt::Debug>(result: crate::R<T>) {
    assert!(
        matches!(result, Err(Fail::Domain(NotesError::OutOfVault))),
        "{result:?}"
    );
}

#[tokio::test]
async fn notes_io_cannot_follow_final_or_parent_links_to_registered_projects() -> T {
    for parent in [false, true] {
        let n = fixture(&[("a.md", "owned note")]).await;
        let project = n.home.join("project");
        std::fs::write(project.join("private.md"), "private registered project")?;
        let path = if parent {
            std::os::unix::fs::symlink(&project, n.vault().join("escape"))?;
            "escape/private.md"
        } else {
            std::os::unix::fs::symlink(project.join("private.md"), n.vault().join("escape.md"))?;
            "escape.md"
        };
        refused(n.svc.read(NotePath { path: path.into() }).await);
        refused(
            n.svc
                .render(RenderNote {
                    path: Some(path.into()),
                    text: None,
                })
                .await,
        );
        // The ordinary document API intentionally still permits registered cross-root links.
        let doc = n
            .docs
            .read_disk(
                &devbox_documents::DocKey {
                    root: "vault".into(),
                    path: path.into(),
                },
                devbox_documents::DocKind::Note,
            )
            .await
            .expect("ordinary documents cross-root read");
        assert_eq!(doc.text.as_deref(), Some("private registered project"));
        std::os::unix::fs::symlink(project.join("private.md"), n.vault().join("inbox.md"))?;
        refused(
            n.svc
                .append(AppendNote {
                    target: AppendTarget::Inbox,
                    text: "must stay in vault".into(),
                    format: AppendFormat::Plain,
                })
                .await,
        );
        assert_eq!(
            std::fs::read_to_string(project.join("private.md"))?,
            "private registered project"
        );
    }
    Ok(())
}

#[tokio::test]
async fn notes_creation_templates_daily_and_assets_reject_registered_parent_links() -> T {
    let n = fixture(&[("a.md", "owned note")]).await;
    let project = n.home.join("project");
    std::fs::write(project.join("private.md"), "private registered project")?;
    std::os::unix::fs::symlink(&project, n.vault().join("escape"))?;
    refused(
        n.svc
            .create(CreateNote {
                folder: "escape/new".into(),
                title: "new".into(),
                template: None,
            })
            .await,
    );
    assert!(
        !project.join("new").exists(),
        "even parent creation must stay in vault"
    );
    let templates = n.svc.s.settings.templates_folder.clone();
    std::fs::remove_dir_all(n.vault().join(&templates)).ok();
    std::os::unix::fs::symlink(&project, n.vault().join(&templates))?;
    assert!(
        n.svc
            .create(CreateNote {
                folder: "".into(),
                title: "new".into(),
                template: Some("private".into())
            })
            .await
            .is_err()
    );
    let daily = n.svc.s.settings.daily_folder.clone();
    std::fs::remove_dir_all(n.vault().join(&daily)).ok();
    std::os::unix::fs::symlink(&project, n.vault().join(&daily))?;
    refused(
        n.svc
            .daily(DailyParams {
                date: Some("2026-10-24".into()),
            })
            .await,
    );
    let attachments = "attachments";
    std::fs::remove_dir_all(n.vault().join(attachments)).ok();
    std::os::unix::fs::symlink(&project, n.vault().join(attachments))?;
    let mut image = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    image.extend(1_u32.to_be_bytes());
    image.extend(1_u32.to_be_bytes());
    image.extend([8, 6, 0, 0, 0]);
    image.resize(40, 0);
    refused(
        n.svc
            .attach(AttachImage {
                note_path: "a.md".into(),
                name: "image.png".into(),
                data: data_encoding::BASE64.encode(&image),
            })
            .await,
    );
    assert!(!project.join("assets").exists());
    Ok(())
}

#[tokio::test]
async fn append_rechecks_vault_descriptor_scope_after_the_read() -> T {
    let n = fixture(&[("inbox.md", "owned inbox")]).await;
    let outside = n.home.join("project/private.md");
    std::fs::write(&outside, "private registered project")?;
    let inbox = n.vault().join("inbox.md");
    let target = outside.clone();
    *n.svc.s.after_read.lock().expect("hook") = Some(std::sync::Arc::new(move || {
        std::fs::remove_file(&inbox).expect("replace fixture");
        std::os::unix::fs::symlink(&target, &inbox).expect("fixture link");
    }));
    refused(
        n.svc
            .append(AppendNote {
                target: AppendTarget::Inbox,
                text: "must stay in vault".into(),
                format: AppendFormat::Plain,
            })
            .await,
    );
    assert_eq!(
        std::fs::read_to_string(outside)?,
        "private registered project"
    );
    Ok(())
}
