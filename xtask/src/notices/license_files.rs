//! The license files of one package: which files count, and their text normalized for comparison.

use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};

/// A file is a license file when its lower-cased name starts with one of these: `LICENSE`, `LICENSE-MIT`,
/// `LICENCE.txt`, `COPYING.LESSER`, `NOTICE`, or `OFL.txt` (the SIL Open Font License that fonts ship).
const LICENSE_FILE_PREFIXES: [&str; 5] = ["license", "licence", "copying", "notice", "ofl"];
/// A folder of other people's packages: its license files are not this package's.
const DEPENDENCY_FOLDER: &str = "node_modules";
/// Extensions of files that are not license text although their names start like a license file.
///
/// The scan covers the whole package folder, where such files exist: source (`src/license.rs`), generated
/// JavaScript (`lib/misc/licenses.js` in `@iconify/utils`), scripts (`license-update.mjs` in cytoscape),
/// SPDX documents (the `LICENSE.spdx` of the Tauri crates), markup and data (`license.html`,
/// `licenses.json`) and binaries. What is left is plain text: no extension, `.txt`, `.md`, `.rst`, or a
/// suffix that names a license or a project (`LICENSE-APACHE-2.0`, `LICENSE.httprouter`, `COPYING.LIB`).
///
/// An unlisted suffix is read: losing a license text is worse than including an odd file. So the list
/// holds only unambiguous types and leaves out suffixes that are also language codes of translated
/// licenses (`.pl`, `.cs`) or that name a plausible project (`.lib`, `.node`, `.cc`). All lower case.
const NOT_TEXT_EXTENSIONS: &[&str] = &[
    // source code
    "rs", "c", "cpp", "cxx", "h", "hh", "hpp", "hxx", "go", "java", "kt", "kts", "scala", "swift",
    "rb", "py", "pyi", "pyc", "php", "lua", "dart", "ex", "exs", "erl", "hs", "zig",
    // JavaScript and TypeScript, source and generated (`.d.ts`, `.min.js`, `.js.map`)
    "js", "mjs", "cjs", "jsx", "ts", "mts", "cts", "tsx", "vue", "svelte", "map",
    // shell and Windows scripts
    "sh", "bash", "zsh", "fish", "bat", "cmd", "ps1",
    // markup, styles, data and configuration; `spdx` is the SPDX document format
    "html", "htm", "xhtml", "xml", "svg", "css", "scss", "sass", "less", "json", "json5", "jsonc",
    "yaml", "yml", "toml", "ini", "cfg", "csv", "tsv", "sql", "proto", "plist", "spdx",
    // images, fonts, documents and archives
    "png", "jpg", "jpeg", "gif", "bmp", "ico", "webp", "avif", "woff", "woff2", "ttf", "otf", "eot",
    "pdf", "doc", "docx", "odt", "zip", "gz", "tgz", "tar", "xz", "bz2", "zst", "7z",
    // compiled code and native libraries
    "wasm", "so", "dll", "dylib", "exe", "obj", "rlib", "jar",
];

/// Normalized text of one license file that is named, not searched for (a bundled font's). It must
/// exist and must not be blank: a notice without its license text is the failure to catch.
pub fn read_license_file(path: &Path) -> Result<String> {
    let bytes = fs::read(path)
        .with_context(|| format!("라이선스 파일을 읽지 못했습니다: {}", path.display()))?;
    let text = normalize_text(&String::from_utf8_lossy(&bytes));
    if text.is_empty() {
        bail!("라이선스 파일이 비어 있습니다: {}", path.display());
    }
    Ok(text)
}

/// Normalized text of the license files of the package in `dir`.
///
/// Every file below `dir`, at any depth, whose name starts like a license file is read, whether or not
/// the package also has a top-level license: a package may vendor code with a license of its own
/// (libmimalloc-sys's C library, ring's fiat-crypto, regex-syntax's Unicode tables). Not read: anything
/// inside `node_modules`, anything reached through a link, and files with a non-text extension. Names
/// are matched case-insensitively. Texts come in order of lower-cased path relative to `dir`; blank
/// files and repeats of a text the package already has are dropped.
pub fn read_license_texts(dir: &Path) -> Result<Vec<String>> {
    let mut texts: Vec<String> = Vec::new();
    for path in license_files(dir)? {
        let bytes = fs::read(&path)
            .with_context(|| format!("라이선스 파일을 읽지 못했습니다: {}", path.display()))?;
        let text = normalize_text(&String::from_utf8_lossy(&bytes));
        if !text.is_empty() && !texts.contains(&text) {
            texts.push(text);
        }
    }
    Ok(texts)
}

/// The license files below `dir`, ordered by lower-cased relative path. `dir` itself may be a link (pnpm
/// links packages), but nothing below it is followed.
fn license_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let unreadable = |folder: &Path| format!("패키지 폴더를 읽지 못했습니다: {}", folder.display());
    let mut files = Vec::new();
    let mut folders = vec![(dir.to_path_buf(), String::new())];
    while let Some((folder, relative)) = folders.pop() {
        for entry in fs::read_dir(&folder).with_context(|| unreadable(&folder))? {
            let entry = entry.with_context(|| unreadable(&folder))?;
            let name = entry.file_name().to_string_lossy().to_lowercase();
            let key = format!("{relative}{name}");
            // `file_type` does not follow links: a linked folder or file is neither a folder nor a
            // regular file here, so only what really lies inside the package is looked at.
            let kind = entry.file_type().with_context(|| unreadable(&folder))?;
            if kind.is_dir() {
                if name != DEPENDENCY_FOLDER {
                    folders.push((entry.path(), format!("{key}/")));
                }
            } else if kind.is_file() && is_license_name(&name) {
                files.push((key, entry.path()));
            }
        }
    }
    files.sort();
    Ok(files.into_iter().map(|(_, path)| path).collect())
}

/// True for a lower-cased file name that starts like a license file and is not code, data or a binary.
fn is_license_name(name: &str) -> bool {
    LICENSE_FILE_PREFIXES
        .iter()
        .any(|prefix| name.starts_with(prefix))
        && !Path::new(name)
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| NOT_TEXT_EXTENSIONS.contains(&extension))
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
    fn a_named_license_file_is_read_normalized_and_must_exist_and_not_be_blank() {
        let dir = tempfile::tempdir().expect("create folder");
        put(
            dir.path(),
            "LICENSE-Font.txt",
            "\u{feff}Copyright (c) NAVER \r\nSIL OFL 1.1 \r\n\r\n",
        );
        assert_eq!(
            read_license_file(&dir.path().join("LICENSE-Font.txt")).expect("read"),
            "Copyright (c) NAVER\nSIL OFL 1.1"
        );
        put(dir.path(), "blank.txt", " \r\n\r\n");
        assert!(read_license_file(&dir.path().join("blank.txt")).is_err());
        assert!(read_license_file(&dir.path().join("missing.txt")).is_err());
    }

    #[test]
    fn license_names_are_matched_case_insensitively_by_prefix() {
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
        // The name of the file counts, not the name of the folder it is in.
        fs::create_dir_all(dir.path().join("LICENSES")).expect("create folder");
        fs::write(dir.path().join("LICENSES/MIT.txt"), "inside a folder").expect("write nested");
        // A license below the top level is read too.
        fs::create_dir_all(dir.path().join("docs")).expect("create folder");
        fs::write(dir.path().join("docs/LICENSE"), "nested").expect("write nested");

        // Ordered by lower-cased path relative to the package folder.
        assert_eq!(
            read_license_texts(dir.path()).expect("scan"),
            [
                "copying",
                "nested",
                "british spelling",
                "plain",
                "dashed lower case",
                "notice"
            ]
        );
    }

    #[test]
    fn nested_licenses_are_read_at_any_depth_next_to_the_top_level_one() {
        // Like libmimalloc-sys: its own license plus the license of the C library it vendors, four
        // levels down. Both ship, so both are read.
        let vendored = tempfile::tempdir().expect("create package folder");
        put(vendored.path(), "LICENSE.txt", "wrapper license");
        put(
            vendored.path(),
            "c_src/mimalloc/v2/LICENSE",
            "vendored license",
        );
        assert_eq!(
            read_license_texts(vendored.path()).expect("scan"),
            ["vendored license", "wrapper license"]
        );

        // Like pretendard: nothing at the top level, the license is one folder down. Deeper counts too.
        let nested = tempfile::tempdir().expect("create package folder");
        put(nested.path(), "dist/LICENSE.txt", "font license");
        put(nested.path(), "a/b/c/d/e/NOTICE", "very deep");
        assert_eq!(
            read_license_texts(nested.path()).expect("scan"),
            ["very deep", "font license"]
        );

        // The same text in several folders is kept once.
        let repeated = tempfile::tempdir().expect("create package folder");
        for path in ["LICENSE", "v2/LICENSE", "v3/LICENSE"] {
            put(repeated.path(), path, "same text");
        }
        assert_eq!(
            read_license_texts(repeated.path()).expect("scan"),
            ["same text"]
        );

        // Nothing anywhere: the caller falls back to the declared SPDX expression.
        let none = tempfile::tempdir().expect("create package folder");
        put(none.path(), "src/lib.rs", "fn main() {}");
        assert!(read_license_texts(none.path()).expect("scan").is_empty());
    }

    #[test]
    fn ofl_files_count_and_node_modules_is_never_entered() {
        let fonts = tempfile::tempdir().expect("create package folder");
        put(fonts.path(), "LICENSE", "code license");
        put(fonts.path(), "OFL-1.1.txt", "top-level font license");
        put(fonts.path(), "fonts/OFL.txt", "open font license");
        // Other people's packages, at any depth and in any case.
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
        put(
            fonts.path(),
            "dist/Node_Modules/dep/COPYING",
            "belongs to a dependency",
        );
        assert_eq!(
            read_license_texts(fonts.path()).expect("scan"),
            [
                "open font license",
                "code license",
                "top-level font license"
            ]
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
            "lib/node_modules/dep/LICENSE",
            "belongs to a dependency",
        );
        assert!(read_license_texts(only.path()).expect("scan").is_empty());
    }

    #[test]
    fn code_data_and_binary_files_are_never_license_texts() {
        // Names that start like a license file but are source code, scripts, markup, data, binaries
        // or archives. Some of them exist in real packages (`licenses.js`, `license-update.mjs`).
        let not_text = [
            "license.rs",
            "licenses.js",
            "license.d.ts",
            "license.min.js",
            "license-update.mjs",
            "license.cjs",
            "LICENSE.TSX",
            "license.py",
            "license.go",
            "license.c",
            "license.h",
            "license.java",
            "license.sh",
            "license.ps1",
            "license.html",
            "licenses.htm",
            "licenses.xml",
            "license.svg",
            "licenses.json",
            "license.yaml",
            "license.toml",
            "license.csv",
            "license.css",
            "LICENSE.spdx",
            "license.js.map",
            "license.png",
            "license.pdf",
            "license.woff2",
            "license.tar.gz",
            "license.zip",
            "license.so",
            "license.dll",
            "license.wasm",
        ];
        // Real license texts, whatever their suffix: a version, a license name or a project name.
        let text = [
            "LICENSE",
            "LICENSE-APACHE-2.0",
            "LICENSE-Apache-2.0_WITH_LLVM-exception",
            "LICENSE.md",
            "LICENSE.txt",
            "LICENSE.rst",
            "LICENSE.bsd-3-clause",
            "LICENSE.httprouter",
            "COPYING.LIB",
            "NOTICE.txt",
            "OFL.txt",
            "license_header",
        ];
        for folder in ["", "src/", "a/b/c/"] {
            let dir = tempfile::tempdir().expect("create package folder");
            for name in not_text {
                put(dir.path(), &format!("{folder}{name}"), "never a license");
            }
            assert!(
                read_license_texts(dir.path()).expect("scan").is_empty(),
                "files in {folder:?} were taken as license text"
            );
            for name in text {
                put(
                    dir.path(),
                    &format!("{folder}{name}"),
                    &format!("text of {name}"),
                );
            }
            let mut found = read_license_texts(dir.path()).expect("scan");
            found.sort();
            let mut expected: Vec<String> = text.iter().map(|n| format!("text of {n}")).collect();
            expected.sort();
            assert_eq!(found, expected, "license texts in {folder:?}");
        }

        // A top level with only code files does not hide a real license further down.
        let nested = tempfile::tempdir().expect("create package folder");
        put(nested.path(), "license-update.mjs", "script");
        put(nested.path(), "docs/COPYING", "real nested license");
        assert_eq!(
            read_license_texts(nested.path()).expect("scan"),
            ["real nested license"]
        );
    }

    #[test]
    fn the_non_text_extension_list_is_lower_case_and_has_no_repeats() {
        let mut seen = std::collections::BTreeSet::new();
        for &extension in NOT_TEXT_EXTENSIONS {
            assert_eq!(
                extension,
                extension.to_lowercase(),
                "{extension} is not lower case"
            );
            assert!(!extension.starts_with('.'), "{extension} starts with a dot");
            assert!(seen.insert(extension), "{extension} is listed twice");
        }
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_license_files_and_folders_are_not_followed() {
        use std::os::unix::fs::symlink;
        let outside = tempfile::tempdir().expect("create folder outside the package");
        put(outside.path(), "secret.txt", "text outside the package");
        put(
            outside.path(),
            "linked/LICENSE",
            "license outside the package",
        );

        let package = tempfile::tempdir().expect("create package folder");
        put(package.path(), "LICENSE-MIT", "real license");
        // A link to a file, a link to a folder that holds a license, and a link to nothing.
        symlink(
            outside.path().join("secret.txt"),
            package.path().join("LICENSE"),
        )
        .expect("link a file");
        symlink(outside.path().join("linked"), package.path().join("vendor"))
            .expect("link a folder");
        symlink(
            outside.path().join("absent"),
            package.path().join("COPYING"),
        )
        .expect("link to nothing");
        assert_eq!(
            read_license_texts(package.path()).expect("scan"),
            ["real license"]
        );

        // The package folder itself may be a link (pnpm links packages): only what is below it is not followed.
        let alias = outside.path().join("alias");
        symlink(package.path(), &alias).expect("link the package folder");
        assert_eq!(read_license_texts(&alias).expect("scan"), ["real license"]);
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
