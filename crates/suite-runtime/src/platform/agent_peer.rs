//! Agent-only peer verification; Suite product authorization remains separate.
use super::{
    component_scope::CapturedScope,
    peer_identity::{PipeWitness, ProcessPeer},
};
use std::sync::Arc;
use windows::Win32::Foundation::HANDLE;

/// Owns the duplicated pipe until the blocking native identity query completes.
pub struct Witness(PipeWitness);
impl Witness {
    pub fn capture(pipe: HANDLE) -> Result<Self, &'static str> {
        PipeWitness::capture(pipe).map(Self)
    }
}
pub struct AgentPeer(ProcessPeer);
impl AgentPeer {
    pub fn product(scope: Arc<CapturedScope>, witness: Witness) -> Result<Self, &'static str> {
        ProcessPeer::from_pipe(scope, witness.0, true).map(Self)
    }
    pub fn agent(scope: Arc<CapturedScope>, witness: Witness) -> Result<Self, &'static str> {
        ProcessPeer::from_agent_pipe(scope, witness.0).map(Self)
    }
    pub fn product_id(&self) -> &str {
        &self.0.product
    }
    pub fn installation_id(&self) -> Result<String, &'static str> {
        use sha2::{Digest, Sha256};
        let (_, image, _) = self.0.scope().member(self.product_id())?;
        let image = image.canonicalize().map_err(|_| "peer_image_unavailable")?;
        Ok(Sha256::digest(image.to_string_lossy().as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect())
    }
    pub fn revalidate(&self) -> Result<(), &'static str> {
        self.0.revalidate()
    }
}
