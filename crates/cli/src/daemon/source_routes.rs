//! Source RPC composition. File operations share the existing DocumentsService.
use super::state::Services;
use devbox_projects::{FilesCreate, FilesDelete, FilesService, FilesStat, FilesTree};
use devbox_protocol::Router;
pub(super) fn register(router: Router<Services>) -> Router<Services> {
    router
        .register::<devbox_projects::FilesPreview, _, _>(|s, _, p| async move {
            FilesService::new(s.documents.clone()).preview(p).await
        })
        .register::<devbox_projects::FilesExternalEditor, _, _>(|s, _, p| async move {
            super::source_external::open(s, p).await
        })
        .register::<FilesTree, _, _>(|s, _, p| async move {
            FilesService::new(s.documents.clone()).tree(p).await
        })
        .register::<FilesStat, _, _>(|s, _, p| async move {
            FilesService::new(s.documents.clone()).stat(p).await
        })
        .register::<FilesCreate, _, _>(|s, _, p| async move {
            FilesService::new(s.documents.clone()).create(p).await
        })
        .register::<FilesDelete, _, _>(|s, _, p| async move {
            FilesService::new(s.documents.clone()).delete(p).await
        })
}
