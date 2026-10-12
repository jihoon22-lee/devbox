#[cfg(all(test, unix))]
mod tests {
    use super::*;
    fn command(script: &str) -> Command {
        let mut c = Command::new("sh");
        c.args(["-c", script]);
        c
    }
    #[tokio::test]
    async fn drains_both_streams_while_writing_input() {
        let out = run(
            command("head -c 131072 /dev/zero >&2; cat"),
            vec![42; 131072],
            Duration::from_secs(3),
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(out.stdout, vec![42; 131072]);
        assert_eq!(out.stderr.len(), 131072);
    }
    #[tokio::test]
    async fn deadline_cancellation_limit_and_exit_status_are_failures() {
        let c = CancellationToken::new();
        c.cancel();
        assert!(matches!(
            run(command("exec sleep 30"), vec![], Duration::from_secs(1), c).await,
            Err(ShellError::Cancelled)
        ));
        assert!(matches!(
            run(
                command("exec sleep 30"),
                vec![],
                Duration::from_millis(30),
                CancellationToken::new()
            )
            .await,
            Err(ShellError::Timeout)
        ));
        assert!(matches!(
            run(
                command("head -c 1048577 /dev/zero"),
                vec![],
                Duration::from_secs(3),
                CancellationToken::new()
            )
            .await,
            Err(ShellError::OutputLimit)
        ));
        assert!(matches!(
            run(
                command("echo failure >&2; exit 7"),
                vec![],
                Duration::from_secs(1),
                CancellationToken::new()
            )
            .await,
            Err(ShellError::Exit { .. })
        ));
    }
}

use std::{
    process::{Output, Stdio},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::Command,
};
use tokio_util::sync::CancellationToken;
#[derive(Debug, thiserror::Error)]
pub enum ShellError {
    #[error("작업을 취소했습니다")]
    Cancelled,
    #[cfg(windows)]
    #[error("WSL PID 1이 systemd가 아닙니다")]
    SystemdOff,
    #[error("WSL 응답 시간이 초과되었습니다")]
    Timeout,
    #[error("WSL 출력 제한을 초과했습니다")]
    OutputLimit,
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("WSL 명령 실패 ({code:?}): {stdout}\n{stderr}")]
    Exit {
        code: Option<i32>,
        stdout: String,
        stderr: String,
    },
    #[error("{0}")]
    Invalid(String),
}
pub const OUTPUT_LIMIT: usize = 1024 * 1024;
pub async fn read_bounded(mut reader: impl AsyncRead + Unpin) -> Result<Vec<u8>, ShellError> {
    let mut bytes = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = reader.read(&mut buf).await?;
        if n == 0 {
            return Ok(bytes);
        }
        if bytes.len() + n > OUTPUT_LIMIT {
            return Err(ShellError::OutputLimit);
        }
        bytes.extend_from_slice(&buf[..n]);
    }
}
/// All pipes are driven concurrently. Cancellation drops pipe futures, kills and reaps the child.
pub async fn run(
    mut command: Command,
    input: Vec<u8>,
    deadline: Duration,
    cancel: CancellationToken,
) -> Result<Output, ShellError> {
    if cancel.is_cancelled() {
        return Err(ShellError::Cancelled);
    }
    let mut child = command
        .kill_on_drop(true)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    #[cfg(windows)]
    let _job = match crate::platform::job::Job::for_child(&child) {
        Ok(job) => job,
        Err(e) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(e.into());
        }
    };
    let operation = async {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| ShellError::Invalid("stdin 없음".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ShellError::Invalid("stdout 없음".into()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| ShellError::Invalid("stderr 없음".into()))?;
        let write = async {
            stdin.write_all(&input).await?;
            stdin.shutdown().await?;
            drop(stdin);
            Ok::<_, ShellError>(())
        };
        let wait = async { Ok::<_, ShellError>(child.wait().await?) };
        let (_, stdout, stderr, status) =
            tokio::try_join!(write, read_bounded(stdout), read_bounded(stderr), wait)?;
        Ok::<_, ShellError>(Output {
            status,
            stdout,
            stderr,
        })
    };
    let result = tokio::select! {biased;_=cancel.cancelled()=>Err(ShellError::Cancelled),r=tokio::time::timeout(deadline,operation)=>r.unwrap_or(Err(ShellError::Timeout))};
    match result {
        Ok(out) if out.status.success() => Ok(out),
        Ok(out) => Err(ShellError::Exit {
            code: out.status.code(),
            stdout: super::wsl::decode_output(&out.stdout),
            stderr: super::wsl::decode_output(&out.stderr),
        }),
        Err(error) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            Err(error)
        }
    }
}
