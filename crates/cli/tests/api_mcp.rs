//! 실제 데몬 `api.mcp_connect`(S5-5): `devbox mcp` 자신을 stdio 대상으로 initialize → tools/list,
//! 스트림을 닫으면 자식이 끝난다. 자식은 데몬의 임시 HOME과 고유 `DEVBOX_INSTANCE`만 보므로 사용자 데몬에 닿지 않는다.
mod support;
use std::time::Duration;
use support::{TestDaemon, bounded};

const T: Duration = Duration::from_secs(10);

/// 환경에 `DEVBOX_INSTANCE=<instance>`가 있는 이 사용자의 프로세스.
fn children(instance: &str) -> Vec<u32> {
    let needle = format!("DEVBOX_INSTANCE={instance}");
    std::fs::read_dir("/proc")
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok())
        .filter(|pid| {
            std::fs::read(format!("/proc/{pid}/environ"))
                .is_ok_and(|env| env.split(|b| *b == 0).any(|kv| kv == needle.as_bytes()))
                && std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|s| {
                    s.rsplit_once(')')
                        .is_some_and(|(_, r)| !r.trim_start().starts_with('Z'))
                })
        })
        .collect()
}

#[tokio::test]
async fn mcp_stdio_lists_devbox_tools() {
    let daemon = TestDaemon::start();
    let client = daemon.client().await;
    let instance = format!("test-{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
    // stdio 초안의 cwd는 등록 프로젝트 안이어야 한다(데몬 임시 HOME 아래 소유 폴더).
    let work = daemon.home.path().join("projects/mcp-work");
    std::fs::create_dir_all(&work).expect("work dir");
    bounded(client.raw_call(
        "projects.add",
        serde_json::json!({ "path": work.display().to_string() }),
        Some("mcp-work".into()),
    ))
    .await
    .expect("register work project");
    let stream = bounded(client.open_stream(
        "api.mcp_connect",
        serde_json::json!({
            "transport": {
                "kind": "stdio",
                "command": env!("CARGO_BIN_EXE_devbox"),
                "args": ["mcp"],
                "cwd": work.display().to_string(),
                "env": [{"name": "DEVBOX_INSTANCE", "value": instance}]
            },
            "env": {"variables": []},
            "source": {"kind": "draft"}
        }),
    ))
    .await
    .expect("open api.mcp_connect");
    let ready = String::from_utf8_lossy(&stream.read_until(b"\"ready\"", T).await).into_owned();
    assert!(ready.contains("\"ready\""), "{ready}");
    assert_eq!(children(&instance).len(), 1, "one devbox mcp child");
    stream
        .write(b"{\"type\":\"list\",\"id\":\"t\",\"what\":\"tools\",\"cursor\":null}\n")
        .await
        .expect("stream input");
    let got = String::from_utf8_lossy(&stream.read_until(b"devbox_projects", T).await).into_owned();
    assert!(
        got.contains("\"result\"") && got.contains("devbox_projects"),
        "{got}"
    );
    stream.close().await.expect("close stream");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    while !children(&instance).is_empty() && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        children(&instance).is_empty(),
        "closing the stream ends the child"
    );
}

#[tokio::test]
async fn mcp_stdio_env_cannot_override_the_runtime_environment() {
    let daemon = TestDaemon::start();
    let client = daemon.client().await;
    let rejected = bounded(client.raw_call(
        "api.mcp_connect",
        serde_json::json!({
            "transport": {
                "kind": "stdio",
                "command": env!("CARGO_BIN_EXE_devbox"),
                "args": ["mcp"],
                "cwd": null,
                "env": [{"name": "HOME", "value": "/"}]
            },
            "env": {"variables": []},
            "source": {"kind": "draft"}
        }),
        None,
    ))
    .await;
    match rejected {
        Err(devbox_cli::client::ClientError::Rpc { code, detail, .. }) => {
            assert_eq!(code, "api.invalid_input");
            assert_eq!(
                detail.expect("detail").get(),
                r#"{"field":"env","reason":"reserved"}"#
            );
        }
        other => panic!("reserved env must be an RPC error: {other:?}"),
    }
}
