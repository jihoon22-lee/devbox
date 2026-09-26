//! secret 봉인/해제 trait + 순수 헬퍼.
//!
//! Windows DPAPI 구현은 `dpapi` 모듈 하나에 둔다. 소비자는 용도별 entropy를 정한다.
//! envelope 버전과 entropy는 기존 암호문 호환을 위해 그대로 유지한다.

#[cfg(windows)]
pub mod dpapi;

use serde::{Deserialize, Serialize};
use std::fmt;
use zeroize::Zeroizing;

/// An opaque, non-secret name used to resolve a value at an execution
/// boundary.  The reference deliberately carries no plaintext or ciphertext;
/// owners keep the short-lived sealed value in their platform layer and only
/// persist this metadata.  This is shared by Workbench project environments
/// and the existing API/Run Manager integrations.
pub const SECRET_REFERENCE_VERSION: &str = "secret-ref/v1";

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[derive(ts_rs::TS)]
pub struct SecretReference {
    #[ts(type = "\"secret-ref/v1\"")]
    pub kind: String,
    pub name: String,
}

impl SecretReference {
    /// Create a reference for a project environment variable.  `name` is
    /// validated by the owning parser; this constructor does not copy a
    /// secret value.
    pub fn project_environment(name: impl Into<String>) -> Self {
        Self {
            kind: SECRET_REFERENCE_VERSION.to_string(),
            name: name.into(),
        }
    }

    pub fn is_project_environment(&self) -> bool {
        self.kind == SECRET_REFERENCE_VERSION && !self.name.is_empty()
    }
}

impl fmt::Debug for SecretReference {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SecretReference")
            .field("kind", &self.kind)
            .field("name", &self.name)
            .finish()
    }
}

#[derive(Debug)]
pub enum SealError {
    /// 입력이 비어 있거나 형식이 잘못됨
    InvalidInput,
    /// 봉인/해제 실패 (플랫폼 크립토)
    CryptoFailure,
    /// blob 버전이 지원되지 않음
    UnsupportedVersion(u8),
}

impl fmt::Display for SealError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput => write!(f, "secret 입력이 올바르지 않다"),
            Self::CryptoFailure => write!(f, "secret 봉인/해제 실패"),
            Self::UnsupportedVersion(v) => write!(f, "지원하지 않는 secret blob 버전: {v}"),
        }
    }
}

impl std::error::Error for SealError {}

/// 플랫폼 크립토의 단일 진입점. DPAPI(Windows) 등 OS별 구현이 이 trait을 구현한다.
pub trait Sealer: Send + Sync {
    /// 평문을 봉인해 바이트 blob으로 만든다.
    fn seal(&self, plaintext: &str) -> Result<Vec<u8>, SealError>;
    /// blob을 평문으로 해제한다. 반환값은 Zeroizing으로 메모리에서도 지워진다.
    fn unseal(&self, blob: &[u8]) -> Result<Zeroizing<String>, SealError>;
}

/// 현재 blob envelope 버전.
pub const BLOB_VERSION: u8 = 1;

/// 버전 byte + 암호문 구조의 envelope로 봉인한다.
pub fn seal_v1(sealer: &dyn Sealer, plaintext: &str) -> Result<Vec<u8>, SealError> {
    let mut out = Vec::with_capacity(1 + plaintext.len());
    out.push(BLOB_VERSION);
    out.extend(sealer.seal(plaintext)?);
    Ok(out)
}

/// envelope을 풀고 버전을 확인한 뒤 해제한다.
pub fn unseal_v1(sealer: &dyn Sealer, blob: &[u8]) -> Result<Zeroizing<String>, SealError> {
    let Some((&version, ciphertext)) = blob.split_first() else {
        return Err(SealError::InvalidInput);
    };
    if version != BLOB_VERSION {
        return Err(SealError::UnsupportedVersion(version));
    }
    sealer.unseal(ciphertext)
}

/// 앞 글자를 전체의 1/3 이하로 제한하고 나머지는 최대 12개의 별로 가린다.
pub fn mask(value: &str, visible_chars: usize) -> String {
    let count = value.chars().count();
    let visible_count = visible_chars.min(count / 3);
    let visible: String = value.chars().take(visible_count).collect();
    format!("{visible}{}", "*".repeat((count - visible_count).min(12)))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockSealer;

    impl Sealer for MockSealer {
        fn seal(&self, plaintext: &str) -> Result<Vec<u8>, SealError> {
            // 테스트용 "암호화": 역순 + 널 패딩
            let mut out: Vec<u8> = plaintext.bytes().rev().collect();
            out.push(0x00);
            Ok(out)
        }
        fn unseal(&self, blob: &[u8]) -> Result<Zeroizing<String>, SealError> {
            let trimmed = blob.strip_suffix(&[0x00]).unwrap_or(blob);
            Ok(Zeroizing::new(
                trimmed.iter().rev().map(|b| *b as char).collect(),
            ))
        }
    }

    #[test]
    fn roundtrips_v1() {
        let s = MockSealer;
        let blob = seal_v1(&s, "hello secret").unwrap();
        assert_eq!(blob[0], 1);
        assert_eq!(&unseal_v1(&s, &blob).unwrap()[..], "hello secret");
    }

    #[test]
    fn rejects_wrong_version() {
        let s = MockSealer;
        let blob = seal_v1(&s, "x").unwrap();
        let mut bad = blob.clone();
        bad[0] = 9;
        assert!(matches!(
            unseal_v1(&s, &bad),
            Err(SealError::UnsupportedVersion(9))
        ));
    }

    #[test]
    fn rejects_empty_blob() {
        let s = MockSealer;
        assert!(matches!(unseal_v1(&s, &[]), Err(SealError::InvalidInput)));
    }

    #[test]
    fn short_secrets_are_never_fully_visible() {
        assert_eq!(mask("abc", 3), "a**");
        assert_eq!(mask("ab", 5), "**");
        assert_eq!(
            mask("secret-token-value", 4),
            format!("secr{}", "*".repeat(12))
        );
        assert_eq!(mask("", 4), "");
        assert_eq!(mask("abcdef", 0), "******");
        assert_eq!(mask("비밀🔐", 10), "비**");
    }

    #[test]
    fn mask_hides_tail() {
        assert_eq!(mask("", 3), "");
        assert_eq!(mask("short", 3), "s****");
        assert_eq!(mask("abc", 3), "a**");
    }

    #[test]
    fn mask_caps_hidden_at_12() {
        assert_eq!(
            mask("abcdefghijklmnop", 1),
            "a".to_owned() + &"*".repeat(12)
        );
    }
}
