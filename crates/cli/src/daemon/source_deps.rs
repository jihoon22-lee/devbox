//! Dependency reads retain the registered document scope and agent exclusions.
use super::state::Services;
use devbox_deps::{
    input::{self, Reader},
    model::*,
    osv::{Auditor, ENDPOINT, NetworkHttp},
    rpc::*,
    service::DepsService,
};
use devbox_documents::{DocKey, DocumentsError};
use devbox_protocol::{Fail, Router};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
static REV: AtomicU64 = AtomicU64::new(0);
pub(super) fn service(hub: &Arc<devbox_core::Hub>) -> Arc<DepsService> {
    let hub = hub.clone();
    DepsService::new(
        Auditor::new(
            Arc::new(NetworkHttp),
            Arc::new(|| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0)
            }),
            ENDPOINT.into(),
        ),
        Arc::new(move |event| {
            hub.publish::<DepsAuditChanged>(REV.fetch_add(1, Ordering::SeqCst) + 1, &event)
        }),
    )
}
async fn reader(s: &Services, project_id: &str) -> Result<Reader, Fail<DepsError>> {
    if s.projects
        .get(project_id)
        .await
        .map_err(|_| DepsError::InvalidProject)?
        .is_none()
    {
        return Err(DepsError::InvalidProject.into());
    }
    let scope = s
        .documents
        .scope()
        .await
        .map_err(|_| DepsError::UnsafePath)?;
    let agents = scope
        .roots()
        .filter(|(id, _)| id.starts_with("agents:"))
        .map(|(_, p)| p.to_path_buf())
        .collect();
    let root = format!("project:{project_id}");
    let documents = Arc::new(s.documents.scoped_to(&root).excluding(agents));
    Ok(Arc::new(move |path| {
        let documents = documents.clone();
        let root = root.clone();
        Box::pin(async move {
            let key = DocKey {
                root: root.clone(),
                path,
            };
            let result = documents
                .read_bytes(&key, devbox_deps::locks::MAX_BYTES as u64)
                .await
                .map_err(|e| match e {
                    Fail::Domain(DocumentsError::TooLarge { .. }) => DepsError::LimitExceeded,
                    Fail::Domain(DocumentsError::OutOfScope | DocumentsError::InvalidInput) => {
                        DepsError::UnsafePath
                    }
                    _ => DepsError::ReadFailed,
                })?;
            match result {
                Some((key, bytes)) if key.root == root => Ok(Some(bytes)),
                None => Ok(None),
                _ => Err(DepsError::UnsafePath),
            }
        })
    }))
}
pub(super) fn register(router: Router<Services>) -> Router<Services> {
    router
        .register::<DepsList, _, _>(|s, _, p| async move {
            input::load(reader(&s, &p.project_id).await?)
                .await
                .map_err(Into::into)
        })
        .register::<DepsAudit, _, _>(|s, _, p| async move {
            let reader = reader(&s, &p.project_id).await?;
            let inventory = input::load(reader.clone()).await?;
            s.deps
                .start(p.project_id, inventory, reader)
                .map_err(Into::into)
        })
        .register::<DepsAuditStatus, _, _>(|s, _, p| async move {
            let _ = reader(&s, &p.project_id).await?;
            s.deps.status(&p.project_id, &p.id).map_err(Into::into)
        })
}
#[cfg(test)]
#[path = "source_deps_tests.rs"]
mod tests;
