use crate::{LspError, adapters::DocumentGrant};
use serde_json::{Value, json};
use std::collections::BTreeMap;
#[derive(Default)]
pub(crate) struct Documents {
    items: BTreeMap<String, Document>,
}
struct Document {
    refs: BTreeMap<u32, Reference>,
    snapshot: i64,
    owner: Option<(u32, String)>,
    owner_version: i64,
    server_version: i64,
    text: String,
    text_hash: String,
    disk_rev: Option<devbox_documents::DiskRev>,
}
struct Reference {
    version: i64,
    snapshot: Option<i64>,
}
impl Documents {
    pub fn notification(
        &mut self,
        client: u32,
        method: &str,
        value: &Value,
        grant: DocumentGrant,
    ) -> Result<Vec<String>, LspError> {
        let uri = grant.uri;
        if method == "textDocument/didClose" {
            return Ok(self.close(client, &uri));
        }
        if method == "textDocument/didSave" {
            let doc = self.items.get_mut(&uri).ok_or(LspError::Protocol)?;
            if !doc.refs.contains_key(&client)
                || grant
                    .lease_id
                    .as_ref()
                    .is_none_or(|lease| doc.owner.as_ref() != Some(&(client, lease.clone())))
            {
                return Err(LspError::LeaseLost);
            }
            // The save ACK can describe an older snapshot than the current dirty buffer.
            let mut params = json!({"textDocument":{"uri":uri}});
            if let Some(saved) = value.pointer("/params/text") {
                let text = saved.as_str().ok_or(LspError::InvalidInput)?;
                if text.len() > crate::framing::MAX_BODY {
                    return Err(LspError::LimitExceeded);
                }
                params["text"] = saved.clone();
            }
            doc.disk_rev = grant.disk_rev;
            return Ok(vec![
                json!({"jsonrpc":"2.0","method":method,"params":params}).to_string(),
            ]);
        }
        let version = value
            .pointer("/params/textDocument/version")
            .and_then(Value::as_i64)
            .ok_or(LspError::InvalidInput)?;
        let text = if method == "textDocument/didOpen" {
            value
                .pointer("/params/textDocument/text")
                .and_then(Value::as_str)
                .ok_or(LspError::InvalidInput)?
        } else {
            let changes = value
                .pointer("/params/contentChanges")
                .and_then(Value::as_array)
                .ok_or(LspError::InvalidInput)?;
            if changes.len() != 1 || changes[0].get("range").is_some() {
                return Err(LspError::InvalidInput);
            }
            changes[0]
                .get("text")
                .and_then(Value::as_str)
                .ok_or(LspError::InvalidInput)?
        };
        if text.len() > crate::framing::MAX_BODY {
            return Err(LspError::LimitExceeded);
        }
        if method == "textDocument/didOpen" && !self.items.contains_key(&uri) {
            if self.items.len() >= 256 || self.bytes() + text.len() > 32 * 1024 * 1024 {
                return Err(LspError::LimitExceeded);
            }
            let language = value
                .pointer("/params/textDocument/languageId")
                .and_then(Value::as_str)
                .filter(|s| s.len() <= 64)
                .ok_or(LspError::InvalidInput)?;
            let supplied = text;
            let text = if grant.lease_id.is_some() {
                text.to_owned()
            } else {
                grant.disk_text
            };
            if text.len() > crate::framing::MAX_BODY {
                return Err(LspError::LimitExceeded);
            }
            let text_hash = crate::workspace::text_hash(&text);
            let doc = Document {
                refs: BTreeMap::from([(
                    client,
                    Reference {
                        version,
                        snapshot: (supplied == text).then_some(1),
                    },
                )]),
                snapshot: 1,
                owner: grant.lease_id.map(|l| (client, l)),
                owner_version: version,
                server_version: 1,
                text,
                text_hash,
                disk_rev: grant.disk_rev,
            };
            let out=json!({"jsonrpc":"2.0","method":method,"params":{"textDocument":{"uri":uri,"languageId":language,"version":1,"text":doc.text}}}).to_string();
            self.items.insert(uri, doc);
            return Ok(vec![out]);
        }
        if self.bytes() + text.len() > 34 * 1024 * 1024 {
            return Err(LspError::LimitExceeded);
        }
        let doc = self.items.get_mut(&uri).ok_or(LspError::Protocol)?;
        if method != "textDocument/didOpen" && !doc.refs.contains_key(&client) {
            return Err(LspError::Protocol);
        }
        let Some(lease) = grant.lease_id else {
            return if method == "textDocument/didOpen" {
                doc.refs.insert(
                    client,
                    Reference {
                        version,
                        snapshot: (text == grant.disk_text && text == doc.text)
                            .then_some(doc.snapshot),
                    },
                );
                Ok(vec![])
            } else {
                Err(LspError::LeaseLost)
            };
        };
        let same_owner = doc.owner.as_ref() == Some(&(client, lease.clone()));
        if same_owner && version <= doc.owner_version {
            return Ok(vec![]);
        }
        doc.owner = Some((client, lease));
        doc.owner_version = version;
        doc.server_version = doc
            .server_version
            .checked_add(1)
            .ok_or(LspError::LimitExceeded)?;
        if doc.text != text {
            doc.snapshot = doc.server_version;
        }
        doc.text = text.to_owned();
        doc.text_hash = crate::workspace::text_hash(text);
        doc.disk_rev = grant.disk_rev;
        doc.refs.insert(
            client,
            Reference {
                version,
                snapshot: Some(doc.snapshot),
            },
        );
        Ok(vec![json!({"jsonrpc":"2.0","method":"textDocument/didChange","params":{"textDocument":{"uri":uri,"version":doc.server_version},"contentChanges":[{"text":doc.text}]}}).to_string()])
    }
    fn bytes(&self) -> usize {
        self.items.values().map(|d| d.text.len()).sum()
    }
    fn close(&mut self, client: u32, uri: &str) -> Vec<String> {
        let Some(doc) = self.items.get_mut(uri) else {
            return vec![];
        };
        doc.refs.remove(&client);
        if doc.owner.as_ref().is_some_and(|(id, _)| *id == client) {
            doc.owner = None;
        }
        if doc.refs.is_empty() {
            self.items.remove(uri);
            vec![json!({"jsonrpc":"2.0","method":"textDocument/didClose","params":{"textDocument":{"uri":uri}}}).to_string()]
        } else {
            vec![]
        }
    }
    pub fn detach(&mut self, client: u32) -> Vec<String> {
        let uris: Vec<_> = self.items.keys().cloned().collect();
        uris.into_iter()
            .flat_map(|uri| self.close(client, &uri))
            .collect()
    }
    pub fn version_current(&self, uri: &str, version: Option<i64>) -> bool {
        version.is_none_or(|v| self.items.get(uri).is_none_or(|d| v >= d.server_version))
    }
    /// A server version names a different counter from each renderer's local version.
    /// Only an exact current server version can be relabeled. Unversioned diagnostics
    /// are the server's assertion about its current snapshot and remain unversioned.
    pub fn renderer_version(
        &self,
        uri: &str,
        client: u32,
        server_version: Option<i64>,
    ) -> Option<i64> {
        let doc = self.items.get(uri)?;
        if server_version.is_some_and(|version| version != doc.server_version) {
            return None;
        }
        let reference = doc.refs.get(&client)?;
        (reference.snapshot == Some(doc.snapshot)).then_some(reference.version)
    }
    /// Translate only the version fields of an LSP WorkspaceEdit, never arbitrary JSON.
    pub fn workspace_versions(&self, client: u32, edit: &mut Value) -> bool {
        let Some(changes) = edit.get_mut("documentChanges") else {
            return true;
        };
        let Some(changes) = changes.as_array_mut() else {
            return false;
        };
        let mut versions = Vec::new();
        for (index, change) in changes.iter().enumerate() {
            let Some(document) = change.get("textDocument") else {
                continue;
            };
            let Some(version) = document.get("version").filter(|v| !v.is_null()) else {
                continue;
            };
            let (Some(uri), Some(version)) = (
                document.get("uri").and_then(Value::as_str),
                version.as_i64(),
            ) else {
                return false;
            };
            let Some(local) = self.renderer_version(uri, client, Some(version)) else {
                return false;
            };
            versions.push((index, local));
        }
        for (index, version) in versions {
            changes[index]["textDocument"]["version"] = version.into();
        }
        true
    }
    pub(crate) fn snapshots(&self, client: u32) -> BTreeMap<String, crate::WorkspaceBaseline> {
        self.items
            .iter()
            .filter_map(|(uri, doc)| {
                let (owner, lease) = doc.owner.as_ref()?;
                if *owner != client {
                    return None;
                }
                Some((
                    uri.clone(),
                    crate::WorkspaceBaseline {
                        uri: uri.clone(),
                        disk_rev: doc.disk_rev.clone(),
                        text_hash: doc.text_hash.clone(),
                        lease_id: Some(lease.clone()),
                        version: self.renderer_version(uri, client, Some(doc.server_version)),
                    },
                ))
            })
            .collect()
    }
    pub(crate) fn compatible_disk(&self, uri: &str, hash: &str) -> bool {
        self.items.get(uri).is_none_or(|doc| doc.text_hash == hash)
    }
    pub(crate) fn owned_versioned(&self, client: u32, edit: &Value) -> bool {
        if edit.get("changes").is_some() {
            return false;
        }
        edit.get("documentChanges")
            .and_then(Value::as_array)
            .is_some_and(|rows| {
                rows.len() <= 256
                    && rows.iter().all(|row| {
                        let Some(uri) = row.pointer("/textDocument/uri").and_then(Value::as_str)
                        else {
                            return false;
                        };
                        let Some(version) =
                            row.pointer("/textDocument/version").and_then(Value::as_i64)
                        else {
                            return false;
                        };
                        self.owner(uri) == Some(client)
                            && self.renderer_version(uri, client, Some(version)).is_some()
                    })
            })
    }
    pub fn owner(&self, uri: &str) -> Option<u32> {
        self.items.get(uri)?.owner.as_ref().map(|(id, _)| *id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn review_saved_ack_and_omitted_text_do_not_rewrite_the_latest_buffer() {
        let mut docs = Documents::default();
        let uri = "file:///owned/a.rs";
        let grant = || DocumentGrant {
            uri: uri.into(),
            disk_text: "saved A".into(),
            disk_rev: None,
            lease_id: Some("L1".into()),
        };
        docs.notification(
            1,
            "textDocument/didOpen",
            &json!({"params":{"textDocument":{"languageId":"rust","version":1,"text":"saved A"}}}),
            grant(),
        )
        .expect("open");
        docs.notification(1,"textDocument/didChange",&json!({"params":{"textDocument":{"version":2},"contentChanges":[{"text":"unsaved B"}]}}),grant()).expect("edit while save pending");
        let saved_rev = devbox_documents::DiskRev {
            hash: "saved-on-disk".into(),
            size: 7,
            mtime_ns: "2".into(),
            ino: "3".into(),
        };
        for saved in [json!({"params":{"text":"saved A"}}), json!({"params":{}})] {
            let mut saved_grant = grant();
            saved_grant.disk_rev = Some(saved_rev.clone());
            let out = docs
                .notification(1, "textDocument/didSave", &saved, saved_grant)
                .expect("save ACK");
            let sent: Value = serde_json::from_str(&out[0]).expect("save notification");
            assert_eq!(sent.pointer("/params/text"), saved.pointer("/params/text"));
            assert_eq!(docs.items[uri].text, "unsaved B");
            assert_eq!(docs.items[uri].owner_version, 2);
            let proof = docs.snapshots(1);
            assert_eq!(
                proof[uri].disk_rev,
                Some(saved_rev.clone()),
                "Rename after save must use the acknowledged raw revision"
            );
            assert_eq!(
                proof[uri].text_hash,
                crate::workspace::text_hash("unsaved B")
            );
        }
        assert!(
            docs.notification(2, "textDocument/didSave", &json!({"params":{}}), grant())
                .is_err()
        );
        let mut old = grant();
        old.lease_id = Some("old lease".into());
        assert!(
            docs.notification(1, "textDocument/didSave", &json!({"params":{}}), old)
                .is_err()
        );
    }
}
