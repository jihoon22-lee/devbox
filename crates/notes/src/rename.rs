//! `notes.rename`: plan → durable progress journal → no-replace move → per-file CAS
//! link rewrites → journal removal. Resumed from `<data>/notes-rename/` at start.
//! A note open with unsaved edits is never written behind its window: the holder
//! gets `notes.rewrite_request` and has 30 s to apply, save and reply.
use crate::model::*;
use crate::rename_edits::{self, Candidates, Move, Planned};
use crate::rename_journal::{FilePlan, Journals, Plan, valid_op_id};
use crate::service::{NotesService, docs_fail, invalid};
use crate::vault::{hidden, is_note, valid_note_path};
use crate::{NotesRewriteRequestTopic, R};
use devbox_documents::{DocKey, DocKind, DocumentsError, Expect};
use devbox_protocol::{Done, Fail, Internal};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use tokio::sync::OwnedMutexGuard;

/// How long a window with unsaved edits has to apply a rewrite request.
pub const REPLY_WAIT_MS: u64 = 30_000;
const KEPT_STATUSES: usize = 64;

/// Test-only stop points (`rename_with`); the daemon reads `DEVBOX_TEST_NOTES_STOP`
/// only in debug builds of test instances.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopAt {
    /// After the journal is durable, before the note moves.
    BeforeMove,
    /// Right after the move, before the journal records it.
    AfterMove,
    /// After `n` files were rewritten and recorded.
    AfterFile(usize),
}
impl StopAt {
    /// `before_move`, `after_move` or `after_file:<n>`.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "before_move" => Some(Self::BeforeMove),
            "after_move" => Some(Self::AfterMove),
            _ => value
                .strip_prefix("after_file:")
                .and_then(|n| n.parse().ok())
                .map(Self::AfterFile),
        }
    }
}

#[derive(Default)]
pub(crate) struct Renames {
    /// One rename at a time (a resumed one included).
    pub(crate) lock: Arc<tokio::sync::Mutex<()>>,
    statuses: Mutex<(HashMap<String, RenameStatus>, VecDeque<String>)>,
    replies: Mutex<HashMap<(String, DocKey), Option<bool>>>,
    /// Notes the running rename moves or rewrites (`notes.delete`/`restore` wait).
    pub(crate) paths: Mutex<HashSet<String>>,
    /// Runs after planning, before the journal is written (tests edit files there).
    #[cfg(test)]
    pub after_plan: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    /// Runs right after the note moved (tests break the journal folder there).
    #[cfg(test)]
    pub after_move: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}

enum Outcome {
    Done,
    Failed(RewriteFailure),
    /// Open with unsaved edits: ask the holder.
    Ask(RewriteRequest),
}

impl NotesService {
    pub(crate) fn journals(&self) -> Journals {
        Journals::new(self.s.rename_dir.clone())
    }
    pub(crate) async fn save_journal(&self, plan: &Plan) -> R<()> {
        let (journals, plan) = (self.journals(), plan.clone());
        tokio::task::spawn_blocking(move || journals.write(&plan))
            .await
            .internal("join rename journal")?
            .internal("write rename journal")
    }
    pub(crate) fn drop_journal(&self, op: &str) {
        if self.journals().remove(op).is_err() {
            tracing::warn!("note rename journal was not removed");
        }
    }
    pub(crate) fn set_status(&self, op: &str, state: RenameState, failed: &[FailedRewrite]) {
        let reason = (state == RenameState::Incomplete).then_some(IncompleteReason::Failed);
        self.set_status_why(op, state, failed, reason);
    }
    pub(crate) fn set_status_why(
        &self,
        op: &str,
        state: RenameState,
        failed: &[FailedRewrite],
        reason: Option<IncompleteReason>,
    ) {
        let mut guard = self.s.renames.statuses.lock().expect("rename statuses");
        let (map, order) = &mut *guard;
        if !map.contains_key(op) {
            order.push_back(op.into());
        }
        let truncated = failed.len() > crate::service::MAX_ROWS;
        let failed = failed
            .iter()
            .take(crate::service::MAX_ROWS)
            .cloned()
            .collect();
        let status = RenameStatus {
            state,
            failed,
            truncated,
            reason,
        };
        map.insert(op.into(), status);
        while order.len() > KEPT_STATUSES {
            let Some(old) = order.pop_front() else { break };
            map.remove(&old);
        }
    }
    pub(crate) fn hold(&self, plan: &Plan) {
        let mut paths = self.s.renames.paths.lock().expect("rename paths");
        paths.clear();
        paths.extend([plan.mv.from.clone(), plan.mv.to.clone()]);
        paths.extend(plan.files.iter().map(|f| f.path.clone()));
    }
    /// True while a rename moves or rewrites `path`.
    pub(crate) fn rename_holds(&self, path: &str) -> bool {
        self.s
            .renames
            .paths
            .lock()
            .expect("rename paths")
            .contains(path)
    }

    pub async fn rename(&self, p: RenameNote) -> R<RenameStarted> {
        self.rename_with(p, self.s.stop).await
    }
    /// `stop` ends the operation at that point as if the daemon exited there.
    #[doc(hidden)]
    pub async fn rename_with(&self, p: RenameNote, stop: Option<StopAt>) -> R<RenameStarted> {
        let valid_target = valid_note_path(&p.new_path)
            && crate::links::path_key(&p.new_path).is_some()
            && p.new_path != p.path;
        if !valid_note_path(&p.path) || !valid_target {
            return invalid();
        }
        let guard = (self.s.renames.lock.clone())
            .try_lock_owned()
            .map_err(|_| NotesError::Busy)?;
        let (from, to) = (self.key(&p.path), self.key(&p.new_path));
        let source = match self.s.docs.read_disk(&from, DocKind::Note).await {
            Ok(doc) if doc.key != from => return invalid(),
            Ok(doc) if doc.rev.is_none() => return Err(NotesError::NotFound.into()),
            Ok(doc) => doc,
            Err(Fail::Domain(DocumentsError::InvalidInput)) => return invalid(),
            Err(e) => return Err(docs_fail(e)),
        };
        match self.s.docs.read_disk(&to, DocKind::Note).await {
            Ok(doc) if doc.rev.is_none() && doc.key == to => {}
            Ok(_) | Err(Fail::Domain(DocumentsError::InvalidInput)) => {
                return Err(NotesError::Exists.into());
            }
            Err(e) => return Err(docs_fail(e)),
        }
        let state = self.s.docs.edit_state(&to).await.map_err(docs_fail)?;
        if state.lease_id.is_some() || state.journal.is_some() {
            return Err(NotesError::TargetUnsaved.into());
        }
        let mv = Move::new(&p.path, &p.new_path, source.text.as_deref().unwrap_or(""));
        let files = if p.rewrite_links {
            self.plan_files(&mv).await?
        } else {
            Vec::new()
        };
        #[cfg(test)]
        if let Some(hook) = self.s.renames.after_plan.lock().expect("hook").clone() {
            hook();
        }
        let op = uuid::Uuid::new_v4().simple().to_string();
        let mut plan = Plan::new(op.clone(), mv, files);
        // Durable before any visible change.
        self.save_journal(&plan).await?;
        self.set_status(&op, RenameState::Running, &[]);
        self.hold(&plan);
        let started = RenameStarted { op_id: op.clone() };
        if stop == Some(StopAt::BeforeMove) {
            return Ok(started);
        }
        if let Err(e) = self.s.docs.move_file(&from, &to).await {
            self.drop_journal(&op);
            self.s
                .renames
                .statuses
                .lock()
                .expect("statuses")
                .0
                .remove(&op);
            self.s.renames.paths.lock().expect("rename paths").clear();
            return Err(match e {
                Fail::Domain(DocumentsError::InvalidInput) => NotesError::InvalidInput.into(),
                e => docs_fail(e),
            });
        }
        #[cfg(test)]
        if let Some(hook) = self.s.renames.after_move.lock().expect("hook").clone() {
            hook();
        }
        if stop == Some(StopAt::AfterMove) {
            return Ok(started);
        }
        // The note has moved: from here on errors must not end the operation early
        // (it would stay `running` and hold its notes). A stale journal is harmless:
        // resume sees the moved note and re-plans every file idempotently.
        plan.moved = true;
        if self.save_journal(&plan).await.is_err() {
            tracing::warn!("note rename progress was not recorded");
        }
        if self.refresh(vec![p.path, p.new_path]).await.is_err() {
            tracing::warn!("note index update failed");
        }
        match stop {
            Some(StopAt::AfterFile(n)) => self.finish(plan, guard, Some(n), false).await,
            _ => {
                let svc = self.clone();
                tokio::spawn(async move { svc.finish(plan, guard, None, false).await });
            }
        }
        Ok(started)
    }

    /// Notes whose links would break, with the rev each plan is based on.
    async fn plan_files(&self, mv: &Move) -> R<Vec<FilePlan>> {
        let mut keys = mv.old_keys.clone();
        keys.extend(mv.new_keys.iter().cloned());
        let mut sources = self
            .s
            .index
            .sources(keys)
            .await
            .internal("read note index")?;
        sources.push(mv.from.clone());
        sources.sort();
        sources.dedup();
        let mut cands = Candidates::new();
        let mut files = Vec::new();
        for src in sources.into_iter().filter(|s| is_note(s) && !hidden(s)) {
            let Ok(doc) = self.s.docs.read_disk(&self.key(&src), DocKind::Note).await else {
                continue;
            };
            let (Some(rev), Some(text)) = (doc.rev, doc.text) else {
                continue;
            };
            if !self.plan_text(&text, mv, &src, &mut cands).await?.clean() {
                let path = if src == mv.from { mv.to.clone() } else { src };
                files.push(FilePlan {
                    path,
                    base_rev: Some(rev),
                    done: false,
                });
            }
        }
        Ok(files)
    }
    async fn plan_text(
        &self,
        text: &str,
        mv: &Move,
        src: &str,
        cands: &mut Candidates,
    ) -> R<Planned> {
        for _ in 0..8 {
            match rename_edits::plan(text, mv, src, cands) {
                Ok(planned) => return Ok(planned),
                Err(keys) => {
                    for key in keys {
                        let found = self.s.index.candidates(key.clone()).await;
                        cands.insert(key, found.internal("read note index")?);
                    }
                }
            }
        }
        Err(Fail::Internal("link candidates did not settle".into()))
    }

    /// Rewrites every pending note, asks windows for open ones, then removes the
    /// journal. `resumed`: a note changed since planning is re-planned on its new
    /// rev instead of being reported as a conflict (the edit happened while down).
    pub(crate) async fn finish(
        &self,
        mut plan: Plan,
        guard: OwnedMutexGuard<()>,
        stop_after: Option<usize>,
        resumed: bool,
    ) {
        let (mut failed, mut asks, mut written) = (Vec::new(), Vec::new(), 0);
        let mut cands = Candidates::new();
        for i in 0..plan.files.len() {
            if plan.files[i].done {
                continue;
            }
            let outcome = self.rewrite(&plan, i, &mut cands, resumed).await;
            let path = plan.files[i].path.clone();
            match outcome {
                Ok(Outcome::Done) => {
                    plan.files[i].done = true;
                    if self.save_journal(&plan).await.is_err() {
                        tracing::warn!("note rename progress was not recorded");
                    }
                    written += 1;
                    if stop_after == Some(written) {
                        return;
                    }
                }
                Ok(Outcome::Failed(reason)) => failed.push(FailedRewrite { path, reason }),
                Ok(Outcome::Ask(request)) => asks.push((i, request)),
                Err(_) => {
                    tracing::warn!("note link rewrite failed");
                    let reason = RewriteFailure::Conflict;
                    failed.push(FailedRewrite { path, reason });
                }
            }
        }
        for (i, reason) in self.ask(&plan, asks, &mut cands).await {
            match reason {
                None => plan.files[i].done = true,
                Some(reason) => failed.push(FailedRewrite {
                    path: plan.files[i].path.clone(),
                    reason,
                }),
            }
        }
        self.drop_journal(&plan.op);
        failed.sort_by(|a, b| a.path.cmp(&b.path));
        let state = if failed.is_empty() {
            RenameState::Done
        } else {
            RenameState::Incomplete
        };
        self.set_status(&plan.op, state, &failed);
        self.s.renames.paths.lock().expect("rename paths").clear();
        if !failed.is_empty() {
            self.notify_incomplete(&plan.op, failed.len(), IncompleteReason::Failed);
        }
        drop(guard);
    }

    async fn rewrite(
        &self,
        plan: &Plan,
        i: usize,
        cands: &mut Candidates,
        resumed: bool,
    ) -> R<Outcome> {
        let (file, mv) = (&plan.files[i], &plan.mv);
        let (key, src) = (self.key(&file.path), mv.before(&file.path));
        let disk = match self.s.docs.read_disk(&key, DocKind::Note).await {
            Ok(doc) => doc,
            Err(Fail::Domain(DocumentsError::NotFound | DocumentsError::OutOfScope)) => {
                return Ok(Outcome::Failed(RewriteFailure::Missing));
            }
            Err(e) => return Err(docs_fail(e)),
        };
        let (Some(rev), Some(text)) = (disk.rev, disk.text) else {
            return Ok(Outcome::Failed(RewriteFailure::Missing));
        };
        let same = file.base_rev.as_ref().is_some_and(|b| b.same_content(&rev));
        if !same && !resumed {
            return Ok(Outcome::Failed(RewriteFailure::Conflict));
        }
        let state = self.s.docs.edit_state(&key).await.map_err(docs_fail)?;
        if let Some((seq, draft)) = state.journal {
            let Some(lease_id) = state.lease_id else {
                return Ok(Outcome::Failed(RewriteFailure::Open));
            };
            let planned = self.plan_text(&draft, mv, &src, cands).await?;
            if planned.edits.is_empty() {
                return Ok(Outcome::Failed(RewriteFailure::Open));
            }
            let edits = planned
                .edits
                .iter()
                .map(|e| TextEdit {
                    from_utf16: rename_edits::utf16_at(&draft, e.range.start) as u32,
                    to_utf16: rename_edits::utf16_at(&draft, e.range.end) as u32,
                    text: e.text.clone(),
                })
                .collect();
            return Ok(Outcome::Ask(RewriteRequest {
                op_id: plan.op.clone(),
                key,
                lease_id,
                journal_seq: seq,
                edits,
            }));
        }
        let planned = self.plan_text(&text, mv, &src, cands).await?;
        if planned.edits.is_empty() {
            return Ok(match planned.unsupported {
                true => Outcome::Failed(RewriteFailure::Unsupported),
                false => Outcome::Done,
            });
        }
        let next = rename_edits::apply(&text, &planned.edits);
        match self
            .s
            .docs
            .write_disk(&key, Some(rev), Expect::Present, &next)
            .await
        {
            Ok(_) => {}
            Err(Fail::Domain(DocumentsError::Conflict { .. })) => {
                return Ok(Outcome::Failed(RewriteFailure::Conflict));
            }
            Err(Fail::Domain(DocumentsError::NotFound)) => {
                return Ok(Outcome::Failed(RewriteFailure::Missing));
            }
            Err(e) => return Err(docs_fail(e)),
        }
        self.refresh(vec![file.path.clone()]).await?;
        Ok(match planned.unsupported {
            true => Outcome::Failed(RewriteFailure::Unsupported),
            false => Outcome::Done,
        })
    }

    /// Publishes the requests and waits up to [`REPLY_WAIT_MS`] (injected clock).
    /// A note counts as done only when its disk content no longer needs edits.
    async fn ask(
        &self,
        plan: &Plan,
        asks: Vec<(usize, RewriteRequest)>,
        cands: &mut Candidates,
    ) -> Vec<(usize, Option<RewriteFailure>)> {
        if asks.is_empty() {
            return Vec::new();
        }
        let slot = |r: &RewriteRequest| (r.op_id.clone(), r.key.clone());
        {
            let mut replies = self.s.renames.replies.lock().expect("rename replies");
            for (_, request) in &asks {
                replies.insert(slot(request), None);
            }
        }
        for (_, request) in &asks {
            self.s
                .hub
                .publish::<NotesRewriteRequestTopic>(self.rev(), request);
        }
        let start = (self.s.clock)();
        loop {
            let answered = {
                let replies = self.s.renames.replies.lock().expect("rename replies");
                asks.iter()
                    .all(|(_, r)| replies.get(&slot(r)).is_some_and(Option::is_some))
            };
            if answered || (self.s.clock)().saturating_sub(start) >= REPLY_WAIT_MS {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        let mut out = Vec::new();
        for (i, request) in asks {
            let reply = self
                .s
                .renames
                .replies
                .lock()
                .expect("rename replies")
                .remove(&slot(&request));
            let fixed = reply == Some(Some(true)) && self.fixed(plan, i, cands).await;
            out.push((i, (!fixed).then_some(RewriteFailure::Open)));
        }
        out
    }
    async fn fixed(&self, plan: &Plan, i: usize, cands: &mut Candidates) -> bool {
        let path = &plan.files[i].path;
        let Ok(doc) = self.s.docs.read_disk(&self.key(path), DocKind::Note).await else {
            return false;
        };
        let Some(text) = doc.text.filter(|_| doc.rev.is_some()) else {
            return false;
        };
        let src = plan.mv.before(path);
        let clean = self.plan_text(&text, &plan.mv, &src, cands).await;
        let ok = clean.is_ok_and(|p| p.clean());
        if ok && self.refresh(vec![path.clone()]).await.is_err() {
            tracing::warn!("note index update failed");
        }
        ok
    }

    pub async fn rename_status(&self, p: RenameOp) -> R<RenameStatus> {
        if !valid_op_id(&p.op_id) {
            return invalid();
        }
        let statuses = self.s.renames.statuses.lock().expect("rename statuses");
        statuses
            .0
            .get(&p.op_id)
            .cloned()
            .ok_or(NotesError::NotFound.into())
    }
    pub async fn rewrite_reply(&self, p: RewriteReply) -> R<Done> {
        if !valid_op_id(&p.op_id) {
            return invalid();
        }
        let mut replies = self.s.renames.replies.lock().expect("rename replies");
        match replies.get_mut(&(p.op_id, p.key)) {
            Some(slot) => *slot = Some(p.applied),
            None => return Err(NotesError::NotFound.into()),
        }
        Ok(Done {
            rev: self.rev().to_string(),
        })
    }

    #[cfg(test)]
    pub(crate) fn on_moved(&self, f: impl Fn() + Send + Sync + 'static) {
        *self.s.renames.after_move.lock().expect("rename hook") = Some(Arc::new(f));
    }
    #[cfg(test)]
    pub(crate) fn on_planned(&self, f: impl Fn() + Send + Sync + 'static) {
        *self.s.renames.after_plan.lock().expect("rename hook") = Some(Arc::new(f));
    }
    /// Waits until `op` is no longer running.
    #[doc(hidden)]
    pub async fn wait_rename(&self, op: &str) -> R<RenameStatus> {
        loop {
            let status = self.rename_status(RenameOp { op_id: op.into() }).await?;
            if status.state != RenameState::Running {
                return Ok(status);
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }
}
