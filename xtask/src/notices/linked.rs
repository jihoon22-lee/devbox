//! Code that the Rust toolchain or an SDK links into the shipped binaries. No lock file lists it, so the
//! notices list it from `LINKED`, with license texts kept in `xtask/notices/linked/`: next to the generator
//! that reads them, outside `app/` so that nothing bundles them, and outside `tests/fixtures` because they
//! are real inputs. The texts are copies of the sources below (the std statement converted to plain text).
//!
//! Where the rows come from. When `rust-toolchain.toml` or webview2-com-sys changes, the tests in
//! `linked/tests.rs` fail until the rows, their folders and texts are taken again from the same places:
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

use super::sources::{Binaries, Component, Ecosystem};
use std::path::Path;

/// The folder of the license texts, relative to the repository root.
pub const DIR: &str = "xtask/notices/linked";
/// The row of the standard library itself, whose version is the toolchain's.
const RUST_STD: &str = "rust-std";
/// The Rust toolchain the Rust rows come with (`channel` of `rust-toolchain.toml`).
const TOOLCHAIN: &str = "1.98.1";

/// The linked code that has no entry, and why. The notices print it below the list of sources. The
/// Visual C++ runtime part states facts only (`staticVCRuntime` defaults to true in tauri-utils, and
/// `app/src-tauri/build.rs` keeps the default); its license terms are a question for the user.
pub const WITHOUT_ENTRY: &str = "Linked code without an entry of its own: the LLVM code the Rust toolchain \
    links in (libunwind and compiler-rt's crtbegin and crtend objects in `devbox-linux`, and the compiler-rt C \
    builtins compiled into `compiler_builtins`, which either binary may contain) is Apache-2.0 WITH \
    LLVM-exception, whose exception lets portions compiled into a program be redistributed without the \
    license's notice conditions, and musl's COPYRIGHT (below) lets its crt files and public headers be used \
    without its notice. `Devbox.exe` also contains the Microsoft Visual C++ runtime (libcmt and \
    libvcruntime), which Tauri links statically by default (`staticVCRuntime`), and calls the Universal C \
    Runtime that Windows provides. Both come from Microsoft's build tools and Windows, not under an \
    open-source license.";

/// What fixes the version of a row, so that a test can tell when the row is stale.
#[derive(Clone, Copy)]
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

pub const LINKED: [Linked; 14] = [
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

#[cfg(test)]
mod tests;
