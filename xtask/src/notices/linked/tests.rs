//! The freshness guards of `LINKED`, which only the tests run, and the tests of the linked code.

use super::*;
use crate::notices::fixture::FINGERPRINT;
use crate::notices::license_files::read_license_file;
use crate::notices::{fence_for, generate};
use std::fs;
use std::path::PathBuf;

/// One message per row that does not come with the Rust of `toolchain` (the text of `rust-toolchain.toml`),
/// saying what to take again.
fn toolchain_mismatches(toolchain: &str) -> Vec<String> {
    let channel = toolchain
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once('=')?;
            (key.trim() == "channel").then(|| value.trim().trim_matches('"'))
        })
        .unwrap_or("(no channel)");
    let update = "for the new toolchain, take every Rust row and its texts in xtask/notices/linked/ again: the \
                  std statement (share/doc/rust/COPYRIGHT-library.html) and licenses/ of the toolchain, the \
                  crates std links for both targets (library/Cargo.lock at the new tag) and the COPYRIGHT of \
                  the musl that libc.a is built from; then run `cargo run -p xtask -- notices`";
    let mut stale: Vec<String> = LINKED
        .iter()
        .filter_map(|row| {
            let Pin::Toolchain(pinned) = row.pin else {
                return None;
            };
            let current = pinned == channel && (row.name != RUST_STD || row.version == channel);
            (!current).then(|| {
                format!(
                    "rust-toolchain.toml pins Rust {channel}, but `{}@{}` in LINKED \
                     (xtask/src/notices/linked.rs) comes with Rust {pinned}: {update}",
                    row.name, row.version
                )
            })
        })
        .collect();
    if !LINKED.iter().any(|row| row.name == RUST_STD) {
        stale.push(format!(
            "LINKED (xtask/src/notices/linked.rs) has no {RUST_STD} row for Rust {channel}: {update}"
        ));
    }
    stale
}

/// One message per row of `rows` whose texts are not in the folder its name and version give. A crate that
/// std links in is below the folder of the standard library, `rust-std-<toolchain>/<name>-<version>`; every
/// other component (the standard library itself, musl and the WebView2 SDK) is at the top, `<name>-<version>`,
/// which for the standard library is `rust-std-<toolchain>` because its version is the toolchain's.
fn misplaced(rows: &[Linked]) -> Vec<String> {
    rows.iter()
        .filter_map(|row| {
            let own = format!("{}-{}", row.name, row.version);
            let std_crate =
                matches!(row.pin, Pin::Toolchain(_)) && ![RUST_STD, "musl"].contains(&row.name);
            let wanted = if std_crate {
                format!("{RUST_STD}-{TOOLCHAIN}/{own}")
            } else {
                own
            };
            (row.dir != wanted).then(|| {
                format!(
                    "`{}@{}` in LINKED (xtask/src/notices/linked.rs) keeps its texts in {}, but its name, \
                     version and the toolchain give {wanted}: move the folder in xtask/notices/linked/ or \
                     fix the row",
                    row.name, row.version, row.dir
                )
            })
        })
        .collect()
}

/// The versions `lock` (the text of a Cargo.lock) has of the package `name`.
fn locked_versions<'a>(lock: &'a str, name: &str) -> Vec<&'a str> {
    let wanted = format!("name = \"{name}\"");
    let lines: Vec<&str> = lock.lines().collect();
    lines
        .windows(2)
        .filter(|pair| pair[0] == wanted)
        .filter_map(|pair| pair[1].strip_prefix("version = \"")?.strip_suffix('"'))
        .collect()
}

/// One message per row whose vendoring crate is not at its pinned version in `app_lock` (the text of
/// `app/src-tauri/Cargo.lock`), saying what to take again.
fn crate_mismatches(app_lock: &str) -> Vec<String> {
    LINKED
        .iter()
        .filter_map(|row| {
            let Pin::Crate(krate, pinned) = row.pin else {
                return None;
            };
            let locked = locked_versions(app_lock, krate);
            (!locked.contains(&pinned)).then(|| {
                format!(
                    "app/src-tauri/Cargo.lock locks {krate} {}, but `{}@{}` in LINKED \
                     (xtask/src/notices/linked.rs) is pinned to {krate} {pinned}: find the WebView2 SDK that \
                     the locked {krate} vendors (WEBVIEW2_VERSION in crates/update-bindings of \
                     wravery/webview2-rs), put LICENSE.txt of that Microsoft.Web.WebView2 NuGet package in \
                     xtask/notices/linked/, update the row and run `cargo run -p xtask -- notices`",
                    if locked.is_empty() {
                        "at no version".to_string()
                    } else {
                        locked.join(" and ")
                    },
                    row.name,
                    row.version
                )
            })
        })
        .collect()
}

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn files_below(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read folder") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            files_below(&path, found);
        } else {
            found.push(path);
        }
    }
}

#[test]
fn the_committed_notices_list_every_linked_component_with_its_license_texts() {
    // The three components the notices must cover (user decision 2026-11-04), and the in-tree
    // compiler_builtins, whose own license the std statement folds into `.` (fix round 1).
    for name in [
        RUST_STD,
        "compiler_builtins",
        "musl",
        "Microsoft.Web.WebView2",
    ] {
        assert!(
            LINKED.iter().any(|row| row.name == name),
            "LINKED has no {name} row"
        );
    }
    let repo = repo();
    let notices = fs::read_to_string(repo.join("THIRD_PARTY_NOTICES.md"))
        .expect("read notices")
        .replace("\r\n", "\n");
    for row in &LINKED {
        let line = format!(
            "- `{}@{}` ({}, {})\n",
            row.name,
            row.version,
            Ecosystem::Linked(row.binaries).label(),
            row.license
        );
        assert!(notices.contains(&line), "{} is not listed", row.name);
        // Each text is a fenced block of its own, not just somewhere in another one.
        for file in row.license_files() {
            let text = read_license_file(&repo.join(&file)).expect("license text");
            let fence = fence_for(&text);
            assert!(
                notices.contains(&format!("\n{fence}text\n{text}\n{fence}\n")),
                "the license text {file} is missing"
            );
        }
    }
    assert!(notices.contains(&format!("\n\n{WITHOUT_ENTRY}\n\n")));
    // Without an entry: the compiler-rt C builtins, not the whole compiler_builtins crate (it has an
    // entry), and the Visual C++ runtime that Tauri links into Devbox.exe, stated as a fact only.
    for named in [
        "the compiler-rt C builtins compiled into `compiler_builtins`",
        "the Microsoft Visual C++ runtime (libcmt and libvcruntime)",
        "Tauri links statically by default (`staticVCRuntime`)",
        "the Universal C Runtime that Windows provides",
    ] {
        assert!(WITHOUT_ENTRY.contains(named), "{named}: {WITHOUT_ENTRY}");
    }
    assert!(!WITHOUT_ENTRY.contains("builtins `compiler_builtins` may add"));
}

#[test]
fn every_row_keeps_its_texts_in_the_folder_its_name_and_version_give() {
    assert!(misplaced(&LINKED).is_empty(), "{:#?}", misplaced(&LINKED));
    // A copy of the table with one row moved to a folder of another version is caught.
    let libc = LINKED
        .iter()
        .position(|row| row.name == "libc")
        .expect("a libc row");
    let mut copy = LINKED;
    copy[libc] = Linked {
        dir: "rust-std-1.98.1/libc-0.2.184",
        ..LINKED[libc]
    };
    let found = misplaced(&copy);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert!(
        found[0].contains("`libc@0.2.185`")
            && found[0].contains("rust-std-1.98.1/libc-0.2.184")
            && found[0].contains(&format!("rust-std-{TOOLCHAIN}/libc-0.2.185")),
        "{}",
        found[0]
    );
}

#[test]
fn a_crate_that_std_links_in_counts_apart_from_the_same_locked_crate() {
    let dir = tempfile::tempdir().expect("create folder");
    fs::write(
        dir.path().join("LICENSE-MIT"),
        "Copyright The Rust Project Developers\n",
    )
    .expect("write");
    let hashbrown = |ecosystem| Component {
        ecosystem,
        name: "hashbrown".into(),
        version: "0.17.1".into(),
        license: "MIT OR Apache-2.0".into(),
        dir: dir.path().to_path_buf(),
        license_files: vec![dir.path().join("LICENSE-MIT")],
    };
    let notices = generate(
        &[
            ("Rust crates", vec![hashbrown(Ecosystem::Cargo)]),
            (
                "Linked into both binaries",
                vec![hashbrown(Ecosystem::Linked(Binaries::Both))],
            ),
        ],
        FINGERPRINT,
    )
    .expect("generate");
    assert!(notices.contains(
        "\n2 distinct packages in total: a package that two sources share counts once; a crate that \
         the standard library links in is counted apart from the same crate in a lock file. "
    ));
    assert!(notices.contains(
        "- `hashbrown@0.17.1` (Rust, MIT OR Apache-2.0)\n- `hashbrown@0.17.1` (linked into \
         devbox-linux and Devbox.exe, MIT OR Apache-2.0)\n"
    ));
}

#[test]
fn every_file_in_the_linked_folder_is_the_license_text_of_a_row() {
    let repo = repo();
    let mut found = Vec::new();
    files_below(&repo.join(DIR), &mut found);
    let listed: Vec<String> = LINKED.iter().flat_map(Linked::license_files).collect();
    for path in &found {
        let relative = path
            .strip_prefix(&repo)
            .expect("inside the repository")
            .to_str()
            .expect("utf-8 path")
            .replace('\\', "/");
        assert!(
            listed.contains(&relative),
            "{relative} is in xtask/notices/linked/ but no row of LINKED lists it"
        );
    }
    assert_eq!(found.len(), listed.len(), "a row lists a missing file");
}

#[test]
fn the_rust_rows_come_with_the_toolchain_of_rust_toolchain_toml() {
    let toolchain =
        fs::read_to_string(repo().join("rust-toolchain.toml")).expect("read rust-toolchain.toml");
    let stale = toolchain_mismatches(&toolchain);
    assert!(stale.is_empty(), "{}", stale.join("\n"));
}

#[test]
fn the_webview2_row_is_pinned_to_the_locked_webview2_com_sys() {
    assert!(
        LINKED
            .iter()
            .any(|row| matches!(row.pin, Pin::Crate("webview2-com-sys", _))),
        "LINKED has no row pinned to webview2-com-sys"
    );
    let lock = fs::read_to_string(repo().join("app/src-tauri/Cargo.lock"))
        .expect("read app/src-tauri/Cargo.lock");
    let stale = crate_mismatches(&lock);
    assert!(stale.is_empty(), "{}", stale.join("\n"));
}

#[test]
fn a_new_toolchain_or_webview2_com_sys_names_what_to_take_again() {
    let stale = toolchain_mismatches("[toolchain]\nchannel = \"1.99.0\"\n");
    let rust_rows = LINKED
        .iter()
        .filter(|row| matches!(row.pin, Pin::Toolchain(_)))
        .count();
    assert!(rust_rows > 0);
    assert_eq!(stale.len(), rust_rows, "{stale:#?}");
    for message in &stale {
        for named in [
            "rust-toolchain.toml pins Rust 1.99.0",
            "COPYRIGHT-library.html",
            "library/Cargo.lock",
            "musl",
            "cargo run -p xtask -- notices",
        ] {
            assert!(message.contains(named), "{named}: {message}");
        }
    }
    let lock = "[[package]]\nname = \"webview2-com-sys\"\nversion = \"0.40.0\"\n";
    let stale = crate_mismatches(lock);
    assert_eq!(stale.len(), 1, "{stale:#?}");
    for named in [
        "locks webview2-com-sys 0.40.0",
        "WEBVIEW2_VERSION",
        "Microsoft.Web.WebView2",
        "LICENSE.txt",
        "cargo run -p xtask -- notices",
    ] {
        assert!(stale[0].contains(named), "{named}: {}", stale[0]);
    }
    assert!(crate_mismatches("")[0].contains("locks webview2-com-sys at no version"));
}

#[test]
fn a_linked_component_is_listed_under_each_of_its_texts_with_its_binaries() {
    let dir = tempfile::tempdir().expect("create folder");
    // A file that the package scan would skip (no license prefix) still counts when it is named.
    fs::write(dir.path().join("COPYRIGHT"), "musl as a whole is MIT \r\n").expect("write");
    fs::write(dir.path().join("LICENSE"), "Copyright Rich Felker\n").expect("write");
    let musl = Component {
        ecosystem: Ecosystem::Linked(Binaries::Linux),
        name: "musl".into(),
        version: "1.2.5".into(),
        license: "MIT".into(),
        dir: dir.path().to_path_buf(),
        license_files: vec![dir.path().join("COPYRIGHT"), dir.path().join("LICENSE")],
    };
    let both = Component {
        ecosystem: Ecosystem::Linked(Binaries::Both),
        name: RUST_STD.into(),
        version: "1.98.1".into(),
        license: "Apache-2.0 OR MIT".into(),
        dir: dir.path().to_path_buf(),
        license_files: vec![dir.path().join("LICENSE")],
    };
    // The standard library is in both binaries: listed by both sources, counted once.
    let notices = generate(
        &[
            ("Linked into devbox-linux", vec![musl, both.clone()]),
            ("Linked into Devbox.exe", vec![both]),
        ],
        FINGERPRINT,
    )
    .expect("generate");
    assert!(notices.contains("- Linked into devbox-linux: 2\n- Linked into Devbox.exe: 1\n"));
    assert!(notices.contains("\n2 distinct packages in total"));
    assert!(notices.contains("\n```text\nmusl as a whole is MIT\n```\n"));
    assert!(notices.contains(
        "- `musl@1.2.5` (linked into devbox-linux, MIT)\n- `rust-std@1.98.1` (linked into devbox-linux \
         and Devbox.exe, Apache-2.0 OR MIT)\n\n```text\nCopyright Rich Felker\n```\n"
    ));
    assert!(notices.contains(&format!("\n\n{WITHOUT_ENTRY}\n\n## License texts\n")));
    // A named file that is missing stops the run.
    let gone = Component {
        ecosystem: Ecosystem::Linked(Binaries::Windows),
        name: "gone".into(),
        version: "1".into(),
        license: "MIT".into(),
        dir: dir.path().to_path_buf(),
        license_files: vec![dir.path().join("missing.txt")],
    };
    assert!(generate(&[("Linked", vec![gone])], FINGERPRINT).is_err());
}
