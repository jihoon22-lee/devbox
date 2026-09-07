//! Versioned, metadata-only storage for Developer Toolbox smart workflows.
//!
//! The file contains tool IDs, pipeline step IDs, and timestamps only.  Input
//! and output text never enters this module.  Callers must supply the
//! app-local-data directory; the Tauri command layer owns that path lookup.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};

pub const WORKFLOW_SCHEMA_VERSION: u8 = 1;
pub const WORKFLOW_FILE_NAME: &str = "smart-workflows.json";
pub const MAX_RECENT_TOOLS: usize = 20;
pub const MAX_FAVORITE_TOOLS: usize = 50;
pub const MAX_PIPELINES: usize = 20;
pub const MAX_PIPELINE_STEPS: usize = 8;
pub const MAX_ID_LENGTH: usize = 64;
pub const MAX_SERIALIZED_BYTES: usize = 64 * 1024;
pub const MAX_TIMESTAMP: u64 = 9_007_199_254_740_991;

const WORKFLOW_STORAGE_ERROR: &str = "Toolbox workflow metadata를 저장할 수 없습니다.";
const ID_CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789-";
const TOOL_IDS: &[&str] = &[
    "json-format",
    "json-minify",
    "json-yaml",
    "json-typescript",
    "byte-codec",
    "radix",
    "html-entity-encode",
    "html-entity-decode",
    "url-encode",
    "url-decode",
    "qr",
    "timestamp",
    "case",
    "lorem",
    "markdown-table",
    "hash",
    "hmac",
    "uuid",
    "regex",
    "diff",
    "jwt",
];
const TRANSFORMER_IDS: &[&str] = &[
    "json-format",
    "json-minify",
    "json-parse",
    "json-to-yaml",
    "yaml-to-json",
    "json-to-typescript",
    "jwt-decode",
    "url-encode",
    "url-decode",
    "base64-encode",
    "base64-decode",
    "base64-to-hex",
    "base64url-encode",
    "base64url-decode",
    "base64url-to-hex",
    "hex-encode",
    "hex-decode",
    "hex-to-base64",
    "case",
];

static WORKFLOW_IO_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn workflow_io_guard() -> MutexGuard<'static, ()> {
    WORKFLOW_IO_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecentToolMetadata {
    pub tool_id: String,
    pub used_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SavedPipelineMetadata {
    pub id: String,
    pub input_type: String,
    pub steps: Vec<PipelineStep>,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PipelineStep {
    pub transformer_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowMetadata {
    pub schema_version: u8,
    pub recent_tools: Vec<RecentToolMetadata>,
    pub favorite_tools: Vec<String>,
    pub pipelines: Vec<SavedPipelineMetadata>,
}

/// A load result distinguishes a missing file (safe to create) from a
/// malformed/oversized file (must be preserved and never silently replaced).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowLoadResult {
    pub metadata: WorkflowMetadata,
    pub writable: bool,
}

impl Default for WorkflowMetadata {
    fn default() -> Self {
        Self {
            schema_version: WORKFLOW_SCHEMA_VERSION,
            recent_tools: Vec::new(),
            favorite_tools: Vec::new(),
            pipelines: Vec::new(),
        }
    }
}

pub fn workflow_path(app_local_data_dir: impl AsRef<Path>) -> PathBuf {
    app_local_data_dir.as_ref().join(WORKFLOW_FILE_NAME)
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_ID_LENGTH
        && value
            .as_bytes()
            .iter()
            .all(|character| ID_CHARS.contains(character))
        && !value.starts_with('-')
        && !value.ends_with('-')
        && !value.contains("--")
}

fn valid_tool_id(value: &str) -> bool {
    valid_id(value) && TOOL_IDS.contains(&value)
}

fn valid_transformer_id(value: &str) -> bool {
    valid_id(value) && TRANSFORMER_IDS.contains(&value)
}

fn valid_pipeline_input_type(value: &str) -> bool {
    matches!(
        value,
        "text"
            | "json"
            | "jwt"
            | "url"
            | "base64"
            | "base64url"
            | "hex"
            | "url-component"
            | "yaml"
            | "typescript"
    )
}

fn transformer_transition(input_type: &str, transformer_id: &str) -> Option<&'static str> {
    let output_type = match transformer_id {
        "json-format" | "json-minify" => {
            if input_type != "json" {
                return None;
            }
            "json"
        }
        "json-parse" => {
            if input_type != "text" {
                return None;
            }
            "json"
        }
        "json-to-yaml" => {
            if input_type != "json" {
                return None;
            }
            "yaml"
        }
        "yaml-to-json" => {
            if input_type != "yaml" {
                return None;
            }
            "json"
        }
        "json-to-typescript" => {
            if input_type != "json" {
                return None;
            }
            "typescript"
        }
        "jwt-decode" => {
            if input_type != "jwt" {
                return None;
            }
            "json"
        }
        "url-encode" => {
            if input_type != "text" {
                return None;
            }
            "url-component"
        }
        "url-decode" => {
            if !matches!(input_type, "url" | "url-component" | "text") {
                return None;
            }
            "text"
        }
        "base64-encode" => {
            if input_type != "text" {
                return None;
            }
            "base64"
        }
        "base64-decode" => {
            if input_type != "base64" {
                return None;
            }
            "text"
        }
        "base64-to-hex" => {
            if input_type != "base64" {
                return None;
            }
            "hex"
        }
        "base64url-encode" => {
            if input_type != "text" {
                return None;
            }
            "base64url"
        }
        "base64url-decode" => {
            if input_type != "base64url" {
                return None;
            }
            "text"
        }
        "base64url-to-hex" => {
            if input_type != "base64url" {
                return None;
            }
            "hex"
        }
        "hex-encode" => {
            if input_type != "text" {
                return None;
            }
            "hex"
        }
        "hex-decode" => {
            if input_type != "hex" {
                return None;
            }
            "text"
        }
        "hex-to-base64" => {
            if input_type != "hex" {
                return None;
            }
            "base64"
        }
        "case" => {
            if input_type != "text" {
                return None;
            }
            "text"
        }
        _ => return None,
    };
    Some(output_type)
}

fn valid_pipeline_steps(input_type: &str, steps: &[PipelineStep]) -> bool {
    let mut current = input_type;
    for step in steps {
        let Some(output_type) = transformer_transition(current, &step.transformer_id) else {
            return false;
        };
        current = output_type;
    }
    true
}

/// Read at most one byte past the metadata limit.  `fs::read` would allocate
/// the complete file before the size check, which would make a corrupt local
/// file an avoidable memory-amplification input.
fn read_bounded(path: &Path) -> std::io::Result<Vec<u8>> {
    let file = fs::File::open(path)?;
    let mut bytes = Vec::with_capacity(8 * 1024);
    file.take((MAX_SERIALIZED_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// Validate the wire shape before it can be written by a direct IPC caller.
/// The native layer repeats the current catalog/transformer allow-list so a
/// direct IPC caller cannot publish an unsupported stage or tool ID.
pub fn validate(metadata: &WorkflowMetadata) -> Result<(), &'static str> {
    if metadata.schema_version != WORKFLOW_SCHEMA_VERSION
        || metadata.recent_tools.len() > MAX_RECENT_TOOLS
        || metadata.favorite_tools.len() > MAX_FAVORITE_TOOLS
        || metadata.pipelines.len() > MAX_PIPELINES
    {
        return Err(WORKFLOW_STORAGE_ERROR);
    }

    let mut recent_ids = HashSet::new();
    for recent in &metadata.recent_tools {
        if !valid_tool_id(&recent.tool_id)
            || recent.used_at > MAX_TIMESTAMP
            || !recent_ids.insert(recent.tool_id.as_str())
        {
            return Err(WORKFLOW_STORAGE_ERROR);
        }
    }

    let mut favorite_ids = HashSet::new();
    for tool_id in &metadata.favorite_tools {
        if !valid_tool_id(tool_id) || !favorite_ids.insert(tool_id.as_str()) {
            return Err(WORKFLOW_STORAGE_ERROR);
        }
    }

    let mut pipeline_ids = HashSet::new();
    for pipeline in &metadata.pipelines {
        if !valid_id(&pipeline.id)
            || !valid_pipeline_input_type(&pipeline.input_type)
            || !valid_id(&pipeline.input_type)
            || pipeline.updated_at > MAX_TIMESTAMP
            || !pipeline_ids.insert(pipeline.id.as_str())
            || pipeline.steps.is_empty()
            || pipeline.steps.len() > MAX_PIPELINE_STEPS
            || !valid_pipeline_steps(&pipeline.input_type, &pipeline.steps)
        {
            return Err(WORKFLOW_STORAGE_ERROR);
        }
        for step in &pipeline.steps {
            if !valid_transformer_id(&step.transformer_id) {
                return Err(WORKFLOW_STORAGE_ERROR);
            }
        }
    }

    Ok(())
}

pub fn load_from_dir_with_status(app_local_data_dir: impl AsRef<Path>) -> WorkflowLoadResult {
    let _guard = workflow_io_guard();
    let path = workflow_path(app_local_data_dir);
    let parent = match path.parent() {
        Some(parent) => parent,
        None => {
            return WorkflowLoadResult {
                metadata: WorkflowMetadata::default(),
                writable: false,
            }
        }
    };
    let parent_identity = match devbox_filesystem::filesystem_identity(parent, true) {
        Ok(identity) => Some(identity),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => {
            return WorkflowLoadResult {
                metadata: WorkflowMetadata::default(),
                writable: false,
            }
        }
    };
    let file_identity = match devbox_filesystem::filesystem_identity(&path, false) {
        Ok(identity) => Some(identity),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => {
            return WorkflowLoadResult {
                metadata: WorkflowMetadata::default(),
                writable: false,
            }
        }
    };
    let bytes = match read_bounded(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return WorkflowLoadResult {
                metadata: WorkflowMetadata::default(),
                writable: true,
            }
        }
        Err(_) => {
            return WorkflowLoadResult {
                metadata: WorkflowMetadata::default(),
                writable: false,
            }
        }
    };
    if file_identity.is_none()
        || devbox_filesystem::filesystem_identity(&path, false).ok() != file_identity
        || parent_identity.is_none()
        || devbox_filesystem::filesystem_identity(parent, true).ok() != parent_identity
    {
        return WorkflowLoadResult {
            metadata: WorkflowMetadata::default(),
            writable: false,
        };
    }
    if bytes.len() > MAX_SERIALIZED_BYTES {
        return WorkflowLoadResult {
            metadata: WorkflowMetadata::default(),
            writable: false,
        };
    }
    let Ok(metadata) = serde_json::from_slice::<WorkflowMetadata>(&bytes) else {
        return WorkflowLoadResult {
            metadata: WorkflowMetadata::default(),
            writable: false,
        };
    };
    if validate(&metadata).is_err() {
        return WorkflowLoadResult {
            metadata: WorkflowMetadata::default(),
            writable: false,
        };
    }
    WorkflowLoadResult {
        metadata,
        writable: true,
    }
}

pub fn save_to_dir(
    app_local_data_dir: impl AsRef<Path>,
    metadata: &WorkflowMetadata,
) -> Result<(), &'static str> {
    let _guard = workflow_io_guard();
    validate(metadata)?;
    let bytes = serde_json::to_vec(metadata).map_err(|_| WORKFLOW_STORAGE_ERROR)?;
    if bytes.len() > MAX_SERIALIZED_BYTES {
        return Err(WORKFLOW_STORAGE_ERROR);
    }
    let directory = app_local_data_dir.as_ref();
    fs::create_dir_all(directory).map_err(|_| WORKFLOW_STORAGE_ERROR)?;
    let directory_identity = devbox_filesystem::filesystem_identity(directory, true)
        .map_err(|_| WORKFLOW_STORAGE_ERROR)?;
    let path = workflow_path(directory);
    match devbox_filesystem::filesystem_identity(&path, false) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(WORKFLOW_STORAGE_ERROR),
    }
    devbox_filesystem::atomic_write(&path, &bytes).map_err(|_| WORKFLOW_STORAGE_ERROR)?;
    if devbox_filesystem::filesystem_identity(directory, true).ok() != Some(directory_identity)
        || devbox_filesystem::filesystem_identity(&path, false).is_err()
    {
        return Err(WORKFLOW_STORAGE_ERROR);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_directory(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock before epoch")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "devbox-toolbox-workflows-{name}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&directory).expect("create test directory");
        directory
    }

    fn sample() -> WorkflowMetadata {
        WorkflowMetadata {
            schema_version: WORKFLOW_SCHEMA_VERSION,
            recent_tools: vec![RecentToolMetadata {
                tool_id: "json-format".into(),
                used_at: 42,
            }],
            favorite_tools: vec!["byte-codec".into()],
            pipelines: vec![SavedPipelineMetadata {
                id: "pipeline-1".into(),
                input_type: "base64".into(),
                steps: vec![PipelineStep {
                    transformer_id: "base64-decode".into(),
                }],
                updated_at: 43,
            }],
        }
    }

    #[test]
    fn defaults_to_versioned_empty_metadata() {
        let metadata = WorkflowMetadata::default();
        assert_eq!(metadata.schema_version, WORKFLOW_SCHEMA_VERSION);
        assert!(metadata.recent_tools.is_empty());
        assert!(validate(&metadata).is_ok());
    }

    #[test]
    fn save_round_trips_metadata_without_text_fields() {
        let directory = test_directory("roundtrip");
        let metadata = sample();
        save_to_dir(&directory, &metadata).expect("save metadata");
        let loaded = load_from_dir_with_status(&directory).metadata;
        assert_eq!(loaded, metadata);

        let bytes = fs::read(workflow_path(&directory)).expect("read metadata");
        let text = String::from_utf8(bytes).expect("metadata is UTF-8");
        assert!(!text.contains("\"input\":"));
        assert!(!text.contains("\"output\":"));
        assert!(!text.contains("secret-value"));
        fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[test]
    fn malformed_or_oversized_files_fail_closed_to_empty_metadata() {
        let directory = test_directory("malformed");
        let malformed_bytes = br#"{"schemaVersion":1,"input":"secret"}"#;
        fs::write(workflow_path(&directory), malformed_bytes).expect("write malformed metadata");
        assert_eq!(
            load_from_dir_with_status(&directory).metadata,
            WorkflowMetadata::default()
        );
        let loaded = load_from_dir_with_status(&directory);
        assert!(!loaded.writable);
        assert_eq!(
            fs::read(workflow_path(&directory)).unwrap(),
            malformed_bytes
        );
        fs::write(
            workflow_path(&directory),
            vec![b'x'; MAX_SERIALIZED_BYTES + 1],
        )
        .expect("write oversized metadata");
        assert_eq!(
            load_from_dir_with_status(&directory).metadata,
            WorkflowMetadata::default()
        );
        assert!(!load_from_dir_with_status(&directory).writable);
        fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[test]
    fn unknown_fields_are_not_accepted_by_the_native_wire_schema() {
        let raw = r#"{
            "schemaVersion":1,
            "recentTools":[],
            "favoriteTools":[],
            "pipelines":[],
            "input":"credential-value"
        }"#;
        assert!(serde_json::from_str::<WorkflowMetadata>(raw).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn linked_metadata_file_is_preserved_and_not_followed() {
        use std::os::unix::fs::symlink;

        let directory = test_directory("linked");
        let outside = test_directory("linked-outside").join("outside.json");
        fs::write(&outside, serde_json::to_vec(&sample()).unwrap()).unwrap();
        symlink(&outside, workflow_path(&directory)).unwrap();

        let loaded = load_from_dir_with_status(&directory);
        assert_eq!(loaded.metadata, WorkflowMetadata::default());
        assert!(!loaded.writable);
        assert!(save_to_dir(&directory, &sample()).is_err());
        assert_eq!(
            fs::read(&outside).unwrap(),
            serde_json::to_vec(&sample()).unwrap()
        );

        fs::remove_file(workflow_path(&directory)).unwrap();
        fs::remove_dir_all(directory).unwrap();
        fs::remove_dir_all(outside.parent().unwrap()).unwrap();
    }

    #[test]
    fn concurrent_saves_leave_one_complete_valid_document() {
        let directory = test_directory("concurrent");
        let mut left = sample();
        left.recent_tools[0].used_at = 100;
        let mut right = sample();
        right.recent_tools[0].used_at = 200;
        let left_dir = directory.clone();
        let right_dir = directory.clone();
        let left_thread = std::thread::spawn(move || save_to_dir(left_dir, &left));
        let right_thread = std::thread::spawn(move || save_to_dir(right_dir, &right));
        left_thread.join().unwrap().unwrap();
        right_thread.join().unwrap().unwrap();

        let loaded = load_from_dir_with_status(&directory);
        assert!(loaded.writable);
        assert!(matches!(loaded.metadata.recent_tools[0].used_at, 100 | 200));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn invalid_ids_and_duplicates_are_not_persistable() {
        let mut metadata = sample();
        metadata.favorite_tools = vec!["../secret".into()];
        assert!(validate(&metadata).is_err());

        let mut metadata = sample();
        metadata.favorite_tools = vec!["json-format".into(), "json-format".into()];
        assert!(validate(&metadata).is_err());

        let mut metadata = sample();
        metadata.pipelines[0].steps[0].transformer_id = "run-shell".into();
        assert!(validate(&metadata).is_err());

        let mut metadata = sample();
        metadata.pipelines[0].input_type = "text".into();
        assert!(validate(&metadata).is_err());

        let mut metadata = sample();
        metadata.pipelines[0].updated_at = MAX_TIMESTAMP + 1;
        assert!(validate(&metadata).is_err());
    }

    #[test]
    fn atomic_save_leaves_no_temporary_siblings() {
        let directory = test_directory("atomic");
        save_to_dir(&directory, &sample()).expect("save metadata");
        let entries = fs::read_dir(&directory)
            .expect("read directory")
            .map(|entry| {
                entry
                    .expect("entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect::<Vec<_>>();
        assert_eq!(entries, vec![WORKFLOW_FILE_NAME.to_string()]);
        fs::remove_dir_all(directory).expect("remove test directory");
    }
}
