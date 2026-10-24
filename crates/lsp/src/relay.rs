//! A generation-scoped JSON-RPC relay. Dynamic JSON never crosses an RPC type boundary.
use crate::{LspError, RelayMessage, adapters::DocumentAccess, relay_documents::Documents};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
const MAX_PENDING: usize = 512;
#[derive(Default, Debug)]
pub struct RelayOutput {
    pub server: Vec<String>,
    pub clients: Vec<(u32, String)>,
    pub close: Vec<u32>,
    pub diagnostics: Vec<crate::diagnostics::Diagnostics>,
}
struct Pending {
    client: u32,
    id: Value,
}
pub struct Relay {
    root: String,
    generation: u64,
    next: u64,
    members: BTreeSet<u32>,
    initialized: BTreeSet<u32>,
    init_id: Option<u64>,
    waiting: Vec<Pending>,
    cache: Option<Value>,
    requests: BTreeMap<u64, Pending>,
    server_requests: BTreeMap<String, Pending>,
    documents: Documents,
}
impl Relay {
    pub fn new(root_uri: String, generation: u64) -> Self {
        Self {
            root: root_uri,
            generation,
            next: 0,
            members: BTreeSet::new(),
            initialized: BTreeSet::new(),
            init_id: None,
            waiting: vec![],
            cache: None,
            requests: BTreeMap::new(),
            server_requests: BTreeMap::new(),
            documents: Documents::default(),
        }
    }
    pub fn attach(&mut self, client: u32) -> Result<(), LspError> {
        if self.members.len() >= 16 || !self.members.insert(client) {
            return Err(LspError::LimitExceeded);
        }
        Ok(())
    }
    fn id(&mut self) -> Result<u64, LspError> {
        self.next = self.next.checked_add(1).ok_or(LspError::LimitExceeded)?;
        Ok(self.next)
    }
    pub async fn client(
        &mut self,
        client: u32,
        message: RelayMessage,
        access: &DocumentAccess,
    ) -> Result<RelayOutput, LspError> {
        if !self.members.contains(&client) {
            return Err(LspError::Protocol);
        }
        let mut value = parse(&message.json)?;
        let mut out = RelayOutput::default();
        let Some(method) = value
            .get("method")
            .and_then(Value::as_str)
            .map(str::to_owned)
        else {
            let key = value
                .get("id")
                .and_then(Value::as_str)
                .ok_or(LspError::Protocol)?
                .to_owned();
            let pending = self.server_requests.get(&key).ok_or(LspError::Protocol)?;
            if pending.client != client {
                return Err(LspError::Protocol);
            }
            crate::uri::normalize(&mut value, access, 0).await?;
            let pending = self
                .server_requests
                .remove(&key)
                .expect("validated response");
            value["id"] = pending.id;
            out.server.push(value.to_string());
            return Ok(out);
        };
        if method == "initialize" {
            return self.initialize(client, value);
        }
        if method == "initialized" {
            return Ok(out);
        }
        if !self.initialized.contains(&client) {
            return Err(LspError::Protocol);
        }
        if method == "shutdown" {
            let id = request_id(&value)?;
            out.clients.push((
                client,
                json!({"jsonrpc":"2.0","id":id,"result":null}).to_string(),
            ));
            return Ok(out);
        }
        if method == "exit" {
            out.close.push(client);
            return Ok(out);
        }
        if method == "$/cancelRequest" {
            let id = value.pointer("/params/id").ok_or(LspError::InvalidInput)?;
            if let Some(global) = self
                .requests
                .iter()
                .find(|(_, p)| p.client == client && &p.id == id)
                .map(|(id, _)| *id)
            {
                self.requests.remove(&global);
                out.server.push(
                    json!({"jsonrpc":"2.0","method":method,"params":{"id":global}}).to_string(),
                );
            }
            return Ok(out);
        }
        // The project workspace is fixed by the daemon, never by a secondary renderer.
        if method == "workspace/didChangeWorkspaceFolders" {
            return Err(LspError::OutsideProject);
        }
        if let Some(uri) = value
            .pointer("/params/textDocument/uri")
            .and_then(Value::as_str)
        {
            let grant = access(uri.into(), message.lease_id.clone()).await?;
            value["params"]["textDocument"]["uri"] = Value::String(grant.uri.clone());
            if [
                "textDocument/didOpen",
                "textDocument/didChange",
                "textDocument/didClose",
                "textDocument/didSave",
            ]
            .contains(&method.as_str())
            {
                out.server = self
                    .documents
                    .notification(client, &method, &value, grant)?;
                return Ok(out);
            }
        }
        crate::uri::normalize(&mut value, access, 0).await?;
        if value.get("id").is_some() {
            let original = request_id(&value)?;
            if self.requests.len() >= MAX_PENDING
                || self
                    .requests
                    .values()
                    .any(|p| p.client == client && p.id == original)
            {
                return Err(LspError::LimitExceeded);
            }
            let global = self.id()?;
            self.requests.insert(
                global,
                Pending {
                    client,
                    id: original,
                },
            );
            value["id"] = global.into();
        }
        out.server.push(value.to_string());
        Ok(out)
    }
    fn initialize(&mut self, client: u32, mut value: Value) -> Result<RelayOutput, LspError> {
        let id = request_id(&value)?;
        let mut out = RelayOutput::default();
        if self.waiting.iter().any(|p| p.client == client) {
            return Err(LspError::Protocol);
        }
        if let Some(result) = &self.cache {
            self.initialized.insert(client);
            out.clients.push((
                client,
                json!({"jsonrpc":"2.0","id":id,"result":result}).to_string(),
            ));
            return Ok(out);
        }
        self.waiting.push(Pending { client, id });
        if self.init_id.is_some() {
            return Ok(out);
        }
        let global = self.id()?;
        self.init_id = Some(global);
        value["id"] = global.into();
        if !value.get("params").is_some_and(Value::is_object) {
            value["params"] = json!({});
        }
        let params = &mut value["params"];
        params["rootUri"] = self.root.clone().into();
        params["rootPath"] = Value::Null;
        params["processId"] = Value::Null;
        params["workspaceFolders"] = json!([{"uri":self.root,"name":"project"}]);
        if !params.get("capabilities").is_some_and(Value::is_object) {
            params["capabilities"] = json!({});
        }
        if !params["capabilities"]
            .get("general")
            .is_some_and(Value::is_object)
        {
            params["capabilities"]["general"] = json!({});
        }
        params
            .as_object_mut()
            .expect("params")
            .remove("offsetEncoding");
        params["capabilities"]
            .as_object_mut()
            .expect("capabilities")
            .remove("offsetEncoding");
        params["capabilities"]["general"]["positionEncodings"] = json!(["utf-16"]);
        out.server.push(value.to_string());
        Ok(out)
    }
    pub async fn server(
        &mut self,
        json: &str,
        access: &DocumentAccess,
    ) -> Result<RelayOutput, LspError> {
        let mut value = parse(json)?;
        let mut out = RelayOutput::default();
        if value.get("method").is_none() {
            let global = value
                .get("id")
                .and_then(Value::as_u64)
                .ok_or(LspError::Protocol)?;
            if self.init_id == Some(global) {
                self.init_id = None;
                let result = value
                    .get("result")
                    .filter(|v| v.is_object())
                    .ok_or(LspError::Protocol)?;
                if result
                    .pointer("/capabilities/positionEncoding")
                    .is_some_and(|v| v.as_str() != Some("utf-16"))
                {
                    return Err(LspError::Protocol);
                }
                self.cache = Some(result.clone());
                for pending in self.waiting.drain(..) {
                    if self.members.contains(&pending.client) {
                        self.initialized.insert(pending.client);
                        out.clients.push((
                            pending.client,
                            json!({"jsonrpc":"2.0","id":pending.id,"result":result}).to_string(),
                        ));
                    }
                }
                out.server
                    .push(json!({"jsonrpc":"2.0","method":"initialized","params":{}}).to_string());
                return Ok(out);
            }
            // Cancelled/disconnected responses have no recipient. Do not let their
            // stale result trigger authority errors in the shared generation.
            if !self.requests.contains_key(&global) {
                return Ok(out);
            }
            crate::uri::normalize(&mut value, access, 0).await?;
            if let Some(pending) = self.requests.remove(&global) {
                value["id"] = pending.id;
                out.clients.push((pending.client, value.to_string()));
            }
            // Late replies to cancelled/disconnected requests are deliberately ignored.
            return Ok(out);
        }
        crate::uri::normalize(&mut value, access, 0).await?;
        if value.get("method").and_then(Value::as_str) == Some("$/cancelRequest") {
            if let Some(original) = value.pointer("/params/id")
                && let Some((id, pending)) =
                    self.server_requests.iter().find(|(_, p)| &p.id == original)
            {
                out.clients.push((
                    pending.client,
                    json!({"jsonrpc":"2.0","method":"$/cancelRequest","params":{"id":id}})
                        .to_string(),
                ));
            }
            return Ok(out);
        }
        if value.get("method").and_then(Value::as_str) == Some("textDocument/publishDiagnostics") {
            let diagnostics = crate::diagnostics::parse(
                &value.get("params").ok_or(LspError::Protocol)?.to_string(),
            )?;
            if !self
                .documents
                .version_current(&diagnostics.uri, diagnostics.version)
            {
                return Ok(out);
            }
            out.diagnostics.push(diagnostics);
        }
        if value.get("id").is_some() {
            let original = request_id(&value)?;
            if value.get("method").and_then(Value::as_str) == Some("workspace/workspaceFolders") {
                out.server.push(json!({"jsonrpc":"2.0","id":original,"result":[{"uri":self.root,"name":"project"}]}).to_string());
                return Ok(out);
            }
            if self.server_requests.len() >= MAX_PENDING
                || self.server_requests.values().any(|p| p.id == original)
            {
                return Err(LspError::LimitExceeded);
            }
            let preferred = value
                .pointer("/params/edit/changes")
                .and_then(Value::as_object)
                .and_then(|changes| changes.keys().find_map(|uri| self.documents.owner(uri)));
            let target = preferred
                .or_else(|| self.initialized.first().copied())
                .or_else(|| self.waiting.first().map(|p| p.client));
            let Some(client) = target else {
                out.server.push(json!({"jsonrpc":"2.0","id":original,"error":{"code":-32800,"message":"No active client"}}).to_string());
                return Ok(out);
            };
            let next = self.id()?;
            let id = format!("devbox-server-{}-{next}", self.generation);
            self.server_requests.insert(
                id.clone(),
                Pending {
                    client,
                    id: original,
                },
            );
            value["id"] = id.into();
            out.clients.push((client, value.to_string()));
            return Ok(out);
        }
        for client in &self.initialized {
            out.clients.push((*client, value.to_string()));
        }
        Ok(out)
    }
    pub fn detach(&mut self, client: u32) -> RelayOutput {
        self.members.remove(&client);
        self.initialized.remove(&client);
        self.waiting.retain(|p| p.client != client);
        let mut out = RelayOutput::default();
        self.requests.retain(|id, p| {
            if p.client != client {
                return true;
            }
            out.server.push(
                json!({"jsonrpc":"2.0","method":"$/cancelRequest","params":{"id":id}}).to_string(),
            );
            false
        });
        self.server_requests.retain(|_,p|{if p.client!=client{return true;}out.server.push(json!({"jsonrpc":"2.0","id":p.id,"error":{"code":-32800,"message":"Client disconnected"}}).to_string());false});
        out.server.extend(self.documents.detach(client));
        out
    }
    pub fn ready(&self) -> bool {
        self.cache.is_some()
    }
    pub fn pending_count(&self) -> usize {
        self.requests.len() + self.server_requests.len()
    }
}
fn parse(text: &str) -> Result<Value, LspError> {
    if text.len() > crate::framing::MAX_BODY {
        return Err(LspError::LimitExceeded);
    }
    let value: Value = serde_json::from_str(text).map_err(|_| LspError::Protocol)?;
    if !value.is_object() || value.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Err(LspError::Protocol);
    }
    if value
        .get("method")
        .is_some_and(|v| v.as_str().is_none_or(|s| s.len() > 256))
    {
        return Err(LspError::Protocol);
    }
    if value.get("method").is_none()
        && (value.get("result").is_some() == value.get("error").is_some())
    {
        return Err(LspError::Protocol);
    }
    Ok(value)
}
fn request_id(value: &Value) -> Result<Value, LspError> {
    let id = value.get("id").ok_or(LspError::Protocol)?;
    if id.as_i64().is_some() || id.as_str().is_some_and(|s| s.len() <= 256) {
        Ok(id.clone())
    } else {
        Err(LspError::Protocol)
    }
}
#[cfg(test)]
mod tests;
