use crate::containers::*;
use crate::*;
use tokio_util::sync::CancellationToken;

const ROW: &str = r#"{"ID":"abc","Names":"owned-web","Image":"nginx:alpine","State":"running","Status":"Up 2 hours","Ports":"127.0.0.1:8080->80/tcp","Labels":"other=hello,com.docker.compose.project=owned,com.docker.compose.service=web"}"#;

#[test]
fn containers_argv_and_json_compose_fixture() {
    assert_eq!(
        container_args("abc", ContainerAction::Restart),
        ["restart", "abc"]
    );
    assert_eq!(
        container_args("abc", ContainerAction::Start),
        ["start", "abc"]
    );
    assert_eq!(
        container_args("abc", ContainerAction::Stop),
        ["stop", "abc"]
    );
    let parsed = parse_ps(ROW.as_bytes(), &[]).expect("fixture result");
    assert_eq!(parsed.items[0].compose_project.as_deref(), Some("owned"));
    assert_eq!(parsed.items[0].ports, "127.0.0.1:8080->80/tcp");
    assert!(parsed.available);
}

// The column set of one `docker ps -a --format json` line for a Compose service.
const COMPOSE_ROW: &str = r#"{"Command":"\"/docker-entrypoint.…\"","CreatedAt":"2026-01-01 00:00:00 +0000 UTC","ID":"0123456789ab","Image":"nginx:alpine","Labels":"com.docker.compose.config-hash=0f0f,com.docker.compose.container-number=1,com.docker.compose.depends_on=db:service_started:false,cache:service_started:false,com.docker.compose.oneoff=False,com.docker.compose.project=app,com.docker.compose.project.config_files=/home/u/projects/app/compose.yaml,com.docker.compose.project.working_dir=/home/u/projects/app,com.docker.compose.service=web,com.docker.compose.version=2.29.0","LocalVolumes":"0","Mounts":"","Names":"app-web-1","Networks":"app_default","Ports":"0.0.0.0:8080->80/tcp, [::]:8080->80/tcp","RunningFor":"2 hours ago","Size":"0B","State":"running","Status":"Up 2 hours"}"#;
fn roots() -> Vec<devbox_core::project_root::ProjectRoot> {
    [
        ("app", "/home/u/projects/app"),
        ("api", "/home/u/projects/app/services/api"),
    ]
    .map(|(id, path)| devbox_core::project_root::ProjectRoot {
        id: id.into(),
        path: path.into(),
    })
    .into()
}
#[test]
fn containers_take_the_project_of_the_compose_working_dir_label() {
    let project = |row: &str| {
        let parsed = parse_ps(row.as_bytes(), &roots()).expect("fixture result");
        assert_eq!(parsed.items.len(), 1);
        parsed.items[0].project_id.clone()
    };
    let in_dir = |dir: &str| {
        COMPOSE_ROW.replace(
            "working_dir=/home/u/projects/app,",
            &format!("working_dir={dir},"),
        )
    };
    assert_eq!(project(COMPOSE_ROW).as_deref(), Some("app"));
    assert_eq!(
        project(&in_dir("/home/u/projects/app/services/api")).as_deref(),
        Some("api"),
        "nested projects: the longest root wins"
    );
    assert_eq!(
        project(&in_dir("/home/u/projects/app2")),
        None,
        "sibling with a common prefix"
    );
    assert_eq!(project(&in_dir("/home/u/projects/app/../other")), None);
    assert_eq!(
        project(ROW),
        None,
        "a container without the label stays unattributed"
    );
    // Without registered projects the rest of the row is unchanged.
    let parsed = parse_ps(COMPOSE_ROW.as_bytes(), &[]).expect("fixture result");
    assert_eq!(parsed.items[0].project_id, None);
    assert_eq!(parsed.items[0].name, "app-web-1");
    assert_eq!(parsed.items[0].compose_project.as_deref(), Some("app"));
}
#[test]
fn containers_wire_shape_has_an_optional_project_id_and_no_labels() {
    let parsed =
        parse_ps(format!("{COMPOSE_ROW}\n{ROW}\n").as_bytes(), &roots()).expect("fixture result");
    let json = serde_json::to_value(&parsed).expect("fixture result");
    assert_eq!(json["items"][0]["projectId"], "app");
    assert!(json["items"][1].get("projectId").is_none());
    let text = json.to_string();
    assert!(!text.contains("working_dir") && !text.contains("/home/u/projects/app"));
}
#[test]
fn containers_labels_that_are_not_text_do_not_fail_the_listing() {
    for odd in ["null", "7", r#"["a=b"]"#, r#"{"a":"b"}"#] {
        let rows = format!(
            "{}\n{COMPOSE_ROW}\n",
            ROW.replace(r#""Labels":"other=hello,com.docker.compose.project=owned,com.docker.compose.service=web""#, &format!(r#""Labels":{odd}"#))
        );
        let parsed = parse_ps(rows.as_bytes(), &roots()).expect("the listing survives odd labels");
        assert_eq!(parsed.items.len(), 2, "{odd}");
        assert_eq!(parsed.items[0].compose_project, None);
        assert_eq!(parsed.items[0].project_id, None);
        assert_eq!(parsed.items[1].project_id.as_deref(), Some("app"));
    }
}

#[tokio::test]
async fn containers_missing_cli_is_unavailable_not_empty() {
    let dir = tempfile::tempdir().expect("fixture result");
    let adapter = DockerContainers::new(dir.path().join("missing"));
    let result = adapter
        .list(&[], CancellationToken::new())
        .await
        .expect("fixture result");
    assert!(!result.available);
    assert_eq!(result.reason, Some(ContainerUnavailableReason::CliMissing));
    assert!(result.items.is_empty());
}

/// A child shell writes the fake CLI. A write handle opened in this test process is copied into
/// any child another test forks while it is open, and stays open until that child starts its
/// program; running the script meanwhile fails with ETXTBSY ("Text file busy"), which `execute`
/// reports as `Unavailable`. CI saw that `Unavailable` once (the errno is not logged in tests).
fn fake(dir: &std::path::Path, body: &str) -> DockerContainers {
    let path = dir.join("docker");
    let written = std::process::Command::new("/bin/sh")
        .args([
            "-c",
            "umask 077; printf '%s' \"$2\" > \"$1\" && /bin/chmod 700 \"$1\"",
            "sh",
        ])
        .arg(&path)
        .arg(format!(
            "#!/bin/sh\ncd -- \"${{0%/*}}\" || exit 91\nprintf '%s\\n' \"$@\" > args\n{body}\n"
        ))
        .output()
        .expect("fixture result");
    assert!(
        written.status.success(),
        "fixture result: {:?} {}",
        written.status,
        String::from_utf8_lossy(&written.stderr)
    );
    DockerContainers::new(path)
}
fn request(action: ContainerAction) -> ContainerActionParams {
    ContainerActionParams {
        id: "owned-web_1.0".into(),
        action,
    }
}
#[tokio::test]
async fn containers_fake_argv_actions_and_stop_undo() {
    let dir = tempfile::tempdir().expect("fixture result");
    let adapter = fake(dir.path(), "exit 0");
    for action in [
        ContainerAction::Start,
        ContainerAction::Stop,
        ContainerAction::Restart,
    ] {
        let result = adapter
            .action(request(action), CancellationToken::new())
            .await
            .expect("fixture result");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("args")).expect("fixture result"),
            format!("{}\nowned-web_1.0\n", container_args("", action)[0])
        );
        assert_eq!(
            result.undo,
            (action == ContainerAction::Stop).then(|| request(ContainerAction::Start))
        );
    }
    let result = adapter
        .list(&[], CancellationToken::new())
        .await
        .expect("fixture result");
    assert!(result.available && result.reason.is_none() && result.items.is_empty());
    assert_eq!(
        std::fs::read_to_string(dir.path().join("args")).expect("fixture result"),
        "ps\n-a\n--format\njson\n"
    );
    assert_eq!(
        <RuntimeContainerAction as devbox_protocol::Method>::CONFIRM,
        devbox_protocol::Confirm::Reversible
    );
}
#[tokio::test]
async fn containers_fake_ndjson_unavailable_and_failure() {
    let dir = tempfile::tempdir().expect("fixture result");
    std::fs::write(dir.path().join("rows"), format!("{ROW}\n\n{COMPOSE_ROW}\n"))
        .expect("fixture result");
    let adapter = fake(dir.path(), "/bin/cat rows");
    let listed = adapter
        .list(&roots(), CancellationToken::new())
        .await
        .expect("fixture result");
    assert_eq!(listed.items.len(), 2);
    assert_eq!(listed.items[0].project_id, None);
    assert_eq!(listed.items[1].project_id.as_deref(), Some("app"));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("args")).expect("fixture result"),
        "ps\n-a\n--format\njson\n",
        "labels come from the one existing call"
    );
    let adapter = fake(dir.path(), "echo private-diagnostic >&2; exit 1");
    let result = adapter
        .list(&[], CancellationToken::new())
        .await
        .expect("fixture result");
    assert!(!result.available);
    assert_eq!(
        result.reason,
        Some(ContainerUnavailableReason::DaemonUnavailable)
    );
    let error = adapter
        .action(request(ContainerAction::Stop), CancellationToken::new())
        .await
        .expect_err("expected failure");
    assert!(matches!(
        error,
        devbox_protocol::Fail::Domain(ContainerError::Failed)
    ));
}
#[tokio::test]
async fn containers_invalid_id_never_spawns() {
    let dir = tempfile::tempdir().expect("fixture result");
    let adapter = fake(dir.path(), "exit 0");
    for id in ["", "-q", "--help", "a b", "a;echo", "a/b", "한글"] {
        let error = adapter
            .action(
                ContainerActionParams {
                    id: id.into(),
                    action: ContainerAction::Start,
                },
                CancellationToken::new(),
            )
            .await
            .expect_err("expected failure");
        assert!(matches!(
            error,
            devbox_protocol::Fail::Domain(ContainerError::InvalidId)
        ));
    }
    assert!(!dir.path().join("args").exists());
}
#[tokio::test]
async fn containers_malformed_and_output_bounds() {
    let dir = tempfile::tempdir().expect("fixture result");
    let adapter = fake(dir.path(), "/bin/cat rows");
    for bytes in [
        b"{oops}".to_vec(),
        vec![255],
        ROW.replace("abc", "--help").into_bytes(),
    ] {
        std::fs::write(dir.path().join("rows"), bytes).expect("fixture result");
        assert!(matches!(
            adapter.list(&[], CancellationToken::new()).await,
            Err(devbox_protocol::Fail::Domain(ContainerError::InvalidOutput))
        ));
    }
    std::fs::write(dir.path().join("rows"), vec![b' '; OUTPUT_LIMIT + 1]).expect("fixture result");
    assert!(matches!(
        adapter.list(&[], CancellationToken::new()).await,
        Err(devbox_protocol::Fail::Domain(ContainerError::OutputLimit))
    ));
    let adapter = fake(dir.path(), "exec /bin/cat rows >&2");
    assert!(matches!(
        adapter.list(&[], CancellationToken::new()).await,
        Err(devbox_protocol::Fail::Domain(ContainerError::OutputLimit))
    ));
}
async fn pid(dir: &std::path::Path) -> u32 {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            if let Ok(text) = std::fs::read_to_string(dir.join("pid"))
                && let Ok(pid) = text.trim().parse()
            {
                return pid;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("fixture result")
}
async fn reaped(pid: u32) {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while std::path::Path::new(&format!("/proc/{pid}")).exists() {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("owned fake CLI must be killed and reaped");
}
#[tokio::test]
async fn containers_timeouts_kill_and_reap_owned_child() {
    let dir = tempfile::tempdir().expect("fixture result");
    let mut adapter = fake(dir.path(), "echo $$ > pid; exec /bin/sleep 60");
    adapter.list_timeout = std::time::Duration::from_millis(100);
    adapter.action_timeout = std::time::Duration::from_millis(100);
    assert!(matches!(
        adapter.list(&[], CancellationToken::new()).await,
        Err(devbox_protocol::Fail::Domain(ContainerError::Timeout))
    ));
    reaped(pid(dir.path()).await).await;
    std::fs::remove_file(dir.path().join("pid")).expect("fixture result");
    assert!(matches!(
        adapter
            .action(request(ContainerAction::Stop), CancellationToken::new())
            .await,
        Err(devbox_protocol::Fail::Domain(ContainerError::Timeout))
    ));
    reaped(pid(dir.path()).await).await;
}
#[tokio::test]
async fn containers_cancellation_and_dropped_future_reap_owned_child() {
    for drop_future in [false, true] {
        let dir = tempfile::tempdir().expect("fixture result");
        let adapter = fake(dir.path(), "echo $$ > pid; exec /bin/sleep 60");
        let cancel = CancellationToken::new();
        let token = cancel.clone();
        let task = tokio::spawn(async move { adapter.list(&[], token).await });
        let owned = pid(dir.path()).await;
        if drop_future {
            task.abort();
            assert!(task.await.expect_err("expected failure").is_cancelled());
        } else {
            cancel.cancel();
            assert!(matches!(
                task.await.expect("fixture result"),
                Err(devbox_protocol::Fail::Domain(ContainerError::Cancelled))
            ));
        }
        reaped(owned).await;
    }
}

#[tokio::test]
async fn containers_output_limit_kills_and_reaps_owned_child() {
    for descriptor in [1, 2] {
        let dir = tempfile::tempdir().expect("fixture result");
        let adapter = fake(
            dir.path(),
            &format!(
                "echo $$ > pid; exec /usr/bin/python3 -c 'import os,time; os.write({descriptor}, b\"x\" * {}); time.sleep(60)'",
                OUTPUT_LIMIT + 1
            ),
        );
        assert!(matches!(
            adapter.list(&[], CancellationToken::new()).await,
            Err(devbox_protocol::Fail::Domain(ContainerError::OutputLimit))
        ));
        reaped(pid(dir.path()).await).await;
    }
}
