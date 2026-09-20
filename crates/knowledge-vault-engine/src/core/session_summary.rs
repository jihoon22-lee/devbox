//! The native Workspace producer and Knowledge receiver use the same bounded contract.
pub use product_contract::session_summary::*;

#[cfg(test)]
pub(crate) fn fixture() -> Metadata {
    serde_json::from_str(include_str!("../../tests/fixtures/session-summary-v1.json")).unwrap()
}
