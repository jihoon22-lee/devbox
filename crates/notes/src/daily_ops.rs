//! `notes.daily`, `notes.append` and the summary write behind `notes.insert_summary`.
//! Writes are CAS (`write_disk` with the rev just read, 3 tries). A note open with
//! unsaved edits is never written behind its window: the lease holder gets one
//! `notes.rewrite_request` (Task 5 path) and has 30 s to apply, save and reply.
use crate::daily::{self, DEFAULT_DAILY, SummaryError};
use crate::model::*;
use crate::ops::template_problem;
use crate::rename::REPLY_WAIT_MS;
use crate::service::{NotesService, docs_fail, invalid};
use crate::templates::{self, MAX_TEMPLATE_CONTENT_BYTES, TemplateInput};
use crate::{NotesRewriteRequestTopic, R};
use devbox_documents::{DiskRev, DocKey, DocKind, DocumentsError, Expect, NOTE_LIMIT, ReadOnly};
use devbox_protocol::Fail;

const TRIES: usize = 3;
/// A summary block is a few hundred lines at most.
const MAX_SUMMARY_BYTES: usize = 256 * 1024;

fn summary_fail(e: SummaryError) -> Fail<NotesError> {
    match e {
        SummaryError::Broken => NotesError::SummaryBroken.into(),
        SummaryError::InvalidPc | SummaryError::InvalidContent => NotesError::InvalidInput.into(),
    }
}
fn read_only(r: Option<ReadOnly>) -> Fail<NotesError> {
    match r.map(|r| r.reason) {
        Some(devbox_documents::ReadOnlyReason::TooLarge) => {
            NotesError::TooLarge { limit: NOTE_LIMIT }.into()
        }
        _ => NotesError::InvalidInput.into(),
    }
}

/// The note an append or summary edits.
struct Edit<'a> {
    path: &'a str,
    key: DocKey,
}

impl NotesService {
    /// This PC's name (`[pc] name`, else the host name).
    pub async fn pc_name(&self) -> String {
        (self.s.pc)().await
    }
    fn date_or_today(&self, date: Option<&str>) -> R<String> {
        let date = date.map_or_else(|| (self.s.today)().0, str::to_owned);
        if !templates::valid_date(&date) {
            return invalid();
        }
        Ok(date)
    }

    pub async fn daily(&self, p: DailyParams) -> R<DailyNote> {
        let date = self.date_or_today(p.date.as_deref())?;
        self.ensure_daily(&date).await
    }
    /// Creates `<daily_folder>/<date>.md` from `<templates>/daily.md` (or the
    /// default) when it does not exist; an existing note is never rewritten.
    pub(crate) async fn ensure_daily(&self, date: &str) -> R<DailyNote> {
        let path = daily::daily_path(&self.s.settings.daily_folder, date);
        let key = self.key(&path);
        let disk = self.s.docs.read_disk(&key, DocKind::Note).await;
        if disk.map_err(docs_fail)?.rev.is_some() {
            return Ok(DailyNote {
                path,
                created: false,
            });
        }
        let input = TemplateInput {
            target: path.clone(),
            title: date.into(),
            date: date.into(),
            time: (self.s.today)().1,
            pc: self.pc_name().await,
        };
        let text =
            templates::render(&self.daily_template().await?, &input).map_err(template_problem)?;
        match self
            .s
            .docs
            .write_disk(&key, None, Expect::Absent, &text)
            .await
        {
            Ok(_) => {
                self.refresh(vec![path.clone()]).await?;
                Ok(DailyNote {
                    path,
                    created: true,
                })
            }
            Err(Fail::Domain(DocumentsError::Exists)) => Ok(DailyNote {
                path,
                created: false,
            }),
            Err(e) => Err(docs_fail(e)),
        }
    }
    async fn daily_template(&self) -> R<String> {
        let path = format!("{}/daily.md", self.s.settings.templates_folder);
        match self.s.docs.read_disk(&self.key(&path), DocKind::Note).await {
            Ok(doc) if doc.rev.is_none() => Ok(DEFAULT_DAILY.into()),
            Ok(doc) => match doc.text {
                Some(text) if text.len() <= MAX_TEMPLATE_CONTENT_BYTES => Ok(text),
                _ => Err(template_problem(TemplateProblem::InvalidContent)),
            },
            Err(Fail::Domain(DocumentsError::NotFound)) => Ok(DEFAULT_DAILY.into()),
            Err(e) => Err(docs_fail(e)),
        }
    }

    pub async fn append(&self, p: AppendNote) -> R<Appended> {
        if devbox_core::input::validate_command_text(&p.text).is_err() || p.text.trim().is_empty() {
            return invalid();
        }
        let (date, time) = (self.s.today)();
        let entry = match p.format {
            AppendFormat::Plain => daily::plain_entry(&p.text),
            AppendFormat::Capture => daily::capture_entry(&time, &p.text),
        };
        let path = match p.target {
            AppendTarget::Inbox => self.s.settings.inbox.clone(),
            AppendTarget::Daily => {
                let date = self.date_or_today(Some(&date))?;
                self.ensure_daily(&date).await?.path
            }
        };
        let rev = self
            .edit_note(
                &path,
                |text| Ok(daily::appended(text.unwrap_or(""), &entry)),
                // The window's save carries one more copy of the entry than the
                // buffer it applied the edit to (later typing may follow it).
                |draft, disk| disk.matches(&entry).count() > draft.matches(&entry).count(),
            )
            .await?;
        Ok(Appended { path, rev })
    }

    /// Writes `markdown` as this PC's block in the daily note of `date`.
    pub async fn insert_summary(&self, date: &str, pc: &str, markdown: &str) -> R<SummaryInserted> {
        let date = self.date_or_today(Some(date))?;
        if markdown.len() > MAX_SUMMARY_BYTES || markdown.contains('\0') {
            return invalid();
        }
        // Refuse a bad PC name or forged markers before creating anything.
        daily::replace_summary("", pc, markdown).map_err(summary_fail)?;
        let path = self.ensure_daily(&date).await?.path;
        let replace = |text: &str| daily::replace_summary(text, pc, markdown);
        let rev = self
            .edit_note(
                &path,
                |text| replace(text.unwrap_or("")).map_err(summary_fail),
                |_, disk| replace(disk).is_ok_and(|out| out == disk),
            )
            .await?;
        Ok(SummaryInserted { path, rev })
    }

    /// `edit` maps the current text (`None` = absent) to the new text; `done(draft,
    /// saved)` tells whether a window's saved note carries the edit it applied to
    /// its buffer `draft`.
    async fn edit_note(
        &self,
        path: &str,
        edit: impl Fn(Option<&str>) -> R<String>,
        done: impl Fn(&str, &str) -> bool,
    ) -> R<DiskRev> {
        self.edit_note_inner(path, edit, done, None).await
    }
    pub(crate) async fn edit_response_note(
        &self,
        path: &str,
        edit: impl Fn(Option<&str>) -> R<String>,
        done: impl Fn(&str, &str) -> bool,
        guard: &crate::response_append::ResponseGuard,
    ) -> R<DiskRev> {
        self.edit_note_inner(path, edit, done, Some(guard)).await
    }
    async fn edit_note_inner(
        &self,
        path: &str,
        edit: impl Fn(Option<&str>) -> R<String>,
        done: impl Fn(&str, &str) -> bool,
        guard: Option<&crate::response_append::ResponseGuard>,
    ) -> R<DiskRev> {
        let at = Edit {
            path,
            key: self.key(path),
        };
        for _ in 0..TRIES {
            if self.rename_holds(path) {
                return Err(NotesError::Busy.into());
            }
            let state = self.s.docs.edit_state(&at.key).await.map_err(docs_fail)?;
            let disk = self.s.docs.read_disk(&at.key, DocKind::Note).await;
            let disk = disk.map_err(docs_fail)?;
            if let Some(guard) = guard {
                guard.validate(disk.rev.as_ref())?;
            }
            if let Some((seq, draft)) = state.journal {
                // A draft nobody holds waits for recovery: never write behind it.
                let Some(lease_id) = state.lease_id else {
                    return Err(NotesError::OpenUnsaved.into());
                };
                let next = edit(Some(&draft))?;
                return self
                    .ask_holder(&at, (lease_id, seq, &draft, &next), disk.rev, &done, guard)
                    .await;
            }
            let text = match (&disk.rev, disk.text) {
                (None, _) => None,
                (Some(_), Some(text)) => Some(text),
                (Some(_), None) => return Err(read_only(disk.read_only)),
            };
            #[cfg(test)]
            if let Some(hook) = self.s.after_read.lock().expect("read hook").clone() {
                hook();
            }
            let next = edit(text.as_deref())?;
            if let (Some(rev), Some(text)) = (&disk.rev, &text)
                && *text == next
            {
                if let Some(guard) = guard {
                    guard.committed(rev.clone()).await?;
                }
                return Ok(rev.clone());
            }
            if let Some(guard) = guard {
                guard.validate(disk.rev.as_ref())?;
                guard
                    .phase(crate::response_journal::Phase::Attempted)
                    .await?;
            }
            let expect = match disk.rev {
                Some(_) => Expect::Present,
                None => Expect::Absent,
            };
            // The limit applies to the bytes on disk (CRLF and BOM included).
            let write = self
                .s
                .docs
                .write_disk_within(&at.key, disk.rev, expect, &next, NOTE_LIMIT);
            match write.await {
                Ok(rev) => {
                    if let Some(guard) = guard {
                        guard.committed(rev.clone()).await?;
                    }
                    self.refresh(vec![path.into()]).await?;
                    return Ok(rev);
                }
                // Changed since the read: read again, never overwrite.
                Err(Fail::Domain(DocumentsError::Conflict { .. } | DocumentsError::Exists)) => {
                    if let Some(guard) = guard {
                        guard.phase(crate::response_journal::Phase::Pending).await?;
                    }
                }
                Err(e) => {
                    if matches!(&e, Fail::Domain(_))
                        && let Some(guard) = guard
                    {
                        guard.phase(crate::response_journal::Phase::Pending).await?;
                    }
                    return Err(docs_fail(e));
                }
            }
        }
        Err(NotesError::Conflict.into())
    }

    /// Sends the holder one edit for its buffer (journal `seq`) and waits for the
    /// saved result. Done only when the disk changed and carries the edit.
    async fn ask_holder(
        &self,
        at: &Edit<'_>,
        (lease_id, seq, draft, next): (String, u64, &str, &str),
        before: Option<DiskRev>,
        done: &impl Fn(&str, &str) -> bool,
        guard: Option<&crate::response_append::ResponseGuard>,
    ) -> R<DiskRev> {
        if next == draft {
            return Err(NotesError::OpenUnsaved.into());
        }
        if guard.is_some() {
            self.s
                .docs
                .validate_note_rewrite(&at.key, &lease_id, next)
                .await
                .map_err(docs_fail)?;
        }
        let request = RewriteRequest {
            op_id: uuid::Uuid::new_v4().simple().to_string(),
            key: at.key.clone(),
            lease_id,
            journal_seq: seq,
            edits: vec![daily::single_edit(draft, next)],
        };
        if let Some(guard) = guard {
            if serde_json::to_vec(&request).map_or(true, |bytes| bytes.len() > 8 * 1024 * 1024) {
                return Err(NotesError::TooLarge {
                    limit: 8 * 1024 * 1024,
                }
                .into());
            }
            guard.validate(before.as_ref())?;
            guard
                .phase(crate::response_journal::Phase::Attempted)
                .await?;
        }
        if self.ask_window(request).await != Some(true) {
            return Err(NotesError::OpenUnsaved.into());
        }
        let disk = self.s.docs.read_disk(&at.key, DocKind::Note).await;
        match disk.map_err(docs_fail)? {
            devbox_documents::DiskDocument {
                rev: Some(rev),
                text: Some(text),
                ..
            } if !before.is_some_and(|b| b.same_content(&rev)) && done(draft, &text) => {
                if let Some(guard) = guard {
                    guard.committed(rev.clone()).await?;
                }
                self.refresh(vec![at.path.into()]).await?;
                Ok(rev)
            }
            _ => Err(NotesError::OpenUnsaved.into()),
        }
    }

    /// Publishes `request` and waits up to [`REPLY_WAIT_MS`] (injected clock) for
    /// `notes.rewrite_reply`.
    pub(crate) async fn ask_window(&self, request: RewriteRequest) -> Option<bool> {
        let slot = (request.op_id.clone(), request.key.clone());
        let replies = &self.s.renames.replies;
        replies
            .lock()
            .expect("rename replies")
            .insert(slot.clone(), None);
        self.s
            .hub
            .publish::<NotesRewriteRequestTopic>(self.rev(), &request);
        let start = (self.s.clock)();
        loop {
            let answered = replies.lock().expect("rename replies").get(&slot).copied();
            if answered.flatten().is_some()
                || (self.s.clock)().saturating_sub(start) >= REPLY_WAIT_MS
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        replies
            .lock()
            .expect("rename replies")
            .remove(&slot)
            .flatten()
    }
}
