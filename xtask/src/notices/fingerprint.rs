//! The input fingerprint on the first line of THIRD_PARTY_NOTICES.md and the check that it is current.

use super::NOTICES_FILE;
use super::bundled::FONTS;
use super::linked::LINKED;
use super::sources::Ecosystem;
use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

const STALE: &str = "THIRD_PARTY_NOTICES.md가 입력(lock 파일, manifest, 함께 싣는 글꼴과 링크되는 코드의 라이선스)과 다릅니다. `cargo run -p xtask -- notices`로 다시 만드세요.";
const LOCK_FILES: [&str; 3] = ["Cargo.lock", "app/src-tauri/Cargo.lock", "pnpm-lock.yaml"];
/// Manifests besides the workspace members': the workspace root, the app crate (outside the workspace)
/// and the app package.
const OTHER_MANIFESTS: [&str; 3] = ["Cargo.toml", "app/src-tauri/Cargo.toml", "app/package.json"];

/// `Cargo.lock=<sha256> app/src-tauri/Cargo.lock=<sha256> pnpm-lock.yaml=<sha256> manifests=<sha256> bundled=<sha256> linked=<sha256>`.
///
/// The lock files do not record everything that decides what ships. A dependency can move from
/// `[dev-dependencies]` to `[dependencies]`, and a feature or a target table can pull in a crate that is
/// already locked, with no change to a lock file. So the manifests are in it (the workspace root, every
/// member, the app crate and the app package), and so are the licenses of what no lock file lists: the
/// bundled fonts and the code the toolchain and the WebView2 SDK link in (with each row as it is printed).
pub fn input_fingerprint(repo: &Path) -> Result<String> {
    let mut parts = Vec::new();
    for name in LOCK_FILES {
        parts.push(format!("{name}={}", sha256_lf(&read(repo, name)?)));
    }
    let mut manifests = String::new();
    for name in manifest_paths(repo)? {
        manifests.push_str(&format!("{name}={}\n", sha256_lf(&read(repo, &name)?)));
    }
    parts.push(format!("manifests={}", digest(&manifests)));
    let mut bundled = String::new();
    for font in &FONTS {
        bundled.push_str(&format!(
            "{}@{} {} {}={}\n",
            font.name,
            font.version,
            font.license,
            font.license_file,
            sha256_lf(&read(repo, font.license_file)?)
        ));
    }
    parts.push(format!("bundled={}", digest(&bundled)));
    let mut linked = String::new();
    for row in &LINKED {
        for file in row.license_files() {
            linked.push_str(&format!(
                "{}@{} {} {} {file}={}\n",
                row.name,
                row.version,
                row.license,
                Ecosystem::Linked(row.binaries).label(),
                sha256_lf(&read(repo, &file)?)
            ));
        }
    }
    parts.push(format!("linked={}", digest(&linked)));
    Ok(parts.join(" "))
}

fn read(repo: &Path, name: &str) -> Result<Vec<u8>> {
    fs::read(repo.join(name)).with_context(|| format!("{name}을 읽지 못했습니다."))
}

fn digest(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

/// The manifests that feed the fingerprint, sorted: the fixed ones and `<member>/Cargo.toml` of every
/// workspace member.
fn manifest_paths(repo: &Path) -> Result<Vec<String>> {
    let root =
        fs::read_to_string(repo.join("Cargo.toml")).context("Cargo.toml을 읽지 못했습니다.")?;
    let mut paths: Vec<String> = OTHER_MANIFESTS
        .iter()
        .map(|name| name.to_string())
        .collect();
    for member in workspace_members(&root)? {
        paths.push(format!("{member}/Cargo.toml"));
    }
    paths.sort();
    paths.dedup();
    Ok(paths)
}

/// The `members` of the workspace root manifest, read as text so that the check needs no TOML parser.
/// Anything it cannot list (a glob, another quoting, a comment inside the list) is an error: a manifest
/// left out would be a stale notice nobody sees.
fn workspace_members(manifest: &str) -> Result<Vec<String>> {
    let unreadable = || {
        anyhow::anyhow!(
            "Cargo.toml의 [workspace] members를 \"경로\" 목록으로 읽지 못했습니다. 지문이 manifest를 하나도 \
             놓치지 않으려면 members에 glob, 다른 따옴표, 주석을 쓰지 않아야 합니다."
        )
    };
    let from_members: Vec<&str> = manifest
        .lines()
        .skip_while(|line| !line.trim_start().starts_with("members"))
        .collect();
    let list = from_members.join("\n");
    let (_, after_open) = list.split_once('[').ok_or_else(unreadable)?;
    let (items, _) = after_open.split_once(']').ok_or_else(unreadable)?;
    let mut members = Vec::new();
    for item in items
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
    {
        let member = item
            .strip_prefix('"')
            .and_then(|item| item.strip_suffix('"'))
            .filter(|member| {
                !member.is_empty()
                    && !member.starts_with('/')
                    && !member.contains(['*', '?', '[', '"', '\\'])
                    && !member
                        .split('/')
                        .any(|part| part == ".." || part.is_empty())
            })
            .ok_or_else(unreadable)?;
        members.push(member.to_string());
    }
    if members.is_empty() {
        return Err(unreadable());
    }
    Ok(members)
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

/// The notices are current when their first line fingerprints the inputs as they are now.
pub fn ensure_current(repo: &Path) -> Result<()> {
    let Ok(text) = fs::read_to_string(repo.join(NOTICES_FILE)) else {
        bail!(STALE);
    };
    if text.lines().next() != Some(header_line(&input_fingerprint(repo)?).as_str()) {
        bail!(STALE);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notices::bundled::FONTS;
    use crate::notices::linked::{LINKED, Linked};

    /// Typed out on purpose: the sentence is a contract, not something to derive from `STALE`.
    const STALE_SENTENCE: &str = "THIRD_PARTY_NOTICES.md가 입력(lock 파일, manifest, 함께 싣는 글꼴과 링크되는 코드의 라이선스)과 다릅니다. `cargo run -p xtask -- notices`로 다시 만드세요.";

    /// Every file the fingerprint reads, with the given line ends.
    const INPUTS: [(&str, &str); 9] = [
        ("Cargo.lock", "a\nb\n"),
        ("app/src-tauri/Cargo.lock", "c\n"),
        ("pnpm-lock.yaml", "d\n"),
        (
            "Cargo.toml",
            "[workspace]\nmembers = [\"crates/one\", \"xtask\"]\nexclude = [\"app/src-tauri\"]\n",
        ),
        ("crates/one/Cargo.toml", "[package]\nname = \"one\"\n"),
        ("xtask/Cargo.toml", "[package]\nname = \"xtask\"\n"),
        ("app/src-tauri/Cargo.toml", "[package]\nname = \"app\"\n"),
        ("app/package.json", "{\n  \"name\": \"app\"\n}\n"),
        ("LICENSE-OF-THE-FONT", "SIL Open Font License\n"),
    ];

    fn put(dir: &Path, path: &str, text: &str) {
        let file = dir.join(path);
        fs::create_dir_all(file.parent().expect("file has a folder")).expect("create folder");
        fs::write(file, text).expect("write input");
    }

    fn write_inputs(dir: &Path, line_end: &str) {
        for (path, text) in INPUTS {
            let path = if path == "LICENSE-OF-THE-FONT" {
                FONTS[0].license_file
            } else {
                path
            };
            put(dir, path, &text.replace('\n', line_end));
        }
        // Every license text of the linked code, each a text of its own.
        for file in LINKED.iter().flat_map(Linked::license_files) {
            put(
                dir,
                &file,
                &format!("license text\nof {file}\n").replace('\n', line_end),
            );
        }
    }

    #[test]
    fn fingerprint_hashes_each_input_after_crlf_to_lf() {
        let lf = tempfile::tempdir().expect("create folder");
        write_inputs(lf.path(), "\n");
        let crlf = tempfile::tempdir().expect("create folder");
        write_inputs(crlf.path(), "\r\n");

        let fingerprint = input_fingerprint(lf.path()).expect("fingerprint");
        assert_eq!(
            fingerprint,
            input_fingerprint(crlf.path()).expect("crlf fingerprint")
        );
        let parts: Vec<_> = fingerprint.split(' ').collect();
        assert_eq!(parts.len(), 6);
        assert!(parts[0].starts_with("Cargo.lock="));
        assert_eq!(
            parts[1],
            "app/src-tauri/Cargo.lock=a3a5e715f0cc574a73c3f9bebb6bc24f32ffd5b67b387244c2c909da779a1478"
        );
        assert!(parts[2].starts_with("pnpm-lock.yaml="));
        assert!(parts[3].starts_with("manifests="));
        assert!(parts[4].starts_with("bundled="));
        assert!(parts[5].starts_with("linked="));
    }

    #[test]
    fn stale_or_missing_notices_report_the_exact_sentence() {
        assert_eq!(STALE, STALE_SENTENCE);
        let dir = tempfile::tempdir().expect("create folder");
        write_inputs(dir.path(), "\n");
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
            header_line(&input_fingerprint(dir.path()).expect("fingerprint"))
        );
        fs::write(&notices, current).expect("write");
        ensure_current(dir.path()).expect("current");

        fs::write(dir.path().join("pnpm-lock.yaml"), b"d changed\n").expect("write");
        assert_eq!(stale(dir.path()), STALE_SENTENCE);
    }

    #[test]
    fn a_change_to_any_input_the_lock_files_do_not_record_makes_the_notices_stale() {
        // A dependency moved from `[dev-dependencies]` to `[dependencies]`, a feature or a target table
        // changes what ships and leaves Cargo.lock as it is; so does another font license text, and so does
        // another license text of any linked component.
        let linked_files: Vec<String> = LINKED.iter().flat_map(Linked::license_files).collect();
        assert!(!linked_files.is_empty());
        let linked = linked_files
            .iter()
            .map(|file| (file.as_str(), "Another license\n"));
        for (path, text) in [
            (
                "Cargo.toml",
                "[workspace]\nmembers = [\"crates/one\", \"xtask\"]\n",
            ),
            (
                "crates/one/Cargo.toml",
                "[package]\nname = \"one\"\n[dependencies]\nx = \"1\"\n",
            ),
            (
                "xtask/Cargo.toml",
                "[package]\nname = \"xtask\"\nedition = \"2024\"\n",
            ),
            (
                "app/src-tauri/Cargo.toml",
                "[package]\nname = \"app\"\n[features]\n",
            ),
            (
                "app/package.json",
                "{\"name\":\"app\",\"dependencies\":{\"x\":\"1\"}}\n",
            ),
            (FONTS[0].license_file, "Another license\n"),
        ]
        .into_iter()
        .chain(linked)
        {
            let dir = tempfile::tempdir().expect("create folder");
            write_inputs(dir.path(), "\n");
            let fingerprint = input_fingerprint(dir.path()).expect("fingerprint");
            let notices = format!("{}\n# Third-party notices\n", header_line(&fingerprint));
            fs::write(dir.path().join(NOTICES_FILE), notices).expect("write");
            ensure_current(dir.path()).expect("current");

            put(dir.path(), path, text);
            let error = ensure_current(dir.path()).expect_err(path);
            assert_eq!(error.to_string(), STALE_SENTENCE, "{path}");
        }
    }

    #[test]
    fn the_workspace_members_come_from_the_root_manifest() {
        let members = |text: &str| workspace_members(text).map_err(|error| error.to_string());
        assert_eq!(
            members("[workspace]\nresolver = \"3\"\nmembers = [\"crates/a\", \"xtask\"]\n"),
            Ok(vec!["crates/a".to_string(), "xtask".to_string()])
        );
        // A list over several lines, with a trailing comma, as rustfmt-style TOML writes it.
        assert_eq!(
            members(
                "[workspace]\nmembers = [\n  \"crates/a\",\n  \"crates/b\",\n]\nexclude = [\"app\"]\n"
            ),
            Ok(vec!["crates/a".to_string(), "crates/b".to_string()])
        );
        // What the fingerprint cannot list is refused, not skipped: a missed manifest is a stale notice.
        for text in [
            "[workspace]\nmembers = [\"crates/*\"]\n",
            "[workspace]\nmembers = ['crates/a']\n",
            "[workspace]\nmembers = [\"../outside\"]\n",
            "[workspace]\nresolver = \"3\"\n",
            "[workspace]\nmembers = []\n",
            // A comment inside the list, after an item or on a line of its own.
            "[workspace]\nmembers = [\n  \"crates/a\", # the a crate\n  \"crates/b\",\n]\n",
            "[workspace]\nmembers = [\n  # core\n  \"crates/a\",\n]\n",
        ] {
            assert!(members(text).is_err(), "{text}");
        }
        // The refusal says what to change: it names the globs, the other quotes and the comments.
        let error = members("[workspace]\nmembers = [\"crates/*\"]\n").expect_err("a glob");
        for named in ["glob", "따옴표", "주석"] {
            assert!(error.contains(named), "{named}: {error}");
        }
    }
}
