use code_pad_lib::lsp::{
    IncomingMessage, JsonRpcMessage, LspProcess, ProcessError, ProcessSpec, ProcessState,
    RequestCancellation, RequestError,
};
use serde_json::json;
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn fixture(mode: &str) -> ProcessSpec {
    let test_binary = std::env::current_exe()
        .expect("test executable path")
        .parent()
        .and_then(|deps| deps.parent())
        .expect("Cargo test executable should live below target/debug/deps")
        .join(if cfg!(windows) {
            "fake-lsp-server.exe"
        } else {
            "fake-lsp-server"
        });
    ProcessSpec::new(test_binary, PathBuf::from(env!("CARGO_MANIFEST_DIR")))
        .arg("--argv-safe; echo should-never-run")
        .env("FAKE_LSP_MODE", mode)
}

async fn receive_method(
    receiver: &mut tokio::sync::broadcast::Receiver<IncomingMessage>,
    expected: &str,
) -> IncomingMessage {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match receiver.recv().await.expect("process event stream closed") {
                ref event @ IncomingMessage::Message(JsonRpcMessage::Notification {
                    ref method,
                    ..
                }) if method == expected => return event.clone(),
                IncomingMessage::ProtocolError(error) => panic!("protocol error: {error}"),
                _ => {}
            }
        }
    })
    .await
    .expect("timed out waiting for fake server notification")
}

async fn stop(process: &LspProcess) {
    process
        .shutdown()
        .await
        .expect("fake server should stop cleanly");
    assert!(matches!(process.state().await, ProcessState::Exited { .. }));
}

#[tokio::test]
async fn fake_server_routes_initialize_documents_and_diagnostics() {
    let process = LspProcess::spawn(fixture("")).await.unwrap();
    let mut events = process.subscribe();
    let initialized = process
        .request(
            "initialize",
            Some(json!({ "rootUri": null })),
            Duration::from_secs(1),
        )
        .await
        .unwrap();
    assert_eq!(initialized["capabilities"]["completionProvider"], true);
    assert_eq!(initialized["argv"][0], "--argv-safe; echo should-never-run");

    process
        .notify(
            "textDocument/didOpen",
            Some(json!({
                "textDocument": { "uri": "file:///fixture.rs", "version": 1, "text": "fn main() {}" }
            })),
        )
        .await
        .unwrap();
    let event = receive_method(&mut events, "textDocument/publishDiagnostics").await;
    assert!(matches!(
        event,
        IncomingMessage::Message(JsonRpcMessage::Notification { params: Some(params), .. })
            if params["uri"] == "file:///fixture.rs"
    ));

    process
        .notify(
            "textDocument/didChange",
            Some(json!({ "textDocument": { "uri": "file:///fixture.rs", "version": 2 } })),
        )
        .await
        .unwrap();
    let _ = receive_method(&mut events, "textDocument/publishDiagnostics").await;
    stop(&process).await;
}

#[tokio::test]
async fn responses_are_routed_by_id_and_server_errors_are_preserved() {
    let process = LspProcess::spawn(fixture("")).await.unwrap();
    let first = process.clone();
    let second = process.clone();
    let (first, second) = tokio::join!(
        first.request("first", None, Duration::from_secs(1)),
        second.request("second", None, Duration::from_secs(1)),
    );
    assert_eq!(first.unwrap(), json!("first"));
    assert_eq!(second.unwrap(), json!("second"));

    let error = process
        .request("fail", None, Duration::from_secs(1))
        .await
        .unwrap_err();
    assert!(matches!(error, RequestError::Remote(error) if error.code == -32001));
    assert_eq!(process.pending_len().await, 0);
    stop(&process).await;
}

#[tokio::test]
async fn unknown_response_is_routed_without_completing_another_request() {
    let process = LspProcess::spawn(fixture("")).await.unwrap();
    let mut events = process.subscribe();
    assert_eq!(
        process
            .request("emitUnknown", None, Duration::from_secs(1))
            .await
            .unwrap(),
        json!("done")
    );
    let event = tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            match events.recv().await.unwrap() {
                IncomingMessage::UnknownResponse(JsonRpcMessage::Response { id, result })
                    if id.as_u64() == Some(999_999) =>
                {
                    return result
                }
                IncomingMessage::ProtocolError(error) => panic!("protocol error: {error}"),
                _ => {}
            }
        }
    })
    .await
    .expect("unknown response should be observable");
    assert_eq!(event, json!("unknown"));
    stop(&process).await;
}

#[tokio::test]
async fn timeout_and_explicit_cancellation_remove_pending_and_send_cancel() {
    let process = LspProcess::spawn(fixture("")).await.unwrap();
    let timeout = process
        .request("slow", None, Duration::from_millis(30))
        .await
        .unwrap_err();
    assert!(matches!(timeout, RequestError::Timeout));
    assert_eq!(process.pending_len().await, 0);

    let cancellation = RequestCancellation::new();
    let request_process = process.clone();
    let request_cancellation = cancellation.clone();
    let request = tokio::spawn(async move {
        request_process
            .request_with_cancel("slow", None, Duration::from_secs(2), request_cancellation)
            .await
    });
    tokio::time::sleep(Duration::from_millis(30)).await;
    cancellation.cancel();
    assert!(matches!(
        request.await.unwrap(),
        Err(RequestError::Cancelled)
    ));
    assert_eq!(process.pending_len().await, 0);
    stop(&process).await;
}

#[tokio::test]
async fn stderr_is_bounded_and_crash_completes_pending_requests() {
    let process = LspProcess::spawn_with_options(fixture("stderr"), Default::default(), 1024)
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    let stderr = process.stderr().await;
    assert_eq!(stderr.len(), 1024);
    assert!(stderr.truncated());
    assert_eq!(stderr.dropped_bytes(), (100 * 1024 - 1024) as u64);
    stop(&process).await;

    let process = LspProcess::spawn(fixture("")).await.unwrap();
    let error = process
        .request("crash", None, Duration::from_secs(1))
        .await
        .unwrap_err();
    assert!(matches!(error, RequestError::Disconnected));
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if matches!(
                process.state().await,
                ProcessState::Exited { code: Some(17) }
            ) {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("crashed fake server should report its exit code");
}

#[tokio::test]
async fn shutdown_sends_exit_then_kills_a_hung_server() {
    let process = LspProcess::spawn(fixture("hang_shutdown")).await.unwrap();
    let started = Instant::now();
    let result = process
        .shutdown_with_timeout(Duration::from_millis(120))
        .await;
    assert!(matches!(result, Err(ProcessError::ShutdownTimeout)));
    assert!(started.elapsed() < Duration::from_secs(2));
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if matches!(process.state().await, ProcessState::Exited { .. }) {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("hung fake server should be force-killed");
}

#[cfg(windows)]
mod windows_ownership {
    use super::*;
    use code_pad_lib::lsp::{
        ResolvedProcess, ResolvedRuntime, RuntimeError, RuntimeKind, RuntimeResolver,
    };
    use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT};
    use windows::Win32::System::Threading::{
        OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE,
    };

    struct OwnedProcess(HANDLE);
    impl OwnedProcess {
        async fn from_marker(marker: &std::path::Path) -> Self {
            let pid = tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    if let Ok(text) = std::fs::read_to_string(marker) {
                        if let Ok(pid) = text.parse::<u32>() {
                            break pid;
                        }
                    }
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            })
            .await
            .expect("fixture did not publish its descendant PID");
            Self(unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) }.unwrap())
        }
        fn assert_running(&self) {
            assert_eq!(unsafe { WaitForSingleObject(self.0, 0) }, WAIT_TIMEOUT);
        }
        fn assert_exited(&self) {
            assert_eq!(unsafe { WaitForSingleObject(self.0, 0) }, WAIT_OBJECT_0);
        }
    }
    impl Drop for OwnedProcess {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    #[tokio::test]
    async fn server_exit_and_forced_stop_confirm_owned_descendant_exit() {
        for mode in ["", "hang_shutdown"] {
            let root = tempfile::tempdir().unwrap();
            let marker = root.path().join("descendant.pid");
            let process = LspProcess::spawn(
                fixture(mode).arg(format!("--fake-descendant-marker={}", marker.display())),
            )
            .await
            .unwrap();
            let descendant = OwnedProcess::from_marker(&marker).await;
            descendant.assert_running();
            assert!(!process.wait_for_exit(Duration::ZERO).await);
            if mode.is_empty() {
                process.shutdown().await.unwrap();
            } else {
                assert!(matches!(
                    process
                        .shutdown_with_timeout(Duration::from_millis(120))
                        .await,
                    Err(ProcessError::ShutdownTimeout)
                ));
            }
            assert!(process.wait_for_exit(Duration::from_secs(2)).await);
            descendant.assert_exited();
        }
    }

    #[tokio::test]
    async fn version_probe_owns_descendants_on_success_error_timeout_and_cancel() {
        for behavior in [
            "success",
            "output-limit",
            "failure",
            "hang",
            "cancel",
            "cancel-result",
        ] {
            let root = tempfile::tempdir().unwrap();
            std::fs::write(root.path().join("probe-mode"), behavior).unwrap();
            let executable = fixture("").executable;
            let resolved = ResolvedProcess {
                executable: executable.clone(),
                args: vec![],
                current_dir: root.path().to_owned(),
                env: Default::default(),
                runtime: Some(ResolvedRuntime {
                    kind: RuntimeKind::Node,
                    executable,
                    version_requirement: None,
                }),
            };
            let resolver = RuntimeResolver::new();
            let cancellation = RequestCancellation::new();
            let mut probe =
                Box::pin(resolver.probe_managed_runtime_cancellable(&resolved, &cancellation));
            let marker = root.path().join("descendant.pid");
            let descendant = tokio::select! {
                result = &mut probe => panic!("probe completed before fixture release: {result:?}"),
                descendant = OwnedProcess::from_marker(&marker) => descendant,
            };
            descendant.assert_running();
            if behavior == "cancel-result" {
                cancellation.cancel();
                assert!(matches!(
                    probe.await,
                    Err(RuntimeError::RuntimeProbeCancelled)
                ));
                descendant.assert_exited();
                continue;
            }
            if behavior == "cancel" {
                // Dropping the future must close its Job even without a result.
                drop(probe);
                tokio::time::timeout(Duration::from_secs(2), async {
                    while unsafe { WaitForSingleObject(descendant.0, 0) } == WAIT_TIMEOUT {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                })
                .await
                .unwrap();
                descendant.assert_exited();
                continue;
            }
            std::fs::write(root.path().join("probe-release"), b"release").unwrap();
            let result = probe.await;
            match behavior {
                "success" => result.unwrap(),
                "output-limit" => {
                    assert!(matches!(result, Err(RuntimeError::RuntimeProbeOutputLimit)))
                }
                "hang" => assert!(matches!(result, Err(RuntimeError::RuntimeProbeTimeout))),
                _ => assert!(matches!(result, Err(RuntimeError::RuntimeProbeFailed))),
            }
            descendant.assert_exited();
        }
    }
}
