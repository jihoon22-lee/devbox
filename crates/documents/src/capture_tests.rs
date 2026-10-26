use super::*;
use crate::fixture::Fx;
use sha2::{Digest, Sha256};
use std::time::Duration;

fn key(path: &str) -> DocKey {
    DocKey {
        root: "project:p".into(),
        path: path.into(),
    }
}
async fn capture(fx: &Fx, priority: Vec<DocKey>, limits: Limits) -> CodeCapture {
    fx.svc
        .capture_code_with(
            "project:p".into(),
            fx.project.clone(),
            priority,
            CancellationToken::new(),
            limits,
        )
        .await
        .expect("capture")
}

#[tokio::test]
async fn capture_uses_code_authority_and_canonical_aliases_with_raw_and_normalized_hashes() {
    let fx = Fx::new();
    std::fs::write(fx.project.join("a.rs"), b"\xef\xbb\xbffoo\r\n").expect("fixture");
    std::fs::write(fx.outside.join("secret.rs"), b"secret").expect("fixture");
    std::os::unix::fs::symlink(fx.project.join("a.rs"), fx.project.join("alias.rs"))
        .expect("alias");
    std::os::unix::fs::symlink(fx.outside.join("secret.rs"), fx.project.join("escape.rs"))
        .expect("escape");
    std::fs::create_dir(fx.project.join(".git")).expect("fixture");
    std::fs::write(fx.project.join(".git/private"), b"internal").expect("fixture");
    let capture = capture(&fx, vec![key("alias.rs")], Limits::default()).await;
    assert!(capture.stopped.is_none());
    assert_eq!(capture.covered.len(), 1);
    let baseline = &capture.covered[0];
    let disk = fx
        .svc
        .code_authority(&key("a.rs"), None)
        .await
        .expect("authority");
    assert_eq!(baseline.key, key("a.rs"));
    assert_eq!(Some(&baseline.rev), disk.rev.as_ref());
    assert_eq!(
        baseline.text_hash,
        format!("{:x}", Sha256::digest(b"foo\n"))
    );
    assert_ne!(baseline.text_hash, baseline.rev.hash);
    assert!(
        capture
            .omitted
            .iter()
            .any(|row| row.key == key("escape.rs") && row.reason == CaptureReason::OutOfScope)
    );
    assert_eq!(capture.bytes_read, 8);
    assert!(
        fx.svc
            .capture_code(
                "project:p".into(),
                fx.outside.clone(),
                vec![],
                CancellationToken::new()
            )
            .await
            .is_err()
    );
}

#[tokio::test]
async fn capture_honors_file_entry_metadata_and_actual_byte_budgets() {
    let fx = Fx::new();
    for name in ["a", "b", "c"] {
        std::fs::write(fx.project.join(name), b"12345678").expect("fixture");
    }
    for (limits, reason) in [
        (
            Limits {
                files: 1,
                ..Limits::default()
            },
            CaptureReason::FileLimit,
        ),
        (
            Limits {
                entries: 1,
                ..Limits::default()
            },
            CaptureReason::EntryLimit,
        ),
        (
            Limits {
                metadata: 1,
                ..Limits::default()
            },
            CaptureReason::MetadataLimit,
        ),
        (
            Limits {
                bytes: 10,
                ..Limits::default()
            },
            CaptureReason::ByteLimit,
        ),
        (
            Limits {
                duration: Duration::ZERO,
                ..Limits::default()
            },
            CaptureReason::Deadline,
        ),
    ] {
        let captured = capture(&fx, vec![], limits).await;
        assert_eq!(captured.stopped, Some(reason));
        assert!(captured.bytes_read <= limits.bytes);
        assert!(captured.entries_scanned <= limits.entries);
        assert!(captured.covered.len() <= limits.files);
    }
}

#[tokio::test]
async fn capture_priority_is_fresh_and_cancellation_never_returns_old_coverage() {
    let fx = Fx::new();
    std::fs::write(fx.project.join("a"), b"a").expect("fixture");
    std::fs::write(fx.project.join("b"), b"b").expect("fixture");
    let first = capture(
        &fx,
        vec![key("a")],
        Limits {
            files: 1,
            ..Limits::default()
        },
    )
    .await;
    assert_eq!(first.covered[0].key, key("a"));
    std::fs::write(fx.project.join("b"), b"new b").expect("fixture");
    let next = capture(
        &fx,
        vec![key("b")],
        Limits {
            files: 1,
            ..Limits::default()
        },
    )
    .await;
    assert_eq!(next.covered[0].key, key("b"));
    assert_eq!(
        next.covered[0].text_hash,
        format!("{:x}", Sha256::digest(b"new b"))
    );
    let cancel = CancellationToken::new();
    cancel.cancel();
    let cancelled = fx
        .svc
        .capture_code(
            "project:p".into(),
            fx.project.clone(),
            vec![key("a")],
            cancel,
        )
        .await
        .expect("cancel result");
    assert_eq!(cancelled.stopped, Some(CaptureReason::Cancelled));
    assert_eq!(cancelled.bytes_read, 0);
    assert!(cancelled.covered.is_empty());
}

#[tokio::test]
async fn capture_refuses_binary_encoding_deleted_and_daemon_private_files() {
    let fx = Fx::new();
    std::fs::write(fx.project.join("binary"), b"a\0b").expect("fixture");
    std::fs::write(fx.project.join("encoding"), b"\xff").expect("fixture");
    let private = fx.project.join("private");
    std::fs::create_dir(&private).expect("fixture");
    std::fs::write(private.join("secret"), b"private").expect("fixture");
    let svc = fx.svc.excluding(vec![private]);
    let result = svc
        .capture_code(
            "project:p".into(),
            fx.project.clone(),
            vec![key("gone")],
            CancellationToken::new(),
        )
        .await
        .expect("capture");
    assert!(result.covered.is_empty());
    for (name, reason) in [
        ("binary", CaptureReason::Binary),
        ("encoding", CaptureReason::Encoding),
        ("gone", CaptureReason::Unavailable),
    ] {
        assert!(
            result
                .omitted
                .iter()
                .any(|r| r.key == key(name) && r.reason == reason)
        );
    }
}

#[tokio::test]
async fn capture_initial_budgets_bound_owned_large_fixture() {
    let fx = Fx::new();
    let bytes = vec![b'a'; crate::CODE_LIMIT as usize];
    for index in 0..8 {
        std::fs::write(fx.project.join(format!("file-{index}")), &bytes).expect("owned fixture");
    }
    let started = Instant::now();
    let captured = capture(&fx, vec![], Limits::default()).await;
    eprintln!(
        "capture owned40MiB: elapsed_ms={} bytes_read={} entries={} covered={} stopped={:?}",
        started.elapsed().as_millis(),
        captured.bytes_read,
        captured.entries_scanned,
        captured.covered.len(),
        captured.stopped
    );
    assert!(captured.bytes_read <= 32 * 1024 * 1024);
    assert!(captured.entries_scanned <= 16_384);
    assert!(matches!(
        captured.stopped,
        Some(CaptureReason::ByteLimit | CaptureReason::Deadline)
    ));
    assert!(
        captured
            .covered
            .iter()
            .all(|file| file.rev.size == crate::CODE_LIMIT)
    );
}
