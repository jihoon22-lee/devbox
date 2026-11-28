use std::{path::Path, process::Command};

/// Writes a small executable fixture without opening it for writing in the test process.
/// A concurrent fork in another test would inherit that write handle until exec, causing
/// an immediate launch of the fixture to fail with ETXTBSY.
pub(crate) fn write_executable(path: &Path, contents: &str) {
    let output = Command::new("/bin/sh")
        .args([
            "-c",
            r#"umask 077; printf '%s' "$2" > "$1" && /bin/chmod 700 "$1""#,
            "sh",
        ])
        .arg(path)
        .arg(contents)
        .output()
        .expect("start executable fixture writer");
    assert!(
        output.status.success(),
        "write {}: {} ({})",
        path.display(),
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
}
