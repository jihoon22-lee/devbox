//! Local daemon authority comes from its OS namespace, never shell environment.
pub(super) async fn local_hostname() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        read_hostname(std::path::Path::new("/proc/sys/kernel/hostname")).await
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

#[cfg(any(target_os = "linux", test))]
async fn read_hostname(path: &std::path::Path) -> Option<String> {
    use tokio::io::AsyncReadExt;
    let file = tokio::fs::File::open(path).await.ok()?;
    let mut bytes = Vec::new();
    file.take(257).read_to_end(&mut bytes).await.ok()?;
    let text = std::str::from_utf8(&bytes).ok()?;
    let text = text.strip_suffix('\n').unwrap_or(text);
    if text.is_empty()
        || text.len() > 255
        || !text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
        || !text.bytes().any(|b| b.is_ascii_alphanumeric())
    {
        return None;
    }
    Some(text.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn hostname_read_is_bounded_and_missing_or_invalid_authority_is_unknown() {
        let fixture = tempfile::tempdir().expect("hostname fixture");
        let path = fixture.path().join("hostname");
        assert!(read_hostname(&path).await.is_none());
        std::fs::write(&path, "LOCAL-WSL.\n").expect("OS fixture");
        assert_eq!(read_hostname(&path).await.as_deref(), Some("LOCAL-WSL."));
        for invalid in [
            "".into(),
            "bad host\n".into(),
            "bad/host\n".into(),
            "local\nremote\n".into(),
            "a".repeat(256),
            "a".repeat(1024),
            "host\0\n".into(),
        ] {
            std::fs::write(&path, invalid).expect("invalid fixture");
            assert!(read_hostname(&path).await.is_none());
        }
    }
}
