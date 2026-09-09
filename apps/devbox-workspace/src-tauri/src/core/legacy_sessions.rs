//! Product session metadata and reviewed Code Pad session conversion. No file
//! path here grants access: the native Files owner supplies eligible paths.
use code_pad_lib::core::session::Session;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
type Result<T> = std::result::Result<T, &'static str>;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Receipt {
    pub snapshot_id: String,
    // Original document IDs are retained verbatim in the destination session.
    pub document_ids: Vec<String>,
    pub recent_files: Vec<String>,
    pub source_workspace: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StoredSession {
    schema_version: u32,
    pub session: Session,
    pub imports: Vec<Receipt>,
}
impl Default for StoredSession {
    fn default() -> Self {
        Self {
            schema_version: 1,
            session: Session::empty(),
            imports: Vec::new(),
        }
    }
}
impl StoredSession {
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let raw: serde_json::Value =
            serde_json::from_slice(bytes).map_err(|_| "invalid_files_store")?;
        let session_bytes = if raw.get("schemaVersion").is_some() {
            serde_json::to_vec(raw.get("session").ok_or("invalid_files_store")?)
                .map_err(|_| "invalid_files_store")?
        } else {
            bytes.to_vec()
        };
        if super::legacy_inventory::inspect(
            super::legacy_inventory::Source::CodePad,
            "session.json",
            &session_bytes,
        )?
        .issue
        .is_some()
        {
            return Err("invalid_files_store");
        }
        let record: Self = if raw.get("schemaVersion").is_some() {
            serde_json::from_value(raw).map_err(|_| "invalid_files_store")?
        } else {
            code_pad_lib::component::validate_persistent_file("session.json", bytes)?;
            Self {
                session: serde_json::from_value(raw).map_err(|_| "invalid_files_store")?,
                ..Self::default()
            }
        };
        record.validate()?;
        Ok(record)
    }
    pub fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        // Preserve the established raw shape until an import needs an atomic
        // receipt. Subsequent editor saves retain the receipt in this file.
        let bytes = if self.imports.is_empty() {
            serde_json::to_vec(&self.session)
        } else {
            serde_json::to_vec(self)
        }
        .map_err(|_| "invalid_files_store")?;
        if bytes.len() > 8 * 1024 * 1024 {
            return Err("files_store_limit");
        }
        Ok(bytes)
    }
    fn validate(&self) -> Result<()> {
        if self.schema_version != 1 || self.imports.len() > 32 || self.session.validate().is_err() {
            return Err("invalid_files_store");
        }
        for receipt in &self.imports {
            if receipt.snapshot_id.len() != 64
                || !receipt
                    .snapshot_id
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                || receipt.document_ids.len() > 64
                || receipt.document_ids.iter().any(|id| id.trim().is_empty())
                || receipt.document_ids.iter().collect::<BTreeSet<_>>().len()
                    != receipt.document_ids.len()
            {
                return Err("invalid_files_store");
            }
        }
        Ok(())
    }
    pub fn apply(&self, candidate: &Candidate, replace_existing: bool) -> Result<Self> {
        if self.imports.contains(&candidate.receipt) {
            return Ok(self.clone());
        }
        if self.session != Session::empty() && !replace_existing {
            return Err("legacy_session_conflict");
        }
        if self.imports.len() >= 32 {
            return Err("legacy_session_limit");
        }
        let mut result = self.clone();
        result.session = candidate.session.clone();
        result.imports.push(candidate.receipt.clone());
        result.validate()?;
        Ok(result)
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub session: Session,
    pub receipt: Receipt,
    pub skipped_documents: usize,
    pub skipped_recent_files: usize,
}
pub fn candidate(
    snapshot_id: String,
    source: &Session,
    workspace: Option<String>,
    mut eligible: impl FnMut(&str) -> bool,
) -> Result<Candidate> {
    source
        .validate()
        .map_err(|_| "legacy_session_unavailable")?;
    let mut session = source.clone();
    session.workspace_folder = workspace;
    session.docs.retain(|doc| eligible(&doc.path));
    if session.docs.len() > 64 {
        return Err("legacy_session_limit");
    }
    let ids = session
        .docs
        .iter()
        .map(|doc| doc.id.clone())
        .collect::<BTreeSet<_>>();
    for (index, view) in session.views.iter_mut().enumerate() {
        view.retain(|id| ids.contains(id));
        if session.active_doc_by_view[index]
            .as_ref()
            .is_some_and(|id| !ids.contains(id))
        {
            session.active_doc_by_view[index] = view.first().cloned();
        }
    }
    session.recent_files.retain(|path| eligible(path));
    let receipt = Receipt {
        snapshot_id,
        document_ids: ids.into_iter().collect(),
        recent_files: session.recent_files.clone(),
        source_workspace: source.workspace_folder.clone(),
    };
    Ok(Candidate {
        skipped_documents: source.docs.len() - session.docs.len(),
        skipped_recent_files: source.recent_files.len() - session.recent_files.len(),
        session,
        receipt,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use code_pad_lib::core::session::SessionDoc;
    #[test]
    fn session_envelopes_reject_future_or_unknown_fields_without_dropping_import_receipts() {
        let session = Session::empty();
        let mut unknown = serde_json::to_value(&session).unwrap();
        unknown["future_setting"] = serde_json::json!(true);
        assert!(StoredSession::decode(&serde_json::to_vec(&unknown).unwrap()).is_err());
        let future = serde_json::json!({"schemaVersion":2,"session":session,"imports":[]});
        assert!(StoredSession::decode(&serde_json::to_vec(&future).unwrap()).is_err());
        let invalid = serde_json::json!({"schemaVersion":1,"session":session,"imports":[{"snapshotId":"../foreign","documentIds":[],"recentFiles":[],"sourceWorkspace":null}]});
        assert!(StoredSession::decode(&serde_json::to_vec(&invalid).unwrap()).is_err());
    }
    #[test]
    fn filtered_import_preserves_ids_bookmarks_views_and_records_repeat_without_erasing_new_edits()
    {
        let mut source = Session::empty();
        source.workspace_folder = Some("/legacy".into());
        source.docs = vec![
            SessionDoc {
                id: "old-id".into(),
                path: "/selected/a.txt".into(),
                cursor: 7,
                bookmarks: vec![1, 4],
            },
            SessionDoc::new("foreign", "/other/b.txt"),
        ];
        source.views = [vec!["foreign".into()], vec!["old-id".into()]];
        source.active_doc_by_view = [Some("foreign".into()), Some("old-id".into())];
        source.recent_files = vec!["/selected/a.txt".into(), "/other/b.txt".into()];
        let imported = candidate("a".repeat(64), &source, Some("/selected".into()), |path| {
            path.starts_with("/selected/")
        })
        .unwrap();
        assert_eq!(imported.skipped_documents, 1);
        assert_eq!(imported.skipped_recent_files, 1);
        assert_eq!(imported.session.docs[0], source.docs[0]);
        assert!(imported.session.views[0].is_empty());
        assert_eq!(imported.session.active_doc_by_view[0], None);
        let mut stored = StoredSession::default().apply(&imported, false).unwrap();
        stored.session.docs[0].cursor = 99;
        let loaded = StoredSession::decode(&stored.encode().unwrap()).unwrap();
        assert_eq!(
            loaded.apply(&imported, false).unwrap().session.docs[0].cursor,
            99
        );
        let another = candidate("b".repeat(64), &source, None, |_| true).unwrap();
        assert!(matches!(
            loaded.apply(&another, false),
            Err("legacy_session_conflict")
        ));
        assert_eq!(loaded.apply(&another, true).unwrap().imports.len(), 2);
        assert_eq!(source.docs.len(), 2);
        assert_eq!(source.docs[0].cursor, 7);
    }
}
