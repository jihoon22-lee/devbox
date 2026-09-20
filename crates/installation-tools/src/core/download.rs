//! 다운로드 검증 순수 로직. IO·네트워크 없음, WSL에서 테스트.

/// SHA-256이 64자 hex인지 검사한다.
pub fn is_valid_sha256(sha: &str) -> bool {
    sha.len() == 64 && sha.chars().all(|c| c.is_ascii_hexdigit())
}

/// 수신 크기가 manifest size와 일치하는지 판정한다.
pub fn validate_size(expected: i64, actual: i64) -> Result<(), String> {
    if expected < 0 {
        return Err("manifest size가 음수다".into());
    }
    if actual != expected {
        return Err(format!(
            "크기 불일치: 기대 {expected}바이트, 수신 {actual}바이트"
        ));
    }
    Ok(())
}

/// 누적 바이트가 manifest size를 초과하면 즉시 중단해야 한다.
#[cfg(test)]
pub fn is_over_limit(received: i64, expected: i64) -> bool {
    expected >= 0 && received > expected
}

/// digest가 manifest와 일치하는지 판정한다.
pub fn validate_digest(expected: &str, actual: &str) -> Result<(), String> {
    if !is_valid_sha256(expected) {
        return Err("manifest sha256 형식이 올바르지 않다".into());
    }
    if !is_valid_sha256(actual) {
        return Err("계산된 sha256 형식이 올바르지 않다".into());
    }
    if expected != actual {
        return Err("SHA-256 불일치 (파일이 변조되었거나 손상됨)".into());
    }
    Ok(())
}

/// 대상 파일 옆의 `.partial` 이름. 전부 검증이 끝난 뒤 최종 경로로 rename한다.
pub fn partial_path(dest: &std::path::Path) -> std::path::PathBuf {
    let file_name = dest
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "download".into());
    dest.with_file_name(format!("{file_name}.partial"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_64_hex_sha() {
        assert!(is_valid_sha256(&"a".repeat(64)));
        assert!(is_valid_sha256(&"0f".repeat(32)));
    }

    #[test]
    fn rejects_short_or_non_hex_sha() {
        assert!(!is_valid_sha256("abc"));
        assert!(!is_valid_sha256(&"g".repeat(64)));
        assert!(!is_valid_sha256(&"a".repeat(63)));
    }

    #[test]
    fn size_match_ok() {
        assert!(validate_size(100, 100).is_ok());
        assert!(validate_size(0, 0).is_ok());
    }

    #[test]
    fn size_mismatch_fails() {
        assert!(validate_size(100, 99).is_err());
        assert!(validate_size(100, 101).is_err());
        assert!(validate_size(-1, 0).is_err());
    }

    #[test]
    fn over_limit_detects_early() {
        assert!(is_over_limit(101, 100));
        assert!(!is_over_limit(100, 100));
        // expected가 없으면(음수) 상한 검사 없음 → false
        assert!(!is_over_limit(1_000_000, -1));
    }

    #[test]
    fn digest_match_ok() {
        assert!(validate_digest(&"a".repeat(64), &"a".repeat(64)).is_ok());
    }

    #[test]
    fn digest_mismatch_fails() {
        let mut other = "a".repeat(64);
        other.replace_range(0..1, "b");
        assert!(validate_digest(&"a".repeat(64), &other).is_err());
        // 잘못된 형식도 실패
        assert!(validate_digest("short", &"a".repeat(64)).is_err());
    }

    #[test]
    fn partial_name_is_sibling() {
        let dest = std::path::Path::new("apps/devbox-manager/apps/x/0.2.0/x.exe");
        let p = partial_path(dest);
        assert_eq!(
            p,
            std::path::Path::new("apps/devbox-manager/apps/x/0.2.0/x.exe.partial")
        );
    }
}
