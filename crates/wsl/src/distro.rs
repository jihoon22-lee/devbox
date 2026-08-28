use crate::WslError;

/// distro 이름 검증. 빈 문자열, 제어 문자, argv 주입 가능한 문자를 거부한다.
///
/// distro 이름은 `wsl.exe -d <distro>`의 단일 argv가 되므로, 셸 메타문자를
/// 포함하면 argv가 깨질 수 있다. (WSL이 직접 argv를 소비하지만 방어선을 둔다)
///
/// 공백은 허용한다 — "Ubuntu 24.04"처럼 실제 WSL distro 이름이 공백을 포함할
/// 수 있고, 단일 argv 요소로 직접 전달되므로 셸 인용 문제가 없다.
pub fn validate_distro_name(name: &str) -> Result<(), WslError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(WslError::InvalidDistro("빈 문자열".into()));
    }
    if name.starts_with('-') {
        return Err(WslError::InvalidDistro("옵션처럼 해석될 수 있음".into()));
    }
    if name.chars().any(|c| c.is_control()) {
        return Err(WslError::InvalidDistro("제어 문자 포함".into()));
    }
    if name.chars().any(|c| {
        matches!(
            c,
            ';' | '&'
                | '|'
                | '<'
                | '>'
                | '`'
                | '$'
                | '"'
                | '\''
                | '\\'
                | '('
                | ')'
                | '{'
                | '}'
                | '*'
                | '?'
                | '['
                | ']'
                | '!'
                | '~'
                | '#'
                | '%'
                | '\r'
                | '\n'
        )
    }) {
        return Err(WslError::InvalidDistro("argv 주입 가능한 문자 포함".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_normal_distro_names() {
        assert!(validate_distro_name("Ubuntu").is_ok());
        assert!(validate_distro_name("docker-desktop").is_ok());
        assert!(validate_distro_name("Debian-12").is_ok());
        assert!(validate_distro_name("kali-linux").is_ok());
        // 공백은 허용 (argv 단일 요소로 직접 전달)
        assert!(validate_distro_name("Ubuntu 24.04").is_ok());
    }

    #[test]
    fn rejects_empty_and_whitespace() {
        assert!(validate_distro_name("").is_err());
        assert!(validate_distro_name("   ").is_err());
        assert!(validate_distro_name("--help").is_err());
    }

    #[test]
    fn rejects_argv_injection_chars() {
        for bad in [
            "a;b", "a&b", "a|b", "a<b", "a>b", "a`b", "a$b", "a\"b", "a'b", "a\\b", "a(b", "a*b",
            "a?b",
        ] {
            assert!(validate_distro_name(bad).is_err(), "should reject: {bad}");
        }
    }

    #[test]
    fn rejects_control_chars() {
        assert!(validate_distro_name("a\nb").is_err());
        assert!(validate_distro_name("a\u{0007}b").is_err());
    }

    #[test]
    fn trims_outer_whitespace_before_checking() {
        // trim 후 유효하면 통과
        assert!(validate_distro_name("  Ubuntu  ").is_ok());
    }
}
