//! Read-only authority check for the LSP adapter; no journal or editor mutation.
use crate::*;
impl DocumentsService {
    pub async fn code_authority(&self, key: &DocKey, lease_id: Option<&str>) -> R<DiskDocument> {
        if !key.root.starts_with("project:") {
            return Err(DocumentsError::OutOfScope.into());
        }
        let disk = self.read_disk(key, DocKind::Code).await?;
        if let Some(id) = lease_id {
            let (lease, _guard) = self.lock_lease(id).await?;
            if lease.kind != DocKind::Code || lease.key != disk.key || disk.read_only.is_some() {
                return Err(DocumentsError::LeaseLost {
                    reason: LeaseLoss::Taken,
                }
                .into());
            }
        }
        Ok(disk)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture::Fx;
    use tokio_util::sync::CancellationToken;
    #[tokio::test]
    async fn code_authority_requires_live_code_lease_for_exact_canonical_key() {
        let fx = Fx::new();
        std::fs::write(fx.project.join("a.rs"), "a").expect("fixture");
        std::fs::write(fx.project.join("b.rs"), "b").expect("fixture");
        let a = DocKey {
            root: "project:p".into(),
            path: "a.rs".into(),
        };
        let b = DocKey {
            path: "b.rs".into(),
            ..a.clone()
        };
        let owner = CancellationToken::new();
        let claimed = fx
            .svc
            .claim(
                owner.clone(),
                ClaimDocument {
                    key: a.clone(),
                    policy: DocPolicy {
                        kind: DocKind::Code,
                    },
                    window_id: "one".into(),
                },
            )
            .await
            .expect("claim")
            .expect("live");
        let lease = claimed.lease_id.expect("lease");
        assert_eq!(
            fx.svc
                .code_authority(&a, Some(&lease))
                .await
                .expect("owner")
                .key,
            a
        );
        assert!(fx.svc.code_authority(&b, Some(&lease)).await.is_err());
        assert!(fx.svc.code_authority(&a, Some("unknown")).await.is_err());
        assert!(fx.svc.code_authority(&a, None).await.is_ok());
        owner.cancel();
        assert!(fx.svc.code_authority(&a, Some(&lease)).await.is_err());
        let note = fx
            .svc
            .claim(
                CancellationToken::new(),
                ClaimDocument {
                    key: a.clone(),
                    policy: DocPolicy {
                        kind: DocKind::Note,
                    },
                    window_id: "note".into(),
                },
            )
            .await
            .expect("claim")
            .expect("live");
        assert!(
            fx.svc
                .code_authority(&a, note.lease_id.as_deref())
                .await
                .is_err()
        );
    }
    #[tokio::test]
    async fn code_authority_rejects_symlink_escape_and_uses_canonical_key() {
        let fx = Fx::new();
        std::fs::write(fx.project.join("a.rs"), "a").expect("fixture");
        std::fs::write(fx.outside.join("secret.rs"), "secret").expect("fixture");
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(fx.project.join("a.rs"), fx.project.join("alias.rs"))
                .expect("alias");
            std::os::unix::fs::symlink(fx.outside.join("secret.rs"), fx.project.join("escape.rs"))
                .expect("alias");
            let key = |path: &str| DocKey {
                root: "project:p".into(),
                path: path.into(),
            };
            assert_eq!(
                fx.svc
                    .code_authority(&key("alias.rs"), None)
                    .await
                    .expect("canonical")
                    .key,
                key("a.rs")
            );
            assert!(
                fx.svc
                    .code_authority(&key("escape.rs"), None)
                    .await
                    .is_err()
            );
        }
    }
}
