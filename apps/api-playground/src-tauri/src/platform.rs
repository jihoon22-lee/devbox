//! secret 봉인 플랫폼 레이어. DPAPI(Windows)는 여기서 구현하고,
//! 순수 envelope·마스킹은 `crates/secrets`(devbox_secrets)를 쓴다 (CONVENTIONS §4).

use devbox_secrets::SealError;
use zeroize::Zeroizing;

/// 플랫폼에 맞는 `Sealer`를 반환한다. Windows가 아니면 봉인 불가(명확한 오류).
pub fn platform_sealer() -> Box<dyn devbox_secrets::Sealer> {
    #[cfg(target_os = "windows")]
    {
        Box::new(DpapiSealer)
    }
    #[cfg(not(target_os = "windows"))]
    {
        Box::new(UnsupportedSealer)
    }
}

#[cfg(target_os = "windows")]
mod windows_impl {
    use super::*;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    const ENTROPY: &[u8] = b"devbox.api-playground.secrets.v1";

    pub(super) struct DpapiSealer;

    impl devbox_secrets::Sealer for DpapiSealer {
        fn seal(&self, plaintext: &str) -> Result<Vec<u8>, SealError> {
            unsafe {
                let input = blob(plaintext.as_bytes())?;
                let entropy = blob(ENTROPY)?;
                let mut output = CRYPT_INTEGER_BLOB::default();
                CryptProtectData(
                    &input,
                    PCWSTR::null(),
                    Some(&entropy as *const _),
                    None,
                    None,
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut output,
                )
                .map_err(|_| SealError::CryptoFailure)?;
                copy_and_free(output)
            }
        }

        fn unseal(&self, ciphertext: &[u8]) -> Result<Zeroizing<String>, SealError> {
            unsafe {
                let input = blob(ciphertext)?;
                let entropy = blob(ENTROPY)?;
                let mut output = CRYPT_INTEGER_BLOB::default();
                CryptUnprotectData(
                    &input,
                    None,
                    Some(&entropy as *const _),
                    None,
                    None,
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut output,
                )
                .map_err(|_| SealError::CryptoFailure)?;
                let bytes = copy_and_free(output)?;
                let text = String::from_utf8(bytes).map_err(|_| SealError::InvalidInput)?;
                Ok(Zeroizing::new(text))
            }
        }
    }

    unsafe fn blob(bytes: &[u8]) -> Result<CRYPT_INTEGER_BLOB, SealError> {
        let cb_data = u32::try_from(bytes.len()).map_err(|_| SealError::InvalidInput)?;
        Ok(CRYPT_INTEGER_BLOB {
            cbData: cb_data,
            pbData: bytes.as_ptr() as *mut u8,
        })
    }

    unsafe fn copy_and_free(blob: CRYPT_INTEGER_BLOB) -> Result<Vec<u8>, SealError> {
        if blob.pbData.is_null() {
            return Err(SealError::CryptoFailure);
        }
        let copied = std::slice::from_raw_parts(blob.pbData, blob.cbData as usize).to_vec();
        let _ = LocalFree(Some(HLOCAL(blob.pbData.cast())));
        if copied.is_empty() {
            Err(SealError::CryptoFailure)
        } else {
            Ok(copied)
        }
    }
}

#[cfg(target_os = "windows")]
pub use windows_impl::DpapiSealer;

struct UnsupportedSealer;

impl devbox_secrets::Sealer for UnsupportedSealer {
    fn seal(&self, _plaintext: &str) -> Result<Vec<u8>, SealError> {
        Err(SealError::CryptoFailure)
    }
    fn unseal(&self, _blob: &[u8]) -> Result<Zeroizing<String>, SealError> {
        Err(SealError::CryptoFailure)
    }
}
