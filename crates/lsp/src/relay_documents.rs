use crate::{LspError, adapters::DocumentGrant};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Default)]
pub(crate) struct Documents {
    items: BTreeMap<String, Document>,
}
struct Document {
    refs: BTreeSet<u32>,
    owner: Option<(u32, String)>,
    owner_version: i64,
    server_version: i64,
    text: String,
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
            let doc = self.items.get(&uri).ok_or(LspError::Protocol)?;
            if !doc.refs.contains(&client)
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
            let text = if grant.lease_id.is_some() {
                text.to_owned()
            } else {
                grant.disk_text
            };
            let doc = Document {
                refs: BTreeSet::from([client]),
                owner: grant.lease_id.map(|l| (client, l)),
                owner_version: version,
                server_version: 1,
                text,
            };
            let out=json!({"jsonrpc":"2.0","method":method,"params":{"textDocument":{"uri":uri,"languageId":language,"version":1,"text":doc.text}}}).to_string();
            self.items.insert(uri, doc);
            return Ok(vec![out]);
        }
        if self.bytes() + text.len() > 34 * 1024 * 1024 {
            return Err(LspError::LimitExceeded);
        }
        let doc = self.items.get_mut(&uri).ok_or(LspError::Protocol)?;
        if method == "textDocument/didOpen" {
            doc.refs.insert(client);
        } else if !doc.refs.contains(&client) {
            return Err(LspError::Protocol);
        }
        let Some(lease) = grant.lease_id else {
            return if method == "textDocument/didOpen" {
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
        doc.text = text.to_owned();
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
        for saved in [json!({"params":{"text":"saved A"}}), json!({"params":{}})] {
            let out = docs
                .notification(1, "textDocument/didSave", &saved, grant())
                .expect("save ACK");
            let sent: Value = serde_json::from_str(&out[0]).expect("save notification");
            assert_eq!(sent.pointer("/params/text"), saved.pointer("/params/text"));
            assert_eq!(docs.items[uri].text, "unsaved B");
            assert_eq!(docs.items[uri].owner_version, 2);
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
