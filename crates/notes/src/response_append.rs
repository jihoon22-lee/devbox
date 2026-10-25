//! Selected vault authority and replay-safe response appends, using the normal CAS/rewrite path.
use crate::response_journal::{self, Phase, Receipt};
use crate::service::docs_fail;
use crate::*;
use devbox_documents::{DiskRev, DocKind};
use devbox_protocol::{Fail, Internal};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub(crate) const RESPONSE_LIMIT: usize = 1024 * 1024 + 16 * 1024;
const SELECTION_TTL: u64 = 10 * 60 * 1000;
const MAX_SELECTIONS: usize = 256;
pub(crate) struct Responses {
    selections: Mutex<HashMap<String, Selection>>,
    serial: tokio::sync::Mutex<()>,
    slots: Arc<tokio::sync::Semaphore>,
}
impl Default for Responses {
    fn default() -> Self {
        Self {
            selections: Mutex::default(),
            serial: tokio::sync::Mutex::new(()),
            slots: Arc::new(tokio::sync::Semaphore::new(4)),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Selection {
    pub path: String,
    target: ResponseNoteTarget,
    root: PathBuf,
    dev: u64,
    ino: u64,
    file_ino: Option<String>,
    date: Option<String>,
    expires: u64,
}
fn changed<T>() -> R<T> {
    Err(NotesError::ResponseTargetChanged.into())
}
fn digest(history: &str, text: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(history.as_bytes());
    hash.update([0]);
    hash.update(text.as_bytes());
    format!("{:x}", hash.finalize())
}
fn marker(receipt: &Receipt) -> String {
    format!(
        "<!-- devbox:api-response:{}:{} -->",
        receipt.request.operation_id, receipt.digest
    )
}
fn valid_request(p: &AppendResponseNote) -> bool {
    crate::rename_journal::valid_op_id(&p.operation_id)
        && crate::rename_journal::valid_op_id(&p.history_id)
        && crate::rename_journal::valid_op_id(&p.selection)
}
impl Selection {
    fn validate(&self, svc: &NotesService, rev: Option<&DiskRev>, exact_file: bool) -> R<()> {
        let root = std::fs::canonicalize(svc.vault())
            .map_err(|_| Fail::from(NotesError::ResponseTargetChanged))?;
        let meta =
            std::fs::metadata(&root).map_err(|_| Fail::from(NotesError::ResponseTargetChanged))?;
        if root != self.root || meta.dev() != self.dev || meta.ino() != self.ino {
            return changed();
        }
        let file = root.join(&self.path);
        if let Ok(real) = std::fs::canonicalize(&file) {
            if real != file {
                return changed();
            }
        } else if matches!(self.target, ResponseNoteTarget::Existing { .. }) {
            return changed();
        }
        if exact_file {
            if matches!(self.target, ResponseNoteTarget::Existing { .. }) && rev.is_none() {
                return changed();
            }
            if let Some(ino) = &self.file_ino {
                if rev.is_none_or(|rev| &rev.ino != ino)
                    || std::fs::symlink_metadata(&file)
                        .ok()
                        .is_none_or(|meta| meta.ino().to_string() != *ino || !meta.is_file())
                {
                    return changed();
                }
            }
        }
        Ok(())
    }
}

/// Held by the runtime-owned worker until actual mutation/ACK processing ends.
pub(crate) struct ResponseGuard {
    svc: NotesService,
    receipt: Mutex<Receipt>,
}
impl ResponseGuard {
    pub(crate) fn validate(&self, rev: Option<&DiskRev>) -> R<()> {
        self.receipt
            .lock()
            .expect("receipt")
            .target
            .validate(&self.svc, rev, true)
    }
    pub(crate) async fn phase(&self, phase: Phase) -> R<()> {
        let next = {
            let mut receipt = self.receipt.lock().expect("receipt");
            receipt.phase = phase;
            receipt.clone()
        };
        self.svc.store_response_receipt(next).await
    }
    pub(crate) async fn committed(&self, rev: DiskRev) -> R<()> {
        let r = self.receipt.lock().expect("receipt").clone();
        self.phase(Phase::Committed {
            result: ResponseNoteAppended {
                path: r.target.path,
                rev,
                operation_id: r.request.operation_id,
            },
        })
        .await
    }
}
impl NotesService {
    fn response_dir(&self) -> PathBuf {
        self.s.rename_dir.join("response-appends")
    }
    async fn response_receipt(&self, p: &AppendResponseNote) -> R<Option<Receipt>> {
        if !valid_request(p) {
            return Err(NotesError::InvalidInput.into());
        }
        let (dir, op) = (self.response_dir(), p.operation_id.clone());
        let receipt = tokio::task::spawn_blocking(move || response_journal::read(&dir, &op))
            .await
            .internal("read response append receipt")?
            .internal("read response append receipt")?;
        if receipt.as_ref().is_some_and(|r| r.request != *p) {
            return Err(NotesError::AppendOperationMismatch.into());
        }
        Ok(receipt)
    }
    async fn store_response_receipt(&self, r: Receipt) -> R<()> {
        let dir = self.response_dir();
        tokio::task::spawn_blocking(move || response_journal::write(&dir, &r))
            .await
            .internal("save response append receipt")?
            .internal("save response append receipt")
    }
    pub async fn select_response_target(&self, p: SelectResponseNote) -> R<ResponseNoteSelection> {
        let (path, date) = match &p.target {
            ResponseNoteTarget::Daily => {
                let date = (self.s.today)().0;
                if !crate::templates::valid_date(&date) {
                    return Err(NotesError::InvalidInput.into());
                }
                (
                    crate::daily::daily_path(&self.s.settings.daily_folder, &date),
                    Some(date),
                )
            }
            ResponseNoteTarget::Existing { path } => {
                if !crate::vault::valid_note_path(path) {
                    return changed();
                }
                (path.clone(), None)
            }
        };
        let root = std::fs::canonicalize(self.vault())
            .map_err(|_| Fail::from(NotesError::ResponseTargetChanged))?;
        let meta =
            std::fs::metadata(&root).map_err(|_| Fail::from(NotesError::ResponseTargetChanged))?;
        let disk = self
            .s
            .docs
            .read_disk(&self.key(&path), DocKind::Note)
            .await
            .map_err(docs_fail)?;
        let selected = Selection {
            path: path.clone(),
            target: p.target.clone(),
            root,
            dev: meta.dev(),
            ino: meta.ino(),
            file_ino: disk.rev.as_ref().map(|r| r.ino.clone()),
            date,
            expires: (self.s.clock)().saturating_add(SELECTION_TTL),
        };
        selected.validate(self, disk.rev.as_ref(), true)?;
        let id = uuid::Uuid::new_v4().simple().to_string();
        let mut selections = self
            .s
            .responses
            .selections
            .lock()
            .expect("response selections");
        selections.retain(|_, s| s.expires > (self.s.clock)());
        if selections.len() >= MAX_SELECTIONS {
            return Err(NotesError::Busy.into());
        }
        selections.insert(id.clone(), selected);
        Ok(ResponseNoteSelection {
            selection: id,
            path,
            target: p.target,
        })
    }
    /// Before API export: a committed retry remains successful even if history later expires.
    pub async fn response_append_result(
        &self,
        p: &AppendResponseNote,
    ) -> R<Option<ResponseNoteAppended>> {
        let _lock = self.s.responses.serial.lock().await;
        let Some(mut receipt) = self.response_receipt(p).await? else {
            return Ok(None);
        };
        match &receipt.phase {
            Phase::Committed { result } => return Ok(Some(result.clone())),
            Phase::Pending => return Ok(None),
            Phase::Attempted => {}
        }
        receipt.target.validate(self, None, false)?;
        let disk = self
            .s
            .docs
            .read_disk(&self.key(&receipt.target.path), DocKind::Note)
            .await
            .map_err(docs_fail)?;
        if disk
            .text
            .as_ref()
            .is_some_and(|text| text.contains(&marker(&receipt)))
            && let Some(rev) = disk.rev
        {
            let result = ResponseNoteAppended {
                path: receipt.target.path.clone(),
                rev,
                operation_id: p.operation_id.clone(),
            };
            receipt.phase = Phase::Committed {
                result: result.clone(),
            };
            self.store_response_receipt(receipt).await?;
            return Ok(Some(result));
        }
        // A failed/timed-out ACK may still save later. Never dispatch another edit for this intent.
        Err(NotesError::AppendUncertain.into())
    }
    pub async fn append_response(
        &self,
        p: AppendResponseNote,
        markdown: String,
    ) -> R<ResponseNoteAppended> {
        if markdown.is_empty()
            || markdown.len() > RESPONSE_LIMIT
            || markdown.contains('\0')
            || !valid_request(&p)
        {
            return Err(NotesError::InvalidInput.into());
        }
        let slot = self
            .s
            .responses
            .slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| Fail::from(NotesError::Busy))?;
        let svc = self.clone();
        tokio::spawn(async move {
            let _slot = slot;
            svc.append_response_inner(p, markdown).await
        })
        .await
        .internal("response append worker")?
    }
    async fn append_response_inner(
        &self,
        p: AppendResponseNote,
        markdown: String,
    ) -> R<ResponseNoteAppended> {
        if let Some(result) = self.response_append_result(&p).await? {
            return Ok(result);
        }
        let _lock = self.s.responses.serial.lock().await;
        // Recheck after taking the mutation lock: another identical caller may have completed.
        let receipt = match self.response_receipt(&p).await? {
            Some(receipt) => {
                if let Phase::Committed { result } = &receipt.phase {
                    return Ok(result.clone());
                }
                if matches!(receipt.phase, Phase::Attempted) {
                    return Err(NotesError::AppendUncertain.into());
                }
                if receipt.digest != digest(&p.history_id, &markdown) {
                    return Err(NotesError::AppendOperationMismatch.into());
                }
                receipt
            }
            None => {
                let target = self
                    .s
                    .responses
                    .selections
                    .lock()
                    .expect("selections")
                    .get(&p.selection)
                    .filter(|s| s.expires > (self.s.clock)())
                    .cloned()
                    .ok_or(NotesError::ResponseTargetChanged)?;
                let receipt = Receipt {
                    version: 1,
                    digest: digest(&p.history_id, &markdown),
                    request: p.clone(),
                    target,
                    phase: Phase::Pending,
                };
                self.store_response_receipt(receipt.clone()).await?;
                receipt
            }
        };
        let disk = self
            .s
            .docs
            .read_disk(&self.key(&receipt.target.path), DocKind::Note)
            .await
            .map_err(docs_fail)?;
        receipt.target.validate(self, disk.rev.as_ref(), true)?;
        if let Some(date) = &receipt.target.date {
            self.ensure_daily(date).await?;
        }
        let mark = marker(&receipt);
        let entry = crate::daily::plain_entry(&format!("{mark}\n{markdown}"));
        let path = receipt.target.path.clone();
        let guard = ResponseGuard {
            svc: self.clone(),
            receipt: Mutex::new(receipt),
        };
        let rev = self
            .edit_response_note(
                &path,
                |text| {
                    let text = text.unwrap_or("");
                    Ok(if text.contains(&mark) {
                        text.to_owned()
                    } else {
                        crate::daily::appended(text, &entry)
                    })
                },
                |_, disk| disk.contains(&mark),
                &guard,
            )
            .await?;
        Ok(ResponseNoteAppended::new(
            p.operation_id,
            Appended { path, rev },
        ))
    }
}
