//! The v0.7 persistent JSON inventory. This parser never resolves a project,
//! starts a distro/server, or converts stored paths into native file grants.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Source {
    Workbench,
    CodePad,
    CodePadLegacy,
    RepoManager,
}

#[derive(Clone, Copy)]
pub struct FileSpec {
    pub name: &'static str,
    pub limit: usize,
}
const MIB: usize = 1024 * 1024;
const WINDOW: FileSpec = FileSpec {
    name: "window-state-v1.json",
    limit: window_state::MAX_STATE_BYTES,
};
impl Source {
    pub fn identifier(self) -> &'static str {
        match self {
            Self::Workbench => "com.devbox.workbench",
            Self::CodePad => "com.devbox.codepad",
            Self::CodePadLegacy => "com.workbench.codepad",
            Self::RepoManager => "com.devbox.repomanager",
        }
    }
    /// Snapshot v1 predates window inventory. Keep its exact fixed file set so
    /// existing content-addressed manifests remain valid without rewriting IDs.
    pub fn snapshot_files(self, version: u32) -> Result<&'static [FileSpec], &'static str> {
        let files = self.files();
        match version {
            1 => Ok(&files[..files.len() - 1]),
            2 => Ok(files),
            _ => Err("invalid_legacy_snapshot"),
        }
    }
    pub fn is_code_pad(self) -> bool {
        matches!(self, Self::CodePad | Self::CodePadLegacy)
    }
    pub fn files(self) -> &'static [FileSpec] {
        match self {
            Self::Workbench => &[
                FileSpec {
                    name: "project-profiles.json",
                    limit: 4 * MIB,
                },
                FileSpec {
                    name: "profile-templates.json",
                    limit: MIB,
                },
                WINDOW,
            ],
            Self::CodePad | Self::CodePadLegacy => &[
                FileSpec {
                    name: "session.json",
                    limit: 8 * MIB,
                },
                FileSpec {
                    name: "recovery.json",
                    limit: 8 * MIB,
                },
                FileSpec {
                    name: "lsp/config.json",
                    limit: 64 * 1024,
                },
                WINDOW,
            ],
            // v0.7 scan root, selected repository and panel preferences are
            // React state only. Dependency enrichment is a derived cache.
            Self::RepoManager => &[WINDOW],
        }
    }
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Inventory {
    pub name: String,
    pub bytes: usize,
    pub sha256: String,
    pub records: Option<usize>,
    pub issue: Option<Issue>,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Issue {
    Corrupt,
    UnsupportedSchema,
    Limit,
}

pub fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

// Serde schemas historically ignored a few unknown Code Pad fields. Compare
// the decoded input with its typed serialization before allowing conversion;
// retain documented cursor aliases without accepting unrelated future data.
fn known_shape(raw: &Value, normalized: &Value, cursor_aliases: bool) -> bool {
    match (raw, normalized) {
        (Value::Object(raw), Value::Object(normalized)) => raw.iter().all(|(key, value)| {
            let key =
                if cursor_aliases && matches!(key.as_str(), "cursorPosition" | "cursor_position") {
                    "cursor"
                } else {
                    key
                };
            normalized
                .get(key)
                .is_some_and(|expected| known_shape(value, expected, cursor_aliases))
        }),
        (Value::Array(raw), Value::Array(normalized)) => {
            raw.len() == normalized.len()
                && raw
                    .iter()
                    .zip(normalized)
                    .all(|(a, b)| known_shape(a, b, cursor_aliases))
        }
        _ => raw == normalized,
    }
}
pub fn inspect(source: Source, name: &str, bytes: &[u8]) -> Result<Inventory, &'static str> {
    let spec = source
        .files()
        .iter()
        .find(|spec| spec.name == name)
        .ok_or("unknown_legacy_file")?;
    let decoded = decode(source, name, bytes, spec.limit);
    Ok(Inventory {
        name: name.into(),
        bytes: bytes.len(),
        sha256: digest(bytes),
        records: decoded.as_ref().ok().copied(),
        issue: decoded.err(),
    })
}
fn decode(source: Source, name: &str, bytes: &[u8], limit: usize) -> Result<usize, Issue> {
    if bytes.len() > limit {
        return Err(Issue::Limit);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| Issue::Corrupt)?;
    let raw: Value = serde_json::from_slice(bytes).map_err(|_| Issue::Corrupt)?;
    if raw
        .get("version")
        .and_then(Value::as_u64)
        .is_some_and(|version| version != 1)
    {
        return Err(Issue::UnsupportedSchema);
    }
    if name == "window-state-v1.json" {
        if raw
            .get("schemaVersion")
            .and_then(Value::as_u64)
            .is_some_and(|version| version != 1)
        {
            return Err(Issue::UnsupportedSchema);
        }
        window_state::decode_state(bytes).map_err(|_| Issue::Corrupt)?;
        return Ok(1);
    }
    let (normalized, count) = match (source, name) {
        (Source::Workbench, "project-profiles.json") => {
            let parsed =
                workbench_lib::component::ProfileStore::load(text).map_err(|_| Issue::Corrupt)?;
            let count = parsed.profiles.len();
            (
                serde_json::to_value(parsed).map_err(|_| Issue::Corrupt)?,
                count,
            )
        }
        (Source::Workbench, "profile-templates.json") => {
            let parsed = workbench_lib::component::ProfileTemplateStore::load(text)
                .map_err(|_| Issue::Corrupt)?;
            let count = parsed.templates.len();
            (
                serde_json::to_value(parsed).map_err(|_| Issue::Corrupt)?,
                count,
            )
        }
        (Source::CodePad | Source::CodePadLegacy, "session.json") => {
            let parsed = code_pad_lib::core::session::Session::from_json(text)
                .map_err(|_| Issue::Corrupt)?;
            let count = parsed.docs.len();
            (
                serde_json::to_value(parsed).map_err(|_| Issue::Corrupt)?,
                count,
            )
        }
        (Source::CodePad | Source::CodePadLegacy, "recovery.json") => {
            code_pad_lib::component::validate_persistent_file(name, bytes)
                .map_err(|_| Issue::Corrupt)?;
            let parsed: code_pad_lib::core::recovery::RecoveryFile =
                serde_json::from_str(text).map_err(|_| Issue::Corrupt)?;
            let count = parsed.entries.len();
            (
                serde_json::to_value(parsed).map_err(|_| Issue::Corrupt)?,
                count,
            )
        }
        (Source::CodePad | Source::CodePadLegacy, "lsp/config.json") => {
            let parsed =
                code_pad_lib::lsp::LspConfig::from_json(text).map_err(|_| Issue::Corrupt)?;
            let count = parsed.server_by_language.len();
            (
                serde_json::to_value(parsed).map_err(|_| Issue::Corrupt)?,
                count,
            )
        }
        _ => return Err(Issue::Corrupt),
    };
    if !known_shape(&raw, &normalized, name == "session.json") {
        return Err(Issue::UnsupportedSchema);
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_future_and_unknown_data_have_no_success_count() {
        for (bytes, issue) in [
            (b"{broken".as_slice(), Issue::Corrupt),
            (br#"{"version":2}"#, Issue::UnsupportedSchema),
        ] {
            let item = inspect(Source::CodePad, "session.json", bytes).unwrap();
            assert_eq!(item.issue, Some(issue));
            assert_eq!(item.records, None);
            assert_eq!(item.sha256, digest(bytes));
        }
        let mut raw = serde_json::to_value(code_pad_lib::core::session::Session::empty()).unwrap();
        raw["futureState"] = serde_json::json!({"preserve":"me"});
        assert_eq!(
            inspect(
                Source::CodePad,
                "session.json",
                &serde_json::to_vec(&raw).unwrap()
            )
            .unwrap()
            .issue,
            Some(Issue::UnsupportedSchema)
        );
    }
    #[test]
    fn bookmarks_cursor_aliases_and_unsaved_crlf_are_authoritative() {
        let bytes=br#"{"version":1,"workspace_folder":null,"docs":[{"id":"a","path":"C:\\fixture\\a.rs","cursorPosition":5,"bookmarks":[1,3]}],"views":[["a"],[]],"active_view":0,"active_doc_by_view":["a",null],"recent_files":["C:\\fixture\\a.rs"]}"#;
        let item = inspect(Source::CodePad, "session.json", bytes).unwrap();
        assert_eq!(item.records, Some(1));
        assert_eq!(item.issue, None);
        let bytes=br#"{"version":1,"entries":[{"path":"C:\\fixture\\a.rs","content":"unsaved\r\n","snapshot_at_ms":1}]}"#;
        assert_eq!(
            inspect(Source::CodePad, "recovery.json", bytes)
                .unwrap()
                .records,
            Some(1)
        );
        let mut raw: Value = serde_json::from_slice(bytes).unwrap();
        raw["entries"][0]["futureBuffer"] = Value::Bool(true);
        assert_eq!(
            inspect(
                Source::CodePad,
                "recovery.json",
                &serde_json::to_vec(&raw).unwrap()
            )
            .unwrap()
            .issue,
            Some(Issue::UnsupportedSchema)
        );
    }
    #[test]
    fn window_future_corruption_and_size_limits_never_claim_success() {
        for source in [
            Source::Workbench,
            Source::CodePad,
            Source::CodePadLegacy,
            Source::RepoManager,
        ] {
            let future =
                inspect(source, "window-state-v1.json", br#"{"schemaVersion":2}"#).unwrap();
            assert_eq!(future.issue, Some(Issue::UnsupportedSchema));
            assert_eq!(future.records, None);
            let corrupt = inspect(
                source,
                "window-state-v1.json",
                br#"{"schemaVersion":1,"bounds":null}"#,
            )
            .unwrap();
            assert_eq!(corrupt.issue, Some(Issue::Corrupt));
            assert_eq!(corrupt.records, None);
            assert_eq!(
                inspect(
                    source,
                    "window-state-v1.json",
                    &vec![b' '; window_state::MAX_STATE_BYTES + 1]
                )
                .unwrap()
                .issue,
                Some(Issue::Limit)
            );
        }
        assert_eq!(
            Source::Workbench
                .snapshot_files(1)
                .unwrap()
                .iter()
                .map(|file| file.name)
                .collect::<Vec<_>>(),
            ["project-profiles.json", "profile-templates.json"]
        );
        assert_eq!(
            Source::CodePad
                .snapshot_files(1)
                .unwrap()
                .iter()
                .map(|file| file.name)
                .collect::<Vec<_>>(),
            ["session.json", "recovery.json", "lsp/config.json"]
        );
    }
    #[test]
    fn inventory_never_invents_repo_preferences_or_admits_project_files() {
        assert_eq!(Source::RepoManager.files()[0].name, "window-state-v1.json");
        assert!(Source::RepoManager.snapshot_files(1).unwrap().is_empty());
        for name in [
            "../session.json",
            ".git/config",
            "lsp/rename-backups/journal.json",
            "settings.json",
        ] {
            assert_eq!(
                inspect(Source::CodePad, name, b"{}"),
                Err("unknown_legacy_file")
            );
        }
        assert_eq!(Source::CodePad.identifier(), "com.devbox.codepad");
    }
}
