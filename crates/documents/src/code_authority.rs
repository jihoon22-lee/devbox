//! Read-only authority check for the LSP adapter; no journal or editor mutation.
use crate::*;
use crate::{
    capture_budget::{Budget, Limits},
    service::{blocking, scope_fail},
};
use std::{path::Path, time::Instant};
use tokio_util::sync::CancellationToken;

impl DocumentsService {
    pub async fn code_authority(&self, key: &DocKey, lease_id: Option<&str>) -> R<DiskDocument> {
        self.code_authority_checked(key, lease_id, None, CancellationToken::new())
            .await
    }

    /// LSP access remains bound to the stream's original canonical project root.
    /// This bounded read does not change ordinary documents.read/CAS semantics.
    pub async fn code_authority_at(
        &self,
        key: &DocKey,
        lease_id: Option<&str>,
        expected_root: &Path,
        cancel: CancellationToken,
    ) -> R<DiskDocument> {
        self.code_authority_checked(key, lease_id, Some(expected_root), cancel)
            .await
    }

    async fn code_authority_checked(
        &self,
        key: &DocKey,
        lease_id: Option<&str>,
        expected_root: Option<&Path>,
        cancel: CancellationToken,
    ) -> R<DiskDocument> {
        let started = Instant::now();
        let _namespace = self.s.namespace.read().await;
        if !key.root.starts_with("project:") {
            return Err(DocumentsError::OutOfScope.into());
        }
        let scoped = self.policy_scope(key, DocKind::Code).await;
        let scope = scoped.scope().await?;
        if expected_root.is_some_and(|root| scope.root(&key.root) != Some(root)) {
            return Err(DocumentsError::OutOfScope.into());
        }
        let key = key.clone();
        let max_wire = self.s.max_wire;
        let disk = blocking(move || {
            let mut budget = Budget::new(
                Limits {
                    bytes: CODE_LIMIT,
                    ..Limits::default()
                },
                cancel,
                started,
            );
            budget.check().map_err(capture_fail)?;
            let at = match scope.resolve(&key, false) {
                Ok(at) => at,
                Err(scope::ScopeError::Missing(key)) if !is_internal(&key.path) => {
                    return Ok(DiskDocument {
                        key,
                        rev: None,
                        text: Some(String::new()),
                        read_only: None,
                    });
                }
                Err(error) => return Err(scope_fail(error)),
            };
            if is_internal(&at.key.path) {
                return Err(DocumentsError::OutOfScope.into());
            }
            budget.check().map_err(capture_fail)?;
            if at.kind.is_none() {
                return Ok(DiskDocument {
                    key: at.key,
                    rev: None,
                    text: Some(String::new()),
                    read_only: None,
                });
            }
            let file =
                capture_io::bounded_read(&at.dir, &at.name, &mut budget).map_err(capture_fail)?;
            let content = sys::classify(&file, max_wire);
            budget.check().map_err(capture_fail)?;
            Ok(DiskDocument {
                key: at.key,
                rev: Some(file.rev),
                text: content.text,
                read_only: content.read_only.map(|reason| ReadOnly { reason }),
            })
        })
        .await?;
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

fn capture_fail(reason: CaptureReason) -> devbox_protocol::Fail<DocumentsError> {
    match reason {
        CaptureReason::TooLarge | CaptureReason::ByteLimit => {
            DocumentsError::TooLarge { limit: CODE_LIMIT }.into()
        }
        CaptureReason::Changed => DocumentsError::Conflict { rev: None }.into(),
        CaptureReason::Unavailable => DocumentsError::NotFound.into(),
        _ => DocumentsError::InvalidInput.into(),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture::Fx;
    use tokio_util::sync::CancellationToken;
    #[tokio::test]
    async fn code_authority_caps_io_and_checks_stream_root_and_cancellation() {
        let fx = Fx::new();
        let large = std::fs::File::create(fx.project.join("large.rs")).expect("fixture");
        large
            .set_len(CODE_LIMIT + 1)
            .expect("sparse oversized fixture");
        let key = DocKey {
            root: "project:p".into(),
            path: "large.rs".into(),
        };
        assert!(matches!(
            fx.svc.code_authority(&key, None).await,
            Err(devbox_protocol::Fail::Domain(DocumentsError::TooLarge {
                limit: CODE_LIMIT
            }))
        ));
        std::fs::write(fx.project.join("a.rs"), "a").expect("fixture");
        let key = DocKey {
            path: "a.rs".into(),
            ..key
        };
        assert!(
            fx.svc
                .code_authority_at(&key, None, &fx.outside, CancellationToken::new())
                .await
                .is_err()
        );
        let cancel = CancellationToken::new();
        cancel.cancel();
        assert!(
            fx.svc
                .code_authority_at(&key, None, &fx.project, cancel)
                .await
                .is_err()
        );
        let disk = fx
            .svc
            .code_authority_at(&key, None, &fx.project, CancellationToken::new())
            .await
            .expect("bounded authority");
        assert_eq!(disk.text.as_deref(), Some("a"));
        assert!(disk.rev.is_some());
        std::fs::remove_file(fx.project.join("a.rs")).expect("delete fixture");
        let missing = fx
            .svc
            .code_authority_at(&key, None, &fx.project, CancellationToken::new())
            .await
            .expect("new document authority");
        assert!(missing.rev.is_none());
        assert_eq!(missing.text.as_deref(), Some(""));
    }
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
