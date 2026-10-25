//! Ephemeral, bounded code provenance. This is a covered set, not a project snapshot.
use crate::{
    DiskRev, DocKey, DocKind, DocumentsError, DocumentsService, R, WriteScope,
    capture_budget::{Budget, Limits},
    capture_io, is_internal,
    service::blocking,
};
use rustix::fs::{Dir, FileType};
use std::{
    collections::{BTreeSet, VecDeque},
    path::PathBuf,
    time::Instant,
};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureReason {
    Cancelled,
    Deadline,
    FileLimit,
    ByteLimit,
    EntryLimit,
    MetadataLimit,
    TooLarge,
    Unavailable,
    Changed,
    Binary,
    Encoding,
    OutOfScope,
}
#[derive(Debug, Clone)]
pub struct CodeBaseline {
    pub key: DocKey,
    pub rev: DiskRev,
    pub text_hash: String,
}
#[derive(Debug, Clone)]
pub struct CodeOmission {
    pub key: DocKey,
    pub reason: CaptureReason,
}
#[derive(Debug, Clone)]
pub struct CodeCapture {
    pub root: PathBuf,
    pub covered: Vec<CodeBaseline>,
    pub omitted: Vec<CodeOmission>,
    pub stopped: Option<CaptureReason>,
    pub bytes_read: u64,
    pub entries_scanned: usize,
}

impl DocumentsService {
    /// Capture before dispatching a request. The caller must supervise this future
    /// through completion: cancellation is cooperative and never detaches its worker.
    /// Priority keys come only from a previous refused response, not an old edit replay.
    pub async fn capture_code(
        &self,
        root_id: String,
        expected_root: PathBuf,
        priority: Vec<DocKey>,
        cancel: CancellationToken,
    ) -> R<CodeCapture> {
        self.capture_code_with(root_id, expected_root, priority, cancel, Limits::default())
            .await
    }

    async fn capture_code_with(
        &self,
        root_id: String,
        expected_root: PathBuf,
        priority: Vec<DocKey>,
        cancel: CancellationToken,
        limits: Limits,
    ) -> R<CodeCapture> {
        let started = Instant::now();
        if !root_id.starts_with("project:") || priority.len() > 256 {
            return Err(DocumentsError::InvalidInput.into());
        }
        let probe = DocKey {
            root: root_id.clone(),
            path: "__devbox_capture__".into(),
        };
        let scoped = self.policy_scope(&probe, DocKind::Code).await;
        let scope = scoped.scope().await?;
        if scope.root(&root_id) != Some(expected_root.as_path()) {
            return Err(DocumentsError::OutOfScope.into());
        }
        blocking(move || {
            let budget = Budget::new(limits, cancel, started);
            Ok(Scanner::new(scope, root_id, expected_root, budget).run(priority))
        })
        .await
    }
}

struct Scanner {
    scope: WriteScope,
    root_id: String,
    result: CodeCapture,
    budget: Budget,
    seen: BTreeSet<DocKey>,
}
impl Scanner {
    fn new(scope: WriteScope, root_id: String, root: PathBuf, budget: Budget) -> Self {
        Self {
            scope,
            root_id,
            budget,
            seen: BTreeSet::new(),
            result: CodeCapture {
                root,
                covered: Vec::new(),
                omitted: Vec::new(),
                stopped: None,
                bytes_read: 0,
                entries_scanned: 0,
            },
        }
    }
    fn omission(&mut self, key: DocKey, reason: CaptureReason) {
        self.result.omitted.push(CodeOmission { key, reason });
    }
    fn folder_omission(&mut self, folder: &str, reason: CaptureReason) {
        if folder.is_empty() {
            self.budget.stop(reason);
        } else if self
            .budget
            .metadata(256 + self.root_id.len() + folder.len())
            .is_ok()
        {
            self.omission(
                DocKey {
                    root: self.root_id.clone(),
                    path: folder.trim_end_matches('/').into(),
                },
                reason,
            );
        }
    }
    fn file(&mut self, key: DocKey) {
        if self.budget.check().is_err() || self.seen.contains(&key) {
            return;
        }
        // Charge both requested and canonical keys, BTree/Vec bookkeeping and
        // hashes/revision strings before retaining any metadata. No refund is
        // needed: this conservative cumulative limit also bounds transient queues.
        if self
            .budget
            .metadata(512 + 4 * (key.root.len() + key.path.len()))
            .is_err()
        {
            return;
        }
        if key.root != self.root_id || is_internal(&key.path) {
            self.omission(key, CaptureReason::OutOfScope);
            return;
        }
        let at = match self.scope.resolve(&key, false) {
            Ok(at) if at.key.root == self.root_id && !is_internal(&at.key.path) => at,
            _ => {
                self.omission(key, CaptureReason::OutOfScope);
                return;
            }
        };
        if self.seen.contains(&at.key) {
            return;
        }
        if self.budget.metadata(4 * at.key.path.len()).is_err() {
            return;
        }
        self.seen.insert(at.key.clone());
        if let Err(reason) = self.budget.file() {
            self.omission(at.key, reason);
            return;
        }
        let baseline =
            capture_io::bounded_read(&at.dir, &at.name, &mut self.budget).and_then(|file| {
                let text_hash = capture_io::text_hash(
                    file.bytes.as_deref().unwrap_or_default(),
                    &mut self.budget,
                )?;
                Ok(CodeBaseline {
                    key: at.key.clone(),
                    rev: file.rev,
                    text_hash,
                })
            });
        match baseline {
            Ok(baseline) => self.result.covered.push(baseline),
            Err(reason) => self.omission(at.key, reason),
        }
    }
    fn run(mut self, priority: Vec<DocKey>) -> CodeCapture {
        for key in priority {
            self.file(key);
            if self.budget.check().is_err() {
                break;
            }
        }
        let mut folders = VecDeque::from([String::new()]);
        while let Some(folder) = folders.pop_front() {
            if self.budget.check().is_err() {
                break;
            }
            let probe = DocKey {
                root: self.root_id.clone(),
                path: format!("{folder}__devbox_capture__"),
            };
            // Enumerate the checked descriptor, never re-open its pathname after
            // scope validation (a directory may have been replaced by a symlink).
            let Ok(at) = self.scope.resolve_entry(&probe, false) else {
                self.folder_omission(&folder, CaptureReason::OutOfScope);
                continue;
            };
            if at.key.root != self.root_id || is_internal(&at.key.path) {
                self.folder_omission(&folder, CaptureReason::OutOfScope);
                continue;
            }
            let Ok(mut dir) = Dir::read_from(&at.dir) else {
                self.folder_omission(&folder, CaptureReason::Unavailable);
                continue;
            };
            loop {
                // Reserve an entry before readdir, so cancellation/deadline and
                // traversal limits are checked before the next filesystem read.
                if self.budget.entry().is_err() {
                    break;
                }
                let Some(row) = dir.next() else {
                    self.budget.entries -= 1;
                    break;
                };
                let Ok(row) = row else {
                    self.folder_omission(&folder, CaptureReason::Unavailable);
                    break;
                };
                let Ok(name) = row.file_name().to_str() else {
                    continue;
                };
                if matches!(name, "." | "..") || is_internal(name) {
                    continue;
                }
                if self
                    .budget
                    .metadata(128 + 2 * (folder.len() + name.len()))
                    .is_err()
                {
                    break;
                }
                let key = DocKey {
                    root: self.root_id.clone(),
                    path: format!("{folder}{name}"),
                };
                if key.path.len() > 4096 {
                    continue;
                }
                match row.file_type() {
                    FileType::Directory => folders.push_back(format!("{}/", key.path)),
                    FileType::RegularFile | FileType::Symlink => self.file(key),
                    _ => {}
                }
            }
        }
        self.result.bytes_read = self.budget.bytes_read;
        self.result.entries_scanned = self.budget.entries;
        self.result.stopped = self.budget.stopped;
        self.result
    }
}

#[cfg(test)]
#[path = "capture_tests.rs"]
mod tests;
