//! Read-only suggestions. No build tool, package manager or repository command is invoked.
use crate::{R, RuntimeError, RuntimeService, merge, service::project_error};
use devbox_core::sh_quote;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
use tokio::io::AsyncReadExt;
use ts_rs::TS;

const INPUT_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum DefinitionKind {
    Task,
    Service,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub enum SuggestionSource {
    #[serde(rename = "package.json")]
    PackageJson,
    #[serde(rename = "Makefile")]
    Makefile,
    #[serde(rename = "justfile")]
    Justfile,
    #[serde(rename = "Cargo.toml")]
    CargoToml,
    #[serde(rename = "compose.yaml")]
    ComposeYaml,
    #[serde(rename = "docker-compose.yml")]
    DockerComposeYml,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Suggestion {
    pub id: String,
    pub command: String,
    pub kind: DefinitionKind,
    pub from: SuggestionSource,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DetectRuns {
    pub project_id: String,
}

impl RuntimeService {
    pub async fn detect(&self, p: DetectRuns) -> R<Vec<Suggestion>> {
        let _guard = self.project_lock(&p.project_id).await;
        let project = self
            .projects
            .get(&p.project_id)
            .await
            .map_err(project_error)?
            .ok_or_else(|| RuntimeError::ProjectNotFound {
                id: p.project_id.clone(),
            })?;
        let state = self
            .projects
            .run_config_status(&p.project_id)
            .await
            .map_err(project_error)?;
        let (local, _) = self.local(&p.project_id).await?;
        let merged = merge(state.config.definitions(), local)?;
        // Read the complete definition set, never the first page of runtime.list.
        let existing = merged
            .defs
            .iter()
            .map(|d| d.def.id().to_owned())
            .collect::<Vec<_>>();
        Ok(detect(Path::new(&project.path), &existing).await?)
    }
}

#[derive(Deserialize)]
struct Package {
    #[serde(default)]
    scripts: BTreeMap<String, String>,
}

/// The first source wins duplicate IDs: package, Makefile, justfile, Cargo, compose.
/// Filter existing IDs only after deduplication, so lower-priority sources cannot revive them.
pub async fn detect(root: &Path, existing: &[String]) -> Result<Vec<Suggestion>, RuntimeError> {
    let mut items = BTreeMap::new();
    if let Some(text) = read(root, "package.json").await? {
        let package: Package =
            serde_json::from_str(&text).map_err(|e| RuntimeError::DetectParse {
                file: "package.json".into(),
                message: e.to_string(),
            })?;
        let manager = manager(root).await?;
        for name in package.scripts.keys() {
            let command = if manager == "pnpm" && name == "dev" {
                "pnpm dev".into()
            } else {
                format!("{manager} run {}", argument(name))
            };
            insert(
                &mut items,
                name,
                command,
                if matches!(name.as_str(), "dev" | "start" | "serve") {
                    DefinitionKind::Service
                } else {
                    DefinitionKind::Task
                },
                SuggestionSource::PackageJson,
            )?;
        }
    }
    if let Some(text) = read(root, "Makefile").await? {
        for name in make_targets(&text) {
            insert(
                &mut items,
                &name,
                format!("make {}", argument(&name)),
                DefinitionKind::Task,
                SuggestionSource::Makefile,
            )?;
        }
    }
    if let Some(text) = read(root, "justfile").await? {
        for name in just_recipes(&text) {
            insert(
                &mut items,
                &name,
                format!("just {}", argument(&name)),
                DefinitionKind::Task,
                SuggestionSource::Justfile,
            )?;
        }
    }
    if exists(root, "Cargo.toml").await? {
        for name in ["build", "test"] {
            insert(
                &mut items,
                name,
                format!("cargo {name}"),
                DefinitionKind::Task,
                SuggestionSource::CargoToml,
            )?;
        }
    }
    for (file, source) in [
        ("compose.yaml", SuggestionSource::ComposeYaml),
        ("docker-compose.yml", SuggestionSource::DockerComposeYml),
    ] {
        if exists(root, file).await? {
            insert(
                &mut items,
                "compose-up",
                "docker compose up".into(),
                DefinitionKind::Service,
                source,
            )?;
        }
    }
    let existing: BTreeSet<_> = existing.iter().map(String::as_str).collect();
    let items: Vec<_> = items
        .into_values()
        .filter(|s| !existing.contains(s.id.as_str()))
        .collect();
    let bytes = serde_json::to_vec(&items).expect("suggestions contain only serializable strings");
    if bytes.len() > crate::service::PAGE_BYTES {
        return Err(RuntimeError::DetectLimit {
            subject: "suggestions".into(),
            limit: crate::service::PAGE_BYTES as u32,
        });
    }
    Ok(items)
}

fn insert(
    items: &mut BTreeMap<String, Suggestion>,
    id: &str,
    command: String,
    kind: DefinitionKind,
    from: SuggestionSource,
) -> Result<(), RuntimeError> {
    // Suggestions can be passed directly to the existing define path.
    let def = crate::RunDef::Task(crate::TaskDef {
        id: id.into(),
        command: command.clone(),
        ..Default::default()
    });
    crate::validate_definition(&def)?;
    items.entry(id.into()).or_insert_with(|| Suggestion {
        id: id.into(),
        command,
        kind,
        from,
    });
    Ok(())
}

fn io_error(file: &str, error: impl std::fmt::Display) -> RuntimeError {
    tracing::warn!(file, error = %error, "read task detection file");
    RuntimeError::DetectIo {
        file: file.into(),
        message: "could not read a regular UTF-8 file".into(),
    }
}

async fn read(root: &Path, file: &str) -> Result<Option<String>, RuntimeError> {
    let Some(meta) = metadata(root, file).await? else {
        return Ok(None);
    };
    if meta.len() > INPUT_BYTES as u64 {
        return Err(RuntimeError::DetectLimit {
            subject: file.into(),
            limit: INPUT_BYTES as u32,
        });
    }
    let input = tokio::fs::File::open(root.join(file))
        .await
        .map_err(|e| io_error(file, e))?;
    let mut bytes = Vec::new();
    input
        .take((INPUT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .await
        .map_err(|e| io_error(file, e))?;
    if bytes.len() > INPUT_BYTES {
        return Err(RuntimeError::DetectLimit {
            subject: file.into(),
            limit: INPUT_BYTES as u32,
        });
    }
    Ok(Some(
        String::from_utf8(bytes).map_err(|e| io_error(file, e))?,
    ))
}

async fn metadata(root: &Path, file: &str) -> Result<Option<std::fs::Metadata>, RuntimeError> {
    match tokio::fs::metadata(root.join(file)).await {
        Ok(meta) if meta.is_file() => Ok(Some(meta)),
        Ok(_) => Err(io_error(file, "expected a regular file")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io_error(file, error)),
    }
}

async fn exists(root: &Path, file: &str) -> Result<bool, RuntimeError> {
    Ok(metadata(root, file).await?.is_some())
}

async fn manager(root: &Path) -> Result<&'static str, RuntimeError> {
    // Deterministic precedence for repositories retaining more than one manager's lock.
    for (file, manager) in [
        ("pnpm-lock.yaml", "pnpm"),
        ("yarn.lock", "yarn"),
        ("package-lock.json", "npm"),
        ("npm-shrinkwrap.json", "npm"),
        ("bun.lock", "bun"),
        ("bun.lockb", "bun"),
    ] {
        if exists(root, file).await? {
            return Ok(manager);
        }
    }
    Ok("npm")
}

fn argument(name: &str) -> String {
    let quoted = if name
        .bytes()
        .all(|c| c.is_ascii_alphanumeric() || b"_-.:/".contains(&c))
    {
        name.to_owned()
    } else {
        sh_quote(name)
    };
    if name.starts_with('-') {
        format!("-- {quoted}")
    } else {
        quoted
    }
}

fn make_targets(text: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    // Join make's ordinary line continuations without evaluating variables or includes.
    let text = text.replace("\\\r\n", " ").replace("\\\n", " ");
    for line in text.lines() {
        if line.starts_with('\t') {
            continue;
        }
        let line = line.split('#').next().unwrap_or_default().trim();
        let Some((left, right)) = line.split_once(':') else {
            continue;
        };
        if right.starts_with('=') || left.contains('=') {
            continue;
        }
        let targets = if left == ".PHONY" { right } else { left };
        for name in targets.split_whitespace() {
            if !name.starts_with('.') && !name.contains(['$', '%', '\\', ':', ';', '=', '|', '&']) {
                names.insert(name.into());
            }
        }
    }
    names
}

fn just_recipes(text: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for line in text.lines() {
        // Indented recipe bodies, assignments, directives and parameterized recipes are not headers.
        if line.starts_with(char::is_whitespace) {
            continue;
        }
        let Some((header, right)) = line.split_once(':') else {
            continue;
        };
        if right.starts_with('=') {
            continue;
        }
        let name = header.trim().strip_prefix('@').unwrap_or(header.trim());
        if !name.is_empty()
            && name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
        {
            names.insert(name.into());
        }
    }
    names
}
