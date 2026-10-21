use std::fs;

use super::{MAX_LINE_BYTES, Stream, Writer, read_range};

#[test]
fn logfile_records_stream_timestamp_and_reopens_for_append() {
    let dir = tempfile::tempdir().expect("fixture operation succeeds");
    let path = dir.path().join("run.log");
    let mut writer = Writer::open(&path).expect("fixture operation succeeds");
    writer
        .write_line(Stream::Out, b"hello")
        .expect("fixture operation succeeds");
    writer
        .write_line(Stream::Err, b"problem")
        .expect("fixture operation succeeds");
    drop(writer);
    Writer::open(&path)
        .expect("fixture operation succeeds")
        .write_line(Stream::System, b"done")
        .expect("fixture operation succeeds");
    let (lines, offset) = read_range(&path, 0, 20).expect("fixture operation succeeds");
    assert_eq!(lines.len(), 3);
    for (line, suffix) in lines
        .iter()
        .zip(["\to\thello", "\te\tproblem", "\ts\tdone"])
    {
        let timestamp = line.split('\t').next().expect("fixture operation succeeds");
        chrono::DateTime::parse_from_rfc3339(timestamp).expect("fixture operation succeeds");
        assert!(timestamp.ends_with('Z'));
        assert!(line.ends_with(suffix));
    }
    assert_eq!(
        offset,
        fs::metadata(&path)
            .expect("fixture operation succeeds")
            .len()
    );
}

#[test]
fn logfile_offsets_count_utf8_bytes_and_preserve_incomplete_tail() {
    let dir = tempfile::tempdir().expect("fixture operation succeeds");
    let path = dir.path().join("run.log");
    fs::write(&path, "first\t한글\nsecond\nunfinished").expect("fixture operation succeeds");
    let (first, offset) = read_range(&path, 0, 1).expect("fixture operation succeeds");
    assert_eq!(first, ["first\t한글"]);
    assert_eq!(offset, "first\t한글\n".len() as u64);
    let (second, next) = read_range(&path, offset, 5).expect("fixture operation succeeds");
    assert_eq!(second, ["second"]);
    assert_eq!(next, offset + 7);
    assert_eq!(
        read_range(&path, next, 5).expect("fixture operation succeeds"),
        (vec![], next)
    );
    fs::write(&path, "new\n").expect("fixture operation succeeds");
    assert_eq!(
        read_range(&path, next, 5).expect("fixture operation succeeds"),
        (vec!["new".into()], 4)
    );
}

#[test]
fn logfile_invalid_utf8_and_raw_byte_line_limit() {
    let dir = tempfile::tempdir().expect("fixture operation succeeds");
    let path = dir.path().join("run.log");
    let mut writer = Writer::open(&path).expect("fixture operation succeeds");
    writer
        .write_line(Stream::Out, &[0xff, b'a'])
        .expect("fixture operation succeeds");
    writer
        .write_line(Stream::Err, &vec![0xff; MAX_LINE_BYTES + 100])
        .expect("fixture operation succeeds");
    let (lines, _) = read_range(&path, 0, 5).expect("fixture operation succeeds");
    assert!(lines[0].ends_with("\to\t\u{fffd}a"));
    let text = lines[1]
        .splitn(3, '\t')
        .nth(2)
        .expect("fixture operation succeeds");
    assert_eq!(
        text,
        format!("{}…(잘림)", "\u{fffd}".repeat(MAX_LINE_BYTES))
    );
}

#[test]
fn logfile_exact_limit_does_not_mark_truncated() {
    let dir = tempfile::tempdir().expect("fixture operation succeeds");
    let path = dir.path().join("run.log");
    Writer::open(&path)
        .expect("fixture operation succeeds")
        .write_line(Stream::Out, &vec![b'x'; MAX_LINE_BYTES])
        .expect("fixture operation succeeds");
    let (lines, _) = read_range(&path, 0, 1).expect("fixture operation succeeds");
    assert_eq!(
        lines[0]
            .splitn(3, '\t')
            .nth(2)
            .expect("fixture operation succeeds")
            .len(),
        MAX_LINE_BYTES
    );
}

#[test]
fn logfile_rotation_retains_whole_recent_lines_and_system_marker() {
    let dir = tempfile::tempdir().expect("fixture operation succeeds");
    let path = dir.path().join("run.log");
    let mut writer = Writer::with_limit(&path, 512).expect("fixture operation succeeds");
    for index in 0..24 {
        writer
            .write_line(Stream::Out, format!("record-{index:02}-한글").as_bytes())
            .expect("fixture operation succeeds");
    }
    let (lines, offset) = read_range(&path, 0, 256).expect("fixture operation succeeds");
    assert!(offset <= 512);
    assert!(lines[0].contains("\ts\t"));
    assert!(lines[0].contains("회전"));
    assert!(
        lines
            .last()
            .expect("fixture operation succeeds")
            .ends_with("record-23-한글")
    );
    assert!(lines.iter().any(|line| line.ends_with("record-22-한글")));
    assert!(!lines.iter().any(|line| line.ends_with("record-00-한글")));
    for line in lines {
        let (timestamp, _) = line.split_once('\t').expect("fixture operation succeeds");
        chrono::DateTime::parse_from_rfc3339(timestamp).expect("fixture operation succeeds");
    }
}

#[test]
fn logfile_reader_caps_line_count_and_rejects_oversized_physical_lines() {
    let dir = tempfile::tempdir().expect("fixture operation succeeds");
    let path = dir.path().join("run.log");
    fs::write(&path, "line\n".repeat(300)).expect("fixture operation succeeds");
    let (lines, offset) = read_range(&path, 0, usize::MAX).expect("fixture operation succeeds");
    assert_eq!(lines.len(), 256);
    assert_eq!(offset, 256 * 5);
    fs::write(&path, vec![b'a'; MAX_LINE_BYTES * 3 + 1024]).expect("fixture operation succeeds");
    assert_eq!(
        read_range(&path, 0, 1)
            .expect_err("oversized record must fail")
            .kind(),
        std::io::ErrorKind::InvalidData
    );
}

#[test]
fn logfile_rotation_restores_the_attempt_at_cut_not_the_latest_attempt() {
    use devbox_core::run_log::{attempt_marker, parse_attempt_marker};
    for cut_in_new in [false, true] {
        let dir = tempfile::tempdir().expect("fixture result");
        let path = dir.path().join("run.log");
        let mut writer = Writer::open(&path).expect("fixture result");
        writer
            .write_line(Stream::System, attempt_marker("old").as_bytes())
            .expect("fixture result");
        for n in 0..20 {
            writer
                .write_line(
                    Stream::Out,
                    format!("old-{n}-{}", "x".repeat(100)).as_bytes(),
                )
                .expect("fixture result");
        }
        writer
            .write_line(Stream::System, attempt_marker("new").as_bytes())
            .expect("fixture result");
        if cut_in_new {
            for n in 0..20 {
                writer
                    .write_line(
                        Stream::Out,
                        format!("new-{n}-{}", "x".repeat(100)).as_bytes(),
                    )
                    .expect("fixture result");
            }
        }
        writer
            .write_line(Stream::Err, b"x.ts(9,1): error current")
            .expect("fixture result");
        drop(writer);
        let before = fs::read_to_string(&path).expect("fixture result");
        let expected_attempt = if cut_in_new { "new" } else { "old" };
        let expected_marker = before
            .lines()
            .find(|line| line.ends_with(&format!("\ts\t{}", attempt_marker(expected_attempt))))
            .expect("fixture value")
            .to_owned();
        let mut writer = Writer::with_limit(&path, 2048).expect("fixture result");
        writer
            .write_line(Stream::Out, b"after rotation")
            .expect("fixture result");
        let (lines, end) = read_range(&path, 0, 256).expect("fixture result");
        assert!(end <= 2048);
        assert_eq!(
            lines[1], expected_marker,
            "restore the original marker timestamp"
        );
        let restored = lines[1].splitn(3, '\t').nth(2).expect("fixture value");
        assert_eq!(
            parse_attempt_marker(restored),
            Some(if cut_in_new { "new" } else { "old" })
        );
        let markers: Vec<_> = lines
            .iter()
            .filter_map(|line| {
                let mut p = line.splitn(3, '\t');
                p.next()?;
                (p.next()? == "s")
                    .then(|| parse_attempt_marker(p.next()?))
                    .flatten()
            })
            .collect();
        assert_eq!(markers.last().copied(), Some("new"));
        assert!(
            lines
                .iter()
                .any(|l| l.ends_with("x.ts(9,1): error current"))
        );
        assert_eq!(
            lines.last().expect("fixture value").splitn(3, '\t').nth(2),
            Some("after rotation")
        );
    }
}
