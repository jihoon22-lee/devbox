use crate::core::oauth::TokenResponse;
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
use zeroize::Zeroizing;
const FAILED: &str = "oauth2_storage_failed";
const SCHEMA: &str = "devbox.api-studio.oauth2-tokens";
const LIMIT: usize = 1024 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredToken {
    key: String,
    access: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    refresh: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    expires_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    scope: Option<String>,
    obtained_at_ms: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    schema: String,
    version: u32,
    tokens: Vec<StoredToken>,
}
impl Default for Document {
    fn default() -> Self {
        Self {
            schema: SCHEMA.into(),
            version: 1,
            tokens: vec![],
        }
    }
}
pub enum CachedToken {
    Valid {
        access: Zeroizing<String>,
        expires_at_ms: Option<u64>,
    },
    Expired {
        refresh: Option<Zeroizing<String>>,
    },
    Missing,
}
impl std::fmt::Debug for CachedToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Valid { .. } => "CachedToken::Valid { redacted }",
            Self::Expired { .. } => "CachedToken::Expired { redacted }",
            Self::Missing => "CachedToken::Missing",
        })
    }
}
#[derive(Clone, Debug, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct TokenStatus {
    #[ts(type = "\"valid\" | \"expired\" | \"missing\"")]
    pub state: &'static str,
    pub expires_at_ms: Option<u64>,
    pub scope: Option<String>,
}
pub struct TokenCache {
    path: PathBuf,
    sealer: Arc<dyn devbox_secrets::Sealer>,
    lock: Mutex<()>,
}
fn expired(token: &StoredToken, now: u64) -> bool {
    token
        .expires_at_ms
        .is_some_and(|expiry| expiry.saturating_sub(60_000) <= now)
}
impl TokenCache {
    pub fn open(path: PathBuf, sealer: Arc<dyn devbox_secrets::Sealer>) -> Self {
        Self {
            path,
            sealer,
            lock: Mutex::new(()),
        }
    }
    fn read(&self) -> Result<Document, &'static str> {
        match std::fs::symlink_metadata(&self.path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Document::default())
            }
            Err(_) => return Err(FAILED),
            Ok(_) => {}
        }
        let bytes = super::super::transfer::read_bounded(&self.path, LIMIT).map_err(|_| FAILED)?;
        let Ok(document) = serde_json::from_slice::<Document>(&bytes) else {
            return Ok(Document::default());
        };
        let mut keys = std::collections::HashSet::new();
        if document.schema != SCHEMA
            || document.version != 1
            || document.tokens.len() > 64
            || document.tokens.iter().any(|token| {
                token.key.is_empty()
                    || token.key.len() > 128
                    || !keys.insert(&token.key)
                    || token.access.len() > 128 * 1024
                    || token
                        .refresh
                        .as_ref()
                        .is_some_and(|value| value.len() > 128 * 1024)
                    || token
                        .scope
                        .as_ref()
                        .is_some_and(|value| value.len() > 32 * 1024)
            })
        {
            return Ok(Document::default());
        }
        Ok(document)
    }
    fn write(&self, document: &Document) -> Result<(), &'static str> {
        let bytes = serde_json::to_vec(document).map_err(|_| FAILED)?;
        if bytes.len() > LIMIT {
            return Err(FAILED);
        }
        let parent = self.path.parent().ok_or(FAILED)?;
        for ancestor in parent.ancestors() {
            match std::fs::symlink_metadata(ancestor) {
                Ok(metadata) => {
                    if !metadata.is_dir() {
                        return Err(FAILED);
                    }
                    devbox_filesystem::ensure_no_links(ancestor).map_err(|_| FAILED)?;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err(FAILED),
            }
        }
        std::fs::create_dir_all(parent).map_err(|_| FAILED)?;
        super::super::transfer::validate_file_path(&self.path, false).map_err(|_| FAILED)?;
        devbox_filesystem::atomic_write(&self.path, &bytes).map_err(|_| FAILED)
    }
    fn unseal(&self, value: &str) -> Result<Zeroizing<String>, &'static str> {
        let bytes = B64.decode(value).map_err(|_| FAILED)?;
        devbox_secrets::unseal_v1(self.sealer.as_ref(), &bytes).map_err(|_| FAILED)
    }
    fn seal(&self, value: &str) -> Result<String, &'static str> {
        devbox_secrets::seal_v1(self.sealer.as_ref(), value)
            .map(|bytes| B64.encode(bytes))
            .map_err(|_| FAILED)
    }
    pub fn get(&self, key: &str, now_ms: u64) -> Result<CachedToken, &'static str> {
        let _guard = self.lock.lock().map_err(|_| FAILED)?;
        let document = self.read()?;
        let Some(token) = document.tokens.iter().find(|token| token.key == key) else {
            return Ok(CachedToken::Missing);
        };
        if expired(token, now_ms) {
            Ok(CachedToken::Expired {
                refresh: token
                    .refresh
                    .as_ref()
                    .map(|value| self.unseal(value))
                    .transpose()?,
            })
        } else {
            Ok(CachedToken::Valid {
                access: self.unseal(&token.access)?,
                expires_at_ms: token.expires_at_ms,
            })
        }
    }
    pub fn put(&self, key: &str, token: &TokenResponse, now_ms: u64) -> Result<(), &'static str> {
        if key.is_empty() || key.len() > 128 {
            return Err(FAILED);
        }
        let _guard = self.lock.lock().map_err(|_| FAILED)?;
        let mut document = self.read()?;
        let entry = StoredToken {
            key: key.into(),
            access: self.seal(&token.access_token)?,
            refresh: token
                .refresh_token
                .as_ref()
                .map(|value| self.seal(value))
                .transpose()?,
            expires_at_ms: token
                .expires_in
                .map(|seconds| now_ms.saturating_add(seconds.saturating_mul(1000))),
            scope: token.scopes.as_ref().map(|scopes| scopes.join(" ")),
            obtained_at_ms: now_ms,
        };
        document.tokens.retain(|token| token.key != key);
        document.tokens.push(entry);
        document.tokens.sort_by_key(|token| token.obtained_at_ms);
        if document.tokens.len() > 64 {
            document.tokens.drain(..document.tokens.len() - 64);
        }
        self.write(&document)
    }
    pub fn remove(&self, key: &str) -> Result<(), &'static str> {
        let _guard = self.lock.lock().map_err(|_| FAILED)?;
        let mut document = self.read()?;
        document.tokens.retain(|token| token.key != key);
        self.write(&document)
    }
    pub fn status(&self, key: &str, now_ms: u64) -> TokenStatus {
        let missing = || TokenStatus {
            state: "missing",
            expires_at_ms: None,
            scope: None,
        };
        let Ok(_guard) = self.lock.lock() else {
            return missing();
        };
        let Ok(document) = self.read() else {
            return missing();
        };
        document
            .tokens
            .iter()
            .find(|token| token.key == key)
            .map_or_else(missing, |token| TokenStatus {
                state: if expired(token, now_ms) {
                    "expired"
                } else {
                    "valid"
                },
                expires_at_ms: token.expires_at_ms,
                scope: token.scope.clone(),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct FakeSealer;
    impl devbox_secrets::Sealer for FakeSealer {
        fn seal(&self, plain: &str) -> std::result::Result<Vec<u8>, devbox_secrets::SealError> {
            Ok(plain.as_bytes().iter().rev().copied().collect())
        }
        fn unseal(
            &self,
            sealed: &[u8],
        ) -> std::result::Result<Zeroizing<String>, devbox_secrets::SealError> {
            String::from_utf8(sealed.iter().rev().copied().collect())
                .map(Zeroizing::new)
                .map_err(|_| devbox_secrets::SealError::InvalidInput)
        }
    }
    fn token(access: &str, refresh: Option<&str>, expires_in: Option<u64>) -> TokenResponse {
        let mut value =
            serde_json::json!({"access_token":access,"token_type":"Bearer","scope":"read"});
        if let Some(refresh) = refresh {
            value["refresh_token"] = refresh.into();
        }
        if let Some(seconds) = expires_in {
            value["expires_in"] = seconds.into();
        }
        crate::core::oauth::parse_token_response(&serde_json::to_vec(&value).unwrap()).unwrap()
    }
    #[test]
    fn tokens_are_sealed_on_disk_and_expire_with_a_safety_margin() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("oauth2-tokens.json");
        let cache = TokenCache::open(path.clone(), Arc::new(FakeSealer));
        cache
            .put("k", &token("access-1", Some("refresh-1"), Some(3600)), 1000)
            .unwrap();
        let raw = std::fs::read_to_string(path).unwrap();
        assert!(!raw.contains("access-1") && !raw.contains("refresh-1"));
        assert!(matches!(
            cache.get("k", 1000 + 3_000_000).unwrap(),
            CachedToken::Valid { .. }
        ));
        match cache.get("k", 1000 + 3_600_000 - 59_000).unwrap() {
            CachedToken::Expired { refresh } => assert_eq!(refresh.unwrap().as_str(), "refresh-1"),
            _ => panic!("expected expired token"),
        };
        assert_eq!(cache.status("k", 1000).state, "valid");
        cache.remove("k").unwrap();
        assert!(matches!(
            cache.get("k", 1000).unwrap(),
            CachedToken::Missing
        ));
    }
    #[test]
    fn corrupt_cache_is_empty_until_rewritten_and_tokens_without_expiry_remain_valid() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("oauth2-tokens.json");
        std::fs::write(&path, "{broken").unwrap();
        let cache = TokenCache::open(path, Arc::new(FakeSealer));
        assert!(matches!(cache.get("k", 1).unwrap(), CachedToken::Missing));
        cache.put("k", &token("access", None, None), 1).unwrap();
        assert!(matches!(
            cache.get("k", 10_000_000_000).unwrap(),
            CachedToken::Valid { .. }
        ));
    }
    #[test]
    fn cache_keeps_at_most_sixty_four_profiles_and_does_not_follow_links() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("oauth2-tokens.json");
        let cache = TokenCache::open(path.clone(), Arc::new(FakeSealer));
        for i in 0..65 {
            cache
                .put(&format!("k{i}"), &token("access", None, None), i)
                .unwrap();
        }
        assert!(matches!(
            cache.get("k0", 100).unwrap(),
            CachedToken::Missing
        ));
        assert!(matches!(
            cache.get("k64", 100).unwrap(),
            CachedToken::Valid { .. }
        ));
        #[cfg(unix)]
        {
            std::fs::remove_file(&path).unwrap();
            let outside = dir.path().join("outside");
            std::fs::write(&outside, "keep").unwrap();
            std::os::unix::fs::symlink(&outside, &path).unwrap();
            assert!(cache.put("k", &token("access", None, None), 1).is_err());
            assert_eq!(std::fs::read_to_string(outside).unwrap(), "keep");
        }
    }
}
