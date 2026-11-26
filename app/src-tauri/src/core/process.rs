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
    async fn answering(script: &str, input: &[u8]) -> (Result<Output, ShellError>, Vec<u8>) {
        let seen = std::sync::Mutex::new(None);
        let result = run_answering(
            command(script),
            Duration::from_secs(3),
            CancellationToken::new(),
            |line| {
                *seen.lock().unwrap() = Some(line.to_vec());
                (line == b"send").then(|| input.to_vec())
            },
        )
        .await;
        let line = seen.lock().unwrap().take().expect("answer was asked");
        (result, line)
    }
    #[tokio::test]
    async fn input_is_sent_only_after_the_command_asks_for_it() {
        // Larger than a pipe buffer in both directions, and stderr is drained meanwhile.
        let input = vec![7; 1024 * 1024];
        let (out, line) = answering(
            "printf 'send\\n'; head -c 131072 /dev/zero >&2; cat | wc -c",
            &input,
        )
        .await;
        let out = out.unwrap();
        assert_eq!(line, b"send");
        assert_eq!(out.stdout, b"send\n1048576\n");
        assert_eq!(out.stderr.len(), 131072);
    }
    #[tokio::test]
    async fn any_other_answer_closes_stdin_at_once_without_writing() {
        // `cat` returns only because stdin is closed; nothing was written to it.
        for (script, line, stdout) in [
            ("printf 'kept\\n'; cat | wc -c", "kept", "kept\n0\n"),
            ("printf 'kept\\r\\n'; cat | wc -c", "kept", "kept\r\n0\n"),
            (
                "printf 'send it\\nsend\\n'; cat | wc -c",
                "send it",
                "send it\nsend\n0\n",
            ),
            ("printf 'sen'", "sen", "sen"),
            ("exit 0", "", ""),
        ] {
            let (out, seen) = answering(script, b"must not be sent").await;
            assert_eq!(seen, line.as_bytes(), "{script}");
            assert_eq!(out.unwrap().stdout, stdout.as_bytes(), "{script}");
        }
    }
    #[tokio::test]
    async fn a_failure_before_or_after_the_answer_is_still_a_failure() {
        let (out, line) = answering("exit 63", b"x").await;
        assert!(matches!(out, Err(ShellError::Exit { code: Some(63), .. })));
        assert_eq!(line, b"");
        let (out, line) = answering("printf 'send\\n'; cat >/dev/null; exit 65", b"x").await;
        assert!(matches!(out, Err(ShellError::Exit { code: Some(65), .. })));
        assert_eq!(line, b"send");
        let cancelled = CancellationToken::new();
        cancelled.cancel();
        assert!(matches!(
            run_answering(
                command("exec sleep 30"),
                Duration::from_secs(1),
                cancelled,
                |_| None
            )
            .await,
            Err(ShellError::Cancelled)
        ));
        // A command that never answers and never reads is bounded by the deadline.
        assert!(matches!(
            run_answering(
                command("exec sleep 30"),
                Duration::from_millis(30),
                CancellationToken::new(),
                |_| None
            )
            .await,
            Err(ShellError::Timeout)
        ));
    }
    #[test]
    fn the_first_line_ends_at_the_first_line_end() {
        assert_eq!(first_line(b"kept\nmore\n"), b"kept");
        assert_eq!(first_line(b"kept\r\n"), b"kept");
        assert_eq!(first_line(b"kept"), b"kept");
        assert_eq!(first_line(b"\nkept\n"), b"");
        assert_eq!(first_line(b""), b"");
    }
}

use std::{
    process::{Output, Stdio},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::{ChildStdin, ChildStdout, Command},
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
    command: Command,
    input: Vec<u8>,
    deadline: Duration,
    cancel: CancellationToken,
) -> Result<Output, ShellError> {
    drive(command, deadline, cancel, |mut stdin, stdout| async move {
        let write = async {
            stdin.write_all(&input).await?;
            stdin.shutdown().await?;
            drop(stdin);
            Ok::<_, ShellError>(())
        };
        let (_, stdout) = tokio::try_join!(write, read_bounded(stdout))?;
        Ok(stdout)
    })
    .await
}
/// Like [`run`], for a command that first says on stdout whether it wants input. `answer` gets
/// the first line (without its line end; empty when the output ended before one) and returns the
/// input to send; `None` closes stdin at once without writing. The output keeps that line.
pub async fn run_answering<A>(
    command: Command,
    deadline: Duration,
    cancel: CancellationToken,
    answer: A,
) -> Result<Output, ShellError>
where
    A: FnOnce(&[u8]) -> Option<Vec<u8>>,
{
    drive(
        command,
        deadline,
        cancel,
        |mut stdin, mut stdout| async move {
            let mut bytes = Vec::new();
            let mut buf = [0u8; 8192];
            while !bytes.contains(&b'\n') {
                let n = stdout.read(&mut buf).await?;
                if n == 0 {
                    break;
                }
                if bytes.len() + n > OUTPUT_LIMIT {
                    return Err(ShellError::OutputLimit);
                }
                bytes.extend_from_slice(&buf[..n]);
            }
            let input = answer(first_line(&bytes));
            let write = async {
                if let Some(input) = input {
                    stdin.write_all(&input).await?;
                    stdin.shutdown().await?;
                }
                drop(stdin);
                Ok::<_, ShellError>(())
            };
            let (_, rest) = tokio::try_join!(write, read_bounded(stdout))?;
            if bytes.len() + rest.len() > OUTPUT_LIMIT {
                return Err(ShellError::OutputLimit);
            }
            bytes.extend_from_slice(&rest);
            Ok(bytes)
        },
    )
    .await
}
/// The bytes before the first line end, or all of them when there is none.
pub fn first_line(bytes: &[u8]) -> &[u8] {
    let line = bytes
        .split(|byte| *byte == b'\n')
        .next()
        .unwrap_or_default();
    line.strip_suffix(b"\r").unwrap_or(line)
}
/// `exchange` owns stdin and stdout and returns the collected stdout.
async fn drive<E, F>(
    mut command: Command,
    deadline: Duration,
    cancel: CancellationToken,
    exchange: E,
) -> Result<Output, ShellError>
where
    E: FnOnce(ChildStdin, ChildStdout) -> F,
    F: Future<Output = Result<Vec<u8>, ShellError>>,
{
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
        let stdin = child
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
        let wait = async { Ok::<_, ShellError>(child.wait().await?) };
        let (stdout, stderr, status) =
            tokio::try_join!(exchange(stdin, stdout), read_bounded(stderr), wait)?;
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
