//! ExecStopPost fallback: preserve a wrapper result and report only this invocation.
use devbox_runtime::exit_record::{
    ExitRecord, ExitSource, ExitStore, Exited, now_ms, validate_identity,
};

#[derive(Debug, thiserror::Error)]
pub enum FallbackError {
    #[error("invalid systemd exit metadata")]
    InvalidMetadata,
}
pub async fn run(run: String, attempt: Option<String>) -> anyhow::Result<()> {
    validate_identity(&run)?;
    let explicit = crate::exec::attempt_id(attempt)?;
    let paths = crate::paths()?;
    let store = ExitStore::new(&paths.data_dir.join("runs"), &run)?;
    let attempt = explicit
        .or(store.current_attempt()?)
        .ok_or(devbox_runtime::exit_record::ExitError::NoAttempt)?;
    let record = from_environment(attempt.clone())?;
    let from_systemd = std::env::var("INVOCATION_ID").ok().as_deref() == Some(&attempt);
    let written = if from_systemd {
        store.finish_systemd(&record)?
    } else {
        store.finish(&record)?
    };
    if written {
        crate::exec::notify(
            &paths,
            Exited {
                run_id: run,
                attempt_id: attempt,
            },
        )
        .await;
    }
    Ok(())
}
fn from_environment(attempt_id: String) -> Result<ExitRecord, FallbackError> {
    let service = std::env::var("SERVICE_RESULT").ok();
    if service.as_deref().is_some_and(|s| {
        ![
            "success",
            "resources",
            "timeout",
            "exit-code",
            "signal",
            "core-dump",
            "watchdog",
            "start-limit-hit",
            "oom-kill",
            "exec-condition",
            "protocol",
        ]
        .contains(&s)
    }) {
        return Err(FallbackError::InvalidMetadata);
    }
    let status = std::env::var("EXIT_STATUS").ok();
    let (code, signal) = match std::env::var("EXIT_CODE").ok().as_deref() {
        Some("exited") => (
            Some(
                status
                    .and_then(|s| s.parse::<u8>().ok())
                    .ok_or(FallbackError::InvalidMetadata)? as i32,
            ),
            None,
        ),
        Some("killed" | "dumped") => {
            let raw = status.ok_or(FallbackError::InvalidMetadata)?;
            let signal = raw
                .parse::<i32>()
                .ok()
                .and_then(|n| nix::sys::signal::Signal::try_from(n).ok())
                .or_else(|| format!("SIG{}", raw.trim_start_matches("SIG")).parse().ok())
                .ok_or(FallbackError::InvalidMetadata)?;
            (None, Some(crate::exec::signal_name(signal as i32)))
        }
        None if status.is_none() => (None, None),
        _ => return Err(FallbackError::InvalidMetadata),
    };
    Ok(ExitRecord {
        attempt_id,
        code,
        signal,
        ended_ms: now_ms(),
        by: ExitSource::Systemd,
        service_result: service,
    })
}
