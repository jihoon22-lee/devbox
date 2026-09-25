//! Shared text redaction and derived-path checks, retained from the data inspector.
use std::{
    fs::{self, Metadata},
    path::{Component, Path, PathBuf},
};
const MAX_CELL_BYTES: usize = 64 * 1024;
#[derive(Debug)]
enum PathState {
    Unsafe,
    Unreadable,
}
pub const REDACTION_VERSION: &str = "v1";
const REDACTED_VALUE: &str = "[REDACTED]";
const REDACTED_PATH: &str = "[REDACTED_PATH]";

pub fn is_sensitive_name(name: &str) -> bool {
    // Normalize separators/casing so aliases such as `apiKey`,
    // `client-secret`, and `refreshToken` cannot evade the name policy.
    let normalized = name
        .bytes()
        .filter(|byte| byte.is_ascii_alphanumeric())
        .map(|byte| byte.to_ascii_lowercase())
        .collect::<Vec<_>>();
    [
        "secret",
        "token",
        "password",
        "passwd",
        "credential",
        "authorization",
        "cookie",
        "apikey",
        "accesskey",
        "privatekey",
        "clientsecret",
        "refreshtoken",
        "rawbody",
        // Usernames and email/login identifiers are personal data too. Treat
        // both compact and separated spellings (`user_name`, `user-name`) as
        // sensitive source columns so a direct SELECT cannot expose them.
        "user",
        "username",
        "userid",
        "login",
        "email",
    ]
    .iter()
    .any(|needle| {
        normalized
            .windows(needle.len())
            .any(|window| window == needle.as_bytes())
    })
}

/// Redact credentials, auth headers, common token formats, and filesystem
/// paths from free-form log/text values. It intentionally errs on the side of
/// masking a value; the inspector is for shape and health, not raw payloads.
pub fn redact_text(input: &str, app_id: &str) -> String {
    let mut limit = input.len().min(MAX_CELL_BYTES);
    while !input.is_char_boundary(limit) {
        limit -= 1;
    }
    let mut output = input[..limit].to_string();
    if limit < input.len() {
        output.push_str("…[truncated]");
    }
    for key in [
        "authorization",
        "proxy-authorization",
        "cookie",
        "set-cookie",
        "password",
        "passwd",
        "secret",
        "token",
        "api_key",
        "apikey",
        "client_secret",
        "refresh_token",
        "username",
        "user_name",
        "user-name",
        "user",
        "login",
        "email",
        "e-mail",
    ] {
        output = redact_key_value(&output, key);
    }
    let mut words = Vec::new();
    for word in output.split_whitespace() {
        let clean = word.trim_matches(|char: char| "\"'`()[]{}<>.,;".contains(char));
        let lower = clean.to_ascii_lowercase();
        if looks_like_secret_token(&lower) || is_sensitive_value(clean) || looks_like_email(clean) {
            words.push(REDACTED_VALUE.to_string());
        } else if looks_like_path(clean) && clean != app_id {
            words.push(REDACTED_PATH.to_string());
        } else {
            words.push(word.to_string());
        }
    }
    words.join(" ")
}

fn looks_like_secret_token(lower: &str) -> bool {
    lower.starts_with("ghp_")
        || lower.starts_with("gho_")
        || lower.starts_with("ghs_")
        || lower.starts_with("ghu_")
        || lower.starts_with("ghr_")
        || lower.starts_with("github_pat_")
        || lower.starts_with("glpat-")
        || lower.starts_with("npm_")
        || lower.starts_with("pypi-")
        || lower.starts_with("sk-")
        || lower.starts_with("sk_live_")
        || lower.starts_with("xoxb-")
        || lower.starts_with("xoxp-")
        || lower.starts_with("xoxs-")
        || lower.starts_with("akia")
        || lower.starts_with("aiza")
        || lower.starts_with("ya29.")
        || (lower.starts_with("eyj") && lower.matches('.').count() >= 2)
}

fn redact_key_value(input: &str, key: &str) -> String {
    let lower = input.to_ascii_lowercase();
    let key_lower = key.to_ascii_lowercase();
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0usize;
    while let Some(relative) = lower[cursor..].find(&key_lower) {
        let start = cursor + relative;
        let before_ok = start == 0
            || !lower.as_bytes()[start - 1].is_ascii_alphanumeric()
                && lower.as_bytes()[start - 1] != b'_'
                && lower.as_bytes()[start - 1] != b'-';
        let after = start + key_lower.len();
        let after_ok = after == lower.len()
            || !lower.as_bytes()[after].is_ascii_alphanumeric()
                && lower.as_bytes()[after] != b'_'
                && lower.as_bytes()[after] != b'-';
        if !before_ok || !after_ok {
            output.push_str(&input[cursor..after]);
            cursor = after;
            continue;
        }
        output.push_str(&input[cursor..after]);
        let mut end = after;
        // Preserve a quoted key's closing quote and the key/value separator,
        // but never copy bytes from the value itself.
        if start > 0
            && end < input.len()
            && matches!(input.as_bytes()[start - 1], b'"' | b'\'')
            && input.as_bytes()[end] == input.as_bytes()[start - 1]
        {
            output.push(input.as_bytes()[end] as char);
            end += 1;
        }
        while end < input.len() && matches!(input.as_bytes()[end], b' ' | b'\t') {
            output.push(input.as_bytes()[end] as char);
            end += 1;
        }
        if end < input.len() && matches!(input.as_bytes()[end], b':' | b'=') {
            output.push(input.as_bytes()[end] as char);
            end += 1;
        } else {
            output.push(':');
        }
        while end < input.len() && matches!(input.as_bytes()[end], b' ' | b'\t') {
            output.push(input.as_bytes()[end] as char);
            end += 1;
        }
        let is_header = matches!(
            key_lower.as_str(),
            "authorization" | "proxy-authorization" | "cookie" | "set-cookie"
        );
        if is_header {
            while end < input.len() && !matches!(input.as_bytes()[end], b'\n' | b'\r') {
                end += 1;
            }
            output.push_str(REDACTED_VALUE);
        } else if end < input.len() && matches!(input.as_bytes()[end], b'"' | b'\'') {
            let quote = input.as_bytes()[end];
            output.push(quote as char);
            end += 1;
            let mut closed = false;
            while end < input.len() {
                match input.as_bytes()[end] {
                    b'\\' if end + 1 < input.len() => end += 2,
                    value if value == quote => {
                        end += 1;
                        closed = true;
                        break;
                    }
                    _ => end += 1,
                }
            }
            output.push_str(REDACTED_VALUE);
            if closed {
                output.push(quote as char);
            }
        } else {
            while end < input.len()
                && !input.as_bytes()[end].is_ascii_whitespace()
                && !matches!(input.as_bytes()[end], b',' | b'}' | b']')
            {
                end += 1;
            }
            output.push_str(REDACTED_VALUE);
        }
        cursor = end;
    }
    output.push_str(&input[cursor..]);
    output
}

fn looks_like_path(value: &str) -> bool {
    value.starts_with('/')
        || value.starts_with("\\\\")
        || value
            .get(1..3)
            .is_some_and(|prefix| prefix == ":\\" || prefix == ":/")
        || value.starts_with("~/")
        || value.starts_with("./")
        || value.starts_with("../")
        || value.starts_with(".\\")
        || value.starts_with("..\\")
        || value.contains('/')
        || value.contains('\\')
        || value.contains("\\Users\\")
        || value.contains("/home/")
}

fn is_sensitive_value(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    [
        "secret",
        "password",
        "passwd",
        "credential",
        "authorization",
        "cookie",
        "api_key",
        "apikey",
        "bearer",
        "token",
    ]
    .iter()
    .any(|needle| lower.contains(*needle))
}

fn looks_like_email(value: &str) -> bool {
    let Some((local, domain)) = value.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && !domain.is_empty()
        && domain.contains('.')
        && !value.chars().any(char::is_control)
}

fn reject_link_components(candidate: &Path) -> Result<(), PathState> {
    let mut current = PathBuf::new();
    for component in candidate.components() {
        match component {
            Component::Prefix(prefix) => current.push(prefix.as_os_str()),
            Component::RootDir => current.push(component.as_os_str()),
            Component::CurDir => continue,
            Component::ParentDir => return Err(PathState::Unsafe),
            Component::Normal(value) => current.push(value),
        }
        if matches!(component, Component::Prefix(_)) {
            continue;
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) if is_link_or_reparse(&metadata) => return Err(PathState::Unsafe),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(_) => return Err(PathState::Unreadable),
        }
    }
    Ok(())
}

/// Check a catalog-derived path without following a link/reparse component.
/// The target may not exist yet (for example, an app's `logs` directory), so
/// this deliberately validates all existing ancestors and lexical containment
/// rather than requiring a final `canonicalize`.
pub(crate) fn safe_derived_path(root: &Path, candidate: &Path) -> bool {
    root.is_absolute() && candidate.starts_with(root) && reject_link_components(candidate).is_ok()
}

#[cfg(not(windows))]
pub(crate) fn is_link_or_reparse(metadata: &Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(windows)]
pub(crate) fn is_link_or_reparse(metadata: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
    metadata.file_type().is_symlink()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn credentials_names_and_paths_are_redacted() {
        for value in [
            "Authorization: Bearer fixture-value",
            "username=alice",
            "C:\\Users\\alice\\file",
            "/home/alice/file",
            "ghp_fixture",
        ] {
            let sanitized = redact_text(value, "fixture");
            assert!(
                !sanitized.contains("alice")
                    && !sanitized.contains("fixture-value")
                    && !sanitized.contains("ghp_fixture")
            );
        }
        assert_eq!(redact_text("git 2.50", "fixture"), "git 2.50");
    }
    #[test]
    fn derived_paths_reject_escape_and_parent_components() {
        let root = std::env::temp_dir();
        assert!(safe_derived_path(
            &root,
            &root.join("devbox-synthetic-missing/logs")
        ));
        assert!(!safe_derived_path(&root, &root.join("../outside")));
        assert!(!safe_derived_path(
            Path::new("relative"),
            Path::new("relative/logs")
        ));
    }
    #[test]
    fn name_and_username_redaction_normalize_common_variants() {
        assert!(is_sensitive_name("client-secret"));
        assert!(is_sensitive_name("refreshToken"));
        assert!(is_sensitive_name("user_name"));
        assert!(is_sensitive_name("email"));
        let redacted = redact_text(
            "username: alice email=alice@example.com user_name=bob",
            "testapp",
        );
        assert!(!redacted.contains("alice"));
        assert!(!redacted.contains("bob"));

        let json = redact_text(
            r#"{"username":"alice","email":"alice@example.com","user_name":"bob","login":"carol"}"#,
            "testapp",
        );
        assert!(!json.contains("alice"));
        assert!(!json.contains("bob"));
        assert!(!json.contains("carol"));
        let escaped = redact_text(r#"{"username":"al\"ice","mode":"safe"}"#, "testapp");
        assert!(!escaped.contains("ice"));
        assert!(escaped.contains("mode"));
    }
}
