//! CurrentUser DPAPI for the application's native-only data-key handshake.
//! No key material or OS diagnostic is included in errors.
#[cfg(windows)]
pub use native::{generate_key, seal, unseal};

#[derive(Debug, Clone, Copy)]
pub struct DpapiError;
impl std::fmt::Display for DpapiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("data key remains locked")
    }
}
impl std::error::Error for DpapiError {}

#[cfg(windows)]
mod native {
    use super::DpapiError;
    use windows::Win32::Foundation::{HLOCAL, LocalFree};
    use windows::Win32::Security::Cryptography::{
        BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom, CRYPT_INTEGER_BLOB,
        CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
    };
    use windows::core::PCWSTR;
    use zeroize::{Zeroize, Zeroizing};

    const KEY_BYTES: usize = 32;
    const MAX_BLOB_BYTES: usize = 64 * 1024;

    #[derive(Default)]
    struct Output(CRYPT_INTEGER_BLOB);
    impl Output {
        fn copy(&self) -> Result<Zeroizing<Vec<u8>>, DpapiError> {
            if self.0.pbData.is_null() || self.0.cbData as usize > MAX_BLOB_BYTES {
                return Err(DpapiError);
            }
            // SAFETY: successful DPAPI calls return cbData initialized bytes owned
            // by this guard; the guard remains alive until after the copy.
            let bytes =
                unsafe { std::slice::from_raw_parts(self.0.pbData, self.0.cbData as usize) };
            Ok(Zeroizing::new(bytes.to_vec()))
        }
    }
    impl Drop for Output {
        fn drop(&mut self) {
            if !self.0.pbData.is_null() {
                // SAFETY: this guard exclusively owns the buffer allocated by
                // DPAPI. Wipe before freeing with the documented LocalFree API.
                unsafe {
                    std::slice::from_raw_parts_mut(self.0.pbData, self.0.cbData as usize).zeroize();
                    let _ = LocalFree(Some(HLOCAL(self.0.pbData.cast())));
                }
            }
        }
    }

    pub fn generate_key() -> Result<Zeroizing<Vec<u8>>, DpapiError> {
        let mut key = Zeroizing::new(vec![0; KEY_BYTES]);
        // SAFETY: the output slice is valid and writable; null algorithm with
        // SYSTEM_PREFERRED_RNG selects the operating system's cryptographic RNG.
        unsafe { BCryptGenRandom(None, key.as_mut_slice(), BCRYPT_USE_SYSTEM_PREFERRED_RNG) }
            .ok()
            .map_err(|_| DpapiError)?;
        Ok(key)
    }

    pub fn seal(key: &[u8]) -> Result<Vec<u8>, DpapiError> {
        if key.len() != KEY_BYTES {
            return Err(DpapiError);
        }
        let input = CRYPT_INTEGER_BLOB {
            cbData: KEY_BYTES as u32,
            pbData: key.as_ptr().cast_mut(),
        };
        let mut output = Output::default();
        // SAFETY: DPAPI treats input bytes as read-only. Input/output remain
        // valid for the synchronous call. No LOCAL_MACHINE flag: CurrentUser.
        unsafe {
            CryptProtectData(
                &input,
                PCWSTR::null(),
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output.0,
            )
        }
        .map_err(|_| DpapiError)?;
        let blob = output.copy()?;
        if blob.is_empty() {
            return Err(DpapiError);
        }
        Ok(blob.to_vec())
    }

    pub fn unseal(blob: &[u8]) -> Result<Zeroizing<Vec<u8>>, DpapiError> {
        if blob.is_empty() || blob.len() > MAX_BLOB_BYTES {
            return Err(DpapiError);
        }
        let input = CRYPT_INTEGER_BLOB {
            cbData: blob.len() as u32,
            pbData: blob.as_ptr().cast_mut(),
        };
        let mut output = Output::default();
        // SAFETY: input is read-only and bounded; output is owned by its guard.
        // Optional entropy matches the daemon's CurrentUser DPAPI contract.
        unsafe {
            CryptUnprotectData(
                &input,
                None,
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output.0,
            )
        }
        .map_err(|_| DpapiError)?;
        if output.0.cbData as usize != KEY_BYTES {
            return Err(DpapiError);
        }
        output.copy()
    }
}
