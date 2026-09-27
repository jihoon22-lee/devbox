use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;
const MAX_FILE_BYTES: usize = 16 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 32 * 1024 * 1024;
type Result<T> = std::result::Result<T, &'static str>;
#[derive(Clone, Copy, Debug, Deserialize, ts_rs::TS, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ImportFileFormat {
    Devbox,
    Postman,
    Insomnia,
    Har,
    Bruno,
    Auto,
}
#[derive(Debug, Serialize, ts_rs::TS)]
pub struct ImportFile {
    pub name: String,
    pub relative_path: String,
    pub text: String,
}
pub fn check_sizes(sizes: &[u64]) -> Result<()> {
    if sizes.len() > 500
        || sizes.iter().any(|&size| size > MAX_FILE_BYTES as u64)
        || sizes
            .iter()
            .try_fold(0u64, |sum, size| sum.checked_add(*size))
            .is_none_or(|total| total > MAX_TOTAL_BYTES)
    {
        return Err("import_file_too_large");
    }
    Ok(())
}
pub fn common_parent(paths: &[PathBuf]) -> Result<PathBuf> {
    let mut common = paths
        .first()
        .and_then(|path| path.parent())
        .ok_or("import_file_invalid")?
        .to_path_buf();
    for path in paths {
        while !path.starts_with(&common) {
            if !common.pop() {
                return Err("import_file_invalid");
            }
        }
    }
    Ok(common)
}
pub fn decode_utf8(bytes: Vec<u8>) -> Result<String> {
    let mut text = String::from_utf8(bytes).map_err(|_| "import_file_invalid")?;
    if text.starts_with('\u{feff}') {
        text.drain(..3);
    }
    Ok(text)
}
pub fn read_files(paths: &[PathBuf]) -> Result<Vec<ImportFile>> {
    let common = common_parent(paths)?;
    let sizes = paths
        .iter()
        .map(|path| {
            if !path.is_absolute() {
                return Err("import_file_invalid");
            }
            super::transfer::validate_file_path(path, true).map_err(|_| "import_file_invalid")?;
            std::fs::symlink_metadata(path)
                .map(|metadata| metadata.len())
                .map_err(|_| "import_file_invalid")
        })
        .collect::<Result<Vec<_>>>()?;
    check_sizes(&sizes)?;
    let mut files = Vec::with_capacity(paths.len());
    let mut actual = 0u64;
    for path in paths {
        let bytes =
            super::transfer::read_bounded(path, MAX_FILE_BYTES).map_err(|error| match error {
                super::transfer::ReadError::TooLarge => "import_file_too_large",
                super::transfer::ReadError::Invalid => "import_file_invalid",
            })?;
        actual = actual
            .checked_add(bytes.len() as u64)
            .ok_or("import_file_too_large")?;
        if actual > MAX_TOTAL_BYTES {
            return Err("import_file_too_large");
        }
        let relative = path
            .strip_prefix(&common)
            .map_err(|_| "import_file_invalid")?;
        files.push(ImportFile {
            name: path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or("import_file_invalid")?
                .into(),
            relative_path: relative
                .to_str()
                .ok_or("import_file_invalid")?
                .replace('\\', "/"),
            text: decode_utf8(bytes)?,
        });
    }
    Ok(files)
}
pub async fn read_import_files(
    app: AppHandle,
    format: ImportFileFormat,
) -> std::result::Result<Option<Vec<ImportFile>>, String> {
    let selected = tauri::async_runtime::spawn_blocking(move || {
        let (label, extensions): (&str, &[&str]) = match format {
            ImportFileFormat::Devbox | ImportFileFormat::Postman | ImportFileFormat::Insomnia => {
                ("JSON", &["json"])
            }
            ImportFileFormat::Har => ("HAR", &["har", "json"]),
            ImportFileFormat::Bruno => ("Bruno", &["bru"]),
            ImportFileFormat::Auto => ("API collection", &["json", "har", "bru"]),
        };
        let picker = app.dialog().file().add_filter(label, extensions);
        if format == ImportFileFormat::Bruno {
            picker.blocking_pick_files()
        } else {
            picker.blocking_pick_file().map(|path| vec![path])
        }
    })
    .await
    .map_err(|_| "import_file_invalid")?;
    let Some(selected) = selected else {
        return Ok(None);
    };
    let paths = selected
        .into_iter()
        .map(|file| file.into_path().map_err(|_| "import_file_invalid"))
        .collect::<Result<Vec<_>>>()?;
    tauri::async_runtime::spawn_blocking(move || read_files(&paths))
        .await
        .map_err(|_| "import_file_invalid")?
        .map(Some)
        .map_err(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    #[test]
    fn common_parent_keeps_relative_collection_folders() {
        assert_eq!(
            common_parent(&[PathBuf::from("a/users/x.bru"), PathBuf::from("a/y.bru")]).unwrap(),
            PathBuf::from("a")
        );
        assert!(common_parent(&[]).is_err());
    }
    #[test]
    fn utf8_bom_is_removed_but_invalid_utf8_is_rejected() {
        assert_eq!(decode_utf8(vec![0xef, 0xbb, 0xbf, b'x']).unwrap(), "x");
        assert_eq!(decode_utf8(vec![0xff]).unwrap_err(), "import_file_invalid");
    }
    #[test]
    fn selection_count_individual_size_and_total_are_bounded() {
        assert!(check_sizes(&[16 * 1024 * 1024, 16 * 1024 * 1024]).is_ok());
        assert_eq!(
            check_sizes(&[16 * 1024 * 1024 + 1]).unwrap_err(),
            "import_file_too_large"
        );
        assert_eq!(
            check_sizes(&[8 * 1024 * 1024; 5]).unwrap_err(),
            "import_file_too_large"
        );
        assert_eq!(check_sizes(&[0; 501]).unwrap_err(), "import_file_too_large");
    }
    #[test]
    fn reads_only_picked_files_and_exposes_relative_names() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("users")).unwrap();
        let a = root.path().join("users/a.bru");
        let b = root.path().join("b.bru");
        std::fs::write(&a, "meta {\n}\n").unwrap();
        std::fs::write(&b, "\u{feff}vars {\n}\n").unwrap();
        let files = read_files(&[a, b]).unwrap();
        assert_eq!(files[0].relative_path, "users/a.bru");
        assert_eq!(files[1].text, "vars {\n}\n");
        #[cfg(unix)]
        {
            let linked = root.path().join("linked.bru");
            std::os::unix::fs::symlink(root.path().join("b.bru"), &linked).unwrap();
            assert_eq!(read_files(&[linked]).unwrap_err(), "import_file_invalid");
        }
    }
}
