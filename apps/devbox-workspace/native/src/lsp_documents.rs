//! Linux LSP documents consume attestations from the independent Files owner.
//! They never adopt a renderer hash or treat a POSIX URI as an open-file grant.
use super::lsp_runtime::{error, value};
use crate::{
    files::{FileOwner, RootLease},
    lsp_authority::Lease,
    lsp_wire::{DocumentMethod as Method, DocumentProof, ProofRequest},
};
use code_pad_lib::{commands::file::OpenFileRequest, lsp::LspManager};
use serde_json::Value;
use std::{collections::BTreeMap, sync::Arc};
type Result<T> = std::result::Result<T, &'static str>;
#[derive(Clone)]
struct Binding {
    proof: DocumentProof,
    text: String,
}
pub(crate) struct Documents {
    lease: Arc<Lease>,
    owner: FileOwner,
    snapshots: BTreeMap<String, (DocumentProof, String)>,
    bindings: BTreeMap<(String, String), Binding>,
}
impl Documents {
    pub(crate) fn new(lease: Arc<Lease>) -> Self {
        Self {
            lease,
            owner: FileOwner::with_admission(crate::linux_files::admit),
            snapshots: BTreeMap::new(),
            bindings: BTreeMap::new(),
        }
    }
    pub(crate) fn contains(&self, language: &str, uri: &str) -> bool {
        self.bindings.contains_key(&(language.into(), uri.into()))
    }
    pub(crate) fn close_language(&mut self, language: Option<&str>) -> Result<()> {
        self.bindings
            .retain(|(id, _), _| language.is_some_and(|language| id != language));
        self.prune()
    }
    fn prune(&mut self) -> Result<()> {
        let unused = self
            .snapshots
            .keys()
            .filter(|path| {
                !self
                    .bindings
                    .values()
                    .any(|binding| &binding.proof.path == *path)
            })
            .cloned()
            .collect::<Vec<_>>();
        for path in unused {
            self.owner
                .close_for_context(Some(&self.lease.context), &path)?;
            self.snapshots.remove(&path);
        }
        Ok(())
    }
    fn same_snapshot(left: &DocumentProof, right: &DocumentProof) -> bool {
        left.context == right.context
            && left.path == right.path
            && left.identity == right.identity
            && left.parents == right.parents
            && left.mtime_nanos == right.mtime_nanos
            && left.size == right.size
            && left.content_hash == right.content_hash
            && left.encoding == right.encoding
            && left.baseline_text_hash == right.baseline_text_hash
    }
    fn admit(&mut self, proof: DocumentProof, verify_disk: bool) -> Result<DocumentProof> {
        proof.validate()?;
        if proof.context != self.lease.context {
            return Err("lsp_document_denied");
        }
        self.lease.revalidate()?;
        let replace = self
            .snapshots
            .get(&proof.path)
            .is_none_or(|(previous, _)| previous.revision != proof.revision);
        if replace {
            self.owner
                .close_for_context(Some(&self.lease.context), &proof.path)?;
            self.snapshots.remove(&proof.path);
            self.owner.open(
                Some((&self.lease.context, self.lease.as_ref())),
                OpenFileRequest {
                    path: proof.path.clone(),
                    encoding: Some(proof.encoding),
                },
            )?;
        } else if self
            .snapshots
            .get(&proof.path)
            .is_none_or(|(previous, _)| !Self::same_snapshot(previous, &proof))
        {
            return Err("file_snapshot_changed");
        }
        let revision = self.owner.document_revision(&proof.path)?;
        let local = self.owner.editor_proof(
            Some((&self.lease.context, self.lease.as_ref())),
            &ProofRequest {
                path: proof.path.clone(),
                native_revision: revision.clone(),
                verify_disk,
            },
            &|| self.lease.revalidate(),
        )?;
        if !Self::same_snapshot(&local, &proof) {
            return Err("file_snapshot_changed");
        }
        self.snapshots
            .insert(proof.path.clone(), (proof.clone(), revision));
        Ok(proof)
    }
    pub(crate) async fn execute(
        &mut self,
        manager: &LspManager,
        method: Method,
        proof: Option<DocumentProof>,
    ) -> Result<Value> {
        if matches!(
            method,
            Method::RequestLspRename { .. } | Method::ApplyLspRename { .. }
        ) {
            return Err("lsp_feature_unsupported");
        }
        if let Method::CancelLspRename { plan_id } = method {
            return value(manager.cancel_rename(&plan_id).await);
        }
        if let Method::DiscardLspRename { plan_id } = method {
            return value(manager.discard_rename(&plan_id).await);
        }
        let (language, target) = method.target();
        let key = (language.to_owned(), target.to_owned());
        if let Method::CloseLspDocument { language_id, uri } = method {
            self.bindings.remove(&key);
            self.prune()?;
            return value(
                manager
                    .close_document(&language_id, &uri)
                    .await
                    .map_err(error)?,
            );
        }
        let proof = proof.ok_or("file_selection_required")?;
        if let Method::OpenLspDocument {
            language_id,
            path,
            text,
            native_revision,
        } = method
        {
            if proof.path != path || proof.revision != native_revision {
                return Err("file_snapshot_changed");
            }
            if self.bindings.len() >= 64
                && !self.bindings.iter().any(|((language, _), binding)| {
                    language == &language_id && binding.proof.path == path
                })
            {
                return Err("file_limit");
            }
            let proof = self.admit(proof, true)?;
            let mut opened = manager
                .open_document(&language_id, std::path::Path::new(&path), text.clone())
                .await
                .map_err(error)?;
            if proof.dirty(&text) {
                opened.version = manager
                    .change_document(&language_id, &opened.uri, text.clone(), true)
                    .await
                    .map_err(error)?
                    .version;
            }
            self.bindings
                .insert((language_id, opened.uri.clone()), Binding { proof, text });
            return value(opened);
        }
        let binding = self
            .bindings
            .get(&key)
            .ok_or("lsp_document_denied")?
            .clone();
        let revision = match &method {
            Method::ChangeLspDocument {
                native_revision, ..
            }
            | Method::ReloadLspDocument {
                native_revision, ..
            }
            | Method::SaveLspDocument {
                native_revision, ..
            } => native_revision,
            _ => &binding.proof.revision,
        };
        if proof.path != binding.proof.path || &proof.revision != revision {
            return Err("file_snapshot_changed");
        }
        let verify = matches!(
            method,
            Method::ReloadLspDocument { .. } | Method::SaveLspDocument { .. }
        );
        let proof = self.admit(proof, verify)?;
        match method {
            Method::ChangeLspDocument {
                language_id,
                uri,
                text,
                ..
            } => {
                let changed = manager
                    .change_document(&language_id, &uri, text.clone(), proof.dirty(&text))
                    .await
                    .map_err(error)?;
                self.bindings.insert(key, Binding { proof, text });
                value(changed)
            }
            Method::ReloadLspDocument {
                language_id,
                uri,
                text,
                ..
            } => {
                if proof.dirty(&text) {
                    return Err("file_snapshot_changed");
                }
                let changed = manager
                    .reload_document(&language_id, &uri, text.clone())
                    .await
                    .map_err(error)?;
                self.bindings.insert(key, Binding { proof, text });
                value(changed)
            }
            Method::SaveLspDocument {
                language_id,
                uri,
                text,
                ..
            } => {
                let text = text.unwrap_or(binding.text);
                if proof.dirty(&text) {
                    return Err("file_snapshot_changed");
                }
                if self.bindings[&key].text != text {
                    manager
                        .reload_document(&language_id, &uri, text.clone())
                        .await
                        .map_err(error)?;
                }
                let saved = manager
                    .save_document(&language_id, &uri)
                    .await
                    .map_err(error)?;
                self.bindings.insert(key, Binding { proof, text });
                value(saved)
            }
            Method::PullLspDiagnostics { language_id, uri } => value(
                manager
                    .pull_diagnostics(&language_id, &uri)
                    .await
                    .map_err(error)?,
            ),
            Method::RequestLspCompletion {
                language_id,
                uri,
                position,
            } => value(
                manager
                    .completion(&language_id, &uri, position)
                    .await
                    .map_err(error)?,
            ),
            Method::RequestLspHover {
                language_id,
                uri,
                position,
            } => value(
                manager
                    .hover(&language_id, &uri, position)
                    .await
                    .map_err(error)?,
            ),
            Method::RequestLspDefinition {
                language_id,
                uri,
                position,
            } => value(
                manager
                    .definition(&language_id, &uri, position)
                    .await
                    .map_err(error)?,
            ),
            Method::RequestLspReferences {
                language_id,
                uri,
                position,
                include_declaration,
            } => value(
                manager
                    .references(&language_id, &uri, position, include_declaration)
                    .await
                    .map_err(error)?,
            ),
            Method::RequestLspFormatting {
                language_id,
                uri,
                tab_size,
                insert_spaces,
            } => {
                let result = manager
                    .formatting(&language_id, &uri, tab_size, insert_spaces)
                    .await
                    .map_err(error)?;
                for document in &result.documents {
                    if document.uri != uri {
                        return Err("lsp_document_denied");
                    }
                    self.bindings.insert(
                        key.clone(),
                        Binding {
                            proof: proof.clone(),
                            text: document.text.clone(),
                        },
                    );
                }
                value(result)
            }
            _ => Err("wsl_request_invalid"),
        }
    }
}
