//! `cargo run -p xtask -- notices`: writes THIRD_PARTY_NOTICES.md, the license texts of the third-party
//! software that ships in the installer: the crates of the WSL `devbox` command, the crates of the
//! Windows app, the production npm packages of its UI, the fonts kept in this repository (`bundled`) and
//! the code the toolchain and the WebView2 SDK link into the two binaries (`linked`).
//!
//! The first line fingerprints the inputs the notices were made from: the lock files, the manifests and
//! the license texts of the bundled fonts and the linked code. `notices_inputs_are_current` recomputes it
//! (hashing only: no cargo, no pnpm, no network), so an input change that is committed without
//! regenerating this file fails `cargo test --workspace`.

mod bundled;
mod fingerprint;
#[cfg(test)]
mod fixture;
mod license_files;
mod linked;
mod sources;

use anyhow::{Context, Result};
use fingerprint::{ensure_current, header_line, input_fingerprint};
use license_files::{read_license_file, read_license_texts};
use sha2::{Digest, Sha256};
use sources::{Component, cargo_components, cargo_metadata, npm_components, pnpm_licenses};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const NOTICES_FILE: &str = "THIRD_PARTY_NOTICES.md";
/// The Windows app ships for this target only (`windows.yml` builds it on a Windows x64 runner).
const APP_TARGET: &str = "x86_64-pc-windows-msvc";

/// A shipped package and the text of its license files (empty when it has none).
struct Entry {
    component: Component,
    texts: Vec<String>,
}

/// Packages that ship one identical license text.
struct TextGroup {
    digest: String,
    text: String,
    packages: Vec<Component>,
}

struct Groups {
    /// Sorted by the digest of the text.
    texts: Vec<TextGroup>,
    /// Packages without license text, by declared SPDX expression.
    unlicensed: Vec<(String, Vec<Component>)>,
}

pub fn run() -> Result<()> {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .canonicalize()
        .context("저장소 루트를 찾지 못했습니다.")?;
    // The `devbox` command is resolved for every platform: an extra crate is harmless, a missing one is not.
    let cli = cargo_metadata(&repo, "Cargo.toml", None)?;
    // The app ships on Windows only, so its crates are resolved for the Windows target: exactly the set
    // that can be linked into Devbox.exe, which makes the list more accurate. (Resolving for every
    // platform would also need the sources of crates that only wasm32, android or apple builds use,
    // for example webpki-root-certs, and list them although they never ship.)
    let app = cargo_metadata(&repo, "app/src-tauri/Cargo.toml", Some(APP_TARGET))?;
    let command_crates = cargo_components(&cli, &["devbox-cli"], &repo)?;
    let app_crates = cargo_components(&app, &["devbox-app"], &repo)?;
    let npm_packages = npm_components(&pnpm_licenses(&repo)?)?;
    let fonts = bundled::components(&repo)?;
    let linked_linux = linked::components(&repo, sources::Binaries::Linux);
    let linked_windows = linked::components(&repo, sources::Binaries::Windows);
    let counts = (
        command_crates.len(),
        app_crates.len(),
        npm_packages.len(),
        fonts.len(),
        linked_linux.len(),
        linked_windows.len(),
    );
    let sources = [
        (
            "Rust crates of the WSL `devbox` command (`Cargo.lock`, every platform)",
            command_crates,
        ),
        (
            "Rust crates of the Windows app (`app/src-tauri/Cargo.lock`, `x86_64-pc-windows-msvc`)",
            app_crates,
        ),
        (
            "npm packages of the app UI (`pnpm-lock.yaml`, production)",
            npm_packages,
        ),
        (
            "Fonts bundled with the app UI (`app/src/assets/fonts`, listed in `xtask/src/notices/bundled.rs`)",
            fonts,
        ),
        (
            "Code the Rust toolchain links into the WSL `devbox` command (`devbox-linux`, \
             `x86_64-unknown-linux-musl`, listed in `xtask/src/notices/linked.rs`)",
            linked_linux,
        ),
        (
            "Code the Rust toolchain and the WebView2 SDK link into the Windows app (`Devbox.exe`, \
             `x86_64-pc-windows-msvc`, listed in `xtask/src/notices/linked.rs`)",
            linked_windows,
        ),
    ];
    let notices = generate(&sources, &input_fingerprint(&repo)?)?;
    fs::write(repo.join(NOTICES_FILE), &notices)
        .with_context(|| format!("{NOTICES_FILE}을 쓰지 못했습니다."))?;
    // The check `notices_inputs_are_current` makes, so a generator that disagrees with it fails here.
    ensure_current(&repo)?;
    println!(
        "notices: wrote {NOTICES_FILE} ({} bytes; packages: Rust command {}, Rust app {}, npm {}, bundled fonts {}, \
         linked into devbox-linux {}, linked into Devbox.exe {})",
        notices.len(),
        counts.0,
        counts.1,
        counts.2,
        counts.3,
        counts.4,
        counts.5
    );
    Ok(())
}

/// Reads the license files of every package, once per package even when several sources ship it.
fn scan<'a>(components: impl IntoIterator<Item = &'a Component>) -> Result<Vec<Entry>> {
    let unique: BTreeMap<_, _> = components.into_iter().map(|c| (c.sort_key(), c)).collect();
    unique
        .into_values()
        .map(|component| {
            let texts = if component.license_files.is_empty() {
                read_license_texts(&component.dir)
            } else {
                read_named_files(&component.license_files)
            }
            .with_context(|| {
                let hint = match component.ecosystem {
                    sources::Ecosystem::Font => {
                        "글꼴 라이선스 파일이 저장소에 있는지 확인하세요(`xtask/src/notices/bundled.rs`)."
                    }
                    sources::Ecosystem::Linked(_) => {
                        "라이선스 파일이 `xtask/notices/linked/`에 있는지 확인하세요(`xtask/src/notices/linked.rs`)."
                    }
                    _ => {
                        "Rust는 `cargo fetch --locked`, npm은 `pnpm install --frozen-lockfile`로 \
                         소스를 받은 뒤 다시 실행하세요."
                    }
                };
                format!(
                    "{} ({}) 라이선스 파일을 읽지 못했습니다. {hint}",
                    component.label(),
                    component.ecosystem.label()
                )
            })?;
            Ok(Entry {
                component: component.clone(),
                texts,
            })
        })
        .collect()
}

/// The texts of the named files in order, a repeated text once: every file must exist and hold text.
fn read_named_files(files: &[PathBuf]) -> Result<Vec<String>> {
    let mut texts: Vec<String> = Vec::new();
    for file in files {
        let text = read_license_file(file)?;
        if !texts.contains(&text) {
            texts.push(text);
        }
    }
    Ok(texts)
}

fn digest(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

/// One group per distinct license text. A package with several license files is in several groups;
/// a package with none is listed by its declared license instead.
fn group(entries: Vec<Entry>) -> Groups {
    let mut texts: BTreeMap<String, TextGroup> = BTreeMap::new();
    let mut unlicensed: BTreeMap<String, Vec<Component>> = BTreeMap::new();
    for Entry {
        component,
        texts: found,
    } in entries
    {
        if found.is_empty() {
            unlicensed
                .entry(component.license.clone())
                .or_default()
                .push(component);
            continue;
        }
        for text in found {
            let digest = digest(&text);
            texts
                .entry(digest.clone())
                .or_insert_with(|| TextGroup {
                    digest,
                    text,
                    packages: Vec::new(),
                })
                .packages
                .push(component.clone());
        }
    }
    let mut texts: Vec<TextGroup> = texts.into_values().collect();
    for group in &mut texts {
        group.packages.sort_by_key(Component::sort_key);
    }
    let unlicensed = unlicensed
        .into_iter()
        .map(|(license, mut packages)| {
            packages.sort_by_key(Component::sort_key);
            (license, packages)
        })
        .collect();
    Groups { texts, unlicensed }
}

/// A code fence longer than any run of backticks in the text, so the text can never close it.
fn fence_for(text: &str) -> String {
    let longest = text.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    "`".repeat(longest.max(2) + 1)
}

/// The notices for `sources` (a name and the packages of each), identical for identical input.
fn generate(sources: &[(&str, Vec<Component>)], fingerprint: &str) -> Result<String> {
    let entries = scan(sources.iter().flat_map(|(_, components)| components))?;
    let total = entries.len();
    let groups = group(entries);
    let mut out = format!("{}\n# Third-party notices\n\n", header_line(fingerprint));
    out.push_str(
        "Devbox is licensed under the MIT License (see `LICENSE`). This file lists the third-party \
         software that ships with it, with the license text each package provides. It is generated \
         by `cargo run -p xtask -- notices` from the inputs fingerprinted in its first line (the lock \
         files, the manifests and the license texts of the bundled fonts and the linked code); do not \
         edit it by hand.\n\n\
         Packages by source:\n\n",
    );
    for (source, components) in sources {
        out.push_str(&format!("- {source}: {}\n", components.len()));
    }
    out.push_str(&format!(
        "\n{total} distinct packages in total: a package that two sources share counts once. \
         Packages that ship the same license text are listed together under one copy of it, and a \
         package with several license files, for example the license of code it vendors, is listed \
         under each.\n\n{}\n\n## License texts\n\n",
        linked::WITHOUT_ENTRY
    ));
    for group in &groups.texts {
        out.push_str(&format!(
            "### License text {}\n\nPackages ({}):\n\n",
            &group.digest[..12],
            group.packages.len()
        ));
        for package in &group.packages {
            out.push_str(&format!(
                "- `{}` ({}, {})\n",
                package.label(),
                package.ecosystem.label(),
                package.license
            ));
        }
        let fence = fence_for(&group.text);
        out.push_str(&format!("\n{fence}text\n{}\n{fence}\n\n", group.text));
    }
    if !groups.unlicensed.is_empty() {
        out.push_str(
            "## Packages without license text\n\nThese packages publish no license text: no plain-text \
             `LICENSE*`, `LICENCE*`, `COPYING*`, `NOTICE*` or `OFL*` file in the package. The license \
             they declare is listed instead.\n\n",
        );
        for (license, packages) in &groups.unlicensed {
            out.push_str(&format!("### {license}\n\n"));
            for package in packages {
                out.push_str(&format!(
                    "- `{}` ({})\n",
                    package.label(),
                    package.ecosystem.label()
                ));
            }
            out.push('\n');
        }
    }
    let end = out.trim_end().len();
    out.truncate(end);
    out.push('\n');
    Ok(out)
}

#[cfg(test)]
use fixture::{APACHE, FINGERPRINT, Fixture, OFL, group_of, labels, mit};

#[test]
fn groups_packages_by_identical_license_text() {
    let groups = Fixture::new().groups();

    // The Apache text of four crates and one npm package is one group, whatever their line ends.
    assert_eq!(
        labels(&group_of(&groups, APACHE).packages),
        [
            "@tauri-apps/api@2.12.3",
            "itoa@1.0.18",
            "serde_json@1.0.154",
            "socket2@0.6.5",
            "windows-sys@0.61.2"
        ]
    );
    // itoa's MIT file has CRLF and trailing blanks, serde_json's does not: same text.
    assert_eq!(
        labels(&group_of(&groups, &mit("David Tolnay")).packages),
        ["itoa@1.0.18", "serde_json@1.0.154"]
    );
    // Two versions of one npm package, and two packages of one author.
    assert_eq!(
        labels(&group_of(&groups, &mit("TJ Holowaychuk")).packages),
        ["commander@15.0.0", "commander@7.2.0"]
    );
    assert_eq!(
        labels(&group_of(&groups, &mit("Meta Platforms")).packages),
        ["react-dom@19.3.0", "react@19.3.0"]
    );
    // A text shared by a crate and an npm package is still one group.
    assert_eq!(
        labels(&group_of(&groups, &mit("Tokio Contributors")).packages),
        ["khroma@2.1.0", "tokio@1.53.2"]
    );
    // A package with several license files is listed under each of its texts. The SPDX document
    // next to them is metadata, not a license text.
    let tauri_groups = groups
        .texts
        .iter()
        .filter(|g| g.packages.iter().any(|p| p.name == "@tauri-apps/api"))
        .count();
    assert_eq!(tauri_groups, 2);
    assert!(groups.texts.iter().all(|g| !g.text.contains("SPDXVersion")));
    // One group per distinct text; texts of packages that do not ship never appear.
    assert_eq!(groups.texts.len(), 11);
    assert!(
        groups
            .texts
            .iter()
            .all(|g| !g.text.contains("never shipped"))
    );
    // Groups are ordered by the digest of their text.
    let digests: Vec<_> = groups.texts.iter().map(|g| g.digest.as_str()).collect();
    assert!(digests.windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
fn missing_license_file_falls_back_to_spdx() {
    let fixture = Fixture::new();
    let groups = fixture.groups();
    // memchr has only source and markup files named like a license, qrcode-generator only a license
    // inside its own node_modules: neither has a license text, so both are listed by their declared
    // license, sorted by that expression.
    let fallback: Vec<(&str, Vec<String>)> = groups
        .unlicensed
        .iter()
        .map(|(license, packages)| (license.as_str(), labels(packages)))
        .collect();
    assert_eq!(
        fallback,
        [
            ("MIT", vec!["qrcode-generator@2.0.4".to_string()]),
            ("Unlicense OR MIT", vec!["memchr@2.8.3".to_string()]),
        ]
    );
    for name in ["memchr", "qrcode-generator"] {
        assert!(
            groups
                .texts
                .iter()
                .all(|g| g.packages.iter().all(|p| p.name != name))
        );
    }
    for marker in ["never read", "never used", "never shipped"] {
        assert!(groups.texts.iter().all(|g| !g.text.contains(marker)));
    }

    let notices = generate(&fixture.sources(false), FINGERPRINT).expect("generate");
    // The section is about license text, not files: a package can ship a license-like file that is
    // not text (the `LICENSE.spdx` of `@tauri-apps/plugin-opener`) and still be listed here.
    assert!(notices.contains("## Packages without license text\n"));
    assert!(notices.contains("These packages publish no license text: no plain-text "));
    assert!(!notices.contains("no license file"));
    assert!(notices.contains(
        "### MIT\n\n- `qrcode-generator@2.0.4` (npm)\n\n### Unlicense OR MIT\n\n- `memchr@2.8.3` (Rust)\n"
    ));
}

#[test]
fn vendored_code_license_is_listed_next_to_the_top_level_license() {
    let fixture = Fixture::new();
    let groups = fixture.groups();
    // libmimalloc-sys has its own license at the top level, shared with the wrapper crate mimalloc...
    assert_eq!(
        labels(&group_of(&groups, &mit("Octavian Oncescu")).packages),
        ["libmimalloc-sys@0.1.49", "mimalloc@0.1.52"]
    );
    // ...and, four levels down, the license of the C library it vendors (v2 and v3: one text).
    assert_eq!(
        labels(&group_of(&groups, &mit("Microsoft Corporation, Daan Leijen")).packages),
        ["libmimalloc-sys@0.1.49"]
    );
    assert!(
        groups
            .unlicensed
            .iter()
            .all(|(_, packages)| packages.iter().all(|p| p.name != "libmimalloc-sys"))
    );
    let notices = generate(&fixture.sources(false), FINGERPRINT).expect("generate");
    assert!(notices.contains("Copyright (c) Microsoft Corporation, Daan Leijen\n"));
}

#[test]
fn nested_license_file_is_used_when_the_top_level_has_none() {
    let fixture = Fixture::new();
    let groups = fixture.groups();
    // pretendard ships its OFL text in dist/LICENSE.txt, one folder below the package root.
    assert_eq!(
        labels(&group_of(&groups, OFL).packages),
        ["pretendard@1.3.9"]
    );
    assert!(
        groups
            .unlicensed
            .iter()
            .all(|(_, packages)| packages.iter().all(|p| p.name != "pretendard"))
    );

    // So the notices print the text of the font license, and no SPDX-only entry for the font.
    let notices = generate(&fixture.sources(false), FINGERPRINT).expect("generate");
    assert!(notices.contains("SIL OPEN FONT LICENSE Version 1.1 - 26 February 2007\n```\n"));
    assert!(notices.contains("- `pretendard@1.3.9` (npm, OFL-1.1)\n"));
    assert!(!notices.contains("### OFL-1.1"));
}

#[test]
fn a_bundled_font_is_listed_under_the_text_of_its_license() {
    let dir = tempfile::tempdir().expect("create folder");
    let license = dir.path().join("LICENSE-Font.txt");
    fs::write(
        &license,
        "Copyright (c) 2015, NAVER\r\nSIL OPEN FONT LICENSE Version 1.1 \r\n",
    )
    .expect("write license");
    let font = Component {
        ecosystem: sources::Ecosystem::Font,
        name: "D2Coding".into(),
        version: "1.4.0".into(),
        license: "OFL-1.1".into(),
        dir: dir.path().to_path_buf(),
        license_files: vec![license],
    };
    let notices =
        generate(&[("Fonts bundled with the app", vec![font])], FINGERPRINT).expect("generate");
    assert!(notices.contains("- Fonts bundled with the app: 1\n"));
    assert!(notices.contains("- `D2Coding@1.4.0` (font, OFL-1.1)\n"));
    assert!(notices.contains(
        "\n```text\nCopyright (c) 2015, NAVER\nSIL OPEN FONT LICENSE Version 1.1\n```\n"
    ));
    // A font whose license file is missing stops the run instead of being listed without its text.
    let missing = Component {
        ecosystem: sources::Ecosystem::Font,
        name: "Gone".into(),
        version: "1".into(),
        license: "OFL-1.1".into(),
        dir: dir.path().to_path_buf(),
        license_files: vec![dir.path().join("missing.txt")],
    };
    assert!(generate(&[("Fonts", vec![missing])], FINGERPRINT).is_err());
}

#[test]
fn generating_twice_gives_identical_bytes_whatever_the_input_order() {
    let fixture = Fixture::new();
    let first = generate(&fixture.sources(false), FINGERPRINT).expect("first run");
    assert_eq!(
        first,
        generate(&fixture.sources(false), FINGERPRINT).expect("second run")
    );
    assert_eq!(
        first,
        generate(&fixture.sources(true), FINGERPRINT).expect("reversed input")
    );
    // Nothing machine-specific: no checkout path, no CR.
    assert!(!first.contains(fixture.root().to_str().expect("utf-8 temp path")));
    assert!(!first.contains('\r'));
}

#[test]
fn notices_start_with_the_fingerprint_and_fence_every_text() {
    let fixture = Fixture::new();
    let notices = generate(&fixture.sources(false), FINGERPRINT).expect("generate");
    assert_eq!(
        notices.lines().next(),
        Some(header_line(FINGERPRINT).as_str())
    );
    assert_eq!(
        header_line(FINGERPRINT),
        format!("<!-- inputs: {FINGERPRINT} -->")
    );
    assert!(notices.ends_with("\n") && !notices.ends_with("\n\n"));
    assert_eq!(
        notices.matches("\n```text\n").count(),
        fixture.groups().texts.len()
    );
    // Counts per source are stated up front.
    assert!(notices.contains("- Rust crates: 8\n"));
    assert!(notices.contains("- npm packages: 8\n"));
}

#[test]
fn fence_is_longer_than_any_backtick_run_in_the_text() {
    assert_eq!(fence_for("plain"), "```");
    assert_eq!(fence_for("has ``` inside"), "````");
    assert_eq!(fence_for("`````"), "``````");
}

#[test]
fn notices_inputs_are_current() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    if let Err(error) = ensure_current(&repo) {
        panic!("{error}");
    }
}
