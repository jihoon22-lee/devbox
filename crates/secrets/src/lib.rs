//! secret 봉인/해제 trait + 순수 헬퍼.
//!
//! 추출 근거: run-manager(첫 소비자)와 api-playground(두 번째 소비자)가 DPAPI 기반
//! secret 보호를 공유한다. CONVENTIONS §4는 crates에 Windows 전용 코드를 금지하므로,
//! **순수 부분(trait·마스킹·버전 blob envelope)만 여기에 두고** 실제
//! CryptProtectData 호출은 각 앱의 platform 레이어가 `Sealer`로 구현한다.

use std::fmt;
use zeroize::Zeroizing;

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

/// 마스킹. 앞 몇 글자만 남기고 나머지를 `*`로 가린다.
pub fn mask(value: &str, visible_chars: usize) -> String {
    if value.is_empty() {
        return String::new();
    }
    let visible = value.chars().take(visible_chars).collect::<String>();
    let hidden = value.chars().count().saturating_sub(visible_chars);
    if hidden == 0 {
        visible
    } else {
        format!("{visible}{}", "*".repeat(hidden.min(12)))
    }
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
    fn mask_hides_tail() {
        assert_eq!(mask("", 3), "");
        assert_eq!(mask("short", 3), "sho**");
        assert_eq!(mask("abc", 3), "abc");
    }

    #[test]
    fn mask_caps_hidden_at_12() {
        assert_eq!(
            mask("abcdefghijklmnop", 1),
            "a".to_owned() + &"*".repeat(12)
        );
    }
}
