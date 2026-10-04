//! Convert an already captured local installation path for ordinary-root checks.
//! This is never a renderer or command-line input normalizer.
pub(crate) fn captured_disk_root(path: &str) -> Result<&str, &'static str> {
    let disk = path.strip_prefix(r"\\?\").unwrap_or(path);
    let bytes = disk.as_bytes();
    if bytes.len() < 3 || !bytes[0].is_ascii_alphabetic() || bytes[1] != b':' || bytes[2] != b'\\' {
        return Err("bootstrap_root_unsafe");
    }
    Ok(disk)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captured_verbatim_disk_root_retains_the_exact_local_path() {
        assert_eq!(
            captured_disk_root(r"\\?\D:\owned fixture\Suite UI Fixture"),
            Ok(r"D:\owned fixture\Suite UI Fixture")
        );
        assert_eq!(
            captured_disk_root(r"D:\owned fixture\Suite UI Fixture"),
            Ok(r"D:\owned fixture\Suite UI Fixture")
        );
        for path in [
            r"\\?\UNC\server\share",
            r"\\?\Volume{fixture}\",
            r"\\.\D:\fixture",
            r"\\server\share",
            r"D:relative",
            r"/tmp/fixture",
            "",
        ] {
            assert_eq!(captured_disk_root(path), Err("bootstrap_root_unsafe"));
        }
    }
}
