mod support;
use serde_json::{Value, json};
use std::time::{Duration, Instant};
use support::{TestDaemon, bounded};

#[tokio::test]
async fn one_byte_input_matches_pty_tmux_return_without_claiming_windows_sc10() {
    let mut d = TestDaemon::fixture();
    d.paths.ensure_dirs().expect("owned fixture");
    std::fs::write(
        d.paths.instance_config_dir.join("tmux.conf"),
        devbox_terminal::TMUX_CONF,
    )
    .expect("owned fixture");
    d.launch();
    let client = d.client().await;
    let program = "import os,tty; tty.setraw(0); os.write(1,b'READY')\nwhile True:\n b=os.read(0,1)\n if not b: break\n os.write(1,b'\\r'+b)";
    let command = format!("python3 -u -c {}", devbox_core::sh_quote(program));
    let terminal: Value = serde_json::from_str(
        &bounded(client.raw_call(
            "terminal.create",
            json!({"cwd":d.home.path(),"command":command}),
            None,
        ))
        .await
        .expect("owned fixture"),
    )
    .expect("owned fixture");
    let stream = bounded(client.open_stream(
        "terminal.attach",
        json!({"id":terminal["id"],"cols":120,"rows":40,"readOnly":false}),
    ))
    .await
    .expect("owned fixture");
    assert!(
        stream
            .read_until(b"READY", Duration::from_secs(2))
            .await
            .windows(5)
            .any(|w| w == b"READY")
    );
    let _ = stream.read_available(Duration::from_millis(100)).await;
    let mut samples = Vec::new();
    for i in 0..60 {
        let byte = b"qzwb"[i % 4];
        let started = Instant::now();
        stream.write(&[byte]).await.expect("owned fixture");
        let output = stream.read_until(&[byte], Duration::from_secs(1)).await;
        assert!(output.contains(&byte), "matching one-byte output deadline");
        if i >= 10 {
            samples.push(started.elapsed().as_secs_f64() * 1000.0);
        }
        stream
            .credit(output.len().try_into().expect("owned fixture"))
            .await
            .expect("owned fixture");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    samples.sort_by(f64::total_cmp);
    eprintln!(
        "SC10 Linux diagnostic only (daemon→PTY→tmux→return; excludes frontend/Tauri/WindowsWSL): n={} warmup=10 p50={:.3}ms p95={:.3}ms; full-path verdict requires app/acceptance",
        samples.len(),
        samples[24],
        samples[47]
    );
}
