//! Test support: the captured cargo and pnpm output, with license files laid out like a registry.

use super::license_files::normalize_text;
use super::sources::CargoMetadata;
use super::*;

pub(super) const CARGO_FIXTURE: &str =
    include_str!("../../tests/fixtures/notices/cargo-metadata.json");
pub(super) const NPM_FIXTURE: &str =
    include_str!("../../tests/fixtures/notices/pnpm-licenses.json");
pub(super) const FINGERPRINT: &str = "Cargo.lock=aa app/src-tauri/Cargo.lock=bb pnpm-lock.yaml=cc";
pub(super) const APACHE: &str = "Apache License\nVersion 2.0, January 2004\n";
pub(super) const SPDX: &str = "SPDXVersion: SPDX-2.1\nDataLicense: CC0-1.0\n";
pub(super) const OFL: &str = "Copyright (c) 2021, Kil Hyung-jin, with Reserved Font Name Pretendard.\n\nSIL OPEN FONT LICENSE Version 1.1 - 26 February 2007\n";

pub(super) fn mit(holder: &str) -> String {
    format!(
        "MIT License\n\nCopyright (c) {holder}\n\nPermission is hereby granted, free of charge, \
         to any person obtaining a copy of this software.\n"
    )
}

fn put(root: &Path, path: &str, text: &str) {
    let file = root.join(path);
    fs::create_dir_all(file.parent().expect("file has a folder")).expect("create folders");
    fs::write(file, text).expect("write fixture file");
}

pub(super) fn labels(packages: &[Component]) -> Vec<String> {
    packages.iter().map(Component::label).collect()
}

/// The fixture packages with license files laid out like the cargo registry and pnpm store.
pub(super) struct Fixture {
    dir: tempfile::TempDir,
}

impl Fixture {
    pub(super) fn new() -> Self {
        let dir = tempfile::tempdir().expect("create fixture repository");
        let root = dir.path();
        let tolnay = mit("David Tolnay");
        let registry = [
            ("serde_json-1.0.154/LICENSE-APACHE", APACHE.to_string()),
            ("serde_json-1.0.154/LICENSE-MIT", tolnay.clone()),
            // The same texts with CRLF and trailing blanks: they must land in the same groups.
            ("itoa-1.0.18/LICENSE-MIT", tolnay.replace('\n', " \r\n")),
            ("itoa-1.0.18/LICENSE-APACHE", APACHE.replace('\n', "\r\n")),
            // No license text anywhere: UNLICENSE does not start like a license file, and source code
            // or markup named after a license is not license text.
            (
                "memchr-2.8.3/UNLICENSE",
                "never read: no license prefix".into(),
            ),
            (
                "memchr-2.8.3/src/license.rs",
                "never read: source code".into(),
            ),
            (
                "memchr-2.8.3/docs/license.html",
                "never read: markup".into(),
            ),
            ("tokio-1.53.2/LICENSE", mit("Tokio Contributors")),
            ("socket2-0.6.5/LICENSE-APACHE", APACHE.to_string()),
            ("socket2-0.6.5/LICENSE-MIT", mit("Alex Crichton")),
            ("windows-sys-0.61.2/license-apache-2.0", APACHE.to_string()),
            (
                "windows-sys-0.61.2/license-mit",
                mit("Microsoft Corporation"),
            ),
            ("mimalloc-0.1.52/LICENSE", mit("Octavian Oncescu")),
            // The real crate has both: its own license at the top level and, four levels down, the
            // license of the C library it vendors (v2 and v3 carry the same text). Each must be listed.
            (
                "libmimalloc-sys-0.1.49/LICENSE.txt",
                mit("Octavian Oncescu"),
            ),
            (
                "libmimalloc-sys-0.1.49/c_src/mimalloc/v2/LICENSE",
                mit("Microsoft Corporation, Daan Leijen"),
            ),
            (
                "libmimalloc-sys-0.1.49/c_src/mimalloc/v3/LICENSE",
                mit("Microsoft Corporation, Daan Leijen"),
            ),
            // Never shipped: a build dependency, dev dependencies and a crate only xtask uses.
            (
                "cc-1.8.0/LICENSE-MIT",
                "never shipped: build dependency".into(),
            ),
            (
                "tempfile-3.27.0/LICENSE-MIT",
                "never shipped: dev dependency".into(),
            ),
            (
                "fastrand-2.5.0/LICENSE-MIT",
                "never shipped: dev dependency".into(),
            ),
            (
                "ts-rs-12.0.1/LICENSE",
                "never shipped: used by xtask only".into(),
            ),
        ];
        for (path, text) in registry {
            put(root, &format!("registry/{path}"), &text);
        }
        let pnpm = "node_modules/.pnpm";
        let tauri = format!("{pnpm}/@tauri-apps+api@2.12.3/node_modules/@tauri-apps/api");
        let npm = [
            (
                format!("{pnpm}/react@19.3.0/node_modules/react/LICENSE"),
                mit("Meta Platforms"),
            ),
            (
                format!("{pnpm}/react-dom@19.3.0_react@19.3.0/node_modules/react-dom/LICENSE"),
                mit("Meta Platforms"),
            ),
            (
                format!("{pnpm}/commander@7.2.0/node_modules/commander/LICENSE"),
                mit("TJ Holowaychuk"),
            ),
            (
                format!("{pnpm}/commander@15.0.0/node_modules/commander/LICENSE"),
                mit("TJ Holowaychuk"),
            ),
            // One package, three license-like files; the SPDX document is metadata, not a license text.
            (format!("{tauri}/LICENSE-APACHE-2.0"), APACHE.to_string()),
            (format!("{tauri}/LICENSE-MIT"), mit("Tauri Programme")),
            (format!("{tauri}/LICENSE.spdx"), SPDX.to_string()),
            // Deliberately the same text as tokio's: a group spans Rust and npm packages.
            (
                format!("{pnpm}/khroma@2.1.0/node_modules/khroma/LICENSE"),
                mit("Tokio Contributors"),
            ),
            // The only license is one folder below the root, like pretendard's dist/LICENSE.txt.
            (
                format!("{pnpm}/pretendard@1.3.9/node_modules/pretendard/package.json"),
                "{}".into(),
            ),
            (
                format!("{pnpm}/pretendard@1.3.9/node_modules/pretendard/dist/LICENSE.txt"),
                OFL.to_string(),
            ),
            // A license only inside the package's own node_modules belongs to a dependency.
            (
                format!(
                    "{pnpm}/qrcode-generator@2.0.4/node_modules/qrcode-generator/node_modules/LICENSE"
                ),
                "never used: belongs to a dependency".into(),
            ),
        ];
        for (path, text) in npm {
            put(root, &path, &text);
        }
        Self { dir }
    }

    pub(super) fn root(&self) -> &Path {
        self.dir.path()
    }

    /// Shipped Rust crates (root `devbox-cli`) and npm packages, optionally fed in reverse order.
    pub(super) fn sources(&self, reversed: bool) -> Vec<(&'static str, Vec<Component>)> {
        let root = self.root().to_str().expect("utf-8 temp path");
        let mut meta: CargoMetadata =
            serde_json::from_str(&CARGO_FIXTURE.replace("@ROOT@", root)).expect("cargo fixture");
        if reversed {
            meta.reverse_order();
        }
        let mut crates = cargo_components(&meta, &["devbox-cli"], self.root()).expect("closure");
        let mut npm = npm_components(&NPM_FIXTURE.replace("@ROOT@", root)).expect("pnpm fixture");
        if reversed {
            crates.reverse();
            npm.reverse();
        }
        vec![("Rust crates", crates), ("npm packages", npm)]
    }

    pub(super) fn groups(&self) -> Groups {
        let sources = self.sources(false);
        group(scan(sources.iter().flat_map(|(_, c)| c)).expect("scan license files"))
    }
}

pub(super) fn group_of<'a>(groups: &'a Groups, text: &str) -> &'a TextGroup {
    let wanted = digest(&normalize_text(text));
    groups
        .texts
        .iter()
        .find(|g| g.digest == wanted)
        .expect("a group for the text")
}
