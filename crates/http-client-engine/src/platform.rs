//! secret 봉인 플랫폼 레이어. DPAPI(Windows)는 여기서 구현하고,
//! 순수 envelope·마스킹은 `crates/secrets`(devbox_secrets)를 쓴다 (CONVENTIONS §4).

use devbox_secrets::SealError;
use zeroize::Zeroizing;

/// 플랫폼에 맞는 `Sealer`를 반환한다. Windows가 아니면 봉인 불가(명확한 오류).
pub fn platform_sealer() -> Box<dyn devbox_secrets::Sealer> {
    #[cfg(target_os = "windows")]
    {
        Box::new(windows_impl::DpapiSealer::environment())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Box::new(UnsupportedSealer)
    }
}

/// gRPC TLS material uses a distinct DPAPI entropy domain so its sealed blobs
/// cannot be replayed as ordinary request-environment secrets (or vice versa).
pub fn platform_grpc_sealer() -> Box<dyn devbox_secrets::Sealer> {
    #[cfg(target_os = "windows")]
    {
        Box::new(windows_impl::DpapiSealer::grpc_tls())
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
    use zeroize::Zeroize;

    const ENVIRONMENT_ENTROPY: &[u8] = b"devbox.api-playground.secrets.v1";
    const GRPC_TLS_ENTROPY: &[u8] = b"devbox.api-playground.grpc-tls-credentials.v1";

    pub(crate) struct DpapiSealer {
        entropy: &'static [u8],
    }

    impl DpapiSealer {
        pub(crate) fn environment() -> Self {
            Self {
                entropy: ENVIRONMENT_ENTROPY,
            }
        }

        pub(crate) fn grpc_tls() -> Self {
            Self {
                entropy: GRPC_TLS_ENTROPY,
            }
        }
    }

    impl devbox_secrets::Sealer for DpapiSealer {
        fn seal(&self, plaintext: &str) -> Result<Vec<u8>, SealError> {
            unsafe {
                let input = blob(plaintext.as_bytes())?;
                let entropy = blob(self.entropy)?;
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
                let entropy = blob(self.entropy)?;
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
                let mut bytes = Zeroizing::new(copy_and_free(output)?);
                let owned = std::mem::take(&mut *bytes);
                let text = match String::from_utf8(owned) {
                    Ok(value) => value,
                    Err(error) => {
                        let mut invalid = error.into_bytes();
                        invalid.zeroize();
                        return Err(SealError::InvalidInput);
                    }
                };
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
        let length = blob.cbData as usize;
        let copied = std::slice::from_raw_parts(blob.pbData, length).to_vec();
        for index in 0..length {
            std::ptr::write_volatile(blob.pbData.add(index), 0);
        }
        let _ = LocalFree(Some(HLOCAL(blob.pbData.cast())));
        if copied.is_empty() {
            Err(SealError::CryptoFailure)
        } else {
            Ok(copied)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::DpapiSealer;
        use devbox_secrets::Sealer;

        #[test]
        fn dpapi_entropy_domains_do_not_cross_unseal() {
            let environment = DpapiSealer::environment();
            let grpc = DpapiSealer::grpc_tls();
            let environment_blob = environment.seal("environment-secret").unwrap();
            let grpc_blob = grpc.seal("grpc-private-key").unwrap();

            assert_eq!(
                environment.unseal(&environment_blob).unwrap().as_str(),
                "environment-secret"
            );
            assert_eq!(
                grpc.unseal(&grpc_blob).unwrap().as_str(),
                "grpc-private-key"
            );
            assert!(grpc.unseal(&environment_blob).is_err());
            assert!(environment.unseal(&grpc_blob).is_err());
        }
    }
}

#[cfg_attr(target_os = "windows", allow(dead_code))]
struct UnsupportedSealer;

impl devbox_secrets::Sealer for UnsupportedSealer {
    fn seal(&self, _plaintext: &str) -> Result<Vec<u8>, SealError> {
        Err(SealError::CryptoFailure)
    }
    fn unseal(&self, _blob: &[u8]) -> Result<Zeroizing<String>, SealError> {
        Err(SealError::CryptoFailure)
    }
}
