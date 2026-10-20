use crate::core::secrets::{KeyProtector, Locked};
use zeroize::Zeroizing;

pub struct CurrentUser;
impl KeyProtector for CurrentUser {
    fn generate(&self) -> Result<Zeroizing<Vec<u8>>, Locked> {
        devbox_win::dpapi::generate_key().map_err(|_| Locked)
    }
    fn seal(&self, key: &[u8]) -> Result<Vec<u8>, Locked> {
        devbox_win::dpapi::seal(key).map_err(|_| Locked)
    }
    fn unseal(&self, blob: &[u8]) -> Result<Zeroizing<Vec<u8>>, Locked> {
        devbox_win::dpapi::unseal(blob).map_err(|_| Locked)
    }
}
