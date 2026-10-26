use super::*;
use crate::capture_budget::Limits;
use std::{io::Write, time::Instant};
use tokio_util::sync::CancellationToken;

fn budget(file_bytes: u64, bytes: u64) -> Budget {
    Budget::new(
        Limits {
            file_bytes,
            bytes,
            ..Limits::default()
        },
        CancellationToken::new(),
        Instant::now(),
    )
}

#[test]
fn capture_large_file_never_reads_past_budget() {
    let dir = tempfile::tempdir().expect("fixture");
    std::fs::write(dir.path().join("large"), vec![b'a'; 128 * 1024]).expect("fixture");
    let fd = sys::open_dir(dir.path()).expect("directory");
    let mut budget = budget(64 * 1024, 64 * 1024);
    assert!(matches!(
        bounded_read(&fd, "large", &mut budget),
        Err(CaptureReason::TooLarge)
    ));
    assert_eq!(
        budget.bytes_read, 0,
        "oversized file is rejected from metadata"
    );
    // Aggregate bytes can also be smaller than one otherwise admissible file.
    budget.limits.file_bytes = 128 * 1024;
    assert!(matches!(
        bounded_read(&fd, "large", &mut budget),
        Err(CaptureReason::ByteLimit)
    ));
    assert_eq!(
        budget.bytes_read,
        64 * 1024,
        "never read the next probe byte"
    );
}

#[test]
fn capture_growing_file_and_cancel_stop_between_chunks() {
    let dir = tempfile::tempdir().expect("fixture");
    let path = dir.path().join("growing");
    std::fs::write(&path, vec![b'a'; CHUNK]).expect("fixture");
    let fd = sys::open_dir(dir.path()).expect("directory");
    let mut budget = budget(3 * CHUNK as u64, 2 * CHUNK as u64);
    let result = read_with(&fd, "growing", &mut budget, || {
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("owned fixture")
            .write_all(&vec![b'b'; CHUNK])
            .expect("grow");
    });
    assert!(matches!(result, Err(CaptureReason::ByteLimit)));
    assert_eq!(budget.bytes_read, 2 * CHUNK as u64);

    let cancel = CancellationToken::new();
    let mut budget = Budget::new(Limits::default(), cancel.clone(), Instant::now());
    assert!(matches!(
        read_with(&fd, "growing", &mut budget, || cancel.cancel()),
        Err(CaptureReason::Cancelled)
    ));
    assert_eq!(budget.bytes_read, CHUNK as u64);
}

#[test]
fn capture_rejects_replacement_and_in_place_change() {
    let dir = tempfile::tempdir().expect("fixture");
    let path = dir.path().join("a");
    let fd = sys::open_dir(dir.path()).expect("directory");
    for replacement in [false, true] {
        std::fs::write(&path, b"original").expect("fixture");
        let result = read_with(&fd, "a", &mut budget(100, 100), || {
            if replacement {
                std::fs::write(dir.path().join("next"), b"changed!").expect("fixture");
                std::fs::rename(dir.path().join("next"), &path).expect("replace");
            } else {
                std::fs::write(&path, b"changed!").expect("change");
            }
        });
        assert!(matches!(result, Err(CaptureReason::Changed)));
    }
}

#[test]
fn capture_exact_limit_and_normalized_hash_match_document_contract() {
    let dir = tempfile::tempdir().expect("fixture");
    let mut raw = b"\xEF\xBB\xBF".to_vec();
    raw.extend(vec![b'a'; CHUNK - 1]);
    raw.extend("\r\n😀\r\r\n끝\r".as_bytes());
    std::fs::write(dir.path().join("a"), &raw).expect("fixture");
    let fd = sys::open_dir(dir.path()).expect("directory");
    let mut budget = budget(raw.len() as u64, raw.len() as u64);
    let file = bounded_read(&fd, "a", &mut budget).expect("exact cap");
    assert_eq!(file.rev.hash, sys::hex(&Sha256::digest(&raw)));
    let decoded = sys::decode_text(&raw).expect("text").0;
    assert_eq!(
        text_hash(&raw, &mut budget).expect("hash"),
        sys::hex(&Sha256::digest(decoded))
    );
    assert_eq!(budget.bytes_read, raw.len() as u64);
    assert_eq!(text_hash(b"a\0b", &mut budget), Err(CaptureReason::Binary));
    assert_eq!(
        text_hash(b"\xff", &mut budget),
        Err(CaptureReason::Encoding)
    );
}
