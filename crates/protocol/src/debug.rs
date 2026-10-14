//! Test-instance stream contract, exported for browser and CLI test clients.
use crate::{Exporter, StreamOpened, domain_error, method};
#[derive(serde::Deserialize, serde::Serialize, ts_rs::TS)]
pub struct EchoParams {
    pub bytes: u32,
}
domain_error! { pub enum DebugError { Unsupported = "debug.unsupported" } }
method!(pub DebugEcho = "debug.echo", stream, EchoParams => StreamOpened, DebugError);
pub fn export<X: Exporter>(x: &mut X) {
    x.method::<DebugEcho>();
}
