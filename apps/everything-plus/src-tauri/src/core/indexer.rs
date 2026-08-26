pub use super::content::is_text_ext;

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
