//! Startup and periodic housekeeping: journal reconciliation, temp-file sweeps
//! and the vault watch.
use crate::journal::JournalFile;
use crate::ledger::{STALE, sweep_dir};
use crate::model::DocKey;
use crate::scope::WriteScope;
use crate::service::{DocumentsService, Shared, VAULT};
use std::sync::Arc;
use std::time::Duration;

impl DocumentsService {
    /// Starts watching and sweeps crash leftovers; the handle ends after the
    /// startup sweep (the hourly ledger sweep keeps running).
    pub fn start(&self) -> tokio::task::JoinHandle<()> {
        self.s.watch.start();
        let s = self.s.clone();
        tokio::spawn(async move {
            let roots = (s.roots)().await;
            let sweep = s.clone();
            let scope = tokio::task::spawn_blocking(move || {
                let scope = WriteScope::new(roots).deny(&sweep.private);
                sweep.reconcile_journals(&scope);
                sweep.ledger.sweep(&scope, STALE);
                if let Some(vault) = scope.root(VAULT) {
                    sweep_dir(&scope, vault, true);
                }
                sweep.dirs.sweep(&scope);
                scope
            })
            .await;
            if let Ok(scope) = scope
                && let Some(vault) = scope.root(VAULT)
            {
                s.watch.watch_vault(VAULT, vault);
            }
            let weak = Arc::downgrade(&s);
            drop(s);
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_secs(3600)).await;
                    let Some(s) = weak.upgrade() else { break };
                    let roots = (s.roots)().await;
                    let _ = tokio::task::spawn_blocking(move || {
                        s.ledger
                            .sweep(&WriteScope::new(roots).deny(&s.private), STALE)
                    })
                    .await;
                }
            });
        })
    }
}

impl Shared {
    /// A crash between a move and its old-key journal removal leaves two copies of
    /// the same journal; keep only those whose document exists.
    fn reconcile_journals(&self, scope: &WriteScope) {
        let Ok(all) = self.journals.list() else {
            return;
        };
        let exists = |k: &DocKey| {
            scope
                .resolve_entry(k, false)
                .is_ok_and(|r| r.kind.is_some())
        };
        for (i, a) in all.iter().enumerate() {
            let twins: Vec<&JournalFile> = all
                .iter()
                .enumerate()
                .filter(|(j, b)| {
                    *j != i && b.seq == a.seq && b.text == a.text && b.base_rev == a.base_rev
                })
                .map(|(_, b)| b)
                .collect();
            if !twins.is_empty() && !exists(&a.key) && twins.iter().any(|b| exists(&b.key)) {
                let _ = self.journals.remove(&a.key);
            }
        }
    }
}
