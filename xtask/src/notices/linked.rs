//! Code that the Rust toolchain or an SDK links into the shipped binaries. No lock file lists it, so the
//! notices list it from `LINKED`, with license texts kept in `xtask/notices/linked/`: next to the generator
//! that reads them, outside `app/` so that nothing bundles them, and outside `tests/fixtures` because they
//! are real inputs. The texts are copies of the sources below (the std statement converted to plain text).
//!
//! Where the rows come from. When `rust-toolchain.toml` or webview2-com-sys changes, the tests below fail
//! until the rows and texts are taken again from the same places:
//! - `rust-std`: the standard library statement of the toolchain, `share/doc/rust/COPYRIGHT-library.html` up
//!   to its out-of-tree section, with the license texts it names from `share/doc/rust/licenses/`.
//! - The std crates: what std links for `x86_64-unknown-linux-musl` and `x86_64-pc-windows-msvc`, from
//!   `library/std/Cargo.toml` and `library/Cargo.lock` at the toolchain's tag of rust-lang/rust, with the
//!   `backtrace` feature its releases build, checked against the rlibs in `lib/rustlib/<target>/lib`. The
//!   out-of-tree list of that HTML is not used: it also names the stdarch tools and other targets' crates,
//!   and it leaves out the backtrace crates (addr2line, adler2, miniz_oxide, object, memchr). The texts are
//!   the crate's top-level files whose names contain copyright, licence, license, author or notice (the rule
//!   the HTML is generated with), from the `.crate` whose sha256 is the checksum in that lock file.
//! - musl: `COPYRIGHT` of the release the toolchain's `libc.a` is built from (`MUSL_VER` in rust-lang/rust's
//!   `src/ci/docker/scripts/musl-toolchain.sh`, `__libc_version` in `libc.a`).
//! - WebView2: `LICENSE.txt` of the `Microsoft.Web.WebView2` NuGet package whose `WebView2LoaderStatic.lib`
//!   webview2-com-sys vendors (`WEBVIEW2_VERSION` in `crates/update-bindings` of wravery/webview2-rs).
//!   Devbox.exe links that library statically.

use super::sources::{Binaries, Component, Ecosystem};
use std::path::Path;

/// The folder of the license texts, relative to the repository root.
pub const DIR: &str = "xtask/notices/linked";
/// The row of the standard library itself, whose version is the toolchain's.
const RUST_STD: &str = "rust-std";

/// The linked code that needs no entry, and why. The notices print it below the list of sources.
pub const WITHOUT_ENTRY: &str = "Linked code without an entry of its own: the LLVM code the Rust toolchain \
    links in (libunwind and compiler-rt's crtbegin and crtend objects in `devbox-linux`, and the compiler-rt \
    builtins `compiler_builtins` may add to either binary) is Apache-2.0 WITH LLVM-exception, whose exception \
    lets portions compiled into a program be redistributed without the license's notice conditions, and \
    musl's COPYRIGHT (below) lets its crt files and public headers be used without its notice.";

/// What fixes the version of a row, so that a test can tell when the row is stale.
#[cfg_attr(not(test), allow(dead_code))] // Only the freshness tests read it.
pub enum Pin {
    /// It comes with the Rust toolchain of `rust-toolchain.toml`, at this version.
    Toolchain(&'static str),
    /// This crate of `app/src-tauri/Cargo.lock`, at this version, vendors it.
    Crate(&'static str, &'static str),
}

/// A component that the toolchain or an SDK links into the shipped binaries.
pub struct Linked {
    pub name: &'static str,
    pub version: &'static str,
    /// SPDX expression of its license.
    pub license: &'static str,
    pub binaries: Binaries,
    #[cfg_attr(not(test), allow(dead_code))] // Only the freshness tests read it.
    pub pin: Pin,
    /// Its folder in `DIR`.
    pub dir: &'static str,
    /// Its license texts, in that folder.
    pub files: &'static [&'static str],
}

impl Linked {
    /// The license texts, relative to the repository root.
    pub fn license_files(&self) -> impl Iterator<Item = String> + '_ {
        self.files
            .iter()
            .map(move |file| format!("{DIR}/{}/{file}", self.dir))
    }
}

/// Apache-2.0 and MIT, the texts of most std crates.
const APACHE_MIT: &[&str] = &["LICENSE-APACHE", "LICENSE-MIT"];

pub const LINKED: [Linked; 13] = [
    Linked {
        name: RUST_STD,
        version: "1.98.1",
        license: "(Apache-2.0 OR MIT) AND Unicode-3.0",
        binaries: Binaries::Both,
        pin: Pin::Toolchain("1.98.1"),
        dir: "rust-std-1.98.1",
        files: &[
            "COPYRIGHT-library-in-tree.txt",
            "licenses/Apache-2.0.txt",
            "licenses/MIT.txt",
            "licenses/Unicode-3.0.txt",
            "licenses/BSD-2-Clause.txt",
        ],
    },
    Linked {
        name: "addr2line",
        version: "0.25.1",
        license: "Apache-2.0 OR MIT",
        binaries: Binaries::Linux,
        pin: Pin::Toolchain("1.98.1"),
        dir: "rust-std-1.98.1/addr2line-0.25.1",
        files: APACHE_MIT,
    },
    Linked {
        name: "adler2",
        version: "2.0.1",
        license: "0BSD OR MIT OR Apache-2.0",
        binaries: Binaries::Linux,
        pin: Pin::Toolchain("1.98.1"),
        dir: "rust-std-1.98.1/adler2-2.0.1",
        files: &["LICENSE-0BSD", "LICENSE-APACHE", "LICENSE-MIT"],
    },
    Linked {
        name: "cfg-if",
        version: "1.0.4",
        license: "MIT OR Apache-2.0",
        binaries: Binaries::Both,
        pin: Pin::Toolchain("1.98.1"),
        dir: "rust-std-1.98.1/cfg-if-1.0.4",
        files: APACHE_MIT,
    },
    Linked {
        name: "gimli",
        version: "0.32.3",
        license: "MIT OR Apache-2.0",
        binaries: Binaries::Linux,
        pin: Pin::Toolchain("1.98.1"),
        dir: "rust-std-1.98.1/gimli-0.32.3",
        files: APACHE_MIT,
    },
    Linked {
        name: "hashbrown",
        version: "0.17.1",
        license: "MIT OR Apache-2.0",
        binaries: Binaries::Both,
        pin: Pin::Toolchain("1.98.1"),
        dir: "rust-std-1.98.1/hashbrown-0.17.1",
        files: APACHE_MIT,
    },
    Linked {
        name: "libc",
        version: "0.2.185",
        license: "MIT OR Apache-2.0",
        binaries: Binaries::Linux,
        pin: Pin::Toolchain("1.98.1"),
        dir: "rust-std-1.98.1/libc-0.2.185",
        files: APACHE_MIT,
    },
    Linked {
        name: "memchr",
        version: "2.7.6",
        license: "Unlicense OR MIT",
        binaries: Binaries::Linux,
        pin: Pin::Toolchain("1.98.1"),
        dir: "rust-std-1.98.1/memchr-2.7.6",
        files: &["LICENSE-MIT", "UNLICENSE"],
    },
    Linked {
        name: "miniz_oxide",
        version: "0.8.9",
        license: "MIT OR Zlib OR Apache-2.0",
        binaries: Binaries::Linux,
        pin: Pin::Toolchain("1.98.1"),
        dir: "rust-std-1.98.1/miniz_oxide-0.8.9",
        files: &[
            "LICENSE",
            "LICENSE-APACHE.md",
            "LICENSE-MIT.md",
            "LICENSE-ZLIB.md",
        ],
    },
    Linked {
        name: "object",
        version: "0.37.3",
        license: "Apache-2.0 OR MIT",
        binaries: Binaries::Linux,
        pin: Pin::Toolchain("1.98.1"),
        dir: "rust-std-1.98.1/object-0.37.3",
        files: APACHE_MIT,
    },
    Linked {
        name: "rustc-demangle",
        version: "0.1.27",
        license: "MIT/Apache-2.0",
        binaries: Binaries::Both,
        pin: Pin::Toolchain("1.98.1"),
        dir: "rust-std-1.98.1/rustc-demangle-0.1.27",
        files: APACHE_MIT,
    },
    // musl 1.2.5 with the CVE patches of the toolchain's build, none of which touches COPYRIGHT.
    Linked {
        name: "musl",
        version: "1.2.5",
        license: "MIT",
        binaries: Binaries::Linux,
        pin: Pin::Toolchain("1.98.1"),
        dir: "musl-1.2.5",
        files: &["COPYRIGHT"],
    },
    // The WebView2 SDK that webview2-com-sys 0.39.1 vendors `x64/WebView2LoaderStatic.lib` from.
    Linked {
        name: "Microsoft.Web.WebView2",
        version: "1.0.3800.47",
        license: "BSD-3-Clause",
        binaries: Binaries::Windows,
        pin: Pin::Crate("webview2-com-sys", "0.39.1"),
        dir: "Microsoft.Web.WebView2-1.0.3800.47",
        files: &["LICENSE.txt"],
    },
];

/// The rows linked into `binary` (`Linux` or `Windows`), as notices entries with their license files.
pub fn components(repo: &Path, binary: Binaries) -> Vec<Component> {
    LINKED
        .iter()
        .filter(|row| row.binaries == binary || row.binaries == Binaries::Both)
        .map(|row| Component {
            ecosystem: Ecosystem::Linked(row.binaries),
            name: row.name.to_string(),
            version: row.version.to_string(),
            license: row.license.to_string(),
            dir: repo.join(DIR).join(row.dir),
            license_files: row.license_files().map(|file| repo.join(file)).collect(),
        })
        .collect()
}

/// One message per row that does not come with the Rust of `toolchain` (the text of `rust-toolchain.toml`),
/// saying what to take again.
#[cfg(test)]
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

/// The versions `lock` (the text of a Cargo.lock) has of the package `name`.
#[cfg(test)]
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
#[cfg(test)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notices::fixture::FINGERPRINT;
    use crate::notices::license_files::read_license_file;
    use crate::notices::{fence_for, generate};
    use std::fs;
    use std::path::PathBuf;

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
        // The three components the notices must cover (user decision 2026-11-04).
        for name in [RUST_STD, "musl", "Microsoft.Web.WebView2"] {
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
        let toolchain = fs::read_to_string(repo().join("rust-toolchain.toml"))
            .expect("read rust-toolchain.toml");
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
}
