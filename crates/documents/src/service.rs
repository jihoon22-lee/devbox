use crate::journal::{JournalFile, JournalStore};
use crate::lease::{Acquired, Format, LeaseView, Leases};
use crate::ledger::{ClaimedDirs, TmpLedger};
use crate::model::*;
use crate::scope::{Resolved, ScopeError, WriteScope, in_trash, is_internal, validate};
use crate::sys::{self, DiskFile, SysError, classify, encode_text, escaped_len};
use crate::watch::Watch;
use crate::{MAX_WIRE_TEXT, R};
use devbox_core::Hub;
use devbox_protocol::router::BoxFuture;
use devbox_protocol::{Done, Fail, Internal};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::{OwnedMutexGuard, RwLock};
use tokio_util::sync::CancellationToken;

pub use crate::lease::Clock;
/// Registered roots, recomputed for every request by the daemon composition.
pub type Roots = Arc<dyn Fn() -> BoxFuture<Vec<crate::Root>> + Send + Sync>;
pub type Hook = Arc<dyn Fn() -> BoxFuture<()> + Send + Sync>;
pub(crate) const VAULT: &str = "vault";

/// Pause points for tests. Production leaves every field empty.
#[derive(Clone, Default)]
pub struct Hooks {
    /// Runs after reading a claimed document, before the lease critical section.
    pub before_lease_insert: Option<Hook>,
    #[cfg(test)]
    pub(crate) before_rename_commit: Option<Hook>,
    #[cfg(test)]
    pub(crate) after_rename_publish: Option<Hook>,
    #[cfg(test)]
    pub(crate) after_tmp_written: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
    /// Runs after a save wrote the file, before it registers the file watch.
    #[cfg(test)]
    pub(crate) before_save_watch: Option<Hook>,
}

pub struct DocumentsConfig {
    pub data_dir: PathBuf,
    pub roots: Roots,
    pub hub: Arc<Hub>,
    pub clock: Clock,
    pub max_wire_text: usize,
    pub hooks: Hooks,
    /// Daemon-owned folders (data, state) refused through every root.
    pub private: Vec<PathBuf>,
}
impl DocumentsConfig {
    pub fn new(data_dir: PathBuf, roots: Roots, hub: Arc<Hub>) -> Self {
        let start = std::time::Instant::now();
        Self {
            data_dir,
            roots,
            hub,
            clock: Arc::new(move || start.elapsed().as_millis() as u64),
            max_wire_text: MAX_WIRE_TEXT,
            hooks: Hooks::default(),
            private: Vec::new(),
        }
    }
}

pub(crate) struct Shared {
    /// Ordinary operations share this barrier; namespace moves exclude all of
    /// them before resolution, including claims for previously unopened children.
    pub namespace: RwLock<()>,
    pub roots: Roots,
    pub hub: Arc<Hub>,
    pub max_wire: usize,
    pub hooks: Hooks,
    pub private: Vec<PathBuf>,
    pub leases: Arc<Leases>,
    pub journals: JournalStore,
    pub ledger: TmpLedger,
    pub dirs: ClaimedDirs,
    pub watch: Arc<Watch>,
    pub rev: Arc<AtomicU64>,
}

#[derive(Clone)]
pub struct DocumentsService {
    pub(crate) s: Arc<Shared>,
    root_constraints: Vec<String>,
}

pub(crate) fn scope_fail(e: ScopeError) -> Fail<DocumentsError> {
    match e {
        ScopeError::Invalid => DocumentsError::InvalidInput.into(),
        ScopeError::OutOfScope => DocumentsError::OutOfScope.into(),
        ScopeError::Missing(_) | ScopeError::NoFolder => DocumentsError::NotFound.into(),
        ScopeError::Io(e) => Fail::Internal(format!("open document folder: {e}")),
    }
}
pub(crate) fn sys_fail(e: SysError) -> Fail<DocumentsError> {
    match e {
        SysError::Conflict(rev) => DocumentsError::Conflict { rev }.into(),
        SysError::Exists => DocumentsError::Exists.into(),
        SysError::NotFound => DocumentsError::NotFound.into(),
        SysError::Unsupported => DocumentsError::Unsupported.into(),
        SysError::Link => DocumentsError::OutOfScope.into(),
        SysError::NotFile => DocumentsError::InvalidInput.into(),
        e => Fail::Internal(format!("document io: {e}")),
    }
}
fn lost(reason: LeaseLoss) -> Fail<DocumentsError> {
    DocumentsError::LeaseLost { reason }.into()
}
pub(crate) async fn blocking<T: Send + 'static>(f: impl FnOnce() -> R<T> + Send + 'static) -> R<T> {
    tokio::task::spawn_blocking(f)
        .await
        .internal("join document io")?
}

/// A resolved key, or the canonical key of a document whose folder does not exist yet.
pub(crate) enum Located {
    At(Resolved),
    Missing(DocKey),
}
impl Located {
    fn key(&self) -> &DocKey {
        match self {
            Located::At(r) => &r.key,
            Located::Missing(k) => k,
        }
    }
}
struct Opened {
    located: Located,
    file: Option<DiskFile>,
    content: sys::Content,
    journal: Option<JournalFile>,
}

impl DocumentsService {
    pub fn new(c: DocumentsConfig) -> Self {
        let rev = Arc::new(AtomicU64::new(0));
        let watch = Arc::new(Watch::new(c.hub.clone(), rev.clone()));
        let (hub, notify_watch, notify_rev) = (c.hub.clone(), watch.clone(), rev.clone());
        let leases = Arc::new(Leases::new(
            c.clock,
            Arc::new(move |key, id, notify| {
                notify_watch.remove(key);
                if notify {
                    let rev = notify_rev.fetch_add(1, Ordering::SeqCst) + 1;
                    hub.publish::<crate::DocumentsLeaseLost>(
                        rev,
                        &LeaseLost {
                            key: key.clone(),
                            lease_id: id.into(),
                        },
                    );
                }
            }),
        ));
        let data = c.data_dir;
        Self {
            root_constraints: Vec::new(),
            s: Arc::new(Shared {
                namespace: RwLock::new(()),
                roots: c.roots,
                hub: c.hub,
                max_wire: c.max_wire_text,
                hooks: c.hooks,
                private: c.private,
                leases,
                journals: JournalStore::new(data.join("journals")),
                ledger: TmpLedger::new(data.join("documents/tmp")),
                dirs: ClaimedDirs::new(data.join("documents/dirs")),
                watch,
                rev,
            }),
        }
    }

    /// A domain-scoped handle sharing the same leases, journals and CAS state.
    /// Roots are filtered before descriptor resolution (including folder creation).
    pub fn scoped_to(&self, root: &str) -> Self {
        let mut scoped = self.clone();
        scoped.root_constraints.push(root.into());
        scoped
    }
    async fn roots(&self) -> Vec<crate::Root> {
        (self.s.roots)()
            .await
            .into_iter()
            .filter(|root| self.root_constraints.iter().all(|id| id == &root.id))
            .collect()
    }
    pub async fn scope(&self) -> R<WriteScope> {
        let (roots, private) = (self.roots().await, self.s.private.clone());
        blocking(move || Ok(WriteScope::new(roots).deny(&private))).await
    }
    pub(crate) async fn locate(&self, key: DocKey, create: bool, entry: bool) -> R<Located> {
        self.locate_with(key, create, entry, false).await
    }
    /// `trash`: a key under `.trash/` (and nothing else internal) is allowed.
    pub(crate) async fn locate_with(
        &self,
        key: DocKey,
        create: bool,
        entry: bool,
        trash: bool,
    ) -> R<Located> {
        let (roots, private) = (self.roots().await, self.s.private.clone());
        let refused = move |path: &str| is_internal(path) && !(trash && in_trash(path));
        blocking(move || {
            let scope = WriteScope::new(roots).deny(&private);
            let r = if entry {
                scope.resolve_entry(&key, create)
            } else {
                scope.resolve(&key, create)
            };
            match r {
                Ok(r) if refused(&r.key.path) => Err(DocumentsError::OutOfScope.into()),
                Ok(r) => Ok(Located::At(r)),
                Err(ScopeError::Missing(k)) if !refused(&k.path) => Ok(Located::Missing(k)),
                Err(e) => Err(scope_fail(e)),
            }
        })
        .await
    }
    pub(crate) fn next_rev(&self) -> u64 {
        self.s.rev.fetch_add(1, Ordering::SeqCst) + 1
    }
    fn done(&self) -> Done {
        Done {
            rev: self.next_rev().to_string(),
        }
    }
    async fn opened(&self, located: Located, kind: DocKind, with_journal: bool) -> R<Opened> {
        let (s, max_wire) = (self.s.clone(), self.s.max_wire);
        blocking(move || {
            let file = match &located {
                Located::At(r) => sys::read_at(&r.dir, &r.name, kind.limit()).map_err(sys_fail)?,
                Located::Missing(_) => None,
            };
            let content = match &file {
                Some(file) => classify(file, max_wire),
                None => sys::Content {
                    text: Some(String::new()),
                    eol: Eol::Lf,
                    bom: false,
                    read_only: None,
                },
            };
            let journal = if with_journal && content.read_only.is_none() {
                s.journals
                    .load(located.key())
                    .internal("read document journal")?
            } else {
                None
            };
            if with_journal && let Located::At(r) = &located {
                s.dirs
                    .note(&r.dir_path)
                    .internal("record document folder")?;
            }
            Ok(Opened {
                located,
                file,
                content,
                journal,
            })
        })
        .await
    }

    /// `Ok(None)`: the owner (channel) closed before the lease was inserted.
    pub async fn claim(&self, owner: CancellationToken, p: ClaimDocument) -> R<Option<Claimed>> {
        self.open(owner, p.key, p.policy.kind, p.window_id, false)
            .await
    }
    pub async fn take(&self, owner: CancellationToken, p: TakeDocument) -> R<Option<Claimed>> {
        let kind = p.policy.map_or(DocKind::Note, |p| p.kind);
        self.open(owner, p.key, kind, p.window_id, true).await
    }
    async fn open(
        &self,
        owner: CancellationToken,
        key: DocKey,
        kind: DocKind,
        window: String,
        take: bool,
    ) -> R<Option<Claimed>> {
        let _namespace = self.s.namespace.read().await;
        if window.is_empty() || window.len() > 128 {
            return Err(DocumentsError::InvalidInput.into());
        }
        let located = self.locate(key, false, false).await?;
        let _guard = self.s.leases.lock(located.key()).await;
        let o = self.opened(located, kind, true).await?;
        if let Some(hook) = &self.s.hooks.before_lease_insert {
            hook().await;
        }
        let rev = o.file.as_ref().map(|f| f.rev.clone());
        let key = o.located.key().clone();
        let reply = |lease_id, text, read_only, journal| Claimed {
            key: key.clone(),
            lease_id,
            rev: rev.clone(),
            text,
            eol: o.content.eol,
            bom: o.content.bom,
            read_only,
            journal,
        };
        if let Some(reason) = o.content.read_only {
            return Ok(Some(reply(None, None, Some(ReadOnly { reason }), None)));
        }
        let format = Format {
            kind,
            eol: o.content.eol,
            bom: o.content.bom,
        };
        // No await from here: insertion, owner watcher and file watch are one step.
        match self.s.leases.acquire(&key, &window, &owner, format, take) {
            Acquired::Cancelled => Ok(None),
            Acquired::Elsewhere => {
                let reason = ReadOnlyReason::LeasedElsewhere;
                Ok(Some(reply(
                    None,
                    o.content.text.clone(),
                    Some(ReadOnly { reason }),
                    None,
                )))
            }
            Acquired::Granted(lease) => {
                if let Located::At(r) = &o.located {
                    let hash = rev.as_ref().map(|r| r.hash.clone());
                    self.s.leases.while_live(&lease.id, &key, || {
                        self.s.watch.add(&key, r.abs(), hash);
                    });
                }
                let cost = o.content.text.as_deref().map_or(0, escaped_len);
                let journal = o.journal.clone().map(|j| {
                    let fits = cost + escaped_len(&j.text) <= self.s.max_wire;
                    JournalView {
                        seq: j.seq,
                        base_rev: j.base_rev,
                        text: fits.then_some(j.text),
                        deferred: !fits,
                    }
                });
                Ok(Some(reply(
                    Some(lease.id),
                    o.content.text.clone(),
                    None,
                    journal,
                )))
            }
        }
    }

    pub async fn renew(&self, p: RenewLeases) -> R<Renewed> {
        let _namespace = self.s.namespace.read().await;
        if p.lease_ids.len() > 1024 {
            return Err(DocumentsError::InvalidInput.into());
        }
        Ok(Renewed {
            lost: self.s.leases.renew(&p.lease_ids),
        })
    }
    pub async fn release(&self, p: LeaseRef) -> R<Done> {
        let _namespace = self.s.namespace.read().await;
        self.s.leases.release(&p.lease_id);
        Ok(self.done())
    }
    pub async fn read(&self, p: ReadDocument) -> R<DiskDocument> {
        let kind = p.policy.map_or(DocKind::Note, |p| p.kind);
        self.read_disk(&p.key, kind).await
    }

    /// Locks the lease's current key; retries when a rename moved it meanwhile.
    pub(crate) async fn lock_lease(&self, id: &str) -> R<(LeaseView, OwnedMutexGuard<()>)> {
        loop {
            let lease = self.s.leases.get(id).map_err(lost)?;
            let guard = self.s.leases.lock(&lease.key).await;
            let again = self.s.leases.get(id).map_err(lost)?;
            if again.key == lease.key {
                return Ok((again, guard));
            }
        }
    }
    pub(crate) fn check_text(&self, text: &str, kind: DocKind) -> R<()> {
        if text.contains('\0') {
            return Err(DocumentsError::InvalidInput.into());
        }
        if text.len() as u64 > kind.limit() {
            return Err(DocumentsError::TooLarge {
                limit: kind.limit(),
            }
            .into());
        }
        if escaped_len(text) > self.s.max_wire {
            return Err(DocumentsError::TooLarge {
                limit: self.s.max_wire as u64,
            }
            .into());
        }
        Ok(())
    }

    pub async fn save(&self, p: SaveDocument) -> R<Saved> {
        let _namespace = self.s.namespace.read().await;
        let (lease, _guard) = self.lock_lease(&p.lease_id).await?;
        self.check_text(&p.text, lease.kind)?;
        let bytes = encode_text(&p.text, lease.eol, lease.bom);
        if bytes.len() as u64 > lease.kind.limit() {
            return Err(DocumentsError::TooLarge {
                limit: lease.kind.limit(),
            }
            .into());
        }
        let Located::At(r) = self.locate(lease.key.clone(), true, false).await? else {
            return Err(DocumentsError::NotFound.into());
        };
        if r.key != lease.key {
            return Err(DocumentsError::Conflict { rev: None }.into());
        }
        #[cfg(test)]
        let stop = self.s.hooks.after_tmp_written.as_ref().is_some_and(|f| f());
        #[cfg(not(test))]
        let stop = false;
        let (s, seq) = (self.s.clone(), p.seq);
        let (rev, key, abs) = blocking(move || {
            let rev = write_checked(&s, &r, &bytes, p.expect, p.base_rev, stop)?;
            if s.journals.drop_upto(&r.key, seq).is_err() {
                tracing::warn!("saved document journal was not removed");
            }
            Ok((rev, r.key.clone(), r.abs()))
        })
        .await?;
        #[cfg(test)]
        if let Some(hook) = &self.s.hooks.before_save_watch {
            hook().await;
        }
        // Also starts watching a document whose folder did not exist at claim, but
        // only while the lease lives (eviction removes watches under the same lock).
        self.s.leases.while_live(&lease.id, &key, || {
            self.s.watch.add(&key, abs, Some(rev.hash.clone()));
        });
        self.s.leases.saved(&lease.id, p.seq);
        Ok(Saved { rev })
    }

    pub async fn journal(&self, p: WriteJournal) -> R<Done> {
        let _namespace = self.s.namespace.read().await;
        let (lease, _guard) = self.lock_lease(&p.lease_id).await?;
        self.check_text(&p.text, lease.kind)?;
        let s = self.s.clone();
        blocking(move || {
            let current = s
                .journals
                .load(&lease.key)
                .internal("read document journal")?;
            if p.seq <= lease.saved_seq || current.is_some_and(|j| p.seq <= j.seq) {
                return Err(DocumentsError::StaleJournal.into());
            }
            let journal = JournalStore::create(&lease.key, p.seq, p.base_rev, p.text);
            s.journals
                .write(&journal)
                .internal("write document journal")
        })
        .await?;
        Ok(self.done())
    }
    pub async fn journal_drop(&self, p: DropJournal) -> R<Done> {
        let _namespace = self.s.namespace.read().await;
        validate(&p.key).map_err(scope_fail)?;
        let _guard = self.s.leases.lock(&p.key).await;
        // Only the lease holder (or anyone, once nobody holds it) may drop the journal.
        match self.s.leases.holder(&p.key) {
            Some(id) if p.lease_id.as_deref() != Some(id.as_str()) => {
                return Err(lost(LeaseLoss::Taken));
            }
            // A journal write still in flight at or below the drop is stale from now on.
            Some(id) => self.s.leases.saved(&id, p.upto_seq),
            None => {}
        }
        let s = self.s.clone();
        blocking(move || {
            s.journals
                .drop_upto(&p.key, p.upto_seq)
                .internal("drop document journal")
        })
        .await?;
        Ok(self.done())
    }
    pub async fn journal_read(&self, p: LeaseRef) -> R<JournalText> {
        let _namespace = self.s.namespace.read().await;
        let (lease, _guard) = self.lock_lease(&p.lease_id).await?;
        let s = self.s.clone();
        let journal = blocking(move || {
            s.journals
                .load(&lease.key)
                .internal("read document journal")
        })
        .await?;
        let journal = journal.ok_or(DocumentsError::NotFound)?;
        if escaped_len(&journal.text) > self.s.max_wire {
            return Err(DocumentsError::TooLarge {
                limit: self.s.max_wire as u64,
            }
            .into());
        }
        Ok(JournalText {
            seq: journal.seq,
            base_rev: journal.base_rev,
            text: journal.text,
        })
    }

    /// Current disk content without a lease (comparison views, other crates).
    pub async fn read_disk(&self, key: &DocKey, kind: DocKind) -> R<DiskDocument> {
        let _namespace = self.s.namespace.read().await;
        let located = self.locate(key.clone(), false, false).await?;
        let o = self.opened(located, kind, false).await?;
        let present = o.file.is_some();
        Ok(DiskDocument {
            key: o.located.key().clone(),
            rev: o.file.map(|f| f.rev),
            text: o.content.text.filter(|_| present),
            read_only: o.content.read_only.map(|reason| ReadOnly { reason }),
        })
    }
}
/// CAS check and publish under the caller's document lock.
pub(crate) fn write_checked(
    s: &Shared,
    r: &Resolved,
    bytes: &[u8],
    expect: Expect,
    base: Option<DiskRev>,
    stop: bool,
) -> R<DiskRev> {
    let current = sys::read_at(&r.dir, &r.name, 0).map_err(sys_fail)?;
    let base = match (expect, current) {
        (Expect::Absent, Some(_)) => return Err(DocumentsError::Exists.into()),
        (Expect::Absent, None) => None,
        (Expect::Present, None) => return Err(DocumentsError::Conflict { rev: None }.into()),
        (Expect::Present, Some(cur)) => match base {
            Some(base) if base.same_content(&cur.rev) => Some(cur.rev),
            _ => return Err(DocumentsError::Conflict { rev: Some(cur.rev) }.into()),
        },
    };
    sys::atomic_replace(
        &r.dir,
        &r.dir_path,
        &r.name,
        bytes,
        base.as_ref(),
        &s.ledger,
        &sys::Probe {
            stop_after_tmp: stop,
            after_exchange: None,
            after_undo: None,
        },
    )
    .map_err(sys_fail)
}
