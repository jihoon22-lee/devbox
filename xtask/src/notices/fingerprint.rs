//! The input fingerprint on the first line of THIRD_PARTY_NOTICES.md and the check that it is current.

use super::NOTICES_FILE;
use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

const STALE: &str = "THIRD_PARTY_NOTICES.md가 lock 파일과 다릅니다. `cargo run -p xtask -- notices`로 다시 만드세요.";
const LOCK_FILES: [&str; 3] = ["Cargo.lock", "app/src-tauri/Cargo.lock", "pnpm-lock.yaml"];

/// `Cargo.lock=<sha256> app/src-tauri/Cargo.lock=<sha256> pnpm-lock.yaml=<sha256>`.
pub fn lock_fingerprint(repo: &Path) -> Result<String> {
    let mut parts = Vec::new();
    for name in LOCK_FILES {
        let bytes =
            fs::read(repo.join(name)).with_context(|| format!("{name}을 읽지 못했습니다."))?;
        parts.push(format!("{name}={}", sha256_lf(&bytes)));
    }
    Ok(parts.join(" "))
}

/// SHA-256 of the bytes as if the file were checked out with LF line ends.
fn sha256_lf(bytes: &[u8]) -> String {
    let mut lf = Vec::with_capacity(bytes.len());
    for (index, byte) in bytes.iter().enumerate() {
        if *byte != b'\r' || bytes.get(index + 1) != Some(&b'\n') {
            lf.push(*byte);
        }
    }
    format!("{:x}", Sha256::digest(&lf))
}

pub fn header_line(fingerprint: &str) -> String {
    format!("<!-- inputs: {fingerprint} -->")
}

/// The notices are current when their first line fingerprints the lock files as they are now.
pub fn ensure_current(repo: &Path) -> Result<()> {
    let Ok(text) = fs::read_to_string(repo.join(NOTICES_FILE)) else {
        bail!(STALE);
    };
    if text.lines().next() != Some(header_line(&lock_fingerprint(repo)?).as_str()) {
        bail!(STALE);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Typed out on purpose: the sentence is a contract, not something to derive from `STALE`.
    const STALE_SENTENCE: &str = "THIRD_PARTY_NOTICES.md가 lock 파일과 다릅니다. `cargo run -p xtask -- notices`로 다시 만드세요.";

    fn write_locks(dir: &Path, cargo: &[u8], app: &[u8], pnpm: &[u8]) {
        fs::create_dir_all(dir.join("app/src-tauri")).expect("create folder");
        fs::write(dir.join("Cargo.lock"), cargo).expect("write lock");
        fs::write(dir.join("app/src-tauri/Cargo.lock"), app).expect("write lock");
        fs::write(dir.join("pnpm-lock.yaml"), pnpm).expect("write lock");
    }

    #[test]
    fn fingerprint_hashes_each_lock_file_after_crlf_to_lf() {
        let lf = tempfile::tempdir().expect("create folder");
        write_locks(lf.path(), b"a\nb\n", b"c\n", b"d\n");
        let crlf = tempfile::tempdir().expect("create folder");
        write_locks(crlf.path(), b"a\r\nb\r\n", b"c\r\n", b"d\r\n");

        let fingerprint = lock_fingerprint(lf.path()).expect("fingerprint");
        assert_eq!(
            fingerprint,
            lock_fingerprint(crlf.path()).expect("crlf fingerprint")
        );
        let parts: Vec<_> = fingerprint.split(' ').collect();
        assert_eq!(parts.len(), 3);
        assert!(parts[0].starts_with("Cargo.lock="));
        assert_eq!(
            parts[1],
            "app/src-tauri/Cargo.lock=a3a5e715f0cc574a73c3f9bebb6bc24f32ffd5b67b387244c2c909da779a1478"
        );
        assert!(parts[2].starts_with("pnpm-lock.yaml="));
    }

    #[test]
    fn stale_or_missing_notices_report_the_exact_sentence() {
        assert_eq!(STALE, STALE_SENTENCE);
        let dir = tempfile::tempdir().expect("create folder");
        write_locks(dir.path(), b"a\n", b"c\n", b"d\n");
        let notices = dir.path().join(NOTICES_FILE);
        let stale = |dir: &Path| ensure_current(dir).expect_err("not current").to_string();

        assert_eq!(stale(dir.path()), STALE_SENTENCE);
        fs::write(
            &notices,
            "<!-- inputs: Cargo.lock=0 -->\n# Third-party notices\n",
        )
        .expect("write");
        assert_eq!(stale(dir.path()), STALE_SENTENCE);

        let current = format!(
            "{}\n# Third-party notices\n",
            header_line(&lock_fingerprint(dir.path()).expect("fingerprint"))
        );
        fs::write(&notices, current).expect("write");
        ensure_current(dir.path()).expect("current");

        fs::write(dir.path().join("pnpm-lock.yaml"), b"d changed\n").expect("write");
        assert_eq!(stale(dir.path()), STALE_SENTENCE);
    }
}
