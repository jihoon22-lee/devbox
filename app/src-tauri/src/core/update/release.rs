use super::{R, UpdateError};
use serde::Deserialize;
pub const REPOSITORY: &str = "jihoon22-lee/devbox";
pub const MAX_INSTALLER: u64 = 256 * 1024 * 1024;
pub const MAX_METADATA: usize = 1024 * 1024;
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub version: String,
    pub id: u64,
    pub size: u64,
    pub digest: [u8; 32],
}
#[derive(Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
}
#[derive(Deserialize)]
struct Asset {
    id: u64,
    name: String,
    state: String,
    size: u64,
    digest: Option<String>,
    url: String,
    browser_download_url: String,
}
pub fn stable(value: &str) -> Option<semver::Version> {
    let parsed = semver::Version::parse(value).ok()?;
    (parsed.pre.is_empty() && parsed.build.is_empty() && parsed.to_string() == value)
        .then_some(parsed)
}
pub fn digest(value: &str) -> R<[u8; 32]> {
    if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(UpdateError::Digest);
    }
    let mut bytes = [0; 32];
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte =
            u8::from_str_radix(&value[i * 2..i * 2 + 2], 16).map_err(|_| UpdateError::Digest)?;
    }
    Ok(bytes)
}
pub fn select(bytes: &[u8], current: &str) -> R<Option<Candidate>> {
    if bytes.len() > MAX_METADATA {
        return Err(UpdateError::Size);
    }
    let release: Release =
        serde_json::from_slice(bytes).map_err(|_| UpdateError::InvalidRelease)?;
    let version = release
        .tag_name
        .strip_prefix('v')
        .and_then(stable)
        .ok_or(UpdateError::InvalidRelease)?;
    if release.draft || release.prerelease {
        return Err(UpdateError::InvalidRelease);
    }
    // Older stable releases are a normal no-update result, including the repository's legacy release.
    if version <= stable(current).ok_or(UpdateError::InvalidRelease)? {
        return Ok(None);
    }
    if release.assets.len() != 1 {
        return Err(UpdateError::InvalidRelease);
    }
    let asset = release
        .assets
        .into_iter()
        .next()
        .ok_or(UpdateError::InvalidRelease)?;
    let name = format!("devbox-{version}-setup.exe");
    if asset.id == 0
        || asset.name != name
        || asset.state != "uploaded"
        || asset.url
            != format!(
                "https://api.github.com/repos/{REPOSITORY}/releases/assets/{}",
                asset.id
            )
        || asset.browser_download_url
            != format!("https://github.com/{REPOSITORY}/releases/download/v{version}/{name}")
    {
        return Err(UpdateError::InvalidRelease);
    }
    if asset.size == 0 || asset.size > MAX_INSTALLER {
        return Err(UpdateError::Size);
    }
    let sha = asset
        .digest
        .as_deref()
        .and_then(|v| v.strip_prefix("sha256:"))
        .ok_or(UpdateError::Digest)?;
    Ok(Some(Candidate {
        version: version.to_string(),
        id: asset.id,
        size: asset.size,
        digest: digest(sha)?,
    }))
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> serde_json::Value {
        serde_json::json!({"tag_name":"v1.1.0","draft":false,"prerelease":false,"assets":[{"id":1,"name":"devbox-1.1.0-setup.exe","state":"uploaded","size":3,"digest":format!("sha256:{}", "00".repeat(32)),"url":"https://api.github.com/repos/jihoon22-lee/devbox/releases/assets/1","browser_download_url":"https://github.com/jihoon22-lee/devbox/releases/download/v1.1.0/devbox-1.1.0-setup.exe"}]})
    }
    #[test]
    fn multiple_release_assets_cannot_be_selected_as_a_trusted_installer() {
        let mut value = fixture();
        let second = value["assets"][0].clone();
        value["assets"].as_array_mut().unwrap().push(second);
        assert_eq!(
            select(&serde_json::to_vec(&value).unwrap(), "1.0.0"),
            Err(UpdateError::InvalidRelease)
        );
    }
    #[test]
    fn stable_versions_are_canonical_and_older_legacy_release_is_no_update() {
        for invalid in [
            "01.0.0",
            "1.0",
            "v1.0.0",
            "1.0.0-dev",
            "1.0.0+build",
            " 1.0.0",
        ] {
            assert!(stable(invalid).is_none());
        }
        let mut value = fixture();
        value["tag_name"] = "v0.9.0".into();
        value["assets"] = serde_json::json!([]);
        assert_eq!(
            select(&serde_json::to_vec(&value).unwrap(), "1.0.0"),
            Ok(None)
        );
        assert_eq!(
            select(&serde_json::to_vec(&fixture()).unwrap(), "1.1.0"),
            Ok(None)
        );
        assert!(
            select(&serde_json::to_vec(&fixture()).unwrap(), "1.0.0")
                .unwrap()
                .is_some()
        );
    }
    #[test]
    fn draft_prerelease_missing_multiple_or_wrong_channel_assets_are_rejected() {
        for (pointer, replacement) in [
            ("/draft", serde_json::json!(true)),
            ("/prerelease", serde_json::json!(true)),
            ("/tag_name", serde_json::json!("v1.2.0-rc.1")),
            ("/assets", serde_json::json!([])),
            ("/assets/0/name", serde_json::json!("Devbox-Accept.exe")),
            ("/assets/0/state", serde_json::json!("new")),
            ("/assets/0/id", serde_json::json!(0)),
            (
                "/assets/0/url",
                serde_json::json!("https://api.github.com/repos/other/repo/releases/assets/1"),
            ),
            (
                "/assets/0/browser_download_url",
                serde_json::json!("https://evil.invalid/body"),
            ),
        ] {
            let mut value = fixture();
            *value.pointer_mut(pointer).unwrap() = replacement;
            assert_eq!(
                select(&serde_json::to_vec(&value).unwrap(), "1.0.0"),
                Err(UpdateError::InvalidRelease),
                "{pointer}"
            );
        }
    }
    #[test]
    fn metadata_size_asset_size_and_sha256_are_required() {
        assert_eq!(
            select(&vec![b' '; MAX_METADATA + 1], "1.0.0"),
            Err(UpdateError::Size)
        );
        for size in [0, MAX_INSTALLER + 1] {
            let mut value = fixture();
            value["assets"][0]["size"] = size.into();
            assert_eq!(
                select(&serde_json::to_vec(&value).unwrap(), "1.0.0"),
                Err(UpdateError::Size)
            );
        }
        for bad in [
            serde_json::Value::Null,
            serde_json::json!("sha256:00"),
            serde_json::json!(format!("sha1:{}", "00".repeat(32))),
            serde_json::json!(format!("sha256:{}", "zz".repeat(32))),
        ] {
            let mut value = fixture();
            value["assets"][0]["digest"] = bad;
            assert_eq!(
                select(&serde_json::to_vec(&value).unwrap(), "1.0.0"),
                Err(UpdateError::Digest)
            );
        }
    }
}
