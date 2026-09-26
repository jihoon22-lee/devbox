pub mod core;
pub mod host;
pub mod platform;
pub mod private_metadata;
pub mod project_owner;
pub use host::Host;
pub use platform::{storage_paths, task_sources};
pub use project_owner::ProjectOwner;
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};
type Result<T> = std::result::Result<T, &'static str>;
pub fn current_deadline(deadline: u64) -> Result<()> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "request_expired")?
        .as_millis();
    if now >= u128::from(deadline) {
        Err("request_expired")
    } else {
        Ok(())
    }
}
pub fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub mod lanes;
pub mod runtime_policy;

pub mod runtime_logs;
