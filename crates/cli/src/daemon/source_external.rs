use super::state::Services;
use devbox_projects::{
    FilesError, FilesService, OpenSourceEditor, SourceEditorOpened, editor_arguments,
};
use devbox_protocol::Fail;
use std::{process::Stdio, sync::Arc, time::Duration};
use tokio::process::Command;
pub(super) async fn open(
    s: Arc<Services>,
    p: OpenSourceEditor,
) -> Result<SourceEditorOpened, Fail<FilesError>> {
    let path = FilesService::new(s.documents.clone())
        .external_path(&p.key)
        .await?;
    let distro = std::env::var("WSL_DISTRO_NAME").ok();
    let Ok((program, args)) = editor_arguments(p.editor, distro.as_deref(), &path) else {
        return Ok(SourceEditorOpened {
            opened: false,
            path,
        });
    };
    let opened = tokio::time::timeout(
        Duration::from_secs(8),
        Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .status(),
    )
    .await
    .is_ok_and(|result| result.is_ok_and(|status| status.success()));
    Ok(SourceEditorOpened { opened, path })
}
