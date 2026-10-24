mod support;
use devbox_documents::{
    ClaimDocument, DocKey, DocKind, DocPolicy, DocumentsClaim, DocumentsRename, DocumentsSave,
    Expect, RenameDocument, SaveDocument,
};
use devbox_projects::{
    DeleteFile, FileKey, FileTreeQuery, FilesCreate, FilesDelete, FilesStat, FilesTree,
};
use support::{TestDaemon, bounded};

#[tokio::test]
async fn source_files_rpc_create_tree_save_rename_and_guarded_delete() {
    let d = TestDaemon::start();
    let client = d.client().await;
    let project = d.home.path().join("source");
    std::fs::create_dir_all(&project).expect("fixture project");
    let id = d
        .add_project(&client, project.to_str().expect("utf8"))
        .await;
    let key = DocKey {
        root: format!("project:{id}"),
        path: "가😀.ts".into(),
    };
    bounded(client.call::<FilesCreate>(&FileKey { key: key.clone() }))
        .await
        .expect("create RPC");
    let tree = bounded(client.call::<FilesTree>(&FileTreeQuery {
        project_id: id,
        dir: "".into(),
        show_hidden: false,
        cursor: None,
    }))
    .await
    .expect("tree RPC");
    assert_eq!(tree.entries[0].key, key);
    let doc = bounded(client.call::<DocumentsClaim>(&ClaimDocument {
        key: key.clone(),
        policy: DocPolicy {
            kind: DocKind::Code,
        },
        window_id: "source".into(),
    }))
    .await
    .expect("claim code");
    let lease = doc.lease_id.expect("lease");
    bounded(client.call::<DocumentsSave>(&SaveDocument {
        lease_id: lease.clone(),
        base_rev: doc.rev,
        seq: 1,
        expect: Expect::Present,
        text: "const n = 1;\n".into(),
    }))
    .await
    .expect("manual save");
    let to = DocKey {
        root: key.root.clone(),
        path: "renamed.ts".into(),
    };
    bounded(client.call::<DocumentsRename>(&RenameDocument {
        key,
        to: to.clone(),
        lease_id: Some(lease),
    }))
    .await
    .expect("rename");
    let stat = bounded(client.call::<FilesStat>(&FileKey { key: to.clone() }))
        .await
        .expect("stat RPC");
    std::fs::write(project.join("renamed.ts"), "outside").expect("external edit");
    let error = bounded(client.call::<FilesDelete>(&DeleteFile {
        key: to.clone(),
        expect: stat.rev,
    }))
    .await
    .expect_err("stale delete");
    assert!(
        matches!(error,devbox_cli::client::ClientError::Rpc{code,..} if code=="files.state_changed")
    );
    assert_eq!(
        std::fs::read_to_string(project.join("renamed.ts")).expect("preserved"),
        "outside"
    );
    let stat = bounded(client.call::<FilesStat>(&FileKey { key: to.clone() }))
        .await
        .expect("new stat");
    bounded(client.call::<FilesDelete>(&DeleteFile {
        key: to,
        expect: stat.rev,
    }))
    .await
    .expect("confirmed delete");
    assert!(!project.join("renamed.ts").exists());
}

#[tokio::test]
async fn code_policy_preserves_bom_crlf_mode_and_rekeys_dirty_journals() {
    use devbox_documents::{
        DocumentsJournal, DocumentsJournalRead, DocumentsRead, DocumentsRelease, LeaseRef,
        ReadDocument, WriteJournal,
    };
    use std::os::unix::fs::PermissionsExt;
    let d = TestDaemon::start();
    let client = d.client().await;
    let project = d.home.path().join("formats");
    std::fs::create_dir_all(&project).expect("owned project");
    let file = project.join("a.ts");
    std::fs::write(&file, b"\xef\xbb\xbfconst a = 1;\r\n").expect("fixture BOM");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o750)).expect("fixture mode");
    let id = d
        .add_project(&client, project.to_str().expect("utf8"))
        .await;
    let key = DocKey {
        root: format!("project:{id}"),
        path: "a.ts".into(),
    };
    let claimed = bounded(client.call::<DocumentsClaim>(&ClaimDocument {
        key: key.clone(),
        policy: DocPolicy {
            kind: DocKind::Code,
        },
        window_id: "code".into(),
    }))
    .await
    .expect("claim");
    assert_eq!(claimed.text.as_deref(), Some("const a = 1;\n"));
    let lease = claimed.lease_id.expect("lease");
    bounded(client.call::<DocumentsJournal>(&WriteJournal {
        lease_id: lease.clone(),
        seq: 1,
        base_rev: claimed.rev.clone(),
        text: "const a = 2;\n".into(),
    }))
    .await
    .expect("dirty journal");
    let to = DocKey {
        root: key.root.clone(),
        path: "nested/moved.ts".into(),
    };
    bounded(client.call::<DocumentsRename>(&RenameDocument {
        key: key.clone(),
        to: to.clone(),
        lease_id: Some(lease.clone()),
    }))
    .await
    .expect("dirty rename");
    let journal = bounded(client.call::<DocumentsJournalRead>(&LeaseRef {
        lease_id: lease.clone(),
    }))
    .await
    .expect("moved journal");
    assert_eq!(journal.text, "const a = 2;\n");
    bounded(client.call::<DocumentsSave>(&SaveDocument {
        lease_id: lease.clone(),
        seq: 1,
        base_rev: claimed.rev,
        expect: Expect::Present,
        text: journal.text,
    }))
    .await
    .expect("save moved buffer");
    bounded(client.call::<DocumentsRelease>(&LeaseRef { lease_id: lease }))
        .await
        .expect("release");
    let reopened = bounded(client.call::<DocumentsClaim>(&ClaimDocument {
        key: to.clone(),
        policy: DocPolicy {
            kind: DocKind::Code,
        },
        window_id: "again".into(),
    }))
    .await
    .expect("reopen");
    assert!(reopened.journal.is_none());
    let old = bounded(client.call::<DocumentsRead>(&ReadDocument {
        key,
        policy: Some(DocPolicy {
            kind: DocKind::Code,
        }),
    }))
    .await
    .expect("old path read");
    assert!(old.rev.is_none());
    assert_eq!(
        std::fs::read(project.join("nested/moved.ts")).expect("bytes"),
        b"\xef\xbb\xbfconst a = 2;\r\n"
    );
    assert_eq!(
        std::fs::metadata(project.join("nested/moved.ts"))
            .expect("mode")
            .permissions()
            .mode()
            & 0o777,
        0o750
    );
    assert_eq!(
        std::fs::read_dir(d.paths.data_dir.join("journals"))
            .expect("journals")
            .count(),
        0
    );
}

#[tokio::test]
async fn external_editor_rpc_uses_owned_launcher_literal_arguments_and_reports_fallback() {
    use devbox_projects::{FilesExternalEditor, OpenSourceEditor, SourceEditor};
    use std::os::unix::fs::PermissionsExt;
    for exit in [0, 1] {
        let mut d = TestDaemon::fixture();
        let bin = d.home.path().join("editor-bin");
        std::fs::create_dir(&bin).expect("owned bin");
        let launcher = bin.join("code");
        std::fs::write(
            &launcher,
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$EDITOR_LOG\"\nexit \"$EDITOR_EXIT\"\n",
        )
        .expect("fake launcher");
        std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o700))
            .expect("owned exec");
        let log = d.home.path().join("editor-argv");
        d.extra_env.extend([
            ("PATH".into(), format!("{}:/usr/bin:/bin", bin.display())),
            ("WSL_DISTRO_NAME".into(), "OwnedDistro".into()),
            ("EDITOR_LOG".into(), log.display().to_string()),
            ("EDITOR_EXIT".into(), exit.to_string()),
        ]);
        d.launch();
        let client = d.client().await;
        let project = d.home.path().join("editor-project");
        std::fs::create_dir(&project).expect("project");
        let path = "가 $(false); & name.ts";
        std::fs::write(project.join(path), b"owned").expect("file");
        let id = d
            .add_project(&client, project.to_str().expect("utf8"))
            .await;
        let result = bounded(client.call::<FilesExternalEditor>(&OpenSourceEditor {
            key: DocKey {
                root: format!("project:{id}"),
                path: path.into(),
            },
            editor: SourceEditor::Code,
        }))
        .await
        .expect("open via fake");
        assert_eq!(result.opened, exit == 0);
        assert_eq!(result.path, project.join(path).display().to_string());
        assert_eq!(
            std::fs::read_to_string(log).expect("argv"),
            format!("--remote\nwsl+OwnedDistro\n{}\n", result.path)
        );
    }
}
