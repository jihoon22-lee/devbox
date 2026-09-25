//! Current-user DPAPI with per-purpose entropy and no interactive UI.
//! Provider buffers and failed UTF-8 copies are scrubbed before release.
use crate::{SealError, Sealer};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{LocalFree, HLOCAL};
use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
};
use zeroize::{Zeroize, Zeroizing};

pub struct DpapiSealer {
    entropy: &'static [u8],
}

impl DpapiSealer {
    pub fn new(entropy: &'static [u8]) -> Self {
        Self { entropy }
    }
}

// Own the allocation even when the provider reports a failure after touching
// the output. A zero-length allocation is still released when it is non-null.
#[derive(Default)]
struct OutputBlob(CRYPT_INTEGER_BLOB);

impl OutputBlob {
    fn copy(&self, allow_empty: bool) -> Result<Zeroizing<Vec<u8>>, SealError> {
        if self.0.cbData == 0 {
            return if allow_empty {
                Ok(Zeroizing::new(Vec::new()))
            } else {
                Err(SealError::CryptoFailure)
            };
        }
        if self.0.pbData.is_null() {
            return Err(SealError::CryptoFailure);
        }
        Ok(Zeroizing::new(unsafe {
            std::slice::from_raw_parts(self.0.pbData, self.0.cbData as usize).to_vec()
        }))
    }
}

impl Drop for OutputBlob {
    fn drop(&mut self) {
        if !self.0.pbData.is_null() {
            unsafe {
                std::slice::from_raw_parts_mut(self.0.pbData, self.0.cbData as usize).zeroize();
                let _ = LocalFree(Some(HLOCAL(self.0.pbData.cast())));
            }
        }
    }
}

impl Sealer for DpapiSealer {
    fn seal(&self, plaintext: &str) -> Result<Vec<u8>, SealError> {
        let input = blob(plaintext.as_bytes())?;
        let entropy = blob(self.entropy)?;
        let mut output = OutputBlob::default();
        unsafe {
            CryptProtectData(
                &input,
                PCWSTR::null(),
                Some(&entropy),
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output.0,
            )
        }
        .map_err(|_| SealError::CryptoFailure)?;
        let mut bytes = output.copy(false)?;
        Ok(std::mem::take(&mut *bytes))
    }

    fn unseal(&self, ciphertext: &[u8]) -> Result<Zeroizing<String>, SealError> {
        if ciphertext.is_empty() {
            return Err(SealError::InvalidInput);
        }
        let input = blob(ciphertext)?;
        let entropy = blob(self.entropy)?;
        let mut output = OutputBlob::default();
        unsafe {
            CryptUnprotectData(
                &input,
                None,
                Some(&entropy),
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output.0,
            )
        }
        .map_err(|_| SealError::CryptoFailure)?;
        let mut bytes = output.copy(true)?;
        match String::from_utf8(std::mem::take(&mut *bytes)) {
            Ok(text) => Ok(Zeroizing::new(text)),
            Err(error) => {
                let mut invalid = error.into_bytes();
                invalid.zeroize();
                Err(SealError::InvalidInput)
            }
        }
    }
}

fn blob(bytes: &[u8]) -> Result<CRYPT_INTEGER_BLOB, SealError> {
    Ok(CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(bytes.len()).map_err(|_| SealError::InvalidInput)?,
        pbData: bytes.as_ptr() as *mut u8,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{seal_v1, unseal_v1};
    const A: &[u8] = b"devbox.test.a.v1";
    const B: &[u8] = b"devbox.test.b.v1";

    #[test]
    fn values_round_trip_including_empty_and_unicode() {
        let sealer = DpapiSealer::new(A);
        for value in ["", "value", "비밀 값 🔐"] {
            let blob = seal_v1(&sealer, value).unwrap();
            assert_eq!(unseal_v1(&sealer, &blob).unwrap().as_str(), value);
        }
    }

    #[test]
    fn another_purpose_cannot_unseal() {
        let blob = seal_v1(&DpapiSealer::new(A), "value").unwrap();
        assert!(matches!(
            unseal_v1(&DpapiSealer::new(B), &blob),
            Err(SealError::CryptoFailure)
        ));
    }

    #[test]
    fn tampered_blobs_fail_closed() {
        let sealer = DpapiSealer::new(A);
        let mut blob = seal_v1(&sealer, "value").unwrap();
        *blob.last_mut().unwrap() ^= 0xff;
        assert!(matches!(
            unseal_v1(&sealer, &blob),
            Err(SealError::CryptoFailure)
        ));
        assert!(matches!(
            sealer.unseal(&[]),
            Err(SealError::InvalidInput) | Err(SealError::CryptoFailure)
        ));
    }
}
