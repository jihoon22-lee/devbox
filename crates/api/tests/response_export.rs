//! Response-to-note export uses only completed daemon-owned, unchanged history.
mod support;
use devbox_api::*;
use support::api::*;
use support::{Reply, Server, handler};

fn history_id(events: &[SendEvent]) -> String {
    events
        .iter()
        .find_map(|e| match e {
            SendEvent::Started { history_id, .. } => Some(history_id.clone()),
            _ => None,
        })
        .expect("started")
}
#[tokio::test]
async fn completed_response_exports_original_masked_metadata_and_large_body() {
    let api = Harness::with_options(ApiOptions::default().with_clock(|| 0)).await;
    let server = Server::start(handler(|_| async {
        Reply::ok(format!(
            "readable-prefix|fixture-export-cookie|{}",
            "a".repeat(70 * 1024)
        ))
        .header("set-cookie", "sid=fixture-export-cookie; Path=/")
        .header("content-type", "text/plain")
    }))
    .await;
    let mut input = with_env(
        snapshot(
            "GET",
            &format!(
                "{}/original/{{{{path}}}}?token=fixture-export-query",
                server.url("")
            ),
        ),
        &[("path", "substituted", false)],
    );
    input.source_root = Some(CollectionRoot::Personal);
    let events = api.complete(input).await;
    let id = history_id(&events);
    let exported = api
        .api
        .export_response(HistoryId { id: id.clone() })
        .await
        .unwrap_or_else(|_| panic!("completed response must export"));
    assert_eq!(exported.history_id, id);
    assert!(exported.markdown.contains("GET"));
    assert!(
        exported
            .markdown
            .contains("/original/substituted?token=[REDACTED]")
    );
    assert!(!exported.markdown.contains("fixture-export-query"));
    assert!(!exported.markdown.contains("fixture-export-cookie"));
    assert!(exported.markdown.contains("200"));
    assert!(
        exported.markdown.contains("1970-01-01 00:00:00.000 UTC"),
        "recorded send timestamp, not export time"
    );
    assert!(exported.markdown.contains("readable-prefix"));
    assert!(
        exported.markdown.len() > 64 * 1024,
        "note export has its own body allowance"
    );
}

fn export_reason(result: R<ResponseNoteExport>) -> String {
    match result {
        Err(devbox_protocol::Fail::Domain(ApiError::ResponseExportFailed { reason })) => reason,
        _ => panic!("expected fixed response export failure"),
    }
}
async fn complete_reply(api: &Harness, reply: Reply) -> String {
    let server = Server::start(handler(move |_| {
        let reply = reply.clone();
        async move { reply }
    }))
    .await;
    let mut input = request(snapshot("GET", &server.url("/original")));
    input.source_root = Some(CollectionRoot::Personal);
    history_id(&api.complete(input).await)
}
async fn exported(api: &Harness, id: &str) -> ResponseNoteExport {
    api.api
        .export_response(HistoryId { id: id.into() })
        .await
        .expect("safe completed export")
}
#[tokio::test]
async fn utf8_export_clips_only_incomplete_tail_and_marks_truncation() {
    const LIMIT: usize = 1024 * 1024;
    let api = Harness::new().await;
    let exact = complete_reply(
        &api,
        Reply::ok("x".repeat(LIMIT)).header("content-type", "text/plain"),
    )
    .await;
    let exact = exported(&api, &exact).await.markdown;
    assert_eq!(
        exact.bytes().filter(|b| *b == b'x').count(),
        LIMIT + 2,
        "only two fence text info strings add x"
    );
    assert!(!exact.contains("본문이 잘렸습니다"));
    let input = format!("{}한tail-must-not-appear", "y".repeat(LIMIT - 1));
    let id = complete_reply(&api, Reply::ok(input).header("content-type", "text/plain")).await;
    let markdown = exported(&api, &id).await.markdown;
    assert!(markdown.contains(&"y".repeat(LIMIT - 1)));
    assert!(!markdown.contains('한') && !markdown.contains('�'));
    assert!(!markdown.contains("tail-must-not-appear"));
    assert!(markdown.contains("본문이 잘렸습니다"));
    assert!(markdown.len() <= LIMIT + 16 * 1024);
}
#[tokio::test]
async fn fences_cannot_close_the_body_and_wrapper_budget_is_separate() {
    let api = Harness::new().await;
    let text = "before\n```````\n# body heading\n~~~~~\nafter";
    let id = complete_reply(&api, Reply::ok(text).header("content-type", "text/plain")).await;
    let markdown = exported(&api, &id).await.markdown;
    assert!(markdown.contains(text));
    assert!(markdown.contains("~~~~~~text\n"));
    assert_eq!(markdown.lines().filter(|l| *l == "~~~~~~").count(), 1);
    let text = format!("{}\n{}", "`".repeat(9000), "~".repeat(9000));
    let id = complete_reply(&api, Reply::ok(text).header("content-type", "text/plain")).await;
    assert_eq!(
        export_reason(api.api.export_response(HistoryId { id }).await),
        "size"
    );
}
#[tokio::test]
async fn binary_export_uses_only_size_and_type() {
    let api = Harness::new().await;
    let bytes = b"binary-body-must-not-be-decoded\xff\x00".to_vec();
    let length = bytes.len();
    let id = complete_reply(
        &api,
        Reply::ok(bytes).header("content-type", "application/octet-stream"),
    )
    .await;
    let markdown = exported(&api, &id).await.markdown;
    assert!(!markdown.contains("binary-body-must-not-be-decoded"));
    assert!(markdown.contains(&format!("{length}바이트")));
    assert!(markdown.contains("application/octet-stream"));
}
#[tokio::test]
async fn old_session_or_unknown_history_cannot_mint_export_provenance() {
    let api = Harness::new().await;
    let id = complete_reply(&api, Reply::ok("safe body")).await;
    exported(&api, &id).await;
    let reopened = ApiService::new(
        api.db.clone(),
        api.index.clone(),
        api.paths.clone(),
        api.projects.clone(),
        api.secrets.clone(),
    )
    .expect("reopened service");
    assert!(
        reopened
            .history_get(HistoryId { id: id.clone() })
            .await
            .is_ok(),
        "persisted history is still readable"
    );
    assert_eq!(
        export_reason(reopened.export_response(HistoryId { id }).await),
        "provenance"
    );
    assert_eq!(
        export_reason(
            api.api
                .export_response(HistoryId { id: "f".repeat(32) })
                .await
        ),
        "provenance"
    );
}
#[tokio::test]
async fn changed_metadata_or_any_external_body_byte_is_rejected() {
    let api = Harness::new().await;
    let id = complete_reply(
        &api,
        Reply::ok("a".repeat(1024 * 1024 + 128)).header("content-type", "text/plain"),
    )
    .await;
    exported(&api, &id).await;
    let files = api.history_files();
    let json = files
        .iter()
        .find(|p| p.extension().is_some_and(|x| x == "json"))
        .expect("json");
    let original = std::fs::read(json).expect("original");
    let mut changed = original.clone();
    let at = changed
        .windows(b"original".len())
        .position(|w| w == b"original")
        .expect("url");
    changed[at] = b'z';
    std::fs::write(json, &changed).expect("same-length metadata mutation");
    assert_eq!(
        export_reason(api.api.export_response(HistoryId { id: id.clone() }).await),
        "history"
    );
    std::fs::write(json, &original).expect("restore owned fixture");
    let body = files
        .iter()
        .find(|p| p.extension().is_some_and(|x| x == "body"))
        .expect("body");
    let mut changed = std::fs::read(body).expect("external body");
    *changed.last_mut().expect("nonempty") = b'b';
    std::fs::write(body, &changed).expect("change beyond exported prefix");
    assert_eq!(
        export_reason(api.api.export_response(HistoryId { id: id.clone() }).await),
        "history"
    );
    std::fs::remove_file(body).expect("remove owned body");
    assert_eq!(
        export_reason(api.api.export_response(HistoryId { id }).await),
        "history"
    );
}
#[tokio::test]
async fn pending_cancelled_write_failed_and_redaction_failed_responses_stay_unexportable() {
    let api = Harness::new().await;
    let server = Server::start(handler(|_| async {
        Reply::ok("late").slow_body(std::time::Duration::from_secs(2))
    }))
    .await;
    let mut sent = api
        .send(request(snapshot("GET", &server.url("/pending"))))
        .await
        .expect("send");
    let first = sent
        .next(std::time::Duration::from_secs(5))
        .await
        .expect("started");
    let id = history_id(&[first]);
    assert_eq!(
        export_reason(api.api.export_response(HistoryId { id: id.clone() }).await),
        "pending"
    );
    sent.close();
    sent.events().await;
    assert_eq!(
        export_reason(api.api.export_response(HistoryId { id }).await),
        "incomplete"
    );
    let failed = Harness::new().await;
    std::fs::write(
        failed.paths.data_dir.join("api-history"),
        b"owned obstruction",
    )
    .expect("block history directory");
    let id = complete_reply(&failed, Reply::ok("UI received this complete body")).await;
    assert_eq!(
        export_reason(failed.api.export_response(HistoryId { id }).await),
        "history"
    );
    let id = complete_reply(
        &api,
        Reply::ok("must not export")
            .header("set-cookie", &format!("sid={}", "s".repeat(256 * 1024 + 1))),
    )
    .await;
    assert_eq!(
        export_reason(api.api.export_response(HistoryId { id }).await),
        "redaction"
    );
}

async fn source_send(
    api: &Harness,
    root: Option<CollectionRoot>,
    origin: Option<RequestOrigin>,
) -> String {
    let server = Server::start(handler(|_| async { Reply::ok("owned source response") })).await;
    let mut input = request(snapshot("GET", &server.url("/source")));
    input.source_root = root;
    input.origin = origin;
    let events = api.complete(input).await;
    assert!(
        events.iter().any(|e| matches!(e, SendEvent::End { .. })),
        "source proof never changes HTTP compatibility"
    );
    history_id(&events)
}
async fn project(api: &Harness) -> (CollectionRoot, std::path::PathBuf) {
    let path = api.dir.path().join("source-project");
    std::fs::create_dir(&path).expect("owned project");
    let project = api
        .projects
        .add(devbox_projects::AddProject {
            path: path.display().to_string(),
        })
        .await
        .expect("register");
    (
        CollectionRoot::Project {
            project_id: project.id,
        },
        path,
    )
}
#[tokio::test]
async fn saved_personal_source_revalidates_path_and_canonical_root() {
    let api = Harness::new().await;
    let dir = api.paths.config_dir.join("api");
    std::fs::create_dir_all(&dir).expect("personal sources");
    let path = dir.join("source.request.json");
    std::fs::write(&path, b"{}").expect("source");
    let origin = Some(RequestOrigin {
        root: CollectionRoot::Personal,
        path: "source.request.json".into(),
    });
    let id = source_send(&api, Some(CollectionRoot::Personal), origin).await;
    exported(&api, &id).await;
    let moved = dir.join("renamed.request.json");
    std::fs::rename(&path, &moved).expect("rename owned source");
    assert_eq!(
        export_reason(api.api.export_response(HistoryId { id: id.clone() }).await),
        "origin"
    );
    std::fs::rename(&moved, &path).expect("restore path");
    exported(&api, &id).await;
    let replaced = api.paths.config_dir.with_extension("owned-old");
    std::fs::rename(&api.paths.config_dir, &replaced).expect("replace personal root");
    std::fs::create_dir_all(api.paths.config_dir.join("api")).expect("new root");
    std::fs::write(api.paths.config_dir.join("api/source.request.json"), b"{}")
        .expect("same path new authority");
    assert_eq!(
        export_reason(api.api.export_response(HistoryId { id }).await),
        "origin"
    );
}
#[tokio::test]
async fn unsaved_project_draft_binds_current_project_and_legacy_omission_is_not_personal() {
    let api = Harness::new().await;
    let (root, _) = project(&api).await;
    let id = source_send(&api, Some(root.clone()), None).await;
    exported(&api, &id).await;
    let legacy = source_send(&api, None, None).await;
    assert_eq!(
        export_reason(api.api.export_response(HistoryId { id: legacy }).await),
        "origin"
    );
    let CollectionRoot::Project { project_id } = root else {
        panic!("project")
    };
    api.projects
        .remove(devbox_projects::RemoveProject { id: project_id })
        .await
        .expect("remove registration");
    assert_eq!(
        export_reason(api.api.export_response(HistoryId { id }).await),
        "origin"
    );
}
#[tokio::test]
async fn saved_project_source_rejects_mismatched_root_and_symlink_replacement() {
    let api = Harness::new().await;
    let (root, path) = project(&api).await;
    let dir = path.join(".devbox/api");
    std::fs::create_dir_all(&dir).expect("collection directory");
    let file = dir.join("source.request.json");
    std::fs::write(&file, b"{}").expect("profile");
    let origin = Some(RequestOrigin {
        root: root.clone(),
        path: "source.request.json".into(),
    });
    let mismatched = source_send(&api, Some(CollectionRoot::Personal), origin.clone()).await;
    assert_eq!(
        export_reason(api.api.export_response(HistoryId { id: mismatched }).await),
        "origin"
    );
    let id = source_send(&api, Some(root), origin).await;
    exported(&api, &id).await;
    let outside = api.dir.path().join("outside.json");
    std::fs::write(&outside, b"{}").expect("outside owned fixture");
    std::fs::remove_file(&file).expect("replace source");
    std::os::unix::fs::symlink(&outside, &file).expect("owned symlink");
    assert_eq!(
        export_reason(api.api.export_response(HistoryId { id }).await),
        "origin"
    );
}
#[tokio::test]
async fn fifo_history_and_source_replacements_fail_without_blocking_open() {
    use std::os::unix::fs::OpenOptionsExt;
    async fn bounded(api: &Harness, id: String, path: &std::path::Path) -> String {
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(300),
            api.api.export_response(HistoryId { id }),
        )
        .await;
        if result.is_err() {
            // Release a regressed blocking reader before failing the test/runtime.
            let _ = std::fs::OpenOptions::new()
                .write(true)
                .custom_flags(nix::libc::O_NONBLOCK)
                .open(path);
        }
        export_reason(result.expect("special file must not block"))
    }
    let api = Harness::new().await;
    let id = complete_reply(&api, Reply::ok("safe body")).await;
    let json = api
        .history_files()
        .into_iter()
        .find(|p| p.extension().is_some_and(|e| e == "json"))
        .expect("history json");
    std::fs::remove_file(&json).expect("replace history");
    nix::unistd::mkfifo(
        &json,
        nix::sys::stat::Mode::S_IRUSR | nix::sys::stat::Mode::S_IWUSR,
    )
    .expect("owned fifo");
    assert_eq!(bounded(&api, id, &json).await, "history");
    let dir = api.paths.config_dir.join("api");
    std::fs::create_dir_all(&dir).expect("source dir");
    let file = dir.join("source.request.json");
    std::fs::write(&file, b"{}").expect("source");
    let id = source_send(
        &api,
        Some(CollectionRoot::Personal),
        Some(RequestOrigin {
            root: CollectionRoot::Personal,
            path: "source.request.json".into(),
        }),
    )
    .await;
    std::fs::remove_file(&file).expect("replace source");
    nix::unistd::mkfifo(
        &file,
        nix::sys::stat::Mode::S_IRUSR | nix::sys::stat::Mode::S_IWUSR,
    )
    .expect("owned fifo");
    assert_eq!(bounded(&api, id, &file).await, "origin");
}
#[tokio::test]
async fn metadata_has_an_independent_wrapper_budget() {
    let api = Harness::new().await;
    let server = Server::ok().await;
    let mut input = request(snapshot(
        "GET",
        &format!("{}/{}", server.url(""), "a".repeat(17 * 1024)),
    ));
    input.source_root = Some(CollectionRoot::Personal);
    let id = history_id(&api.complete(input).await);
    assert_eq!(
        export_reason(api.api.export_response(HistoryId { id }).await),
        "size"
    );
}

#[tokio::test]
async fn unregistered_sidecar_cannot_replace_verified_inline_body() {
    let api = Harness::new().await;
    let id = complete_reply(&api, Reply::ok("verified-inline-body")).await;
    let json = api
        .history_files()
        .into_iter()
        .find(|p| p.extension().is_some_and(|e| e == "json"))
        .expect("history json");
    std::fs::write(json.with_extension("body"), b"unregistered-sidecar-content")
        .expect("owned unexpected sidecar");
    let markdown = exported(&api, &id).await.markdown;
    assert!(markdown.contains("verified-inline-body"));
    assert!(!markdown.contains("unregistered-sidecar-content"));
}
