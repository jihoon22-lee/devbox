//! Private Windows/native LSP transport. A proof is supplied only by the
//! Windows Files owner after reading its authenticated native Files pipe.
use code_pad_lib::{core::encoding::Encoding, lsp::LspPosition};
use product_contract::ProjectContext;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
type Result<T> = std::result::Result<T, &'static str>;
fn input<T: serde::de::DeserializeOwned>(value: Value) -> Result<T> {
    serde_json::from_value(value).map_err(|_| "wsl_request_invalid")
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProofRequest {
    pub path: String,
    pub native_revision: String,
    pub verify_disk: bool,
}
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DocumentProof {
    pub context: ProjectContext,
    pub path: String,
    pub revision: String,
    pub identity: (u64, u64),
    pub parents: Vec<(String, (u64, u64))>,
    pub mtime_nanos: String,
    pub size: u64,
    pub content_hash: String,
    pub encoding: Encoding,
    pub baseline_text_hash: [u8; 32],
    pub buffer_text_hash: [u8; 32],
}
impl DocumentProof {
    pub fn dirty(&self, text: &str) -> bool {
        use sha2::{Digest, Sha256};
        <[u8; 32]>::from(Sha256::digest(text.as_bytes())) != self.baseline_text_hash
    }
    pub fn validate(&self) -> Result<()> {
        self.context.validate().map_err(|_| "wsl_context_invalid")?;
        if self.path.is_empty()
            || self.path.len() > 32768
            || self.revision.is_empty()
            || self.revision.len() > 128
            || self.parents.is_empty()
            || self.parents.len() > 128
            || self.size > code_pad_lib::core::guard::MAX_EDITABLE_BYTES
            || self.content_hash.len() != 64
            || !self
                .content_hash
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            || self.mtime_nanos.parse::<i64>().is_err()
        {
            return Err("wsl_document_proof_invalid");
        }
        if !matches!(
            &self.context.target,
            product_contract::ExecutionTarget::Wsl { .. }
        ) {
            return Err("wsl_context_invalid");
        }
        for (path, parent) in std::iter::once((&self.path, false))
            .chain(self.parents.iter().map(|(path, _)| (path, true)))
        {
            // A filesystem root is evidence for a parent, never a document or
            // a new project grant. The native consumer compares every identity.
            if parent && path == "/" {
                continue;
            }
            let parsed = devbox_filesystem::parse_safe_project_path(path)
                .ok_or("wsl_document_proof_invalid")?;
            if parsed.kind() != devbox_filesystem::ProjectPathKind::Posix {
                return Err("wsl_document_proof_invalid");
            }
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(
    tag = "method",
    content = "args",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum DocumentMethod {
    RequestLspRename {
        language_id: String,
        uri: String,
        position: LspPosition,
        new_name: String,
    },
    ApplyLspRename {
        plan_id: String,
    },
    CancelLspRename {
        plan_id: String,
    },
    DiscardLspRename {
        plan_id: String,
    },
    OpenLspDocument {
        language_id: String,
        path: String,
        text: String,
        native_revision: String,
    },
    ChangeLspDocument {
        language_id: String,
        uri: String,
        text: String,
        dirty: bool,
        native_revision: String,
    },
    ReloadLspDocument {
        language_id: String,
        uri: String,
        text: String,
        native_revision: String,
    },
    SaveLspDocument {
        language_id: String,
        uri: String,
        native_revision: String,
        #[serde(default)]
        text: Option<String>,
    },
    CloseLspDocument {
        language_id: String,
        uri: String,
    },
    PullLspDiagnostics {
        language_id: String,
        uri: String,
    },
    RequestLspCompletion {
        language_id: String,
        uri: String,
        position: LspPosition,
    },
    RequestLspHover {
        language_id: String,
        uri: String,
        position: LspPosition,
    },
    RequestLspDefinition {
        language_id: String,
        uri: String,
        position: LspPosition,
    },
    RequestLspReferences {
        language_id: String,
        uri: String,
        position: LspPosition,
        include_declaration: bool,
    },
    RequestLspFormatting {
        language_id: String,
        uri: String,
        tab_size: u32,
        insert_spaces: bool,
    },
}
impl DocumentMethod {
    pub fn control(&self) -> bool {
        matches!(
            self,
            Self::CancelLspRename { .. } | Self::DiscardLspRename { .. }
        )
    }
    pub fn parse(method: &str, args: Value) -> Result<Self> {
        let method: Self = input(json!({"method":method,"args":args}))?;
        let (language, target) = method.target();
        if language.is_empty()
            || language.len() > 64
            || !language
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'+'))
            || target.is_empty()
            || target.len() > 32 * 1024
        {
            return Err("invalid_request");
        }
        match &method {
            Self::OpenLspDocument {
                text,
                native_revision,
                ..
            }
            | Self::ChangeLspDocument {
                text,
                native_revision,
                ..
            }
            | Self::ReloadLspDocument {
                text,
                native_revision,
                ..
            } if text.len() > 16 * 1024 * 1024
                || native_revision.len() > 128
                || native_revision.is_empty() =>
            {
                return Err("invalid_request")
            }
            Self::SaveLspDocument {
                text: Some(text),
                native_revision,
                ..
            } if text.len() > 16 * 1024 * 1024
                || native_revision.is_empty()
                || native_revision.len() > 128 =>
            {
                return Err("invalid_request");
            }
            _ => {}
        }
        Ok(method)
    }
    pub fn target(&self) -> (&str, &str) {
        match self {
            Self::ApplyLspRename { plan_id }
            | Self::CancelLspRename { plan_id }
            | Self::DiscardLspRename { plan_id } => ("native", plan_id),
            Self::RequestLspRename {
                language_id, uri, ..
            } => (language_id, uri),
            Self::OpenLspDocument {
                language_id, path, ..
            } => (language_id, path),
            Self::ChangeLspDocument {
                language_id, uri, ..
            }
            | Self::ReloadLspDocument {
                language_id, uri, ..
            }
            | Self::SaveLspDocument {
                language_id, uri, ..
            }
            | Self::CloseLspDocument { language_id, uri }
            | Self::PullLspDiagnostics { language_id, uri }
            | Self::RequestLspCompletion {
                language_id, uri, ..
            }
            | Self::RequestLspHover {
                language_id, uri, ..
            }
            | Self::RequestLspDefinition {
                language_id, uri, ..
            }
            | Self::RequestLspReferences {
                language_id, uri, ..
            }
            | Self::RequestLspFormatting {
                language_id, uri, ..
            } => (language_id, uri),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(
    tag = "method",
    content = "args",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum LifecycleMethod {
    StartLanguageServer {
        language_id: String,
        operation_id: String,
    },
    RestartLanguageServer {
        language_id: String,
        operation_id: String,
    },
    StopLanguageServer {
        language_id: String,
        operation_id: Option<String>,
    },
    StopAllLanguageServers {
        #[serde(default)]
        operation_ids: Vec<String>,
    },
    LanguageServerStatuses {},
    LanguageServerLogs {},
    LspPoll {},
}
#[derive(Debug, Clone)]
pub enum Command {
    Lifecycle(LifecycleMethod),
    Document(DocumentMethod),
}
impl Command {
    pub fn parse(method: &str, args: Value) -> Result<Self> {
        if matches!(
            method,
            "start_language_server"
                | "restart_language_server"
                | "stop_language_server"
                | "stop_all_language_servers"
                | "language_server_statuses"
                | "language_server_logs"
                | "lsp_poll"
        ) {
            let method: LifecycleMethod = input(json!({"method":method,"args":args}))?;
            let (language, operations) = match &method {
                LifecycleMethod::StartLanguageServer {
                    language_id,
                    operation_id,
                }
                | LifecycleMethod::RestartLanguageServer {
                    language_id,
                    operation_id,
                } => (Some(language_id), vec![operation_id]),
                LifecycleMethod::StopLanguageServer {
                    language_id,
                    operation_id,
                } => (Some(language_id), operation_id.iter().collect()),
                LifecycleMethod::StopAllLanguageServers { operation_ids } => {
                    (None, operation_ids.iter().collect())
                }
                _ => (None, vec![]),
            };
            if language.is_some_and(|language| {
                language.is_empty()
                    || language.len() > 64
                    || !language.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'+')
                    })
            }) || operations.len() > 64
                || operations
                    .iter()
                    .any(|id| id.is_empty() || id.len() > 128 || id.chars().any(char::is_control))
            {
                return Err("wsl_request_invalid");
            }
            Ok(Self::Lifecycle(method))
        } else {
            DocumentMethod::parse(method, args).map(Self::Document)
        }
    }
    pub fn starts(&self) -> bool {
        matches!(
            self,
            Self::Lifecycle(
                LifecycleMethod::StartLanguageServer { .. }
                    | LifecycleMethod::RestartLanguageServer { .. }
            )
        )
    }
    pub fn stops(&self) -> bool {
        matches!(
            self,
            Self::Lifecycle(
                LifecycleMethod::StopLanguageServer { .. }
                    | LifecycleMethod::StopAllLanguageServers { .. }
            )
        )
    }
    pub fn document(&self) -> Option<&DocumentMethod> {
        match self {
            Self::Document(method) => Some(method),
            _ => None,
        }
    }
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Execution {
    pub context: ProjectContext,
    pub digest: String,
    pub method: String,
    pub args: Value,
    pub document: Option<DocumentProof>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventName {
    #[serde(rename = "lsp/status")]
    Status,
    #[serde(rename = "lsp/diagnostics")]
    Diagnostics,
}
impl EventName {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Status => "lsp/status",
            Self::Diagnostics => "lsp/diagnostics",
        }
    }
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Event {
    pub name: EventName,
    pub value: Value,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Reply {
    pub result: std::result::Result<Value, String>,
    pub events: Vec<Event>,
}
