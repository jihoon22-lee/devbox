//! Where the notices come from: the Cargo crates and npm packages that ship, and their license files.

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Shown when a package does not declare a license at all.
const UNDECLARED: &str = "not declared";
const LICENSE_FILE_PREFIXES: [&str; 4] = ["license", "licence", "copying", "notice"];

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Ecosystem {
    Cargo,
    Npm,
}

impl Ecosystem {
    pub fn label(self) -> &'static str {
        match self {
            Ecosystem::Cargo => "Rust",
            Ecosystem::Npm => "npm",
        }
    }
}

/// One third-party package that ships with Devbox.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Component {
    pub ecosystem: Ecosystem,
    pub name: String,
    pub version: String,
    /// SPDX expression as the package declares it.
    pub license: String,
    /// Package root, where its license files live.
    pub dir: PathBuf,
}

impl Component {
    pub fn label(&self) -> String {
        format!("{}@{}", self.name, self.version)
    }

    /// Order of every package list: by `name@version`, then ecosystem.
    pub fn sort_key(&self) -> (String, Ecosystem) {
        (self.label(), self.ecosystem)
    }
}

/// The parts of `cargo metadata --format-version 1` that the notices use.
#[derive(Debug, Deserialize)]
pub struct CargoMetadata {
    packages: Vec<CargoPackage>,
    workspace_members: Vec<String>,
    resolve: Resolve,
}

#[derive(Debug, Deserialize)]
struct CargoPackage {
    id: String,
    name: String,
    version: String,
    license: Option<String>,
    source: Option<String>,
    manifest_path: PathBuf,
}

#[derive(Debug, Deserialize)]
struct Resolve {
    nodes: Vec<Node>,
}

#[derive(Debug, Deserialize)]
struct Node {
    id: String,
    deps: Vec<Dep>,
}

#[derive(Debug, Deserialize)]
struct Dep {
    pkg: String,
    dep_kinds: Vec<DepKind>,
}

#[derive(Debug, Deserialize)]
struct DepKind {
    /// `None` is a normal dependency; build and dev dependencies never ship.
    kind: Option<String>,
}

impl CargoMetadata {
    /// Reorders everything cargo reports, to prove the output does not depend on that order.
    #[cfg(test)]
    pub fn reverse_order(&mut self) {
        self.packages.reverse();
        self.workspace_members.reverse();
        self.resolve.nodes.reverse();
        for node in &mut self.resolve.nodes {
            node.deps.reverse();
        }
    }
}

/// `cargo metadata` of `manifest` (relative to `repo`), for `platform` only when given.
///
/// It must not touch the network: `--offline`, so sources that are missing locally fail with the
/// `cargo fetch` hint instead of being downloaded.
pub fn cargo_metadata(
    repo: &Path,
    manifest: &str,
    platform: Option<&str>,
) -> Result<CargoMetadata> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(cargo);
    command.current_dir(repo).args([
        "metadata",
        "--format-version",
        "1",
        "--locked",
        "--offline",
        "--manifest-path",
        manifest,
    ]);
    if let Some(platform) = platform {
        command.args(["--filter-platform", platform]);
    }
    let output = command.output().context("cargo를 실행하지 못했습니다.")?;
    if !output.status.success() {
        return Err(metadata_failure(
            manifest,
            &String::from_utf8_lossy(&output.stderr),
        ));
    }
    serde_json::from_slice(&output.stdout)
        .with_context(|| format!("{manifest}의 cargo metadata 출력을 읽지 못했습니다."))
}

fn metadata_failure(manifest: &str, stderr: &str) -> anyhow::Error {
    anyhow!(
        "`cargo metadata`가 {manifest}에서 실패했습니다.\n{}\n받지 못한 crate가 있으면 \
         `cargo fetch --locked --manifest-path {manifest}`로 받은 뒤 다시 실행하세요. \
         이 명령은 네트워크를 쓰지 않습니다.",
        stderr.trim()
    )
}

/// The JSON of `pnpm --filter app licenses list --prod --json`, which reads the installed `node_modules`.
pub fn pnpm_licenses(repo: &Path) -> Result<String> {
    if !repo.join("node_modules/.pnpm").is_dir() {
        bail!(
            "node_modules가 없습니다. `pnpm install --frozen-lockfile`로 설치한 뒤 다시 실행하세요."
        );
    }
    let output = Command::new("pnpm")
        .current_dir(repo)
        .env("npm_config_offline", "true")
        .args(["--filter", "app", "licenses", "list", "--prod", "--json"])
        .output()
        .context("pnpm을 실행하지 못했습니다. Node 24와 pnpm 9가 필요합니다.")?;
    if !output.status.success() {
        bail!(
            "`pnpm licenses`가 실패했습니다.\n{}\n`pnpm install --frozen-lockfile`로 node_modules를 \
             맞춘 뒤 다시 실행하세요.",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    String::from_utf8(output.stdout).context("pnpm licenses 출력이 UTF-8이 아닙니다.")
}

/// Third-party crates that the `roots` (workspace members that ship) link through normal dependencies.
///
/// Dev and build dependencies are not followed. Dependencies behind a `target` cfg are, because the
/// `devbox` command ships on Linux and the app on Windows: an extra crate is harmless, a missing one is not.
/// Path crates inside `repo` are this project's own code (covered by LICENSE) and are left out.
pub fn cargo_components(
    meta: &CargoMetadata,
    roots: &[&str],
    repo: &Path,
) -> Result<Vec<Component>> {
    let packages: HashMap<&str, &CargoPackage> =
        meta.packages.iter().map(|p| (p.id.as_str(), p)).collect();
    let nodes: HashMap<&str, &Node> = meta
        .resolve
        .nodes
        .iter()
        .map(|n| (n.id.as_str(), n))
        .collect();
    let mut pending = Vec::new();
    for root in roots {
        let member = meta
            .workspace_members
            .iter()
            .map(String::as_str)
            .find(|id| packages.get(id).is_some_and(|p| p.name == *root));
        let Some(member) = member else {
            bail!("워크스페이스 멤버 `{root}`를 cargo metadata에서 찾지 못했습니다.");
        };
        pending.push(member);
    }
    let mut reached: BTreeSet<&str> = pending.iter().copied().collect();
    while let Some(id) = pending.pop() {
        let node = nodes
            .get(id)
            .with_context(|| format!("cargo metadata에 {id}의 resolve 항목이 없습니다."))?;
        for dep in &node.deps {
            let normal = dep.dep_kinds.iter().any(|k| k.kind.is_none());
            if normal && reached.insert(dep.pkg.as_str()) {
                pending.push(dep.pkg.as_str());
            }
        }
    }
    let mut components = Vec::new();
    for id in reached {
        let package = packages
            .get(id)
            .with_context(|| format!("cargo metadata에 {id} 패키지가 없습니다."))?;
        if package.source.is_none() && package.manifest_path.starts_with(repo) {
            continue;
        }
        components.push(Component {
            ecosystem: Ecosystem::Cargo,
            name: package.name.clone(),
            version: package.version.clone(),
            license: declared(package.license.as_deref()),
            dir: package
                .manifest_path
                .parent()
                .with_context(|| format!("{}의 manifest 경로가 비어 있습니다.", package.name))?
                .to_path_buf(),
        });
    }
    components.sort_by_key(Component::sort_key);
    Ok(components)
}

/// One entry of `pnpm licenses list --json`: a name with one folder per installed version.
#[derive(Debug, Deserialize)]
struct NpmPackage {
    name: String,
    versions: Vec<String>,
    paths: Vec<PathBuf>,
    license: String,
}

/// Production npm packages from `pnpm --filter app licenses list --prod --json`.
pub fn npm_components(json: &str) -> Result<Vec<Component>> {
    let by_license: BTreeMap<String, Vec<NpmPackage>> =
        serde_json::from_str(json).context("pnpm licenses 출력을 읽지 못했습니다.")?;
    let mut components = Vec::new();
    for package in by_license.into_values().flatten() {
        if package.versions.len() != package.paths.len() {
            bail!(
                "pnpm licenses: {}의 버전 {}개에 폴더 {}개가 있습니다.",
                package.name,
                package.versions.len(),
                package.paths.len()
            );
        }
        for (version, dir) in package.versions.iter().zip(&package.paths) {
            components.push(Component {
                ecosystem: Ecosystem::Npm,
                name: package.name.clone(),
                version: version.clone(),
                license: declared(Some(&package.license)),
                dir: dir.clone(),
            });
        }
    }
    components.sort_by_key(Component::sort_key);
    Ok(components)
}

fn declared(license: Option<&str>) -> String {
    match license.map(str::trim) {
        Some(license) if !license.is_empty() => license.to_string(),
        _ => UNDECLARED.to_string(),
    }
}

/// Normalized text of the `LICENSE*`, `LICENCE*`, `COPYING*` and `NOTICE*` files in `dir` (top level only).
///
/// The files are matched case-insensitively and read in file-name order. Blank files and repeats of
/// a text the package already has are dropped.
pub fn read_license_texts(dir: &Path) -> Result<Vec<String>> {
    let mut files = Vec::new();
    let entries = fs::read_dir(dir)
        .with_context(|| format!("패키지 폴더를 읽지 못했습니다: {}", dir.display()))?;
    for entry in entries {
        let entry =
            entry.with_context(|| format!("패키지 폴더를 읽지 못했습니다: {}", dir.display()))?;
        let name = entry.file_name().to_string_lossy().to_lowercase();
        let path = entry.path();
        if LICENSE_FILE_PREFIXES
            .iter()
            .any(|prefix| name.starts_with(prefix))
            && path.is_file()
        {
            files.push((name, path));
        }
    }
    files.sort();
    let mut texts: Vec<String> = Vec::new();
    for (_, path) in files {
        let bytes = fs::read(&path)
            .with_context(|| format!("라이선스 파일을 읽지 못했습니다: {}", path.display()))?;
        let text = normalize_text(&String::from_utf8_lossy(&bytes));
        if !text.is_empty() && !texts.contains(&text) {
            texts.push(text);
        }
    }
    Ok(texts)
}

/// CRLF to LF, trailing whitespace trimmed, so the same license compares equal whatever its checkout.
pub fn normalize_text(raw: &str) -> String {
    let raw = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    let lines: Vec<&str> = raw.lines().map(str::trim_end).collect();
    lines.join("\n").trim_matches('\n').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const CARGO_FIXTURE: &str = include_str!("../../tests/fixtures/notices/cargo-metadata.json");
    const NPM_FIXTURE: &str = include_str!("../../tests/fixtures/notices/pnpm-licenses.json");

    fn cargo_fixture(root: &str) -> CargoMetadata {
        serde_json::from_str(&CARGO_FIXTURE.replace("@ROOT@", root)).expect("cargo fixture parses")
    }

    fn labels(components: &[Component]) -> Vec<String> {
        components.iter().map(Component::label).collect()
    }

    #[test]
    fn follows_only_normal_dependencies_from_the_shipped_root() {
        let components =
            cargo_components(&cargo_fixture("/repo"), &["devbox-cli"], Path::new("/repo"))
                .expect("closure");
        // Left out: devbox-cli and devbox-core (this repository, covered by LICENSE), `cc` (build
        // dependency of libmimalloc-sys), `tempfile` and `fastrand` (dev dependencies) and `ts-rs`
        // (only the non-shipping xtask uses it). `windows-sys` is a `cfg(windows)` normal
        // dependency and stays.
        assert_eq!(
            labels(&components),
            [
                "itoa@1.0.18",
                "libmimalloc-sys@0.1.49",
                "memchr@2.8.3",
                "mimalloc@0.1.52",
                "serde_json@1.0.154",
                "socket2@0.6.5",
                "tokio@1.53.2",
                "windows-sys@0.61.2",
            ]
        );
    }

    #[test]
    fn package_folder_and_license_come_from_cargo_metadata() {
        let components =
            cargo_components(&cargo_fixture("/repo"), &["devbox-cli"], Path::new("/repo"))
                .expect("closure");
        let serde_json = components
            .iter()
            .find(|c| c.name == "serde_json")
            .expect("serde_json ships");
        assert_eq!(serde_json.ecosystem, Ecosystem::Cargo);
        assert_eq!(serde_json.license, "MIT OR Apache-2.0");
        assert_eq!(
            serde_json.dir,
            Path::new("/repo/registry/serde_json-1.0.154")
        );
    }

    #[test]
    fn path_crates_outside_the_repository_are_third_party() {
        let components = cargo_components(
            &cargo_fixture("/repo"),
            &["devbox-cli"],
            Path::new("/elsewhere"),
        )
        .expect("closure");
        assert!(labels(&components).contains(&"devbox-core@1.0.0-rc.1".to_string()));
    }

    #[test]
    fn a_root_that_is_not_a_workspace_member_is_an_error() {
        let error = cargo_components(
            &cargo_fixture("/repo"),
            &["devbox-missing"],
            Path::new("/repo"),
        )
        .expect_err("unknown root");
        assert!(error.to_string().contains("devbox-missing"), "{error}");
    }

    #[test]
    fn pnpm_entries_pair_each_version_with_its_own_folder() {
        let components =
            npm_components(&NPM_FIXTURE.replace("@ROOT@", "/repo")).expect("pnpm output parses");
        assert_eq!(
            labels(&components),
            [
                "@tauri-apps/api@2.12.3",
                "commander@15.0.0",
                "commander@7.2.0",
                "khroma@2.1.0",
                "pretendard@1.3.9",
                "react-dom@19.3.0",
                "react@19.3.0",
            ]
        );
        let old = components
            .iter()
            .find(|c| c.label() == "commander@7.2.0")
            .expect("commander 7");
        assert_eq!(old.ecosystem, Ecosystem::Npm);
        assert_eq!(old.license, "MIT");
        assert_eq!(
            old.dir,
            Path::new("/repo/node_modules/.pnpm/commander@7.2.0/node_modules/commander")
        );
    }

    #[test]
    fn pnpm_entry_with_mismatched_versions_and_paths_is_rejected() {
        let json =
            r#"{"MIT":[{"name":"x","versions":["1.0.0","2.0.0"],"paths":["/a"],"license":"MIT"}]}"#;
        assert!(npm_components(json).is_err());
    }

    #[test]
    fn license_files_are_matched_case_insensitively_at_the_top_level() {
        let dir = tempfile::tempdir().expect("create package folder");
        for (name, text) in [
            ("LICENSE", "plain"),
            ("license-mit", "dashed lower case"),
            ("Licence.txt", "british spelling"),
            ("COPYING.LESSER", "copying"),
            ("NOTICE", "notice"),
            ("README.md", "not a license file"),
            ("UNLICENSE", "does not start with a license prefix"),
            ("licensing.md", "neither does this"),
        ] {
            fs::write(dir.path().join(name), text).expect("write license fixture");
        }
        // A folder is not a file, and nothing below the top level counts.
        fs::create_dir_all(dir.path().join("LICENSES")).expect("create folder");
        fs::write(dir.path().join("LICENSES/MIT.txt"), "inside a folder").expect("write nested");
        fs::create_dir_all(dir.path().join("docs")).expect("create folder");
        fs::write(dir.path().join("docs/LICENSE"), "nested").expect("write nested");

        // Ordered by lower-cased file name.
        assert_eq!(
            read_license_texts(dir.path()).expect("scan"),
            [
                "copying",
                "british spelling",
                "plain",
                "dashed lower case",
                "notice"
            ]
        );
    }

    #[test]
    fn identical_and_blank_license_files_are_kept_once() {
        let dir = tempfile::tempdir().expect("create package folder");
        fs::write(dir.path().join("LICENSE"), "same text\r\n").expect("write");
        fs::write(dir.path().join("LICENSE.md"), "same text   \n\n").expect("write");
        fs::write(dir.path().join("COPYING"), "  \n\n").expect("write");
        assert_eq!(read_license_texts(dir.path()).expect("scan"), ["same text"]);
    }

    #[test]
    fn license_text_that_is_not_utf8_is_kept_lossily() {
        let dir = tempfile::tempdir().expect("create package folder");
        fs::write(dir.path().join("LICENSE"), b"\xA9 2026 Someone\n").expect("write latin-1");
        assert_eq!(
            read_license_texts(dir.path()).expect("scan"),
            ["\u{fffd} 2026 Someone"]
        );
    }

    #[test]
    fn failed_metadata_tells_how_to_get_the_sources() {
        let message = metadata_failure(
            "app/src-tauri/Cargo.toml",
            "error: failed to download `webpki-root-certs v1.0.11`\n",
        )
        .to_string();
        assert!(message.contains("failed to download `webpki-root-certs v1.0.11`"));
        assert!(message.contains("cargo fetch --locked --manifest-path app/src-tauri/Cargo.toml"));
    }

    #[test]
    fn a_missing_package_folder_is_an_error() {
        let dir = tempfile::tempdir().expect("create folder");
        assert!(read_license_texts(&dir.path().join("absent")).is_err());
    }

    #[test]
    fn text_is_normalized_before_it_is_compared() {
        assert_eq!(
            normalize_text("\u{feff}\r\n MIT  \r\n\r\nline two\t\r\n\r\n"),
            " MIT\n\nline two"
        );
    }
}
