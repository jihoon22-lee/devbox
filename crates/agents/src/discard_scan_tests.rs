use super::*;
/// The fingerprint alone, with nothing ignored.
fn scan(dir: &Path, seed: &[u8], limits: Limits) -> ScanResult<String> {
    hash(dir, seed, &Ignored::default(), limits).map(|walked| walked.fingerprint)
}
#[cfg(unix)]
#[test]
fn deep_tree_keeps_streaming_buffer_off_the_recursive_stack() {
    const CHILD: &str = "DEVBOX_TEST_DEEP_HASH_CHILD";
    if let Some(path) = std::env::var_os(CHILD) {
        scan(Path::new(&path), b"index", LIMITS).expect("deep tree hash");
        return;
    }
    let root = tempfile::tempdir().expect("fixture");
    let mut directory = root.path().to_owned();
    for _ in 0..40 {
        directory = directory.join("d");
        fs::create_dir(&directory).expect("owned directory");
    }
    fs::write(directory.join("file"), "owned bytes").expect("file");
    // Disable only the owned child's core dump; a failing regression must leave no host artifact.
    let status = std::process::Command::new("bash")
        .args(["-c", "ulimit -c 0\nexec \"$@\"", "--"])
        .arg(std::env::current_exe().expect("test executable"))
        .args([
            "--exact",
            "discard_snapshot::tests::deep_tree_keeps_streaming_buffer_off_the_recursive_stack",
            "--nocapture",
        ])
        .env(CHILD, root.path())
        .status()
        .expect("owned child");
    assert!(
        status.success(),
        "a small deep tree must not exhaust the scanner's stack: {status}"
    );
}
#[cfg(unix)]
#[test]
fn fifo_replacement_cannot_block_the_open_before_deadline_checks() {
    const CHILD: &str = "DEVBOX_TEST_FIFO_OPEN_CHILD";
    if let Some(path) = std::env::var_os(CHILD) {
        assert!(
            open(Path::new(&path), false).is_err(),
            "fstat must reject a FIFO after nonblocking open"
        );
        return;
    }
    let root = tempfile::tempdir().expect("fixture");
    let path = root.path().join("replacement");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&path)
            .status()
            .expect("owned FIFO")
            .success()
    );
    struct OwnedChild(std::process::Child);
    impl Drop for OwnedChild {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut child = OwnedChild(std::process::Command::new(std::env::current_exe().expect("test executable"))
        .args(["--exact", "discard_snapshot::tests::fifo_replacement_cannot_block_the_open_before_deadline_checks", "--nocapture"])
        .env(CHILD, &path).spawn().expect("owned child"));
    let started = Instant::now();
    loop {
        if let Some(status) = child.0.try_wait().expect("child state") {
            assert!(status.success());
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "regular-file open blocked on a raced FIFO before any deadline check"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}
#[test]
fn fingerprint_reads_bytes_modes_links_and_fails_closed_on_limits() {
    let root = tempfile::tempdir().expect("fixture");
    fs::write(root.path().join("file"), "one").expect("file");
    let first = scan(root.path(), b"index", LIMITS).expect("hash");
    fs::write(root.path().join("file"), "two").expect("file");
    assert_ne!(first, scan(root.path(), b"index", LIMITS).expect("hash"));
    assert_ne!(
        first,
        scan(root.path(), b"different-index", LIMITS).expect("hash")
    );
    let mut small = LIMITS;
    small.file = 2;
    assert_eq!(scan(root.path(), b"", small), Err("file_bytes"));
    small = LIMITS;
    small.deadline = Duration::ZERO;
    assert_eq!(scan(root.path(), b"", small), Err("deadline"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let before = scan(root.path(), b"index", LIMITS).expect("hash");
        fs::set_permissions(root.path().join("file"), fs::Permissions::from_mode(0o700))
            .expect("mode");
        assert_ne!(before, scan(root.path(), b"index", LIMITS).expect("hash"));
        std::os::unix::fs::symlink("/unreadable-external-target", root.path().join("link"))
            .expect("link");
        assert!(
            scan(root.path(), b"index", LIMITS).is_ok(),
            "link target must never be opened"
        );
    }
}
#[cfg(unix)]
#[test]
fn metadata_stamp_sees_every_change_without_opening_files() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().expect("fixture");
    let file = root.path().join("file");
    fs::write(&file, "one").expect("file");
    let first = metadata_stamp(root.path(), LIMITS).expect("stamp");
    // The hash outside the lock and the check under it stamp an unchanged tree alike.
    let hashed = hash(root.path(), b"index", &Ignored::default(), LIMITS).expect("hash");
    assert_eq!(hashed.stamp, first);
    // Same size and mtime, new bytes: the change time still moves.
    std::thread::sleep(Duration::from_millis(20));
    let modified = fs::metadata(&file)
        .expect("file")
        .modified()
        .expect("mtime");
    fs::write(&file, "two").expect("file");
    File::options()
        .write(true)
        .open(&file)
        .expect("file")
        .set_modified(modified)
        .expect("mtime");
    let rewritten = metadata_stamp(root.path(), LIMITS).expect("stamp");
    assert_ne!(rewritten, first);
    fs::rename(&file, root.path().join("renamed")).expect("rename");
    let renamed = metadata_stamp(root.path(), LIMITS).expect("stamp");
    assert_ne!(renamed, rewritten);
    // Never opened: a file nobody may read is stamped all the same.
    fs::set_permissions(
        root.path().join("renamed"),
        fs::Permissions::from_mode(0o000),
    )
    .expect("mode");
    assert!(metadata_stamp(root.path(), LIMITS).is_ok());
}
