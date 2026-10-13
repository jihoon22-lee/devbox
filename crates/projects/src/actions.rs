use crate::*;
use std::path::Path;

impl ProjectsService {
    pub async fn config(&self, p: GetProjectConfig) -> R<ProjectConfigResult> {
        let project = self
            .get(&p.id)
            .await?
            .ok_or(ProjectsError::NotFound { id: p.id })?;
        tokio::task::spawn_blocking(move || match config::load(Path::new(&project.path)) {
            Ok(config) => ProjectConfigResult {
                config,
                error: None,
            },
            Err(error) => ProjectConfigResult {
                config: config::ProjectConfig::default(),
                error: Some(ProjectConfigIssue {
                    message: error.to_string(),
                }),
            },
        })
        .await
        .internal("read project configuration")
    }

    pub async fn prompts(&self, p: ProjectPrompts) -> R<PromptList> {
        let project = self
            .get(&p.id)
            .await?
            .ok_or(ProjectsError::NotFound { id: p.id })?;
        tokio::task::spawn_blocking(move || prompts::read(Path::new(&project.path)))
            .await
            .internal("read project prompts")?
    }

    pub async fn set_favorite(&self, p: SetFavorite) -> R<Done> {
        let hub = self.hub.clone();
        let (_, rev) = self
            .db
            .write_checked_with_commit(
                move |tx| {
                    if store::set_favorite(tx, &p.id, p.favorite)? == 0 {
                        return Ok(Err(ProjectsError::NotFound { id: p.id }));
                    }
                    Ok(Ok(()))
                },
                move |_, rev| publish_changed(&hub, rev),
            )
            .await
            .internal("set project favorite")??;
        Ok(Done {
            rev: rev.to_string(),
        })
    }
}
