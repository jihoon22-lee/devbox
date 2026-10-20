//! Native-only key exchange. Never exported to renderer method declarations.
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;
pub const NATIVE_CHANNEL: u32 = 0;
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KeyStatus {
    pub locked: bool,
    #[serde(default)]
    pub key_mismatch: bool,
    pub key_id: Option<String>,
    pub blob: Option<Vec<u8>>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Bootstrap {
    pub blob: Vec<u8>,
    pub key_id: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Unlock {
    pub key: Vec<u8>,
}
impl Drop for Unlock {
    fn drop(&mut self) {
        self.key.zeroize();
    }
}
