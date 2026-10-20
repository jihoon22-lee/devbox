use serde::{Deserialize, Serialize};
use zeroize::Zeroize;
devbox_protocol::domain_error! {pub enum SecretsError {
 Locked = "secrets_locked",
 KeyMismatch = "secrets.key_mismatch",
 InvalidInput = "secrets.invalid_input",
 Missing { names:Vec<String> } = "secrets.missing",
 Unavailable = "secrets.unavailable",
}}
#[derive(Serialize, Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
pub struct Empty {}
#[derive(Serialize, Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
pub struct Name {
    pub name: String,
}
#[derive(Serialize, Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
pub struct SetSecret {
    pub name: String,
    pub value: String,
}
impl Drop for SetSecret {
    fn drop(&mut self) {
        self.value.zeroize();
    }
}
#[derive(Serialize, Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
pub struct Recovery {
    pub recovery: String,
}
impl Drop for Recovery {
    fn drop(&mut self) {
        self.recovery.zeroize();
    }
}
#[derive(Debug, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct SecretInfo {
    pub name: String,
    pub has_value: bool,
}
pub fn valid_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    name.len() <= 128
        && bytes
            .next()
            .is_some_and(|b| b == b'_' || b.is_ascii_alphabetic())
        && bytes.all(|b| b == b'_' || b.is_ascii_alphanumeric())
}
