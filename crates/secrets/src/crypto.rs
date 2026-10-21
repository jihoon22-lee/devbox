use crate::SecretsError;
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, OsRng, Payload, rand_core::RngCore},
};
use sha2::{Digest, Sha256};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct Key([u8; 32]);
impl Key {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
    pub fn random() -> Self {
        let mut bytes = [0; 32];
        OsRng.fill_bytes(&mut bytes);
        Self(bytes)
    }
    pub fn bytes(&self) -> &[u8; 32] {
        &self.0
    }
    pub fn id(&self) -> String {
        Sha256::digest(self.0)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }
    pub fn recovery(&self) -> Zeroizing<String> {
        let text = Zeroizing::new(data_encoding::BASE32_NOPAD.encode(&self.0));
        Zeroizing::new(
            text.as_bytes()
                .chunks(4)
                .map(|c| std::str::from_utf8(c).expect("base32"))
                .collect::<Vec<_>>()
                .join("-"),
        )
    }
    pub fn recover(text: &str) -> Result<Self, SecretsError> {
        let compact = Zeroizing::new(
            text.chars()
                .filter(|c| *c != '-' && !c.is_ascii_whitespace())
                .collect::<String>(),
        );
        let bytes = Zeroizing::new(
            data_encoding::BASE32_NOPAD
                .decode(compact.as_bytes())
                .map_err(|_| SecretsError::InvalidInput)?,
        );
        Ok(Self(
            bytes
                .as_slice()
                .try_into()
                .map_err(|_| SecretsError::InvalidInput)?,
        ))
    }
}
fn aad(secret: &str, key_id: &str) -> Vec<u8> {
    format!("{secret}|{key_id}").into_bytes()
}
pub fn seal(key: &Key, secret: &str, key_id: &str, plain: &[u8]) -> Result<Vec<u8>, SecretsError> {
    let mut nonce = [0; 24];
    OsRng.fill_bytes(&mut nonce);
    let mut sealed = nonce.to_vec();
    sealed.extend(
        XChaCha20Poly1305::new(key.bytes().into())
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: plain,
                    aad: &aad(secret, key_id),
                },
            )
            .map_err(|_| SecretsError::KeyMismatch)?,
    );
    Ok(sealed)
}
pub fn open(
    key: &Key,
    secret: &str,
    key_id: &str,
    sealed: &[u8],
) -> Result<Zeroizing<Vec<u8>>, SecretsError> {
    if sealed.len() < 40 {
        return Err(SecretsError::KeyMismatch);
    }
    XChaCha20Poly1305::new(key.bytes().into())
        .decrypt(
            XNonce::from_slice(&sealed[..24]),
            Payload {
                msg: &sealed[24..],
                aad: &aad(secret, key_id),
            },
        )
        .map(Zeroizing::new)
        .map_err(|_| SecretsError::KeyMismatch)
}
