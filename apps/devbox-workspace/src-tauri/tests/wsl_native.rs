#![cfg(windows)]
use devbox_workspace_lib::platform::wsl_distro;
use devbox_workspace_lib::platform::wsl_helper::Connection;
use std::{path::Path, time::Duration};

#[test]
#[ignore = "requires the exclusively owned GitHub Windows WSL fixture and packaged helper"]
fn actual_packaged_helper_observes_owned_wsl_and_requires_explicit_start() {
    assert_eq!(std::env::var("GITHUB_ACTIONS").unwrap(), "true");
    assert_eq!(
        std::env::var("RUNNER_ENVIRONMENT").unwrap(),
        "github-hosted"
    );
    let name = std::env::var("DEVBOX_KNOWLEDGE_WSL_DISTRO").unwrap();
    let run = std::env::var("GITHUB_RUN_ID").unwrap();
    assert!(name.starts_with(&format!("DevboxKnowledgeFixture-{run}-")));
    let distro = wsl_distro::list()
        .unwrap()
        .into_iter()
        .find(|d| d.name == name)
        .unwrap();
    let resources = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/wsl");
    let nonce = uuid::Uuid::new_v4();
    let root = format!("/home/devbox-fixture/workspace 한글 {nonce}");
    let unc = format!(
        r"\\wsl.localhost\{}\home\devbox-fixture\workspace 한글 {}",
        name, nonce
    );
    std::fs::create_dir(&unc).unwrap();
    let marker = Path::new(&unc).join("original.txt");
    std::fs::write(&marker, b"unchanged synthetic fixture\n").unwrap();
    let editable = Path::new(&unc).join("edit.txt");
    std::fs::write(&editable, b"original\r\n").unwrap();
    let lease =
        wsl_distro::Lease::capture(&distro.id, false).expect("owned WSL registry/storage capture");
    lease
        .helper_args(&resources, &uuid::Uuid::new_v4().to_string(), false)
        .expect("owned WSL registry remains stable before launch");
    drop(lease);
    let mut first =
        Connection::connect(&resources, &distro.id, false).expect("owned WSL helper launch/hello");
    let report = first.observe(&root).unwrap();
    assert_eq!(report.root, root);
    first.validate(&report.token).unwrap();
    assert!(first.validate(&uuid::Uuid::new_v4().to_string()).is_err());
    let scope = first.lease().scope(&report.root_object.scope);
    let registry_directory = tempfile::tempdir().unwrap();
    let owner =
        devbox_workspace_lib::project_owner::ProjectOwner::open(registry_directory.path()).unwrap();
    let empty = owner.snapshot().unwrap();
    let cancelled = owner
        .preview_wsl(&resources, &distro.id, &root, false)
        .unwrap();
    assert_eq!(owner.snapshot().unwrap(), empty);
    owner.cancel(&cancelled.preview_id).unwrap();
    assert!(owner
        .apply(
            &cancelled.preview_id,
            "cancelled",
            devbox_workspace_lib::project_owner::RegistrationAction::Register
        )
        .is_err());
    let reviewed = owner
        .preview_wsl(&resources, &distro.id, &root, false)
        .unwrap();
    let binding = reviewed.binding.clone();
    let (registered, context) = owner
        .apply(
            &reviewed.preview_id,
            "WSL fixture",
            devbox_workspace_lib::project_owner::RegistrationAction::Register,
        )
        .unwrap();
    assert_eq!(registered.worktrees.len(), 1);
    assert!(registered.worktrees[0].trusted_digest.is_none());
    assert!(matches!(
        owner.admit(&context),
        Err("wsl_admission_required")
    ));
    let bridge_path = format!("{root}/bridge.txt");
    let bridge_file = Path::new(&unc).join("bridge.txt");
    std::fs::write(&bridge_file, b"bridge baseline\r\n").unwrap();
    let admitted = owner.admit_selection(&resources, &context).unwrap();
    assert_eq!(admitted, binding);
    let mut bridge =
        devbox_workspace_lib::platform::wsl_files::WslFiles::open(&owner, &resources, &context)
            .unwrap();
    let bridge_opened = bridge
        .execute(
            &owner,
            &context,
            "open_file",
            serde_json::json!({"request":{"path":bridge_path,"encoding":null}}),
            u64::MAX,
        )
        .unwrap();
    bridge
        .execute(
            &owner,
            &context,
            "watch_file",
            serde_json::json!({"path":bridge_path}),
            u64::MAX,
        )
        .unwrap();
    assert_eq!(bridge.poll(&owner, &context).unwrap().len(), 1);
    assert!(bridge.poll(&owner, &context).unwrap().is_empty());
    std::fs::write(&bridge_file, b"external bridge\r\n").unwrap();
    assert_eq!(bridge.poll(&owner, &context).unwrap().len(), 1);
    assert_eq!(
        bridge.documents.revision(&bridge_path).unwrap(),
        bridge_opened["nativeRevision"].as_str().unwrap()
    );
    assert!(bridge
        .recover(
            &owner,
            &context,
            &bridge_path,
            "stale recovery\n",
            bridge_opened["nativeRevision"].as_str().unwrap(),
            u64::MAX
        )
        .is_err());
    let bridge_current = bridge
        .execute(
            &owner,
            &context,
            "open_file",
            serde_json::json!({"request":{"path":bridge_path,"encoding":null}}),
            u64::MAX,
        )
        .unwrap();
    bridge
        .recover(
            &owner,
            &context,
            &bridge_path,
            "검토한 복구\n",
            bridge_current["nativeRevision"].as_str().unwrap(),
            u64::MAX,
        )
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(&bridge_file).unwrap(),
        "검토한 복구\r\n"
    );
    bridge
        .documents
        .validate_paths(std::slice::from_ref(&bridge_path))
        .unwrap();
    let mut wrong_context = context.clone();
    wrong_context.revision += 1;
    assert!(bridge
        .execute(
            &owner,
            &wrong_context,
            "open_file",
            serde_json::json!({"request":{"path":bridge_path,"encoding":null}}),
            u64::MAX
        )
        .is_err());
    bridge.close(&bridge_path).unwrap();
    assert!(!bridge.documents.has_documents());
    bridge.shutdown().unwrap();
    bridge.shutdown().unwrap();
    drop(bridge);
    std::fs::remove_file(&bridge_file).unwrap();
    first
        .file_request(
            "files_attach",
            &report.token,
            serde_json::json!({"context":context}),
        )
        .expect("native Linux file admission");
    let request = serde_json::json!({"context":context,"request":{"path":format!("{root}/original.txt"),"encoding":null}});
    assert!(first
        .file_request("files_open", "invalid", request.clone())
        .is_err());
    let opened = first
        .file_request("files_open", &report.token, request.clone())
        .expect("actual native WSL file read");
    assert_eq!(opened["text"], "unchanged synthetic fixture\n");
    assert!(workspace_wsl::token(
        opened["nativeRevision"].as_str().unwrap()
    ));
    let mut foreign = request;
    foreign["context"]["revision"] = serde_json::json!(context.revision + 1);
    assert!(matches!(
        first.file_request("files_open", &report.token, foreign),
        Err("file_context_changed")
    ));
    first
        .file_request(
            "files_close",
            &report.token,
            serde_json::json!({"context":context,"path":opened["path"]}),
        )
        .unwrap();
    let edit = first
        .file_request(
            "files_open",
            &report.token,
            serde_json::json!({"context":context,"request":{"path":format!("{root}/edit.txt"),"encoding":null}}),
        )
        .unwrap();
    let save = serde_json::json!({"context":context,"nativeRevision":edit["nativeRevision"],"request":{
        "path":edit["path"],"text":"saved 한글\n","encoding":edit["encoding"],"lineEnding":edit["lineEnding"],
        "expectedMtimeNanos":edit["mtimeNanos"],"expectedSize":edit["size"],"expectedContentHash":edit["contentHash"],"sourceLossy":false
    }});
    let saved = first
        .file_request("files_save", &report.token, save.clone())
        .expect("actual native WSL atomic save");
    assert!(workspace_wsl::token(
        saved["nativeRevision"].as_str().unwrap()
    ));
    assert_ne!(saved["nativeRevision"], edit["nativeRevision"]);
    assert!(matches!(
        first.file_request("files_save", &report.token, save),
        Err("file_snapshot_changed")
    ));
    drop(first); // EOF must retire the Linux helper before restart.
    assert_eq!(
        std::fs::read(&editable).unwrap(),
        "saved 한글\r\n".as_bytes()
    );
    let mut second = Connection::connect(&resources, &distro.id, false).unwrap();
    let repeated = second.observe(&root).unwrap();
    assert_eq!(report.root_object, repeated.root_object);
    assert_eq!(scope, second.lease().scope(&repeated.root_object.scope));
    assert_ne!(report.token, repeated.token);
    second
        .file_request(
            "files_attach",
            &repeated.token,
            serde_json::json!({"context":context}),
        )
        .unwrap();
    let listed = second
        .file_request(
            "files_list",
            &repeated.token,
            serde_json::json!({"context":context,"path":root}),
        )
        .unwrap();
    assert_eq!(listed["files"].as_array().unwrap().len(), 2);
    let edit = second
        .file_request(
            "files_open",
            &repeated.token,
            serde_json::json!({
                "context":context,"request":{"path":format!("{root}/edit.txt"),"encoding":null}
            }),
        )
        .unwrap();
    let mut rename = serde_json::json!({"context":context,"nativeRevision":edit["nativeRevision"],"request":{
        "path":edit["path"],"expectedMtimeNanos":edit["mtimeNanos"],"expectedSize":edit["size"],"expectedContentHash":edit["contentHash"],"newName":"original.txt"
    }});
    assert!(matches!(
        second.file_request("files_rename", &repeated.token, rename.clone()),
        Err("file_rename_conflict")
    ));
    rename["request"]["newName"] = serde_json::json!("review 한글.md");
    let renamed = second
        .file_request("files_rename", &repeated.token, rename)
        .expect("native WSL non-overwriting rename");
    let preview = second.file_request("files_preview", &repeated.token, serde_json::json!({
        "context":context,"path":renamed["path"],"content":"# Native WSL preview","workspaceRoot":root
    })).unwrap();
    assert_eq!(preview["kind"], "markdown");
    assert!(preview["html"]
        .as_str()
        .unwrap()
        .contains("Native WSL preview"));
    let mut delete = serde_json::json!({"context":context,"nativeRevision":edit["nativeRevision"],"request":{
        "path":renamed["path"],"expectedMtimeNanos":renamed["mtimeNanos"],"expectedSize":renamed["size"],"expectedContentHash":renamed["contentHash"]
    }});
    assert!(matches!(
        second.file_request("files_delete", &repeated.token, delete.clone()),
        Err("file_snapshot_changed")
    ));
    delete["nativeRevision"] = renamed["nativeRevision"].clone();
    second
        .file_request("files_delete", &repeated.token, delete)
        .expect("native WSL snapshot-checked delete");
    second.release(&repeated.token).unwrap();
    assert!(second.validate(&repeated.token).is_err());
    drop(second);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime
        .block_on(wsl_distro::output(
            &["--terminate".into(), name.clone()],
            Duration::from_secs(5),
        ))
        .unwrap();
    assert!(
        !wsl_distro::list()
            .unwrap()
            .iter()
            .find(|d| d.id == distro.id)
            .unwrap()
            .running
    );
    assert!(matches!(
        Connection::connect(&resources, &distro.id, false),
        Err("wsl_distro_stopped")
    ));
    assert!(
        !wsl_distro::list()
            .unwrap()
            .iter()
            .find(|d| d.id == distro.id)
            .unwrap()
            .running
    );
    let mut started = Connection::connect(&resources, &distro.id, true).unwrap();
    let after = started.observe(&root).unwrap();
    assert_eq!(report.root_object, after.root_object);
    drop(started);
    let after_restart = owner
        .preview_wsl(&resources, &distro.id, &root, false)
        .unwrap();
    assert_eq!(after_restart.binding, binding);
    assert!(matches!(
        after_restart.discovery,
        devbox_workspace_lib::core::registry::Discovery::Known { .. }
    ));
    owner.cancel(&after_restart.preview_id).unwrap();
    assert_eq!(owner.snapshot().unwrap(), registered);
    drop(owner);
    assert_eq!(
        std::fs::read(&marker).unwrap(),
        b"unchanged synthetic fixture\n"
    );
    std::fs::remove_file(marker).unwrap();
    assert!(!editable.exists());
    assert!(!Path::new(&unc).join("review 한글.md").exists());
    std::fs::remove_dir(&unc).unwrap();
}
