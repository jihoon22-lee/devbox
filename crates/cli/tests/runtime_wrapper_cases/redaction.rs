use super::*;

fn assert_no_secret_piece(log: &str, secret: &str) {
    for piece in secret.as_bytes().windows(5) {
        assert!(
            !log.as_bytes().windows(5).any(|w| w == piece),
            "leaked fixture piece {}",
            String::from_utf8_lossy(piece)
        );
    }
}
#[test]
fn safe_nonsecret_line_reaches_log_before_eof() {
    let f = Fixture::new();
    // A service writes a 32-byte secret in two pieces (the first lands in the held tail), then an
    // unrelated line, and keeps running.
    let script = r#"import os,pathlib,time
v=os.environ['TOKEN'].encode()
assert len(v)==32
os.write(1,b'key='+v[:9]);time.sleep(.2);os.write(1,v[9:]+b'\n')
os.write(1,b'ready\n')
pathlib.Path(os.environ['READY']).write_text(str(os.getpid()))
while not pathlib.Path(os.environ['FINISH']).exists(): time.sleep(.01)
"#;
    let secret = "owned-token-for-task3-0123456789";
    assert_eq!(secret.len(), 32);
    let input = format!("TOKEN={secret}\0");
    let mut owned = f.spawn(script, "safe-tail", Some(input.as_bytes()));
    f.ready();
    f.wait_log("\to\tready\n");
    assert!(
        owned.child.try_wait().expect("wrapper status").is_none(),
        "the service must still run"
    );
    let log = f.log();
    assert!(log.contains("\to\tkey=[REDACTED]\n"), "{log}");
    assert_no_secret_piece(&log, secret);
    std::fs::write(f.temp.path().join("finish"), "").expect("release");
    assert!(owned.wait().success());
    assert_no_secret_piece(&f.log(), secret);
}
