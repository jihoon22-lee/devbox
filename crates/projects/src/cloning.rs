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
            // Checkout precedes project consent, including globally configured hooks.
            "-c",
            "core.hooksPath=/dev/null",
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
            Ok((_, _, stderr)) => {
                return Err(ProjectsError::CloneFailed {
                    path,
                    message: clone_failure_message(&stderr),
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

fn clone_failure_message(stderr: &str) -> String {
    const LIMIT: usize = 4096;
    let mut message = String::new();
    for line in stderr.lines() {
        let line: String = line
            .chars()
            .filter(|character| !character.is_control())
            .collect();
        let diagnostic = if line.to_ascii_lowercase().contains("authorization:") {
            "[redacted authentication header]".to_owned()
        } else {
            let mut words = Vec::new();
            for word in line.split_whitespace() {
                if let Some((key, _)) = word.split_once(['=', ':'])
                    && matches!(
                        key.trim_start_matches(['-', '\'', '"'])
                            .to_ascii_lowercase()
                            .as_str(),
                        "password" | "passwd" | "token" | "access_token" | "secret" | "api_key"
                    )
                {
                    // Helpers may emit quoted credentials containing spaces. Do not
                    // expose the rest of that line after recognizing a credential field.
                    words.push(format!("{key}=[redacted]"));
                    break;
                }
                words.push(redact_clone_url(word));
            }
            words.join(" ")
        };
        if diagnostic.is_empty() {
            continue;
        }
        if !message.is_empty() {
            message.push(' ');
        }
        message.push_str(&diagnostic);
        if message.len() > LIMIT {
            let mut end = LIMIT - '…'.len_utf8();
            while !message.is_char_boundary(end) {
                end -= 1;
            }
            message.truncate(end);
            message.push('…');
            break;
        }
    }
    if message.is_empty() {
        "Git clone failed; check the repository URL, access, and destination.".into()
    } else {
        message
    }
}

fn redact_clone_url(word: &str) -> String {
    let Some(scheme) = word.find("://") else {
        return word.to_owned();
    };
    let (prefix, remainder) = word.split_at(scheme + 3);
    let end = remainder.find(['/', '?', '#']).unwrap_or(remainder.len());
    let (authority, suffix) = remainder.split_at(end);
    let authority = match authority.rsplit_once('@') {
        Some((_, host)) => format!("[redacted]@{host}"),
        None => authority.to_owned(),
    };
    let suffix = match suffix.find(['?', '#']) {
        Some(index) => format!("{}[redacted]", &suffix[..index]),
        None => suffix.to_owned(),
    };
    format!("{prefix}{authority}{suffix}")
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
    fn clone_failure_preserves_actionable_diagnostic() {
        assert_eq!(
            clone_failure_message("fatal: repository '/tmp/absent.git' does not exist\n"),
            "fatal: repository '/tmp/absent.git' does not exist"
        );
        assert_eq!(
            clone_failure_message("fatal: Authentication failed"),
            "fatal: Authentication failed"
        );
    }

    #[test]
    fn clone_failure_redacts_credentials_and_removes_controls() {
        let message = clone_failure_message(
            "fatal:\u{1b} repository 'https://fixture-user:fixture-password@host/repo?access_token=fixture-token#fixture-fragment' failed\r\nAuthorization: Bearer fixture-bearer\npassword=fixture-password token=fixture-token\u{0}",
        );
        assert!(message.contains("fatal:"));
        assert!(message.contains("host/repo"));
        for secret in [
            "fixture-user",
            "fixture-password",
            "fixture-token",
            "fixture-fragment",
            "fixture-bearer",
        ] {
            assert!(!message.contains(secret), "credential leaked: {secret}");
        }
        assert!(!message.chars().any(char::is_control));
    }

    #[test]
    fn clone_failure_redacts_credentials_with_spaces_or_embedded_controls() {
        for diagnostic in [
            "fatal: authentication failed password='fixture-password fixture-extra'",
            "fatal: authentication failed password: fixture-password fixture-extra",
            "fatal: https://fixture-user:\u{1b}fixture-password@host/repo rejected",
            "fatal: https://fixture-user:\tfixture-password@host/repo rejected",
        ] {
            let message = clone_failure_message(diagnostic);
            assert!(message.contains("fatal:"));
            for secret in ["fixture-user", "fixture-password", "fixture-extra"] {
                assert!(!message.contains(secret), "credential leaked: {secret}");
            }
        }
    }

    #[test]
    fn clone_failure_message_is_bounded_with_an_empty_stderr_fallback() {
        let message = clone_failure_message(&format!("fatal: {}", "é".repeat(8192)));
        assert!(message.starts_with("fatal:"));
        assert!(message.len() <= 4096);
        assert!(!clone_failure_message(" \n\r\t").is_empty());
    }
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
