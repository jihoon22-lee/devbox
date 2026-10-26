//! Bounded, one-shot provenance retained until the renderer finishes its all-target preflight.
use crate::{LspError, WorkspaceBaseline, WorkspaceIdentity, WorkspaceProof, WorkspaceValidation};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

pub(crate) fn text_hash(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}
pub(crate) struct Captured {
    pub client: u32,
    pub request: Value,
    pub open: BTreeMap<String, WorkspaceBaseline>,
    pub covered: BTreeMap<String, WorkspaceBaseline>,
    pub expires: Instant,
    pub targets: Option<Vec<WorkspaceBaseline>>,
    pub validated: bool,
    pub omitted: Vec<devbox_documents::CodeOmission>,
    pub stopped: Option<devbox_documents::CaptureReason>,
}
#[derive(Default)]
pub(crate) struct Proofs {
    next: u64,
    items: BTreeMap<String, Captured>,
}
impl Proofs {
    pub fn insert(&mut self, generation: u64, captured: Captured) -> String {
        self.next += 1;
        let token = format!("{generation}-{}", self.next);
        self.items.insert(token.clone(), captured);
        token
    }
    pub fn expire(&mut self) {
        self.items.retain(|_, p| p.expires > Instant::now());
    }
    pub fn detach(&mut self, client: u32) {
        self.items.retain(|_, p| p.client != client);
    }
    pub fn cancel(&mut self, client: u32, request: &Value) {
        self.items
            .retain(|_, p| p.client != client || &p.request != request);
    }
    pub fn pending(&mut self, client: u32) -> bool {
        self.expire();
        self.items.values().any(|p| p.client == client)
    }
    pub fn remove(&mut self, token: &str) {
        self.items.remove(token);
    }
    pub fn matches(
        &self,
        client: u32,
        root: &str,
        generation: u64,
        id: &WorkspaceIdentity,
    ) -> bool {
        id.root_uri == root
            && id.generation == generation.to_string()
            && self.items.get(&id.token).is_some_and(|p| {
                p.client == client
                    // Exact wire form, as minted in `response`; `Value == String` would compare
                    // only JSON string ids by content and is not the same check.
                    && serde_json::to_string(&p.request).is_ok_and(|wire| wire == id.request_id)
                    && p.expires > Instant::now()
            })
    }
    pub fn finish(&mut self, client: u32, root: &str, generation: u64, id: &WorkspaceIdentity) {
        if self.matches(client, root, generation, id) {
            self.remove(&id.token);
        }
    }
    pub fn response(
        &mut self,
        client: u32,
        request: &Value,
        root: &str,
        generation: u64,
        edit: &Value,
    ) -> Result<WorkspaceProof, LspError> {
        self.expire();
        let (token, captured) = self
            .items
            .iter_mut()
            .find(|(_, p)| p.client == client && &p.request == request)
            .ok_or(LspError::InvalidInput)?;
        if captured.targets.is_some() {
            return Err(LspError::InvalidInput);
        }
        let uris = targets(edit)?;
        let mut targets = Vec::new();
        for uri in uris {
            let baseline = captured
                .open
                .get(&uri)
                .or_else(|| captured.covered.get(&uri))
                .ok_or(LspError::LimitExceeded)?;
            targets.push(baseline.clone());
        }
        captured.targets = Some(targets.clone());
        Ok(WorkspaceProof {
            token: token.clone(),
            request_id: request.to_string(),
            root_uri: root.into(),
            generation: generation.to_string(),
            targets,
        })
    }
    pub fn validation(
        &self,
        client: u32,
        root: &str,
        generation: u64,
        value: &WorkspaceValidation,
    ) -> Result<&Captured, LspError> {
        if !self.matches(client, root, generation, &value.identity) {
            return Err(LspError::InvalidInput);
        }
        let captured = &self.items[&value.identity.token];
        let targets = captured.targets.as_ref().ok_or(LspError::InvalidInput)?;
        if captured.validated || value.targets.len() != targets.len() || value.targets.len() > 256 {
            return Err(LspError::InvalidInput);
        }
        for (current, expected) in value.targets.iter().zip(targets) {
            if current.uri != expected.uri
                || current.disk_rev != expected.disk_rev
                || current.text_hash != expected.text_hash
                || current.lease_id.is_none()
                || (expected.lease_id.is_some() && current.lease_id != expected.lease_id)
            {
                return Err(LspError::InvalidInput);
            }
        }
        Ok(captured)
    }
    pub fn validated(&mut self, token: &str) {
        if let Some(p) = self.items.get_mut(token) {
            p.validated = true;
        }
    }
}
pub(crate) fn captured(
    client: u32,
    request: Value,
    open: BTreeMap<String, WorkspaceBaseline>,
    covered: BTreeMap<String, WorkspaceBaseline>,
) -> Captured {
    Captured {
        client,
        request,
        open,
        covered,
        expires: Instant::now() + Duration::from_secs(30),
        targets: None,
        validated: false,
        omitted: vec![],
        stopped: None,
    }
}
pub(crate) fn targets(edit: &Value) -> Result<Vec<String>, LspError> {
    let mut uris = if edit.is_null() {
        Vec::new()
    } else if let Some(changes) = edit.get("changes").and_then(Value::as_object) {
        if edit.get("documentChanges").is_some() {
            return Err(LspError::InvalidInput);
        }
        changes.keys().cloned().collect::<Vec<_>>()
    } else if let Some(changes) = edit.get("documentChanges").and_then(Value::as_array) {
        changes
            .iter()
            .map(|row| {
                row.pointer("/textDocument/uri")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .ok_or(LspError::InvalidInput)
            })
            .collect::<Result<Vec<_>, _>>()?
    } else {
        return Err(LspError::InvalidInput);
    };
    if uris.len() > 256 {
        return Err(LspError::LimitExceeded);
    }
    uris.sort();
    if uris.windows(2).any(|w| w[0] == w[1]) {
        return Err(LspError::InvalidInput);
    }
    Ok(uris)
}
pub(crate) fn refused(id: Value) -> String {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":-32801,"message":"Workspace provenance unavailable; retry with a smaller scope"}}).to_string()
}

#[cfg(test)]
mod tests;
