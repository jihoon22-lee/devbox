//! Tool hooks are best-effort: failures must not block the invoking tool.
use devbox_agents::{hooks::HookSource, model::AgentEventParams};
use devbox_protocol::message::ClientKind;
use std::io::Read;
use std::time::Duration;

const MAX_PAYLOAD: usize = 1024 * 1024;

/// What an Antigravity hook command must print: agy reads a hook's stdout as JSON, and `{}` lets
/// the agent go on.
pub const ANTIGRAVITY_REPLY: &str = "{}";

/// Reads the hook's stdin, at most one byte past [`MAX_PAYLOAD`].
async fn read_input() -> std::io::Result<String> {
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
    rx.await
        .unwrap_or_else(|_| Err(std::io::Error::other("hook input reader ended")))
}

/// The payload a hook of `source` reports, from `read`, what it read on stdin. Any hook of an
/// Antigravity launch tells the daemon that agy got an instruction, so one whose payload cannot be
/// read or is over the limit (a long first prompt in `lastUserInput`, cut inside a character)
/// still reports, with an empty payload: the daemon then only cancels a first instruction still
/// queued for the launch. Claude's hook reports nothing then.
fn reported(source: HookSource, read: std::io::Result<String>) -> anyhow::Result<String> {
    match (source, read) {
        (HookSource::Antigravity, Ok(payload)) if payload.len() <= MAX_PAYLOAD => Ok(payload),
        (HookSource::Antigravity, _) => Ok(String::new()),
        (_, read) => {
            let payload = read?;
            anyhow::ensure!(payload.len() <= MAX_PAYLOAD, "hook payload limit exceeded");
            Ok(payload)
        }
    }
}

pub async fn run(source: HookSource, code: Option<i32>, payload: Option<String>) {
    // Before anything can fail or wait: agy holds the agent until the hook has answered and
    // ended, whether or not the report reaches the daemon.
    if matches!(source, HookSource::Antigravity) {
        use std::io::Write;
        let mut out = std::io::stdout();
        let _ = writeln!(out, "{ANTIGRAVITY_REPLY}").and_then(|()| out.flush());
    }
    let Ok(agent_id) = std::env::var("DEVBOX_AGENT_ID") else {
        return;
    };
    if agent_id.is_empty() {
        return;
    }
    let operation = async {
        let payload = if matches!(source, HookSource::Setup | HookSource::Exit) {
            std::env::var("DEVBOX_AGENT_LAUNCH").unwrap_or_default()
        } else if matches!(source, HookSource::Claude | HookSource::Antigravity) {
            reported(source, read_input().await)?
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_antigravity_hook_reports_a_payload_it_cannot_read_or_that_is_too_long_as_empty() {
        let invalid = || std::io::Error::from(std::io::ErrorKind::InvalidData);
        // A long first prompt in `lastUserInput`: over the limit, or cut inside a character.
        let long = format!(
            "{{\"lastUserInput\":\"{}\"}}",
            "가".repeat(MAX_PAYLOAD / 3 + 1)
        );
        assert!(long.len() > MAX_PAYLOAD);
        let read = |source, input: std::io::Result<String>| reported(source, input).ok();
        assert_eq!(
            read(HookSource::Antigravity, Ok("{}".into())).as_deref(),
            Some("{}")
        );
        assert_eq!(
            read(HookSource::Antigravity, Ok(long.clone())).as_deref(),
            Some("")
        );
        assert_eq!(
            read(HookSource::Antigravity, Err(invalid())).as_deref(),
            Some("")
        );
        // Claude's hook reports nothing then, as before.
        assert_eq!(read(HookSource::Claude, Ok(long)), None);
        assert_eq!(read(HookSource::Claude, Err(invalid())), None);
        assert_eq!(
            read(HookSource::Claude, Ok("{}".into())).as_deref(),
            Some("{}")
        );
    }
}
