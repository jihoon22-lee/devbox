//! Platform boundary for protected job environments.
//!
//! The command layer is the only place that receives plaintext values. It
//! turns them into [`SecretEnvironment`], encrypts them through this module,
//! and drops the wrapper before storage is called. Reads expose only the
//! configured flag; execution code must explicitly ask this boundary for a
//! short-lived, zeroizing plaintext view.

use crate::core::models::{EnvironmentProtectionError, EnvironmentProtector};
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;
use zeroize::Zeroize;

#[derive(Clone)]
pub struct EnvironmentProtectorState {
    protector: Arc<dyn EnvironmentProtector>,
}

impl EnvironmentProtectorState {
    pub fn new() -> Self {
        Self {
            protector: Arc::new(platform_protector()),
        }
    }

    #[cfg(test)]
    fn from_protector(protector: Arc<dyn EnvironmentProtector>) -> Self {
        Self { protector }
    }

    pub fn is_available(&self) -> bool {
        self.protector.is_available()
    }

    /// Encrypt a borrowed map for callers that already own a zeroizing
    /// wrapper. Commands should prefer [`Self::encrypt_owned`] so the input is
    /// consumed at this boundary.
    pub fn encrypt_for_storage(
        &self,
        environment: &BTreeMap<String, String>,
    ) -> Result<Vec<u8>, EnvironmentProtectionError> {
        crate::core::shell::validate_environment(environment)
            .map_err(|_| EnvironmentProtectionError::InvalidInput)?;
        self.protector.encrypt(environment)
    }

    /// Consume plaintext ownership before returning ciphertext. The
    /// `SecretEnvironment` destructor zeroizes both keys and values even when
    /// encryption fails.
    pub fn encrypt_owned(
        &self,
        environment: SecretEnvironment,
    ) -> Result<Vec<u8>, EnvironmentProtectionError> {
        let result = self.encrypt_for_storage(environment.as_map());
        drop(environment);
        result
    }

    /// Decrypt only at the execution boundary. The returned wrapper must stay
    /// in scope while an adapter consumes `as_map()` and zeroizes on drop.
    pub fn decrypt_for_execution(
        &self,
        ciphertext: &[u8],
    ) -> Result<SecretEnvironment, EnvironmentProtectionError> {
        let environment = SecretEnvironment::new(self.protector.decrypt(ciphertext)?);
        crate::core::shell::validate_environment(environment.as_map())
            .map_err(|_| EnvironmentProtectionError::InvalidCiphertext)?;
        Ok(environment)
    }

    /// The no-environment case is represented by an empty zeroizing wrapper,
    /// while a present ciphertext always has to pass through the protector.
    pub fn decrypt_optional_for_execution(
        &self,
        ciphertext: Option<&[u8]>,
    ) -> Result<SecretEnvironment, EnvironmentProtectionError> {
        ciphertext.map_or_else(
            || Ok(SecretEnvironment::default()),
            |ciphertext| self.decrypt_for_execution(ciphertext),
        )
    }
}

impl Default for EnvironmentProtectorState {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for EnvironmentProtectorState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EnvironmentProtectorState")
            .field("available", &self.is_available())
            .finish()
    }
}

/// Owned plaintext used only between decrypt and process creation. This type
/// intentionally has no `Serialize` implementation and its `Debug` output
/// never includes keys or values.
pub struct SecretEnvironment {
    values: BTreeMap<String, String>,
}

impl SecretEnvironment {
    pub fn new(values: BTreeMap<String, String>) -> Self {
        Self { values }
    }

    pub fn as_map(&self) -> &BTreeMap<String, String> {
        &self.values
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

impl Default for SecretEnvironment {
    fn default() -> Self {
        Self::new(BTreeMap::new())
    }
}

impl fmt::Debug for SecretEnvironment {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretEnvironment(<redacted>)")
    }
}

impl Drop for SecretEnvironment {
    fn drop(&mut self) {
        // BTreeMap does not expose mutable keys. Taking ownership lets us
        // zeroize both strings before their allocations are released.
        let values = std::mem::take(&mut self.values);
        for (mut key, mut value) in values {
            key.zeroize();
            value.zeroize();
        }
    }
}

#[cfg(windows)]
fn platform_protector() -> windows_impl::DpapiProtector {
    windows_impl::DpapiProtector
}

#[cfg(not(windows))]
fn platform_protector() -> unavailable::UnavailableProtector {
    unavailable::UnavailableProtector
}

#[cfg(windows)]
mod windows_impl {
    use super::*;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };
    use zeroize::Zeroizing;

    /// This value is application-fixed, not a user secret. It prevents other
    /// DPAPI consumers from accidentally unprotecting this app's blobs while
    /// DPAPI still scopes the key to the current Windows user.
    const OPTIONAL_ENTROPY: &[u8] = b"devbox.run-manager.environment.v1";

    pub(super) struct DpapiProtector;

    impl EnvironmentProtector for DpapiProtector {
        fn encrypt(
            &self,
            environment: &BTreeMap<String, String>,
        ) -> Result<Vec<u8>, EnvironmentProtectionError> {
            crate::core::shell::validate_environment(environment)
                .map_err(|_| EnvironmentProtectionError::InvalidInput)?;
            let serialized = Zeroizing::new(
                serde_json::to_vec(environment)
                    .map_err(|_| EnvironmentProtectionError::CryptoFailure)?,
            );
            protect_blob(&serialized)
        }

        fn decrypt(
            &self,
            ciphertext: &[u8],
        ) -> Result<BTreeMap<String, String>, EnvironmentProtectionError> {
            if ciphertext.is_empty() {
                return Err(EnvironmentProtectionError::InvalidCiphertext);
            }
            let serialized = unprotect_blob(ciphertext)?;
            let environment = serde_json::from_slice(&serialized)
                .map_err(|_| EnvironmentProtectionError::InvalidCiphertext)?;
            if crate::core::shell::validate_environment(&environment).is_err() {
                drop(SecretEnvironment::new(environment));
                return Err(EnvironmentProtectionError::InvalidCiphertext);
            }
            Ok(environment)
        }
    }

    fn protect_blob(bytes: &[u8]) -> Result<Vec<u8>, EnvironmentProtectionError> {
        let input = blob(bytes)?;
        let entropy = blob(OPTIONAL_ENTROPY)?;
        let mut output = CRYPT_INTEGER_BLOB::default();
        unsafe {
            CryptProtectData(
                &input,
                PCWSTR::null(),
                Some(&entropy as *const _),
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
        .map_err(|_| EnvironmentProtectionError::CryptoFailure)?;
        copy_and_free(output)
    }

    fn unprotect_blob(bytes: &[u8]) -> Result<Zeroizing<Vec<u8>>, EnvironmentProtectionError> {
        let input = blob(bytes)?;
        let entropy = blob(OPTIONAL_ENTROPY)?;
        let mut output = CRYPT_INTEGER_BLOB::default();
        unsafe {
            CryptUnprotectData(
                &input,
                None,
                Some(&entropy as *const _),
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
        .map_err(|_| EnvironmentProtectionError::InvalidCiphertext)?;
        copy_and_zeroize_free(output)
    }

    fn blob(bytes: &[u8]) -> Result<CRYPT_INTEGER_BLOB, EnvironmentProtectionError> {
        let cb_data =
            u32::try_from(bytes.len()).map_err(|_| EnvironmentProtectionError::InvalidInput)?;
        Ok(CRYPT_INTEGER_BLOB {
            cbData: cb_data,
            pbData: bytes.as_ptr() as *mut u8,
        })
    }

    fn copy_and_free(blob: CRYPT_INTEGER_BLOB) -> Result<Vec<u8>, EnvironmentProtectionError> {
        if blob.pbData.is_null() {
            return Err(EnvironmentProtectionError::CryptoFailure);
        }
        let copied =
            unsafe { std::slice::from_raw_parts(blob.pbData, blob.cbData as usize).to_vec() };
        unsafe {
            let _ = LocalFree(Some(HLOCAL(blob.pbData.cast())));
        }
        if copied.is_empty() {
            Err(EnvironmentProtectionError::CryptoFailure)
        } else {
            Ok(copied)
        }
    }

    /// Copy a DPAPI plaintext result while it is still in the LocalAlloc
    /// buffer, then scrub that buffer before releasing it. `CryptUnprotectData`
    /// owns this allocation and the caller must not leave plaintext behind in
    /// it; the returned `Zeroizing` value covers the Rust-owned copy.
    fn copy_and_zeroize_free(
        blob: CRYPT_INTEGER_BLOB,
    ) -> Result<Zeroizing<Vec<u8>>, EnvironmentProtectionError> {
        if blob.pbData.is_null() {
            return Err(EnvironmentProtectionError::InvalidCiphertext);
        }
        let length = blob.cbData as usize;
        let mut copied = unsafe { std::slice::from_raw_parts(blob.pbData, length).to_vec() };
        unsafe {
            std::slice::from_raw_parts_mut(blob.pbData, length).zeroize();
            let _ = LocalFree(Some(HLOCAL(blob.pbData.cast())));
        }
        if copied.is_empty() {
            copied.zeroize();
            Err(EnvironmentProtectionError::InvalidCiphertext)
        } else {
            Ok(Zeroizing::new(copied))
        }
    }
}

#[cfg(not(windows))]
mod unavailable {
    use super::*;

    pub(super) struct UnavailableProtector;

    impl EnvironmentProtector for UnavailableProtector {
        fn encrypt(
            &self,
            _environment: &BTreeMap<String, String>,
        ) -> Result<Vec<u8>, EnvironmentProtectionError> {
            Err(EnvironmentProtectionError::Unavailable)
        }

        fn decrypt(
            &self,
            _ciphertext: &[u8],
        ) -> Result<BTreeMap<String, String>, EnvironmentProtectionError> {
            Err(EnvironmentProtectionError::Unavailable)
        }

        fn is_available(&self) -> bool {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::models::EnvironmentProtectionError;
    use std::sync::Arc;

    struct MockProtector;

    impl EnvironmentProtector for MockProtector {
        fn encrypt(
            &self,
            environment: &BTreeMap<String, String>,
        ) -> Result<Vec<u8>, EnvironmentProtectionError> {
            serde_json::to_vec(environment).map_err(|_| EnvironmentProtectionError::CryptoFailure)
        }

        fn decrypt(
            &self,
            ciphertext: &[u8],
        ) -> Result<BTreeMap<String, String>, EnvironmentProtectionError> {
            serde_json::from_slice(ciphertext)
                .map_err(|_| EnvironmentProtectionError::InvalidCiphertext)
        }
    }

    #[test]
    fn mock_protector_round_trips_only_at_explicit_boundaries() {
        let state = EnvironmentProtectorState::from_protector(Arc::new(MockProtector));
        let mut values = BTreeMap::new();
        values.insert("TOKEN".to_owned(), "test-only-value".to_owned());
        let ciphertext = state.encrypt_owned(SecretEnvironment::new(values)).unwrap();
        let plaintext = state.decrypt_for_execution(&ciphertext).unwrap();
        assert_eq!(
            plaintext.as_map().get("TOKEN"),
            Some(&"test-only-value".to_owned())
        );
        assert!(!format!("{plaintext:?}").contains("test-only-value"));
    }

    #[test]
    fn optional_decrypt_has_an_empty_zeroizing_scope_without_ciphertext() {
        let state = EnvironmentProtectorState::from_protector(Arc::new(MockProtector));
        let plaintext = state.decrypt_optional_for_execution(None).unwrap();
        assert!(plaintext.is_empty());
    }

    #[cfg(not(windows))]
    #[test]
    fn non_windows_protector_is_explicitly_unavailable() {
        let state = EnvironmentProtectorState::new();
        assert!(!state.is_available());
        let values = BTreeMap::from([(String::from("TOKEN"), String::from("value"))]);
        assert_eq!(
            state.encrypt_for_storage(&values),
            Err(EnvironmentProtectionError::Unavailable)
        );
    }

    #[cfg(windows)]
    #[test]
    fn dpapi_current_user_round_trip_uses_the_platform_boundary() {
        let state = EnvironmentProtectorState::new();
        assert!(state.is_available());
        let values = BTreeMap::from([(String::from("TOKEN"), String::from("value"))]);
        let ciphertext = state
            .encrypt_owned(SecretEnvironment::new(values))
            .expect("DPAPI should be available for the current user");
        let plaintext = state.decrypt_for_execution(&ciphertext).unwrap();
        assert!(plaintext
            .as_map()
            .get("TOKEN")
            .is_some_and(|value| value == "value"));
    }
}
