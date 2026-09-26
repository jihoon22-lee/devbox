//! Bounded, one-shot receipts for objects created by this Knowledge process.
use super::vault::VaultIdentity;
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};
const MAX_GRANTS: usize = 32;
// UI offers last eight seconds; leave bounded time for an already-clicked IPC.
const TTL: Duration = Duration::from_secs(30);
struct Grant {
    vault: VaultIdentity,
    path: String,
    revision: String,
    issued: Instant,
}
#[derive(Default)]
pub struct CreatedNotes {
    grants: VecDeque<Grant>,
}
impl CreatedNotes {
    pub fn record(&mut self, vault: VaultIdentity, path: String, revision: String) {
        self.record_at(vault, path, revision, Instant::now());
    }
    pub fn take(&mut self, vault: &VaultIdentity, path: &str, revision: &str) -> bool {
        self.take_at(vault, path, revision, Instant::now())
    }
    fn expire(&mut self, now: Instant) {
        self.grants.retain(|grant| {
            now.checked_duration_since(grant.issued)
                .is_some_and(|age| age <= TTL)
        });
    }
    fn record_at(&mut self, vault: VaultIdentity, path: String, revision: String, now: Instant) {
        self.expire(now);
        if self.grants.len() == MAX_GRANTS {
            self.grants.pop_front();
        }
        self.grants.push_back(Grant {
            vault,
            path,
            revision,
            issued: now,
        });
    }
    fn take_at(&mut self, vault: &VaultIdentity, path: &str, revision: &str, now: Instant) -> bool {
        self.expire(now);
        let Some(index) = self
            .grants
            .iter()
            .position(|grant| grant.path == path && grant.revision == revision)
        else {
            return false;
        };
        self.grants
            .remove(index)
            .is_some_and(|grant| &grant.vault == vault && grant.vault.revalidate().is_ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_a_recent_creation_in_the_same_vault_can_be_consumed_once() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let a = VaultIdentity::inspect(first.path()).unwrap();
        let b = VaultIdentity::inspect(second.path()).unwrap();
        let now = Instant::now();
        let mut grants = CreatedNotes::default();
        assert!(!grants.take_at(&a, "Inbox/a.md", "revision", now));
        grants.record_at(a.clone(), "Inbox/a.md".into(), "revision".into(), now);
        assert!(!grants.take_at(&b, "Inbox/a.md", "revision", now));
        grants.record_at(a.clone(), "Inbox/a.md".into(), "revision".into(), now);
        assert!(grants.take_at(&a, "Inbox/a.md", "revision", now));
        assert!(!grants.take_at(&a, "Inbox/a.md", "revision", now));
        grants.record_at(a.clone(), "Inbox/a.md".into(), "revision".into(), now);
        assert!(!grants.take_at(
            &a,
            "Inbox/a.md",
            "revision",
            now + TTL + Duration::from_secs(1)
        ));
    }
    #[test]
    fn creation_receipts_are_bounded_without_deleting_any_notes() {
        let root = tempfile::tempdir().unwrap();
        let vault = VaultIdentity::inspect(root.path()).unwrap();
        let now = Instant::now();
        let mut grants = CreatedNotes::default();
        for index in 0..MAX_GRANTS + 1 {
            grants.record_at(
                vault.clone(),
                format!("{index}.md"),
                format!("r-{index}"),
                now,
            );
        }
        assert!(!grants.take_at(&vault, "0.md", "r-0", now));
        assert!(grants.take_at(
            &vault,
            &format!("{MAX_GRANTS}.md"),
            &format!("r-{MAX_GRANTS}"),
            now
        ));
    }
}
