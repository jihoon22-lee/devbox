use crate::*;
use std::path::{Component, Path};

impl ProjectsService {
    #[cfg(target_os = "linux")]
    pub async fn clone_repository(&self, p: CloneProject) -> R<Project> {
        let name = repository_name(&p.url).ok_or(ProjectsError::CloneInvalidUrl)?;
        let target = p
            .dir
            .map(|dir| expand_home(&dir, &self.home))
            .unwrap_or_else(|| self.home.join("projects").join(name));
        if !target.is_absolute()
            || target
                .components()
                .any(|part| matches!(part, Component::ParentDir))
        {
            return Err(ProjectsError::PathInvalid {
                path: target.display().to_string(),
            }
            .into());
        }
        let reservation =
            tokio::task::spawn_blocking(move || clone_target::CloneTarget::reserve(&target))
                .await
                .internal("reserve clone directory")??;
        let path = reservation
            .path
            .to_str()
            .ok_or_else(|| ProjectsError::PathInvalid {
                path: reservation.path.display().to_string(),
            })?
            .to_owned();
        let args = [
            "-c",
            "protocol.allow=never",
            "-c",
            "protocol.http.allow=always",
            "-c",
            "protocol.https.allow=always",
            "-c",
            "protocol.ssh.allow=always",
            "-c",
            "protocol.git.allow=always",
            "-c",
            "protocol.file.allow=always",
            "-c",
            "credential.interactive=false",
            "-c",
            "core.askPass=",
            "clone",
            "--template=",
            "--",
            &p.url,
            ".",
        ];
        // The working directory is the reserved inode, never the caller's replaceable path.
        // A failed/cancelled clone preserves all exposed resources for inspection.
        match devbox_git::run::clone_output(&reservation.directory, &args).await {
            Ok((0, _, _)) => (),
            Ok((_, _, _)) => {
                return Err(ProjectsError::CloneFailed {
                    path,
                    message: "Git clone failed; check the repository URL, access, and destination."
                        .into(),
                }
                .into());
            }
            Err(error) => {
                return Err(ProjectsError::CloneFailed {
                    path,
                    message: format!(
                        "Git clone stopped ({})",
                        devbox_protocol::to_wire(&error).code
                    ),
                }
                .into());
            }
        }
        self.register_clone(reservation).await
    }

    #[cfg(target_os = "linux")]
    pub(crate) async fn register_clone(
        &self,
        reservation: clone_target::CloneTarget,
    ) -> R<Project> {
        reservation.verify()?;
        let path = reservation
            .path
            .to_str()
            .ok_or_else(|| ProjectsError::PathInvalid {
                path: reservation.path.display().to_string(),
            })?
            .to_owned();
        let project = Project {
            id: uuid::Uuid::new_v4().simple().to_string(),
            name: reservation
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| ProjectsError::PathInvalid { path: path.clone() })?
                .to_owned(),
            path,
            favorite: false,
            added_ms: now_ms(),
        };
        let row = project.clone();
        let hub = self.hub.clone();
        self.db
            .write_checked_with_commit(
                move |tx| {
                    // Construct the row from the pinned reservation; projects.add would reopen a
                    // pathname that could have changed after the pre-registration verification.
                    if let Err(error) = reservation.verify() {
                        return Ok(Err(error));
                    }
                    if store::exists_path(tx, &row.path)? {
                        return Ok(Err(ProjectsError::AlreadyExists {
                            path: row.path.clone(),
                        }));
                    }
                    store::insert(tx, &row)?;
                    if let Err(error) = reservation.verify() {
                        return Ok(Err(error));
                    }
                    Ok(Ok(()))
                },
                move |_, rev| publish_changed(&hub, rev),
            )
            .await
            .internal("register cloned project")??;
        Ok(project)
    }

    #[cfg(not(target_os = "linux"))]
    pub async fn clone_repository(&self, _: CloneProject) -> R<Project> {
        Err(ProjectsError::CloneFailed {
            path: String::new(),
            message: "Project cloning requires the Linux daemon.".into(),
        }
        .into())
    }
}

fn repository_name(url: &str) -> Option<&str> {
    if url.is_empty() || url.starts_with('-') || url.chars().any(char::is_control) {
        return None;
    }
    if let Some((scheme, remainder)) = url.split_once("://") {
        if !matches!(scheme, "http" | "https" | "ssh" | "git" | "file") {
            return None;
        }
        let authority = remainder.split('/').next()?;
        // HTTP usernames may themselves contain tokens. Never persist URI secrets as
        // remote.origin.url; noninteractive helpers provide HTTPS authentication instead.
        if let Some((userinfo, _)) = authority.rsplit_once('@')
            && (scheme != "ssh" || userinfo.is_empty() || userinfo.contains([':', '%']))
        {
            return None;
        }
    } else {
        if url.contains("::") || (!Path::new(url).is_absolute() && !url.contains(':')) {
            return None;
        }
    }
    let last = url.trim_end_matches('/').rsplit(['/', ':']).next()?;
    let name = last.strip_suffix(".git").unwrap_or(last);
    if name.is_empty() || matches!(name, "." | "..") || name.contains(['\\', '?', '#']) {
        None
    } else {
        Some(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clone_urls_cannot_supply_secrets_options_or_remote_helpers() {
        for url in [
            "",
            "--upload-pack=evil",
            "ext::evil",
            "https::evil",
            "evil://host/repo",
            "relative/repo",
            "https://host/../",
            "https://host/a?x",
            "https://host/a\n",
            "https://token@host/repo.git",
            "https://user:password@host/repo.git",
            "http://token@host/repo.git",
            "ssh://git:password@host/repo.git",
            "file://user:password@localhost/tmp/repo.git",
        ] {
            assert!(repository_name(url).is_none(), "fixture URL was accepted");
        }
        for url in [
            "http://host/repo.git",
            "https://host/repo.git",
            "https://[::1]/repo.git",
            "ssh://git@host/repo.git",
            "git@host:repo.git",
            "/tmp/repo.git",
            "file:///tmp/repo.git",
        ] {
            assert_eq!(repository_name(url), Some("repo"), "{url}");
        }
    }
}
