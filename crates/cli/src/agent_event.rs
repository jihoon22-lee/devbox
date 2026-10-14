//! Tool hooks are best-effort: failures must not block the invoking tool.
use devbox_agents::{hooks::HookSource, model::AgentEventParams};
use devbox_protocol::message::ClientKind;
use std::io::Read;
use std::time::Duration;

const MAX_PAYLOAD: usize = 1024 * 1024;

pub async fn run(source: HookSource, code: Option<i32>, payload: Option<String>) {
    let Ok(agent_id) = std::env::var("DEVBOX_AGENT_ID") else {
        return;
    };
    if agent_id.is_empty() {
        return;
    }
    let operation = async {
        let payload = if matches!(source, HookSource::Setup | HookSource::Exit) {
            std::env::var("DEVBOX_AGENT_LAUNCH").unwrap_or_default()
        } else if matches!(source, HookSource::Claude) {
            let (tx, rx) = tokio::sync::oneshot::channel();
            // A blocked stdin reader must not keep Tokio's blocking pool alive on exit.
            std::thread::Builder::new()
                .name("agent-hook-input".into())
                .spawn(move || {
                    let mut input = String::new();
                    let result = std::io::stdin()
                        .take((MAX_PAYLOAD + 1) as u64)
                        .read_to_string(&mut input)
                        .map(|_| input);
                    let _ = tx.send(result);
                })?;
            rx.await??
        } else {
            payload.unwrap_or_default()
        };
        anyhow::ensure!(payload.len() <= MAX_PAYLOAD, "hook payload limit exceeded");
        let paths = crate::paths()?;
        let client =
            crate::client::Client::connect(&paths.socket, ClientKind::Cli, paths.instance.as_str())
                .await?;
        let params = AgentEventParams {
            agent_id,
            launch_id: std::env::var("DEVBOX_AGENT_LAUNCH").ok(),
            source,
            payload,
            code,
        };
        client
            .raw_call("agents.event", serde_json::to_value(params)?, None)
            .await?;
        Ok::<_, anyhow::Error>(())
    };
    // Include stdin, connection setup, and RPC in one deadline; do not print hook payloads.
    let _ = tokio::time::timeout(Duration::from_secs(3), operation).await;
}
