#![cfg(unix)]
mod support;
use devbox_lsp::{Language, LspError, OpenLsp, ProjectContext, adapters::CaptureAccess};
use serde_json::json;
use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;
use support::*;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

// These tests share the process-wide four-worker admission budget.
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
struct ReleaseWorkers(Arc<(Mutex<bool>, Condvar)>);
impl ReleaseWorkers {
    fn release(&self) {
        let (lock, wake) = &*self.0;
        *lock.lock().expect("worker gate") = true;
        wake.notify_all();
    }
}
impl Drop for ReleaseWorkers {
    fn drop(&mut self) {
        self.release();
    }
}
fn held_capture() -> (
    CaptureAccess,
    ReleaseWorkers,
    mpsc::UnboundedReceiver<CancellationToken>,
    Arc<AtomicUsize>,
) {
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let release = ReleaseWorkers(gate.clone());
    let exited = Arc::new(AtomicUsize::new(0));
    let completed = exited.clone();
    let (started, starts) = mpsc::unbounded_channel();
    let capture: CaptureAccess = Arc::new(move |_priority, cancel| {
        let gate = gate.clone();
        let started = started.clone();
        let completed = completed.clone();
        Box::pin(async move {
            // A real blocking worker deliberately delays its cooperative exit.
            // Dropping its async join must not free daemon capture admission.
            tokio::task::spawn_blocking(move || {
                started.send(cancel).expect("observe worker start");
                let (lock, wake) = &*gate;
                let mut released = lock.lock().expect("worker gate");
                while !*released {
                    released = wake.wait(released).expect("worker gate wait");
                }
                completed.fetch_add(1, Ordering::SeqCst);
                Err(LspError::LimitExceeded)
            })
            .await
            .expect("owned blocking worker")
        })
    });
    (capture, release, starts, exited)
}
async fn connect(fx: &Fixture, project: ProjectContext) -> (Client, u64) {
    let stream = fx
        .service
        .open(
            fx.ctx(),
            OpenLsp {
                project_id: "p".into(),
                language: Language::Rust,
            },
            project,
        )
        .await
        .expect("open owned LSP");
    let mut client = fx.client(stream.stream_id);
    client
        .send(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}))
        .await;
    let pid = client.reply().await["result"]["pid"]
        .as_u64()
        .expect("owned server pid");
    (client, pid)
}
async fn rename(client: &Client, id: i64) {
    client.send(json!({"jsonrpc":"2.0","id":id,"method":"textDocument/rename","params":{"textDocument":{"uri":"file:///owned/main.rs"},"position":{"line":0,"character":0},"newName":"next"}})).await;
}
async fn entered(starts: &mut mpsc::UnboundedReceiver<CancellationToken>) -> CancellationToken {
    tokio::time::timeout(Duration::from_secs(2), starts.recv())
        .await
        .expect("worker entered")
        .expect("worker token")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actor_remains_responsive_and_cancelled_workers_hold_admission_until_actual_exit() {
    let _serial = SERIAL.lock().await;
    let fx = Fixture::new();
    let (capture, workers, mut starts, exited) = held_capture();
    let mut project = fx.project();
    project.capture = capture;
    let mut clients = Vec::new();
    let mut pid = 0;
    for _ in 0..5 {
        let (client, child) = connect(&fx, project.clone()).await;
        pid = child;
        clients.push(client);
    }
    let mut tokens = Vec::new();
    for (i, client) in clients.iter().take(4).enumerate() {
        rename(client, 10 + i as i64).await;
        tokens.push(entered(&mut starts).await);
    }
    clients[4]
        .send(json!({"jsonrpc":"2.0","id":90,"method":"test/echo","params":"responsive"}))
        .await;
    let echo = tokio::time::timeout(Duration::from_secs(1), clients[4].reply())
        .await
        .expect("actor cannot await capture inline");
    assert_eq!(echo["result"], "responsive");
    rename(&clients[4], 91).await;
    assert_eq!(clients[4].reply().await["error"]["code"], -32801);
    clients[0]
        .send(json!({"jsonrpc":"2.0","method":"$/cancelRequest","params":{"id":10}}))
        .await;
    tokio::time::timeout(Duration::from_secs(1), tokens[0].cancelled())
        .await
        .expect("cancel reached worker");
    assert_eq!(exited.load(Ordering::SeqCst), 0);
    rename(&clients[0], 92).await;
    assert_eq!(
        clients[0].reply().await["error"]["code"],
        -32801,
        "same client remains accounted"
    );
    rename(&clients[4], 93).await;
    assert_eq!(
        clients[4].reply().await["error"]["code"],
        -32801,
        "cancel must not free global permit before worker exit"
    );
    assert!(
        starts.try_recv().is_err(),
        "no fifth blocking worker launched"
    );

    let shutdown = fx.service.shutdown();
    tokio::pin!(shutdown);
    // Poll shutdown until its owned fake process is gone, isolating the worker
    // wait from normal child process cleanup latency.
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            tokio::select! {
                _ = &mut shutdown => panic!("shutdown returned while blocking workers remained"),
                _ = tokio::time::sleep(Duration::from_millis(10)) => {}
            }
            if !std::path::Path::new(&format!("/proc/{pid}")).exists() {
                break;
            }
        }
    })
    .await
    .expect("owned LSP process stopped");
    assert!(
        tokio::time::timeout(Duration::from_millis(100), &mut shutdown)
            .await
            .is_err()
    );
    assert_eq!(exited.load(Ordering::SeqCst), 0);
    workers.release();
    tokio::time::timeout(Duration::from_secs(2), shutdown)
        .await
        .expect("shutdown awaits actual worker exit");
    assert_eq!(exited.load(Ordering::SeqCst), 4);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn restart_cancels_old_capture_without_stalling_a_new_server_generation() {
    let _serial = SERIAL.lock().await;
    let fx = Fixture::new();
    let (capture, workers, mut starts, exited) = held_capture();
    let mut project = fx.project();
    project.capture = capture;
    let (old, old_pid) = connect(&fx, project.clone()).await;
    let generation = fx.service.servers("p").await.items[0].generation;
    rename(&old, 10).await;
    let token = entered(&mut starts).await;
    fx.service
        .restart(
            OpenLsp {
                project_id: "p".into(),
                language: Language::Rust,
            },
            project.clone(),
        )
        .await
        .expect("restart responsive with pending capture");
    assert!(token.is_cancelled());
    assert!(old.closed.is_cancelled());
    assert_eq!(exited.load(Ordering::SeqCst), 0);
    let (mut fresh, new_pid) = connect(&fx, project).await;
    assert_ne!(new_pid, old_pid);
    assert!(fx.service.servers("p").await.items[0].generation > generation);
    fresh
        .send(json!({"jsonrpc":"2.0","id":20,"method":"test/echo","params":"new generation"}))
        .await;
    assert_eq!(fresh.reply().await["result"], "new generation");
    workers.release();
    fresh.close();
    fx.service.shutdown().await;
    assert_eq!(exited.load(Ordering::SeqCst), 1);
}
