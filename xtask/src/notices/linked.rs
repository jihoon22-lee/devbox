//! Code that the Rust toolchain or an SDK links into the shipped binaries, and the code that Tauri's
//! bundler puts into the installer. No lock file lists it, so the notices list it from `LINKED`, with
//! license texts kept in `xtask/notices/linked/`: next to the generator that reads them, outside `app/` so
//! that nothing bundles them, and outside `tests/fixtures` because they are real inputs. The texts are
//! copies of the sources below (the std statement converted to plain text, NSIS's `COPYING` with LF ends).
//!
//! Where the rows come from. When `rust-toolchain.toml`, webview2-com-sys or `@tauri-apps/cli` changes, the
//! tests in `linked/tests.rs` fail until the rows, their folders and texts are taken again from the same
//! places:
//! - `rust-std`: the standard library statement of the toolchain, `share/doc/rust/COPYRIGHT-library.html` up
//!   to its out-of-tree section, with the license texts it names from `share/doc/rust/licenses/`.
//! - The std crates: what std links for `x86_64-unknown-linux-musl` and `x86_64-pc-windows-msvc`, from
//!   `library/std/Cargo.toml` and `library/Cargo.lock` at the toolchain's tag of rust-lang/rust, with the
//!   `backtrace` feature its releases build, checked against the rlibs in `lib/rustlib/<target>/lib`. The
//!   out-of-tree list of that HTML is not used: it also names the stdarch tools and other targets' crates,
//!   and it leaves out the backtrace crates (addr2line, adler2, miniz_oxide, object, memchr). The texts are
//!   the crate's top-level files whose names contain copyright, licence, license, author or notice (the rule
//!   the HTML is generated with), from the `.crate` whose sha256 is the checksum in that lock file. In-tree
//!   crates with a LICENSE of their own get a row too, with that LICENSE copied from the tag, because the
//!   HTML folds them into `.`: compiler_builtins (`library/compiler-builtins/LICENSE.txt` and its
//!   `libm/LICENSE.txt`).
//! - musl: `COPYRIGHT` of the release the toolchain's `libc.a` is built from (`MUSL_VER` in rust-lang/rust's
//!   `src/ci/docker/scripts/musl-toolchain.sh`, `__libc_version` in `libc.a`).
//! - WebView2: `LICENSE.txt` of the `Microsoft.Web.WebView2` NuGet package whose `WebView2LoaderStatic.lib`
//!   webview2-com-sys vendors (`WEBVIEW2_VERSION` in `crates/update-bindings` of wravery/webview2-rs).
//!   Devbox.exe links that library statically.
//! - The installer: what the bundler of the locked `@tauri-apps/cli` downloads, as named at the CLI's tag
//!   of tauri-apps/tauri in `crates/tauri-bundler/src/bundle/windows/nsis/mod.rs`. `NSIS_URL` is the NSIS
//!   zip (checked with `NSIS_SHA1`), whose `COPYING` is the text; it equals `COPYING` at that NSIS tag of
//!   nsis-dev/nsis (`v311`) but for line ends. `NSIS_TAURI_UTILS_URL` is `nsis_tauri_utils.dll` (checked with
//!   `NSIS_TAURI_UTILS_SHA1`), whose texts are `LICENSE_MIT` and `LICENSE_APACHE-2.0` of
//!   tauri-apps/nsis-tauri-utils at the DLL's tag (`nsis_tauri_utils-v0.5.3`).

use super::sources::{Binaries, Component, Ecosystem};
use std::path::Path;

/// The folder of the license texts, relative to the repository root.
pub const DIR: &str = "xtask/notices/linked";
/// The row of the standard library itself, whose version is the toolchain's.
const RUST_STD: &str = "rust-std";
/// The Rust toolchain the Rust rows come with (`channel` of `rust-toolchain.toml`).
const TOOLCHAIN: &str = "1.98.1";
/// The npm package of the Tauri CLI, whose bundler builds the installer.
const TAURI_CLI_PACKAGE: &str = "@tauri-apps/cli";
/// The Tauri CLI the installer rows come with (`@tauri-apps/cli` in `pnpm-lock.yaml`).
const TAURI_CLI: &str = "2.12.2";

/// The linked code that has no entry, and why, then the facts of the installer that its rows do not show.
/// The notices print it below the list of sources, above the license texts. Both parts state facts only.
/// For the Visual C++ runtime, `staticVCRuntime` defaults to true in tauri-utils and `app/src-tauri/build.rs`
/// keeps the default; its license terms are a question for the user. For the installer, as checked at the
/// Tauri CLI's tag and in the two downloads (branch review I-1): `tauri.conf.json` sets no NSIS compression,
/// and `nsis_tauri_utils.dll` is `#![no_std]`, imports only Windows system DLLs and starts at its own
/// `DllMain`.
pub const WITHOUT_ENTRY: &str = "Linked code without an entry of its own: the LLVM code the Rust toolchain \
    links in (libunwind and compiler-rt's crtbegin and crtend objects in `devbox-linux`, and the compiler-rt C \
    builtins compiled into `compiler_builtins`, which either binary may contain) is Apache-2.0 WITH \
    LLVM-exception, whose exception lets portions compiled into a program be redistributed without the \
    license's notice conditions, and musl's COPYRIGHT (below) lets its crt files and public headers be used \
    without its notice. `Devbox.exe` also contains the Microsoft Visual C++ runtime (libcmt and \
    libvcruntime), which Tauri links statically by default (`staticVCRuntime`), and calls the Universal C \
    Runtime that Windows provides. Both come from Microsoft's build tools and Windows, not under an \
    open-source license.\n\n\
    The installer `devbox-<version>-setup.exe` and the `uninstall.exe` it writes into the install folder are \
    built by Tauri's bundler in `@tauri-apps/cli` 2.12.2 with NSIS 3.11, using NSIS's LZMA compression, \
    Tauri's default (Devbox sets none). Their script is generated from the bundler's NSIS templates: \
    tauri-bundler 2.10.2 is Apache-2.0 OR MIT, and its license files name the Cargo-Bundle developers and the \
    Tauri Apps Contributors, whose MIT text is that of the `tauri` crates below. `nsis_tauri_utils.dll` is a \
    Rust DLL built without `std` (`#![no_std]`): it contains Rust's `core` and `alloc` libraries and the \
    `semver` crate, and links no C runtime. NSIS 3.11's source is at \
    https://github.com/nsis-dev/nsis/tree/v311, which mirrors the NSIS project at https://nsis.sourceforge.io.";

/// What fixes the version of a row, so that a test can tell when the row is stale.
#[derive(Clone, Copy)]
#[cfg_attr(not(test), allow(dead_code))] // Only the freshness tests read it.
pub enum Pin {
    /// It comes with the Rust toolchain of `rust-toolchain.toml`, at this version.
    Toolchain(&'static str),
    /// This crate of `app/src-tauri/Cargo.lock`, at this version, vendors it.
    Crate(&'static str, &'static str),
    /// This npm package of `pnpm-lock.yaml`, at this version, puts it into the installer.
    Npm(&'static str, &'static str),
}

/// A component that the toolchain or an SDK links into the shipped binaries, or that Tauri's bundler puts
/// into the installer.
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

pub const LINKED: [Linked; 16] = [
    Linked {
        name: RUST_STD,
        version: TOOLCHAIN,
        license: "(Apache-2.0 OR MIT) AND Unicode-3.0",
        binaries: Binaries::Both,
        pin: Pin::Toolchain(TOOLCHAIN),
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
        pin: Pin::Toolchain(TOOLCHAIN),
        dir: "rust-std-1.98.1/addr2line-0.25.1",
        files: APACHE_MIT,
    },
    Linked {
        name: "adler2",
        version: "2.0.1",
        license: "0BSD OR MIT OR Apache-2.0",
        binaries: Binaries::Linux,
        pin: Pin::Toolchain(TOOLCHAIN),
        dir: "rust-std-1.98.1/adler2-2.0.1",
        files: &["LICENSE-0BSD", "LICENSE-APACHE", "LICENSE-MIT"],
    },
    Linked {
        name: "cfg-if",
        version: "1.0.4",
        license: "MIT OR Apache-2.0",
        binaries: Binaries::Both,
        pin: Pin::Toolchain(TOOLCHAIN),
        dir: "rust-std-1.98.1/cfg-if-1.0.4",
        files: APACHE_MIT,
    },
    // In-tree (`library/compiler-builtins`), with a license of its own: its Rust code and its libm.
    Linked {
        name: "compiler_builtins",
        version: "0.1.160",
        license: "MIT AND Apache-2.0 WITH LLVM-exception AND (MIT OR Apache-2.0)",
        binaries: Binaries::Both,
        pin: Pin::Toolchain(TOOLCHAIN),
        dir: "rust-std-1.98.1/compiler_builtins-0.1.160",
        files: &["LICENSE.txt", "libm/LICENSE.txt"],
    },
    Linked {
        name: "gimli",
        version: "0.32.3",
        license: "MIT OR Apache-2.0",
        binaries: Binaries::Linux,
        pin: Pin::Toolchain(TOOLCHAIN),
        dir: "rust-std-1.98.1/gimli-0.32.3",
        files: APACHE_MIT,
    },
    Linked {
        name: "hashbrown",
        version: "0.17.1",
        license: "MIT OR Apache-2.0",
        binaries: Binaries::Both,
        pin: Pin::Toolchain(TOOLCHAIN),
        dir: "rust-std-1.98.1/hashbrown-0.17.1",
        files: APACHE_MIT,
    },
    Linked {
        name: "libc",
        version: "0.2.185",
        license: "MIT OR Apache-2.0",
        binaries: Binaries::Linux,
        pin: Pin::Toolchain(TOOLCHAIN),
        dir: "rust-std-1.98.1/libc-0.2.185",
        files: APACHE_MIT,
    },
    Linked {
        name: "memchr",
        version: "2.7.6",
        license: "Unlicense OR MIT",
        binaries: Binaries::Linux,
        pin: Pin::Toolchain(TOOLCHAIN),
        dir: "rust-std-1.98.1/memchr-2.7.6",
        files: &["LICENSE-MIT", "UNLICENSE"],
    },
    Linked {
        name: "miniz_oxide",
        version: "0.8.9",
        license: "MIT OR Zlib OR Apache-2.0",
        binaries: Binaries::Linux,
        pin: Pin::Toolchain(TOOLCHAIN),
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
        pin: Pin::Toolchain(TOOLCHAIN),
        dir: "rust-std-1.98.1/object-0.37.3",
        files: APACHE_MIT,
    },
    Linked {
        name: "rustc-demangle",
        version: "0.1.27",
        license: "MIT OR Apache-2.0",
        binaries: Binaries::Both,
        pin: Pin::Toolchain(TOOLCHAIN),
        dir: "rust-std-1.98.1/rustc-demangle-0.1.27",
        files: APACHE_MIT,
    },
    // musl 1.2.5 with the CVE patches of the toolchain's build, none of which touches COPYRIGHT.
    Linked {
        name: "musl",
        version: "1.2.5",
        license: "MIT",
        binaries: Binaries::Linux,
        pin: Pin::Toolchain(TOOLCHAIN),
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
    // NSIS 3.11 (`NSIS_URL`), which builds the installer and its `uninstall.exe`: its installer code, its
    // LZMA decompressor and the plug-ins the script calls. The SPDX ids are those whose templates its
    // `COPYING` matches: the zlib/libpng, bzip2 and CPL-1.0 texts and the LZMA exception.
    Linked {
        name: "nsis",
        version: "3.11",
        license: "Zlib AND bzip2-1.0.6 AND CPL-1.0 WITH LZMA-exception",
        binaries: Binaries::Installer,
        pin: Pin::Npm(TAURI_CLI_PACKAGE, TAURI_CLI),
        dir: "nsis-3.11",
        files: &["COPYING"],
    },
    // The NSIS plug-in of tauri-apps (`NSIS_TAURI_UTILS_URL`) that the installer and uninstaller call. It
    // declares `MIT or Apache-2.0`, written here in SPDX's case.
    Linked {
        name: "nsis_tauri_utils",
        version: "0.5.3",
        license: "MIT OR Apache-2.0",
        binaries: Binaries::Installer,
        pin: Pin::Npm(TAURI_CLI_PACKAGE, TAURI_CLI),
        dir: "nsis_tauri_utils-0.5.3",
        files: &["LICENSE_MIT", "LICENSE_APACHE-2.0"],
    },
];

/// The rows in `binary` (`Linux`, `Windows` or `Installer`), as notices entries with their license files.
pub fn components(repo: &Path, binary: Binaries) -> Vec<Component> {
    LINKED
        .iter()
        .filter(|row| row.binaries.includes(binary))
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

#[cfg(test)]
mod tests;
