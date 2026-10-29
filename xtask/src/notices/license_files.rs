//! The license files of one package: which files count, and their text normalized for comparison.

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

const LICENSE_FILE_PREFIXES: [&str; 4] = ["license", "licence", "copying", "notice"];
/// The nested search also takes SIL Open Font License files (`OFL.txt`), which font packages ship.
const NESTED_PREFIXES: [&str; 5] = ["license", "licence", "copying", "notice", "ofl"];
/// How far below the package root the nested search looks, counted like `find -maxdepth`: a file in the
/// root is at depth 1, one in a folder directly below it (`dist/LICENSE.txt`) at depth 2.
const NESTED_DEPTH: usize = 2;
/// A folder of other people's packages: its license files are not this package's.
const DEPENDENCY_FOLDER: &str = "node_modules";
/// Code and metadata files whose names only start like a license (`license-update.mjs`, `LICENSE.spdx`).
const CODE_EXTENSIONS: [&str; 8] = ["js", "mjs", "cjs", "ts", "json", "spdx", "sh", "py"];

/// Normalized text of the license files of the package in `dir`.
///
/// The `LICENSE*`, `LICENCE*`, `COPYING*` and `NOTICE*` files at the top level are used. A package that
/// has none is searched down to `NESTED_DEPTH` (now also for `OFL*`) and never inside `node_modules`: a
/// font may ship its license as `dist/LICENSE.txt`. Names are matched case-insensitively and files with
/// a code or metadata extension are skipped. Texts are read in path order; blank files and repeats of a
/// text the package already has are dropped.
pub fn read_license_texts(dir: &Path) -> Result<Vec<String>> {
    let top_level = read_texts(&license_files(dir, &LICENSE_FILE_PREFIXES, 1)?)?;
    if !top_level.is_empty() {
        return Ok(top_level);
    }
    read_texts(&license_files(dir, &NESTED_PREFIXES, NESTED_DEPTH)?)
}

/// Files below `dir`, at most `max_depth` path components deep, whose lower-cased name starts with one of
/// `prefixes`, ordered by lower-cased relative path. Symlinked folders are not entered.
fn license_files(dir: &Path, prefixes: &[&str], max_depth: usize) -> Result<Vec<PathBuf>> {
    let unreadable = |folder: &Path| format!("패키지 폴더를 읽지 못했습니다: {}", folder.display());
    let mut files = Vec::new();
    let mut folders = vec![(dir.to_path_buf(), String::new(), 1)];
    while let Some((folder, relative, depth)) = folders.pop() {
        for entry in fs::read_dir(&folder).with_context(|| unreadable(&folder))? {
            let entry = entry.with_context(|| unreadable(&folder))?;
            let name = entry.file_name().to_string_lossy().to_lowercase();
            let path = entry.path();
            let key = format!("{relative}{name}");
            if entry
                .file_type()
                .with_context(|| unreadable(&folder))?
                .is_dir()
            {
                if depth < max_depth && name != DEPENDENCY_FOLDER {
                    folders.push((path, format!("{key}/"), depth + 1));
                }
            } else if prefixes.iter().any(|prefix| name.starts_with(prefix))
                && !is_code_file(&name)
                && path.is_file()
            {
                files.push((key, path));
            }
        }
    }
    files.sort();
    Ok(files.into_iter().map(|(_, path)| path).collect())
}

/// True for a lower-cased file name with a code or metadata extension.
fn is_code_file(name: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| CODE_EXTENSIONS.contains(&extension))
}

fn read_texts(files: &[PathBuf]) -> Result<Vec<String>> {
    let mut texts: Vec<String> = Vec::new();
    for path in files {
        let bytes = fs::read(path)
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

    fn put(root: &Path, path: &str, text: &str) {
        let file = root.join(path);
        fs::create_dir_all(file.parent().expect("file has a folder")).expect("create folders");
        fs::write(file, text).expect("write fixture file");
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
    fn nested_licenses_are_used_only_when_the_top_level_has_none() {
        // Like pretendard: the only license is in dist/, one folder below the root.
        let nested = tempfile::tempdir().expect("create package folder");
        put(nested.path(), "dist/LICENSE.txt", "font license");
        // Three levels below the root is too deep.
        put(nested.path(), "dist/web/LICENSE", "too deep");
        assert_eq!(
            read_license_texts(nested.path()).expect("scan"),
            ["font license"]
        );

        // A top-level license wins: the nested one is not read at all.
        let top = tempfile::tempdir().expect("create package folder");
        put(top.path(), "LICENSE", "top level");
        put(top.path(), "dist/LICENSE.txt", "nested");
        assert_eq!(read_license_texts(top.path()).expect("scan"), ["top level"]);

        // Nothing anywhere: the caller falls back to the declared SPDX expression.
        let none = tempfile::tempdir().expect("create package folder");
        put(none.path(), "src/lib.rs", "fn main() {}");
        assert!(read_license_texts(none.path()).expect("scan").is_empty());
    }

    #[test]
    fn nested_search_takes_ofl_files_and_never_enters_node_modules() {
        let fonts = tempfile::tempdir().expect("create package folder");
        put(fonts.path(), "fonts/OFL.txt", "open font license");
        put(
            fonts.path(),
            "node_modules/LICENSE",
            "belongs to a dependency",
        );
        put(
            fonts.path(),
            "node_modules/dep/LICENSE",
            "belongs to a dependency",
        );
        assert_eq!(
            read_license_texts(fonts.path()).expect("scan"),
            ["open font license"]
        );

        // An OFL file next to the code counts when no other license file exists.
        let top = tempfile::tempdir().expect("create package folder");
        put(top.path(), "OFL-1.1.txt", "top-level font license");
        assert_eq!(
            read_license_texts(top.path()).expect("scan"),
            ["top-level font license"]
        );

        // A license only inside node_modules is a dependency's, never the package's.
        let only = tempfile::tempdir().expect("create package folder");
        put(
            only.path(),
            "node_modules/LICENSE",
            "belongs to a dependency",
        );
        put(
            only.path(),
            "Node_Modules/dep/COPYING",
            "belongs to a dependency",
        );
        assert!(read_license_texts(only.path()).expect("scan").is_empty());
    }

    #[test]
    fn code_and_metadata_files_are_never_license_texts() {
        let dir = tempfile::tempdir().expect("create package folder");
        for name in [
            "LICENSE",
            "license-update.mjs",
            "LICENSE.spdx",
            "LICENSE.json",
            "COPYING.py",
            "NOTICE.sh",
            "LICENSE.cjs",
            "LICENSE.TS",
            "LICENSE.js",
        ] {
            put(dir.path(), name, &format!("text of {name}"));
        }
        // A version number is not an extension, and prose extensions are fine.
        put(dir.path(), "LICENSE-APACHE-2.0", "apache");
        put(dir.path(), "NOTICE.txt", "notice");
        assert_eq!(
            read_license_texts(dir.path()).expect("scan"),
            ["text of LICENSE", "apache", "notice"]
        );

        // The same below the top level: code files do not count, a real nested license does.
        let nested = tempfile::tempdir().expect("create package folder");
        put(nested.path(), "license-update.mjs", "script");
        put(nested.path(), "dist/LICENSE.mjs", "script");
        put(nested.path(), "dist/LICENSE.json", "metadata");
        assert!(read_license_texts(nested.path()).expect("scan").is_empty());
        put(nested.path(), "docs/COPYING", "real nested license");
        assert_eq!(
            read_license_texts(nested.path()).expect("scan"),
            ["real nested license"]
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
