//! Actual Windows actor -> native Linux LSP test using only disposable inputs.
use super::{
    approval::{Activities, Approvals},
    settings::Settings,
    wsl_actor::{deadline, Actor},
    wsl_approval::Snapshot,
};
use crate::{files_host::FilesHost, host::Host};
use code_pad_lib::lsp::RequestCancellation;
use product_contract::ProjectContext;
use serde_json::{json, Value};
use std::{
    fs,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

pub(crate) fn check_owned_fixture(
    host: Arc<Host>,
    context: &ProjectContext,
    program: &str,
    disk: &Path,
    files: Arc<Mutex<FilesHost>>,
    alive: &dyn Fn(u32) -> bool,
) {
    let root = program.rsplit_once('/').unwrap().0;
    let disk_root = disk.parent().unwrap();
    for state in ["lsp-ready", "lsp-slow"] {
        fs::create_dir(disk_root.join(state)).unwrap();
    }
    let settings = Settings::open(&host, context).unwrap();
    let mut config = settings.view().unwrap()["config"].clone();
    config["enabled"] = json!(true);
    config["custom_servers"] = json!([]);
    config["server_by_language"] = json!({
        "rust":{"kind":"custom","executable":program,"args":["--state",format!("{root}/lsp-ready")]},
        "python":{"kind":"custom","executable":program,"args":["--state",format!("{root}/lsp-slow"),"--slow","--detach"]}
    });
    let config_revision = settings.revision().unwrap();
    settings
        .save(
            &host,
            json!({"config":config,"nativeRevision":config_revision,"recoverInvalid":false}),
            deadline(),
        )
        .unwrap();
    let mut approvals = Approvals::default();
    let preview = approvals
        .preview(Snapshot::capture(host.clone(), context, deadline(), false).unwrap())
        .unwrap();
    approvals
        .approve(context, preview["previewId"].as_str().unwrap(), deadline())
        .unwrap();
    let snapshot = Snapshot::capture(host.clone(), context, deadline(), true).unwrap();
    let events = Arc::new(Mutex::new(Vec::<(String, Value)>::new()));
    let collected = events.clone();
    let activities = Activities::default();
    let actor = Actor::spawn(
        Arc::new(move |name, value| collected.lock().unwrap().push((name.into(), value))),
        snapshot,
        activities.clone(),
        files.clone(),
        RequestCancellation::default(),
    )
    .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let request =
        |method: &str, args: Value| runtime.block_on(actor.request(method, args, deadline(), None));
    let file_request = |method: &str, args: Value| {
        files
            .lock()
            .unwrap()
            .execute_wsl_editor_fixture(&host, context, method, args, deadline())
            .unwrap()
    };
    request(
        "start_language_server",
        json!({"languageId":"rust","operationId":"native-runtime-ready"}),
    )
    .unwrap();
    let statuses = request("language_server_statuses", json!({})).unwrap();
    assert!(statuses
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["languageId"] == "rust" && s["processState"] == "running"));
    let path = format!("{root}/편집 문서.rs");
    let disk_file = disk_root.join("편집 문서.rs");
    let original = "a🙂 old\n";
    fs::write(&disk_file, original).unwrap();
    let opened = file_request(
        "open_file",
        json!({"request":{"path":path,"encoding":null}}),
    );
    let baseline = opened.clone();
    let revision = opened["nativeRevision"].clone();
    let opened = request(
        "open_lsp_document",
        json!({"languageId":"rust","path":path,"nativeRevision":revision,"text":original}),
    )
    .unwrap();
    let uri = opened["uri"].clone();
    assert!(uri.as_str().unwrap().starts_with("file:///home/"));
    assert!(uri.as_str().unwrap().contains("%20"));
    assert!(request(
        "request_lsp_completion",
        json!({"languageId":"rust","uri":uri,"position":{"line":0,"character":2}})
    )
    .is_err());
    let completion = request(
        "request_lsp_completion",
        json!({"languageId":"rust","uri":uri,"position":{"line":0,"character":3}}),
    )
    .unwrap();
    assert!(completion.to_string().contains("fixtureComplete"));
    let changed = "a🙂 Windows actor edit\n";
    file_request(
        "sync_editor_document",
        json!({"path":path,"nativeRevision":revision,"text":changed}),
    );
    request("change_lsp_document",json!({"languageId":"rust","uri":uri,"nativeRevision":revision,"text":changed,"dirty":false})).unwrap();
    assert_eq!(fs::read_to_string(&disk_file).unwrap(), original);
    let hover = request(
        "request_lsp_hover",
        json!({"languageId":"rust","uri":uri,"position":{"line":0,"character":3}}),
    )
    .unwrap();
    assert!(hover.to_string().contains("fixture hover"));
    assert_eq!(hover["metadata"]["version"], 2);
    let formatted = request(
        "request_lsp_formatting",
        json!({"languageId":"rust","uri":uri,"tabSize":2,"insertSpaces":true}),
    )
    .unwrap();
    assert_eq!(fs::read_to_string(&disk_file).unwrap(), original);
    assert!(request("request_lsp_rename",json!({"languageId":"rust","uri":uri,"position":{"line":0,"character":0},"newName":"blocked"})).is_err());
    let text = formatted["documents"][0]["text"].as_str().unwrap();
    file_request(
        "sync_editor_document",
        json!({"path":path,"nativeRevision":revision,"text":text}),
    );
    let saved = file_request(
        "save_file",
        json!({"request":{"path":path,"text":text,"encoding":baseline["encoding"],"lineEnding":baseline["lineEnding"],"expectedMtimeNanos":baseline["mtimeNanos"],"expectedSize":baseline["size"],"expectedContentHash":baseline["contentHash"],"sourceLossy":false,"nativeRevision":revision}}),
    );
    request(
        "save_lsp_document",
        json!({"languageId":"rust","uri":uri,"nativeRevision":saved["nativeRevision"],"text":text}),
    )
    .unwrap();
    assert_eq!(fs::read_to_string(&disk_file).unwrap(), text);
    let ready_pid = fs::read_to_string(disk_root.join("lsp-ready/server.pid"))
        .unwrap()
        .parse::<u32>()
        .unwrap();
    assert!(
        alive(ready_pid),
        "owned PID observer could not see the running native server"
    );
    // The native admission barrier keeps both Windows permits until the slow
    // unpublished server and its detached child have actually been reaped.
    let cancel = Arc::new(AtomicBool::new(false));
    thread::scope(|scope| {
        let cancel_flag = cancel.clone();
        let activity = activities.clone();
        let state = disk_root.join("lsp-slow");
        let watcher = scope.spawn(move || {
            let until = Instant::now() + Duration::from_secs(20);
            while !state.join("initializing").exists() || !state.join("detached.pid").exists() {
                if Instant::now() >= until {
                    cancel_flag.store(true, Ordering::Release);
                    panic!("native startup did not initialize");
                }
                thread::sleep(Duration::from_millis(20));
            }
            let context_busy = activity.context.enter(true).is_err();
            let filesystem_busy = activity.filesystem.enter(true).is_err();
            cancel_flag.store(true, Ordering::Release);
            assert!(
                context_busy && filesystem_busy,
                "startup released Windows activity before native acknowledgement"
            );
        });
        assert_eq!(
            runtime
                .block_on(actor.request(
                    "start_language_server",
                    json!({"languageId":"python","operationId":"cancel-native-start"}),
                    deadline(),
                    Some(cancel)
                ))
                .unwrap_err(),
            "lsp_operation_cancelled"
        );
        watcher.join().unwrap();
    });
    // proc observations use the exact fixture PIDs in its own distro only.
    for leaf in ["server.pid", "detached.pid"] {
        let pid = fs::read_to_string(disk_root.join("lsp-slow").join(leaf)).unwrap();
        assert!(pid.parse::<u32>().unwrap() > 1);
        assert!(
            !alive(pid.parse().unwrap()),
            "cancelled native child remained visible: {pid}"
        );
    }
    request(
        "stop_language_server",
        json!({"languageId":"python","operationId":"cancel-native-start"}),
    )
    .unwrap();
    let statuses = request("language_server_statuses", json!({})).unwrap();
    assert!(statuses
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["languageId"] == "rust" && s["processState"] == "running"));
    request("close_lsp_document", json!({"languageId":"rust","uri":uri})).unwrap();
    files
        .lock()
        .unwrap()
        .set_wsl_document_fixture(&host, context, &path, false)
        .unwrap();
    request("stop_all_language_servers", json!({})).unwrap();
    runtime.block_on(actor.retire()).unwrap();
    assert!(!alive(ready_pid));
    assert!(activities.context.enter(true).is_ok());
    assert!(activities.filesystem.enter(true).is_ok());
    assert!(events
        .lock()
        .unwrap()
        .iter()
        .any(|(name, value)| name == "lsp/status" && value["nativeContext"] == json!(context)));
    approvals.revoke(&host, context, deadline()).unwrap();
    for state in ["lsp-ready", "lsp-slow"] {
        fs::remove_dir_all(disk_root.join(state)).unwrap();
    }
    fs::remove_file(disk_file).unwrap();
    println!("WSL LSP runtime: actual Windows actor, native UTF-16 editor, context events, cancellation barrier and detached-child retirement passed");
}
