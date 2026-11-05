//! Third-party files that live in this repository and ship in the app. They are in no lock file, so the
//! notices list them from this table. `every_font_face_file_under_app_src_has_a_notices_entry` fails when
//! a stylesheet loads a font that is not listed here.

use super::sources::{Component, Ecosystem};
use anyhow::{Result, bail};
use std::path::Path;

/// A font file the UI loads from `app/src`, which Vite puts into the app.
pub struct BundledFont {
    pub name: &'static str,
    pub version: &'static str,
    /// SPDX expression of the font's license.
    pub license: &'static str,
    /// The font file, relative to the repository root.
    pub file: &'static str,
    /// The text of its license, relative to the repository root.
    pub license_file: &'static str,
}

/// D2Coding, loaded by `app/src/styles/fonts.css`, whose first line names the release and where it came from:
/// "D2Coding 1.4.0, naver/d2codingfont b0dc372e28d7abdba7f3cba854855ceffbd490a8 (OFL)".
pub const FONTS: [BundledFont; 1] = [BundledFont {
    name: "D2Coding",
    version: "1.4.0",
    license: "OFL-1.1",
    file: "app/src/assets/fonts/D2Coding.woff2",
    license_file: "app/src/assets/fonts/LICENSE-D2Coding.txt",
}];

/// The bundled fonts as notices entries, each with the one file that holds its license text. A font
/// whose file is gone is an error: the table must not list what does not ship.
pub fn components(repo: &Path) -> Result<Vec<Component>> {
    FONTS
        .iter()
        .map(|font| {
            let file = repo.join(font.file);
            if !file.is_file() {
                bail!(
                    "글꼴 파일이 없습니다: {} (`xtask/src/notices/bundled.rs`의 FONTS)",
                    font.file
                );
            }
            Ok(Component {
                ecosystem: Ecosystem::Font,
                name: font.name.to_string(),
                version: font.version.to_string(),
                license: font.license.to_string(),
                dir: file
                    .parent()
                    .map_or_else(|| repo.to_path_buf(), Path::to_path_buf),
                license_files: vec![repo.join(font.license_file)],
            })
        })
        .collect()
}

/// The files the `@font-face` rules of one stylesheet load, as repository paths (`sheet` is the
/// stylesheet's own path). A `data:` or remote URL comes back as written: no entry can match it.
#[cfg(test)]
fn font_face_files(sheet: &str, css: &str) -> Vec<String> {
    let css = without_comments(css);
    let mut files = Vec::new();
    let mut rest = css.as_str();
    while let Some(at) = rest.find("@font-face") {
        rest = &rest[at + "@font-face".len()..];
        let (Some(open), Some(close)) = (rest.find('{'), rest.find('}')) else {
            break;
        };
        if close < open {
            continue;
        }
        let mut block = &rest[open + 1..close];
        while let Some(url) = block.find("url(") {
            block = &block[url + "url(".len()..];
            let Some(end) = block.find(')') else {
                break;
            };
            let quoted = block[..end].trim();
            files.push(resolved(sheet, quoted.trim_matches(['"', '\''])));
            block = &block[end..];
        }
        rest = &rest[close..];
    }
    files
}

#[cfg(test)]
fn without_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        rest = rest[start + 2..]
            .find("*/")
            .map_or("", |end| &rest[start + 2 + end + 2..]);
    }
    out.push_str(rest);
    out
}

/// `url` as a path from the repository root, given the stylesheet's own path.
#[cfg(test)]
fn resolved(sheet: &str, url: &str) -> String {
    if url.starts_with("data:") || url.starts_with('/') || url.contains("://") {
        return url.to_string();
    }
    let local = url.split(['?', '#']).next().unwrap_or_default();
    let mut parts: Vec<&str> = sheet.split('/').collect();
    parts.pop();
    for part in local.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notices::license_files::read_license_file;
    use std::fs;
    use std::path::{Path, PathBuf};

    fn repo() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
    }

    fn css_files(dir: &Path, found: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).expect("read folder") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                css_files(&path, found);
            } else if path.extension().is_some_and(|extension| extension == "css") {
                found.push(path);
            }
        }
    }

    #[test]
    fn font_face_files_are_found_and_resolved_from_the_stylesheet() {
        let css = "/* @font-face { src: url(\"../gone.woff2\") } */\n\
                   @font-face {\n  font-family: \"A\";\n  src: local(\"A\"), url('./a.woff2?v=1#x') format(\"woff2\"),\n    url(../fonts/b.woff) format(\"woff\");\n}\n\
                   .rule { background: url(\"../img/not-a-font.png\"); }\n\
                   @font-face { src: url(data:font/woff2;base64,AAAA); }\n\
                   @font-face { src: url(\"https://example.com/c.woff2\"); }\n";
        assert_eq!(
            font_face_files("app/src/styles/fonts.css", css),
            [
                "app/src/styles/a.woff2",
                "app/src/fonts/b.woff",
                "data:font/woff2;base64,AAAA",
                "https://example.com/c.woff2",
            ]
        );
    }

    #[test]
    fn every_font_face_file_under_app_src_has_a_notices_entry() {
        let repo = repo();
        let mut sheets = Vec::new();
        css_files(&repo.join("app/src"), &mut sheets);
        sheets.sort();
        let mut used = Vec::new();
        for sheet in &sheets {
            let relative = sheet
                .strip_prefix(&repo)
                .expect("inside the repository")
                .to_str()
                .expect("utf-8 path")
                .replace('\\', "/");
            let css = fs::read_to_string(sheet).expect("read stylesheet");
            used.extend(font_face_files(&relative, &css));
        }
        assert!(
            !used.is_empty(),
            "no @font-face found: the scan would pass for nothing"
        );
        for file in &used {
            assert!(
                FONTS.iter().any(|font| font.file == file),
                "{file} is loaded by an @font-face and ships with the app, but THIRD_PARTY_NOTICES.md has no \
                 entry for it: add it to FONTS in xtask/src/notices/bundled.rs and run `cargo run -p xtask -- notices`"
            );
        }
        for font in &FONTS {
            assert!(
                used.iter().any(|file| file == font.file),
                "{} is not loaded",
                font.file
            );
            assert!(repo.join(font.file).is_file(), "{} is missing", font.file);
        }
    }

    #[test]
    fn the_committed_notices_list_every_bundled_font_with_its_license_text() {
        let repo = repo();
        let notices = fs::read_to_string(repo.join("THIRD_PARTY_NOTICES.md"))
            .expect("read notices")
            .replace("\r\n", "\n");
        for font in &FONTS {
            let text = read_license_file(&repo.join(font.license_file)).expect("license text");
            assert!(
                notices.contains(&format!(
                    "- `{}@{}` (font, {})\n",
                    font.name, font.version, font.license
                )),
                "{} is not listed",
                font.name
            );
            assert!(
                notices.contains(&text),
                "the license text of {} is missing",
                font.name
            );
        }
    }
}
