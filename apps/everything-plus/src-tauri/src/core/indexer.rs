/// 내용 인덱싱 대상 텍스트 확장자 (소문자)
pub const TEXT_EXTENSIONS: &[&str] = &[
    "md", "txt", "json", "csv", "tsv", "toml", "yaml", "yml", "xml", "html", "css", "js", "jsx",
    "ts", "tsx", "py", "java", "go", "rs", "c", "h", "cpp", "hpp", "rb", "php", "sh", "sql", "ini",
    "cfg", "log",
];

pub fn is_text_ext(ext: &str) -> bool {
    TEXT_EXTENSIONS.contains(&ext.to_lowercase().as_str())
}

/// 내용 인덱싱 시 1개 파일당 읽을 최대 크기 (1MB). 초과 시 스킵.
pub const MAX_CONTENT_BYTES: u64 = 1_000_000;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_ext_detection() {
        assert!(is_text_ext("md"));
        assert!(is_text_ext("RS"));
        assert!(is_text_ext("json"));
        assert!(!is_text_ext("png"));
        assert!(!is_text_ext("exe"));
        assert!(!is_text_ext(""));
    }
}
