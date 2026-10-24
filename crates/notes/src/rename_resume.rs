//! Startup half of `notes.rename`: journals left by an earlier run are continued,
//! abandoned with a reason, or (when unreadable) set aside and reported once.
use crate::NotesRenameIncompleteTopic;
use crate::R;
use crate::model::*;
use crate::service::NotesService;
use devbox_documents::{DocKey, DocKind, DocumentsError};
use devbox_protocol::{Fail, Internal};

impl NotesService {
    pub(crate) fn notify_incomplete(&self, op: &str, count: usize, reason: IncompleteReason) {
        self.s.hub.publish::<NotesRenameIncompleteTopic>(
            self.rev(),
            &RenameIncomplete {
                op_id: op.into(),
                count: count as u32,
                reason,
            },
        );
    }

    /// Continues every journal left by an earlier run (called from `start`).
    /// Unreadable journals are set aside as `<op>.json.corrupt` and reported once.
    pub async fn resume(&self) -> R<()> {
        let journals = self.journals();
        let (plans, unreadable) = tokio::task::spawn_blocking(move || journals.list())
            .await
            .internal("list rename journals")?;
        for op in unreadable {
            let journals = self.journals();
            let (aside, name) = (op.clone(), op.clone());
            let moved = tokio::task::spawn_blocking(move || journals.set_aside(&aside)).await;
            if !matches!(moved, Ok(Ok(()))) {
                tracing::warn!("unreadable note rename journal was not set aside");
            }
            let reason = IncompleteReason::Unreadable;
            self.set_status_why(&name, RenameState::Incomplete, &[], Some(reason));
            self.notify_incomplete(&name, 0, reason);
        }
        for mut plan in plans {
            let guard = self.s.renames.lock.clone().lock_owned().await;
            self.set_status(&plan.op, RenameState::Running, &[]);
            self.hold(&plan);
            if !plan.moved {
                let (from, to) = (self.key(&plan.mv.from), self.key(&plan.mv.to));
                let there = |key: DocKey| async move {
                    let doc = self.s.docs.read_disk(&key, DocKind::Note).await;
                    doc.is_ok_and(|d| d.rev.is_some())
                };
                let abandoned = match (there(from.clone()).await, there(to.clone()).await) {
                    (true, false) => match self.s.docs.move_file(&from, &to).await {
                        Ok(_) => None,
                        Err(Fail::Domain(DocumentsError::Exists)) => {
                            Some(IncompleteReason::TargetExists)
                        }
                        Err(_) => Some(IncompleteReason::MoveFailed),
                    },
                    (false, true) => None,
                    (true, true) => Some(IncompleteReason::TargetExists),
                    (false, false) => Some(IncompleteReason::SourceMissing),
                };
                if let Some(reason) = abandoned {
                    // Nothing visible happened (or someone else took the name): give up.
                    tracing::warn!("interrupted note rename was abandoned");
                    self.drop_journal(&plan.op);
                    self.set_status_why(&plan.op, RenameState::Incomplete, &[], Some(reason));
                    self.s.renames.paths.lock().expect("rename paths").clear();
                    self.notify_incomplete(&plan.op, 0, reason);
                    continue;
                }
                plan.moved = true;
                if self.save_journal(&plan).await.is_err() {
                    tracing::warn!("note rename progress was not recorded");
                }
            }
            let moved = vec![plan.mv.from.clone(), plan.mv.to.clone()];
            if self.refresh(moved).await.is_err() {
                tracing::warn!("note index update failed");
            }
            self.finish(plan, guard, None, true).await;
        }
        Ok(())
    }
}
