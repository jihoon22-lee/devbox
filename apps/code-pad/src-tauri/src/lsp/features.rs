//! Bounded, protocol-only adapters for the user-facing LSP features.
//!
//! The process/client modules deliberately expose JSON-RPC values.  This
//! module is the trust boundary between those values and the editor buffer:
//! locations are restricted to the selected workspace, positions used for
//! edits are converted strictly, and a workspace edit is staged completely
//! before the document store is mutated.

use super::documents::{
    AtomicDocumentChange, DocumentError, DocumentSnapshot, DocumentStore, RequestSnapshot,
    WorkspaceRoot,
};
use super::positions::{LspPosition, LspRange, PositionEncoding};
use super::transport::RpcId;
use lsp_types as lsp;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use url::Url;

/// Metadata captured when a request is sent.  It is intentionally separate
/// from LSP params because document versions are a client-side stale-result
/// guard and are not part of most LSP request parameter objects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestMetadata {
    pub uri: String,
    pub version: i32,
}

impl RequestMetadata {
    pub fn new(uri: impl Into<String>, version: i32) -> Self {
        Self {
            uri: uri.into(),
            version,
        }
    }
}

/// A response paired with the request snapshot that it belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FeatureResponse<T> {
    pub metadata: RequestMetadata,
    pub value: T,
    pub stale: bool,
}

impl<T> FeatureResponse<T> {
    pub fn new(metadata: RequestMetadata, value: T, stale: bool) -> Self {
        Self {
            metadata,
            value,
            stale,
        }
    }
}

/// The source of a diagnostic result.  Pull reports use the request snapshot
/// as their version because LSP's pull report itself has no document version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticOrigin {
    Push,
    Pull,
}

/// A diagnostic representation safe to pass to a frontend lint adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidatedDiagnostic {
    pub range: LspRange,
    pub severity: Option<u32>,
    pub code: Option<String>,
    pub source: Option<String>,
    pub message: String,
}

/// A validated push or pull diagnostic result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticResult {
    pub uri: String,
    pub version: Option<i32>,
    pub diagnostics: Vec<ValidatedDiagnostic>,
    pub origin: DiagnosticOrigin,
    #[serde(default)]
    pub result_id: Option<String>,
    #[serde(default)]
    pub unchanged: bool,
    #[serde(default)]
    pub stale: bool,
}

/// The in-memory diagnostic cache.  Stale results are observable by callers
/// but never replace the most recent accepted result.
#[derive(Debug, Default, Clone)]
pub struct DiagnosticStore {
    latest: BTreeMap<String, DiagnosticResult>,
}

impl DiagnosticStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn latest(&self, uri: &str) -> Option<&DiagnosticResult> {
        self.latest.get(uri)
    }

    pub fn clear(&mut self, uri: &str) {
        self.latest.remove(uri);
    }

    pub fn clear_all(&mut self) {
        self.latest.clear();
    }

    pub fn len(&self) -> usize {
        self.latest.len()
    }

    pub fn is_empty(&self) -> bool {
        self.latest.is_empty()
    }

    /// Apply a validated response.  A stale response is returned to the
    /// caller for logging/UI state but does not overwrite `latest`.
    pub fn apply(
        &mut self,
        response: FeatureResponse<DiagnosticResult>,
    ) -> FeatureResponse<DiagnosticResult> {
        if !response.stale {
            if response.value.unchanged {
                if let Some(previous) = self.latest.get(&response.value.uri) {
                    let mut refreshed = response.value.clone();
                    refreshed.diagnostics = previous.diagnostics.clone();
                    self.latest.insert(response.value.uri.clone(), refreshed);
                }
            } else {
                self.latest
                    .insert(response.value.uri.clone(), response.value.clone());
            }
        }
        response
    }

    pub fn accept_push(
        &mut self,
        result: impl Into<DiagnosticResult>,
        current: &RequestMetadata,
    ) -> Result<FeatureResponse<DiagnosticResult>, FeatureError> {
        let response = validate_push_diagnostics(result, current)?;
        Ok(self.apply(response))
    }

    pub fn accept_pull(
        &mut self,
        result: impl Into<DiagnosticResult>,
        current: &RequestMetadata,
    ) -> Result<FeatureResponse<DiagnosticResult>, FeatureError> {
        let response = validate_pull_diagnostics(result, current)?;
        Ok(self.apply(response))
    }

    /// Mark every cached report stale when its server session disappears. The
    /// stale response is emitted to the UI, but remains in the cache so the
    /// last useful messages can still be inspected while the server restarts.
    pub fn mark_all_stale(
        &mut self,
        current: &[(String, i32)],
    ) -> Vec<FeatureResponse<DiagnosticResult>> {
        let versions = current
            .iter()
            .map(|(uri, version)| (uri.as_str(), *version))
            .collect::<BTreeMap<_, _>>();
        self.latest
            .values_mut()
            .map(|result| {
                result.stale = true;
                let version = versions
                    .get(result.uri.as_str())
                    .copied()
                    .or(result.version)
                    .unwrap_or_default();
                FeatureResponse::new(
                    RequestMetadata::new(result.uri.clone(), version),
                    result.clone(),
                    true,
                )
            })
            .collect()
    }
}

impl From<&DiagnosticResult> for DiagnosticResult {
    fn from(value: &DiagnosticResult) -> Self {
        value.clone()
    }
}

/// Locations are normalized to strings so that the frontend never has to
/// understand the two LSP definition response shapes (Location and
/// LocationLink).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocationTarget {
    pub uri: String,
    pub range: LspRange,
    #[serde(default)]
    pub selection_range: Option<LspRange>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilteredLocations {
    pub locations: Vec<LocationTarget>,
    pub rejected: usize,
}

/// Completion data kept deliberately close to the LSP representation.  The
/// adapter never executes `CompletionItem.command`; command/code-action
/// execution is outside this bounded feature surface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionResult {
    pub is_incomplete: bool,
    pub items: Vec<lsp::CompletionItem>,
}

/// A normalized hover result.  `MarkedString` language blocks are represented
/// as fenced markdown, while a `MarkupContent` retains whether it is markdown
/// or plaintext until sanitization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", tag = "kind", content = "value")]
pub enum HoverText {
    Markdown(String),
    Plaintext(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HoverResult {
    pub text: HoverText,
    #[serde(default)]
    pub range: Option<LspRange>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SanitizedHover {
    pub text: String,
    pub markdown: bool,
    #[serde(default)]
    pub range: Option<LspRange>,
}

/// A text edit whose range has been checked against the current document.
/// Offsets are kept private so callers cannot accidentally bypass the
/// position conversion when constructing an edit for mutation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidatedTextEdit {
    pub uri: String,
    pub range: LspRange,
    pub new_text: String,
    #[serde(skip)]
    start_offset: usize,
    #[serde(skip)]
    end_offset: usize,
}

impl ValidatedTextEdit {
    pub fn start_offset(&self) -> usize {
        self.start_offset
    }

    pub fn end_offset(&self) -> usize {
        self.end_offset
    }
}

/// A fully staged workspace edit.  `changes` contains one final replacement
/// per open document; no mutation has happened when this plan is returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceEditPlan {
    pub changes: Vec<AtomicDocumentChange>,
    pub edits: Vec<ValidatedTextEdit>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedWorkspaceEdit {
    pub changes: Vec<super::documents::DidChange>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeatureError {
    InvalidParams(String),
    InvalidUri(String),
    OutsideWorkspace(String),
    DocumentNotOpen(String),
    ResponseForDifferentDocument {
        expected: String,
        actual: String,
    },
    Stale {
        uri: String,
        expected: i32,
        actual: Option<i32>,
    },
    InvalidRange {
        uri: String,
        message: String,
    },
    OverlappingEdits {
        uri: String,
    },
    VersionConflict {
        uri: String,
        expected: i32,
        actual: i32,
    },
    NonNormalizedText {
        uri: String,
    },
    UnsupportedWorkspaceOperation(String),
    InvalidCompletionEdit(String),
    Document(String),
}

impl fmt::Display for FeatureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidParams(message) => write!(f, "invalid LSP feature parameters: {message}"),
            Self::InvalidUri(uri) => write!(f, "invalid LSP URI: {uri}"),
            Self::OutsideWorkspace(uri) => write!(f, "LSP URI is outside the workspace: {uri}"),
            Self::DocumentNotOpen(uri) => write!(f, "LSP document is not open: {uri}"),
            Self::ResponseForDifferentDocument { expected, actual } => write!(
                f,
                "LSP response is for {actual}, but request was for {expected}"
            ),
            Self::Stale {
                uri,
                expected,
                actual,
            } => write!(
                f,
                "stale LSP result for {uri}: expected version {expected}, got {actual:?}"
            ),
            Self::InvalidRange { uri, message } => {
                write!(f, "invalid LSP range for {uri}: {message}")
            }
            Self::OverlappingEdits { uri } => write!(f, "overlapping LSP edits for {uri}"),
            Self::VersionConflict {
                uri,
                expected,
                actual,
            } => write!(
                f,
                "LSP edit version conflict for {uri}: expected {expected}, current {actual}"
            ),
            Self::NonNormalizedText { uri } => {
                write!(f, "LSP edit for {uri} contains CR instead of LF")
            }
            Self::UnsupportedWorkspaceOperation(operation) => {
                write!(f, "unsupported WorkspaceEdit operation: {operation}")
            }
            Self::InvalidCompletionEdit(message) => write!(f, "invalid completion edit: {message}"),
            Self::Document(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for FeatureError {}

impl From<DocumentError> for FeatureError {
    fn from(error: DocumentError) -> Self {
        match error {
            DocumentError::DocumentNotOpen(uri) => Self::DocumentNotOpen(uri),
            DocumentError::VersionMismatch {
                uri,
                expected,
                actual,
            } => Self::VersionConflict {
                uri,
                expected,
                actual,
            },
            DocumentError::PathOutsideWorkspace(path) => {
                Self::OutsideWorkspace(path.display().to_string())
            }
            other => Self::Document(other.to_string()),
        }
    }
}

/// Build the common text-document/position request shape.
pub fn build_completion_params<T: RequestTarget>(target: T, position: LspPosition) -> Value {
    position_params(target, position)
}

pub fn build_hover_params<T: RequestTarget>(target: T, position: LspPosition) -> Value {
    position_params(target, position)
}

pub fn build_definition_params<T: RequestTarget>(target: T, position: LspPosition) -> Value {
    position_params(target, position)
}

pub fn build_reference_params<T: RequestTarget>(
    target: T,
    position: LspPosition,
    include_declaration: bool,
) -> Value {
    let mut params = position_params(target, position);
    params["context"] = json!({ "includeDeclaration": include_declaration });
    params
}

/// Build a document diagnostic pull request.  The request snapshot version is
/// intentionally kept in `RequestMetadata`; LSP pull reports do not echo it.
pub fn build_pull_diagnostics_params<T: RequestTarget>(target: T) -> Value {
    json!({
        "textDocument": { "uri": target.uri() },
    })
}

pub fn build_rename_params<T: RequestTarget>(
    target: T,
    position: LspPosition,
    new_name: impl Into<String>,
) -> Value {
    let mut params = position_params(target, position);
    params["newName"] = Value::String(new_name.into());
    params
}

pub fn build_formatting_params<T: RequestTarget, O: FormattingOptionsInput>(
    target: T,
    options: O,
) -> Value {
    json!({
        "textDocument": { "uri": target.uri() },
        "options": options.into_value(),
    })
}

fn position_params<T: RequestTarget>(target: T, position: LspPosition) -> Value {
    json!({
        "textDocument": { "uri": target.uri() },
        "position": position,
    })
}

/// Metadata extraction used by all feature request builders.
pub trait RequestTarget {
    fn uri(&self) -> &str;
    fn version(&self) -> i32;
}

impl RequestTarget for RequestMetadata {
    fn uri(&self) -> &str {
        &self.uri
    }

    fn version(&self) -> i32 {
        self.version
    }
}

impl RequestTarget for &RequestMetadata {
    fn uri(&self) -> &str {
        &self.uri
    }

    fn version(&self) -> i32 {
        self.version
    }
}

impl RequestTarget for RequestSnapshot {
    fn uri(&self) -> &str {
        &self.uri
    }

    fn version(&self) -> i32 {
        self.version
    }
}

impl RequestTarget for &RequestSnapshot {
    fn uri(&self) -> &str {
        &self.uri
    }

    fn version(&self) -> i32 {
        self.version
    }
}

impl RequestTarget for DocumentSnapshot {
    fn uri(&self) -> &str {
        &self.uri
    }

    fn version(&self) -> i32 {
        self.version
    }
}

impl RequestTarget for &DocumentSnapshot {
    fn uri(&self) -> &str {
        &self.uri
    }

    fn version(&self) -> i32 {
        self.version
    }
}

pub trait FormattingOptionsInput {
    fn into_value(self) -> Value;
}

impl FormattingOptionsInput for lsp::FormattingOptions {
    fn into_value(self) -> Value {
        serde_json::to_value(self).expect("FormattingOptions is serializable")
    }
}

impl FormattingOptionsInput for &lsp::FormattingOptions {
    fn into_value(self) -> Value {
        serde_json::to_value(self).expect("FormattingOptions is serializable")
    }
}

impl FormattingOptionsInput for (u32, bool) {
    fn into_value(self) -> Value {
        json!({ "tabSize": self.0, "insertSpaces": self.1 })
    }
}

/// Build `$/cancelRequest` params without exposing a way to execute an
/// arbitrary server command.
pub fn cancel_request<I: CancelRequestId>(id: I) -> Value {
    json!({ "id": id.into_value() })
}

pub trait CancelRequestId {
    fn into_value(self) -> Value;
}

impl CancelRequestId for u64 {
    fn into_value(self) -> Value {
        Value::from(self)
    }
}

impl CancelRequestId for i64 {
    fn into_value(self) -> Value {
        Value::from(self)
    }
}

impl CancelRequestId for String {
    fn into_value(self) -> Value {
        Value::String(self)
    }
}

impl CancelRequestId for &str {
    fn into_value(self) -> Value {
        Value::String(self.to_owned())
    }
}

impl CancelRequestId for RpcId {
    fn into_value(self) -> Value {
        match self {
            RpcId::Number(value) => Value::Number(value),
            RpcId::String(value) => Value::String(value),
            RpcId::Null => Value::Null,
        }
    }
}

impl CancelRequestId for &RpcId {
    fn into_value(self) -> Value {
        self.clone().into_value()
    }
}

/// Parse a `textDocument/publishDiagnostics` notification's params.
pub fn parse_publish_diagnostics(value: &Value) -> Result<DiagnosticResult, FeatureError> {
    let object = value.as_object().ok_or_else(|| {
        FeatureError::InvalidParams("publishDiagnostics params must be an object".into())
    })?;
    let uri = required_uri(object.get("uri"))?;
    let version = parse_optional_i32(object.get("version"), "diagnostics.version")?;
    let diagnostics = parse_diagnostics(object.get("diagnostics"), &uri)?;
    Ok(DiagnosticResult {
        uri,
        version,
        diagnostics,
        origin: DiagnosticOrigin::Push,
        result_id: None,
        unchanged: false,
        stale: false,
    })
}

/// Parse a document diagnostic pull response.  The request snapshot supplies
/// the version that the response describes because LSP pull reports do not
/// carry one themselves.
pub fn parse_pull_diagnostics(
    value: &Value,
    request: &RequestMetadata,
) -> Result<DiagnosticResult, FeatureError> {
    let (diagnostics, result_id, unchanged) = if value.is_null() {
        (Vec::new(), None, false)
    } else {
        let object = value.as_object().ok_or_else(|| {
            FeatureError::InvalidParams("pull diagnostic result must be an object".into())
        })?;
        let kind = object.get("kind").and_then(Value::as_str).ok_or_else(|| {
            FeatureError::InvalidParams("pull diagnostic result.kind is required".into())
        })?;
        match kind {
            "full" => (
                parse_diagnostics(object.get("items"), &request.uri)?,
                object
                    .get("resultId")
                    .map(|value| parse_string(value, "diagnostic resultId"))
                    .transpose()?,
                false,
            ),
            "unchanged" => (
                Vec::new(),
                Some(parse_string(
                    object.get("resultId").ok_or_else(|| {
                        FeatureError::InvalidParams(
                            "unchanged diagnostic report requires resultId".into(),
                        )
                    })?,
                    "diagnostic resultId",
                )?),
                true,
            ),
            other => {
                return Err(FeatureError::InvalidParams(format!(
                    "unknown pull diagnostic report kind {other:?}"
                )))
            }
        }
    };
    Ok(DiagnosticResult {
        uri: request.uri.clone(),
        version: Some(request.version),
        diagnostics,
        origin: DiagnosticOrigin::Pull,
        result_id,
        unchanged,
        stale: false,
    })
}

pub fn validate_push_diagnostics(
    result: impl Into<DiagnosticResult>,
    current: &RequestMetadata,
) -> Result<FeatureResponse<DiagnosticResult>, FeatureError> {
    validate_diagnostics(result.into(), DiagnosticOrigin::Push, current)
}

pub fn validate_pull_diagnostics(
    result: impl Into<DiagnosticResult>,
    current: &RequestMetadata,
) -> Result<FeatureResponse<DiagnosticResult>, FeatureError> {
    validate_diagnostics(result.into(), DiagnosticOrigin::Pull, current)
}

fn validate_diagnostics(
    mut result: DiagnosticResult,
    expected_origin: DiagnosticOrigin,
    current: &RequestMetadata,
) -> Result<FeatureResponse<DiagnosticResult>, FeatureError> {
    if result.origin != expected_origin {
        return Err(FeatureError::InvalidParams(format!(
            "expected {expected_origin:?} diagnostics, got {:?}",
            result.origin
        )));
    }
    if expected_origin == DiagnosticOrigin::Push && result.version.is_none() {
        return Err(FeatureError::InvalidParams(
            "versionless publishDiagnostics is ignored".into(),
        ));
    }
    if result.uri != current.uri {
        return Err(FeatureError::ResponseForDifferentDocument {
            expected: current.uri.clone(),
            actual: result.uri.clone(),
        });
    }
    for diagnostic in &result.diagnostics {
        if !range_is_ordered(diagnostic.range) {
            return Err(FeatureError::InvalidRange {
                uri: result.uri.clone(),
                message: "diagnostic end precedes start".into(),
            });
        }
    }
    let stale = result.version != Some(current.version);
    result.stale = stale;
    Ok(FeatureResponse::new(current.clone(), result, stale))
}

fn parse_diagnostics(
    value: Option<&Value>,
    uri: &str,
) -> Result<Vec<ValidatedDiagnostic>, FeatureError> {
    let array = value.and_then(Value::as_array).ok_or_else(|| {
        FeatureError::InvalidParams(format!("diagnostics for {uri} must be an array"))
    })?;
    array
        .iter()
        .map(|value| {
            let diagnostic: lsp::Diagnostic =
                serde_json::from_value(value.clone()).map_err(|error| {
                    FeatureError::InvalidParams(format!("invalid diagnostic for {uri}: {error}"))
                })?;
            let range = from_lsp_range(diagnostic.range);
            if !range_is_ordered(range) {
                return Err(FeatureError::InvalidRange {
                    uri: uri.to_owned(),
                    message: "diagnostic end precedes start".into(),
                });
            }
            let severity = diagnostic
                .severity
                .map(|severity| serde_json::to_value(severity).expect("severity is serializable"))
                .and_then(|value| value.as_u64())
                .and_then(|value| u32::try_from(value).ok());
            let code = diagnostic.code.map(|code| match code {
                lsp::NumberOrString::Number(number) => number.to_string(),
                lsp::NumberOrString::String(string) => string,
            });
            Ok(ValidatedDiagnostic {
                range,
                severity,
                code,
                source: diagnostic.source,
                message: diagnostic.message,
            })
        })
        .collect()
}

/// Parse completion results into a stable array regardless of whether the
/// server returned the short array or the `CompletionList` form.
pub fn parse_completion_response(value: &Value) -> Result<CompletionResult, FeatureError> {
    if value.is_null() {
        return Ok(CompletionResult {
            is_incomplete: false,
            items: Vec::new(),
        });
    }
    let response: lsp::CompletionResponse =
        serde_json::from_value(value.clone()).map_err(|error| {
            FeatureError::InvalidParams(format!("invalid completion response: {error}"))
        })?;
    let mut result = match response {
        lsp::CompletionResponse::Array(items) => CompletionResult {
            is_incomplete: false,
            items,
        },
        lsp::CompletionResponse::List(list) => CompletionResult {
            is_incomplete: list.is_incomplete,
            items: list.items,
        },
    };
    // Completion commands and opaque `data` are server-provided instructions
    // for a later resolve/execute round trip.  The bounded adapter exposes
    // text/documentation only and never forwards either to the UI.
    for item in &mut result.items {
        item.command = None;
        item.data = None;
    }
    Ok(result)
}

/// Validate completion text edits without applying them.  Snippets are kept
/// as data for a future editor adapter; no command attached to an item is ever
/// executed by this module.
pub fn validate_completion_response(
    result: impl Into<CompletionResult>,
    text: &str,
    request_position: LspPosition,
    encoding: PositionEncoding,
) -> Result<CompletionResult, FeatureError> {
    let mut result = result.into();
    for item in &mut result.items {
        item.command = None;
        item.data = None;
    }
    for item in &result.items {
        if item.label.trim().is_empty() {
            return Err(FeatureError::InvalidCompletionEdit(
                "completion label cannot be empty".into(),
            ));
        }
        let mut ranges = Vec::new();
        if let Some(text_edit) = &item.text_edit {
            match text_edit {
                lsp::CompletionTextEdit::Edit(edit) => {
                    let (start, end) =
                        strict_edit_offsets("completion", text, edit.range, encoding)?;
                    if edit.range.start.line != edit.range.end.line
                        || !position_between(request_position, edit.range.start, edit.range.end)
                    {
                        return Err(FeatureError::InvalidCompletionEdit(format!(
                            "main text edit for {:?} does not contain the request position",
                            item.label
                        )));
                    }
                    ranges.push((start, end));
                }
                lsp::CompletionTextEdit::InsertAndReplace(edit) => {
                    let (insert_start, insert_end) =
                        strict_edit_offsets("completion insert", text, edit.insert, encoding)?;
                    let (replace_start, replace_end) =
                        strict_edit_offsets("completion replace", text, edit.replace, encoding)?;
                    if edit.insert.start.line != edit.insert.end.line
                        || edit.replace.start.line != edit.replace.end.line
                        || !position_between(request_position, edit.insert.start, edit.insert.end)
                        || !position_between(request_position, edit.replace.start, edit.replace.end)
                        || edit.insert.start != edit.replace.start
                        || insert_start < replace_start
                        || insert_end > replace_end
                    {
                        return Err(FeatureError::InvalidCompletionEdit(format!(
                            "insert/replace ranges for {:?} are invalid",
                            item.label
                        )));
                    }
                    // The insert range is an alternate selection mode, not a
                    // second edit. The replace range is the main edit used
                    // for overlap checking below.
                    ranges.push((replace_start, replace_end));
                }
            }
        }
        if let Some(additional) = &item.additional_text_edits {
            for edit in additional {
                ranges.push(strict_edit_offsets(
                    "completion additional",
                    text,
                    edit.range,
                    encoding,
                )?);
            }
        }
        ranges.sort_unstable();
        for pair in ranges.windows(2) {
            if pair[0].1 > pair[1].0 || (pair[0].0 == pair[1].0 && pair[0].1 == pair[1].1) {
                return Err(FeatureError::InvalidCompletionEdit(format!(
                    "edits for {:?} overlap",
                    item.label
                )));
            }
        }
    }
    Ok(result)
}

/// Parse `textDocument/hover`, including the legal `null` result.
pub fn parse_hover_response(value: &Value) -> Result<Option<HoverResult>, FeatureError> {
    if value.is_null() {
        return Ok(None);
    }
    let hover: lsp::Hover = serde_json::from_value(value.clone())
        .map_err(|error| FeatureError::InvalidParams(format!("invalid hover response: {error}")))?;
    let text = match hover.contents {
        lsp::HoverContents::Scalar(marked) => {
            marked_strings_to_markdown(std::slice::from_ref(&marked))
        }
        lsp::HoverContents::Array(marked) => marked_strings_to_markdown(&marked),
        lsp::HoverContents::Markup(markup) => match markup.kind {
            lsp::MarkupKind::Markdown => HoverText::Markdown(markup.value),
            lsp::MarkupKind::PlainText => HoverText::Plaintext(markup.value),
        },
    };
    let range = hover.range.map(from_lsp_range);
    if range.is_some_and(|range| !range_is_ordered(range)) {
        return Err(FeatureError::InvalidParams(
            "hover range end precedes start".into(),
        ));
    }
    Ok(Some(HoverResult { text, range }))
}

pub fn sanitize_hover(hover: impl Into<HoverResult>) -> SanitizedHover {
    let hover = hover.into();
    let (text, markdown) = match hover.text {
        HoverText::Markdown(value) => (sanitize_markup(&value), true),
        HoverText::Plaintext(value) => (sanitize_markup(&value), false),
    };
    SanitizedHover {
        text,
        markdown,
        range: hover.range,
    }
}

impl From<&HoverResult> for HoverResult {
    fn from(value: &HoverResult) -> Self {
        value.clone()
    }
}

fn marked_strings_to_markdown(values: &[lsp::MarkedString]) -> HoverText {
    let text = values
        .iter()
        .map(|value| match value {
            lsp::MarkedString::String(value) => value.clone(),
            lsp::MarkedString::LanguageString(value) => {
                format!("```{}\n{}\n```", value.language, value.value)
            }
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    HoverText::Markdown(text)
}

fn sanitize_markup(value: &str) -> String {
    // LSP markdown is untrusted server output.  Remove raw tags (including
    // script/event-handler tags) and neutralize javascript/data navigation
    // schemes before the frontend's markdown renderer sees the text.
    let mut output = String::with_capacity(value.len());
    let mut in_tag = false;
    for character in value.chars() {
        match character {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if in_tag => {}
            _ => output.push(character),
        }
    }
    let lower = output.to_ascii_lowercase();
    let mut safe = String::with_capacity(output.len());
    let mut cursor = 0;
    while cursor < output.len() {
        let rest = &lower[cursor..];
        let (prefix_len, replacement) = if rest.starts_with("javascript:") {
            ("javascript:".len(), "")
        } else if rest.starts_with("data:") {
            ("data:".len(), "")
        } else {
            (0, "")
        };
        if prefix_len != 0 {
            cursor += prefix_len;
            safe.push_str(replacement);
        } else if let Some(character) = output[cursor..].chars().next() {
            safe.push(character);
            cursor += character.len_utf8();
        } else {
            break;
        }
    }
    safe
}

/// Parse the scalar/array/LocationLink variants of definition responses.
pub fn parse_definition_response(value: &Value) -> Result<Vec<LocationTarget>, FeatureError> {
    if value.is_null() {
        return Ok(Vec::new());
    }
    let response: lsp::GotoDefinitionResponse =
        serde_json::from_value(value.clone()).map_err(|error| {
            FeatureError::InvalidParams(format!("invalid definition response: {error}"))
        })?;
    Ok(match response {
        lsp::GotoDefinitionResponse::Scalar(location) => vec![location_target(location)],
        lsp::GotoDefinitionResponse::Array(locations) => {
            locations.into_iter().map(location_target).collect()
        }
        lsp::GotoDefinitionResponse::Link(links) => links
            .into_iter()
            .map(|link| LocationTarget {
                uri: link.target_uri.as_str().to_owned(),
                range: from_lsp_range(link.target_range),
                selection_range: Some(from_lsp_range(link.target_selection_range)),
            })
            .collect(),
    })
}

pub fn parse_reference_locations(value: &Value) -> Result<Vec<LocationTarget>, FeatureError> {
    if value.is_null() {
        return Ok(Vec::new());
    }
    let locations: Vec<lsp::Location> = serde_json::from_value(value.clone()).map_err(|error| {
        FeatureError::InvalidParams(format!("invalid references response: {error}"))
    })?;
    Ok(locations.into_iter().map(location_target).collect())
}

fn location_target(location: lsp::Location) -> LocationTarget {
    LocationTarget {
        uri: location.uri.as_str().to_owned(),
        range: from_lsp_range(location.range),
        selection_range: None,
    }
}

/// Keep only existing local files below the canonical workspace root.
pub fn filter_definition_response(
    workspace: &WorkspaceRoot,
    locations: &[LocationTarget],
) -> FilteredLocations {
    filter_locations(workspace, locations)
}

pub fn filter_reference_locations(
    workspace: &WorkspaceRoot,
    locations: &[LocationTarget],
) -> FilteredLocations {
    filter_locations(workspace, locations)
}

fn filter_locations(workspace: &WorkspaceRoot, locations: &[LocationTarget]) -> FilteredLocations {
    let mut accepted = Vec::with_capacity(locations.len());
    let mut rejected = 0;
    for location in locations {
        let Ok(uri) = Url::parse(&location.uri) else {
            rejected += 1;
            continue;
        };
        let Ok(path) = workspace.resolve_uri(&uri) else {
            rejected += 1;
            continue;
        };
        if !range_is_ordered(location.range)
            || location
                .selection_range
                .is_some_and(|range| !range_is_ordered(range))
        {
            rejected += 1;
            continue;
        }
        let Ok(canonical) = super::documents::file_uri_from_absolute_path(&path) else {
            rejected += 1;
            continue;
        };
        let mut normalized = location.clone();
        normalized.uri = canonical.as_str().to_owned();
        accepted.push(normalized);
    }
    FilteredLocations {
        locations: accepted,
        rejected,
    }
}

/// Validate and stage a text-only WorkspaceEdit.  Plain `changes` entries are
/// associated with the current open-document version; versioned
/// `documentChanges` entries must match that version exactly.
pub fn preflight_workspace_edit(
    store: &DocumentStore,
    edit: &lsp::WorkspaceEdit,
) -> Result<WorkspaceEditPlan, FeatureError> {
    if edit.changes.is_some() && edit.document_changes.is_some() {
        return Err(FeatureError::InvalidParams(
            "WorkspaceEdit cannot contain both changes and documentChanges".into(),
        ));
    }

    let mut documents = BTreeMap::<String, StagedDocument>::new();
    if let Some(changes) = &edit.changes {
        for (uri, edits) in changes {
            stage_document_edits(store, &mut documents, uri.as_str(), None, edits)?;
        }
    }
    if let Some(document_changes) = &edit.document_changes {
        match document_changes {
            lsp::DocumentChanges::Edits(edits) => {
                for edit in edits {
                    let text_edits = extract_text_edits(&edit.edits);
                    stage_document_edits(
                        store,
                        &mut documents,
                        edit.text_document.text_document_uri(),
                        edit.text_document.version,
                        &text_edits,
                    )?;
                }
            }
            lsp::DocumentChanges::Operations(operations) => {
                for operation in operations {
                    match operation {
                        lsp::DocumentChangeOperation::Edit(edit) => {
                            let text_edits = extract_text_edits(&edit.edits);
                            stage_document_edits(
                                store,
                                &mut documents,
                                edit.text_document.text_document_uri(),
                                edit.text_document.version,
                                &text_edits,
                            )?;
                        }
                        lsp::DocumentChangeOperation::Op(operation) => {
                            return Err(FeatureError::UnsupportedWorkspaceOperation(
                                resource_operation_name(operation).to_owned(),
                            ))
                        }
                    }
                }
            }
        }
    }

    let mut changes = Vec::with_capacity(documents.len());
    let mut edits = Vec::new();
    for (_, staged) in documents {
        edits.extend(staged.edits.clone());
        changes.push(AtomicDocumentChange {
            uri: staged.uri,
            expected_version: staged.expected_version,
            text: staged.new_text,
        });
    }
    Ok(WorkspaceEditPlan { changes, edits })
}

/// Apply only a previously preflighted plan.  `DocumentStore` rechecks every
/// expected version on a cloned map, so a race after preflight still cannot
/// produce a partial edit.
pub fn apply_workspace_edit(
    store: &mut DocumentStore,
    plan: &WorkspaceEditPlan,
) -> Result<AppliedWorkspaceEdit, FeatureError> {
    let changes = store.apply_atomic_changes(&plan.changes)?;
    Ok(AppliedWorkspaceEdit { changes })
}

/// Input accepted by the explicit formatting adapter.  Supporting both raw
/// JSON (the process boundary) and typed edits keeps parsing at this boundary
/// without adding a second command path.
pub trait FormattingEditsInput {
    fn into_edits(self) -> Result<Vec<lsp::TextEdit>, FeatureError>;
}

impl FormattingEditsInput for Vec<lsp::TextEdit> {
    fn into_edits(self) -> Result<Vec<lsp::TextEdit>, FeatureError> {
        Ok(self)
    }
}

impl FormattingEditsInput for &[lsp::TextEdit] {
    fn into_edits(self) -> Result<Vec<lsp::TextEdit>, FeatureError> {
        Ok(self.to_vec())
    }
}

impl FormattingEditsInput for &Vec<lsp::TextEdit> {
    fn into_edits(self) -> Result<Vec<lsp::TextEdit>, FeatureError> {
        Ok(self.clone())
    }
}

impl FormattingEditsInput for Value {
    fn into_edits(self) -> Result<Vec<lsp::TextEdit>, FeatureError> {
        serde_json::from_value(self).map_err(|error| {
            FeatureError::InvalidParams(format!("invalid formatting edits: {error}"))
        })
    }
}

impl FormattingEditsInput for &Value {
    fn into_edits(self) -> Result<Vec<lsp::TextEdit>, FeatureError> {
        serde_json::from_value(self.clone()).map_err(|error| {
            FeatureError::InvalidParams(format!("invalid formatting edits: {error}"))
        })
    }
}

pub fn apply_formatting_edits<I: FormattingEditsInput>(
    store: &mut DocumentStore,
    uri: &str,
    expected_version: i32,
    edits: I,
) -> Result<AppliedWorkspaceEdit, FeatureError> {
    let plan = preflight_formatting_edits(store, uri, expected_version, edits)?;
    apply_workspace_edit(store, &plan)
}

/// Validate formatting edits without changing the store. This mirrors
/// [`preflight_workspace_edit`] so callers can recheck request-time versions
/// immediately before committing a server response.
pub fn preflight_formatting_edits<I: FormattingEditsInput>(
    store: &DocumentStore,
    uri: &str,
    expected_version: i32,
    edits: I,
) -> Result<WorkspaceEditPlan, FeatureError> {
    let snapshot = store
        .snapshot(uri)
        .ok_or_else(|| FeatureError::DocumentNotOpen(uri.to_owned()))?;
    if snapshot.version != expected_version {
        return Err(FeatureError::VersionConflict {
            uri: uri.to_owned(),
            expected: expected_version,
            actual: snapshot.version,
        });
    }
    let uri = uri
        .parse::<lsp::Uri>()
        .map_err(|_| FeatureError::InvalidUri(uri.to_owned()))?;
    let edits = edits.into_edits()?;
    if edits.is_empty() {
        return Ok(WorkspaceEditPlan {
            changes: Vec::new(),
            edits: Vec::new(),
        });
    }
    let workspace_edit = lsp::WorkspaceEdit {
        changes: Some(HashMap::from([(uri, edits)])),
        document_changes: None,
        change_annotations: None,
    };
    preflight_workspace_edit(store, &workspace_edit)
}

struct StagedDocument {
    uri: String,
    expected_version: i32,
    new_text: String,
    edits: Vec<ValidatedTextEdit>,
}

fn stage_document_edits(
    store: &DocumentStore,
    documents: &mut BTreeMap<String, StagedDocument>,
    raw_uri: &str,
    expected_version: Option<i32>,
    edits: &[lsp::TextEdit],
) -> Result<(), FeatureError> {
    let uri = canonical_open_uri(store, raw_uri)?;
    if documents.contains_key(&uri) {
        return Err(FeatureError::InvalidParams(format!(
            "document appears more than once in WorkspaceEdit: {uri}"
        )));
    }
    let snapshot = store
        .snapshot(&uri)
        .ok_or_else(|| FeatureError::DocumentNotOpen(uri.clone()))?;
    let expected_version = expected_version.unwrap_or(snapshot.version);
    if expected_version != snapshot.version {
        return Err(FeatureError::VersionConflict {
            uri,
            expected: expected_version,
            actual: snapshot.version,
        });
    }
    if edits.is_empty() {
        return Ok(());
    }
    let mut validated = edits
        .iter()
        .map(|edit| validate_text_edit(&uri, &snapshot.text, edit, store.position_encoding()))
        .collect::<Result<Vec<_>, _>>()?;
    validated.sort_by_key(ValidatedTextEdit::start_offset);
    for pair in validated.windows(2) {
        if pair[0].end_offset > pair[1].start_offset
            || (pair[0].start_offset == pair[1].start_offset
                && pair[0].end_offset == pair[1].end_offset)
        {
            return Err(FeatureError::OverlappingEdits { uri });
        }
    }
    let mut new_text = snapshot.text.clone();
    for edit in validated.iter().rev() {
        new_text.replace_range(edit.start_offset..edit.end_offset, &edit.new_text);
    }
    if new_text.contains('\r') {
        return Err(FeatureError::NonNormalizedText { uri });
    }
    documents.insert(
        uri.clone(),
        StagedDocument {
            uri,
            expected_version,
            new_text,
            edits: validated,
        },
    );
    Ok(())
}

fn validate_text_edit(
    uri: &str,
    text: &str,
    edit: &lsp::TextEdit,
    encoding: PositionEncoding,
) -> Result<ValidatedTextEdit, FeatureError> {
    if edit.new_text.contains('\r') {
        return Err(FeatureError::NonNormalizedText {
            uri: uri.to_owned(),
        });
    }
    let (start_offset, end_offset) = strict_edit_offsets(uri, text, edit.range, encoding)?;
    Ok(ValidatedTextEdit {
        uri: uri.to_owned(),
        range: from_lsp_range(edit.range),
        new_text: edit.new_text.clone(),
        start_offset,
        end_offset,
    })
}

fn strict_edit_offsets(
    uri: &str,
    text: &str,
    range: lsp::Range,
    encoding: PositionEncoding,
) -> Result<(usize, usize), FeatureError> {
    let start = to_lsp_position(range.start);
    let end = to_lsp_position(range.end);
    let start_offset =
        super::positions::position_to_offset(text, start, encoding).map_err(|error| {
            FeatureError::InvalidRange {
                uri: uri.to_owned(),
                message: error.to_string(),
            }
        })?;
    let end_offset =
        super::positions::position_to_offset(text, end, encoding).map_err(|error| {
            FeatureError::InvalidRange {
                uri: uri.to_owned(),
                message: error.to_string(),
            }
        })?;
    if start_offset > end_offset {
        return Err(FeatureError::InvalidRange {
            uri: uri.to_owned(),
            message: "end precedes start".into(),
        });
    }
    Ok((start_offset, end_offset))
}

fn extract_text_edits(
    edits: &[lsp::OneOf<lsp::TextEdit, lsp::AnnotatedTextEdit>],
) -> Vec<lsp::TextEdit> {
    edits
        .iter()
        .map(|edit| match edit {
            lsp::OneOf::Left(edit) => edit.clone(),
            lsp::OneOf::Right(edit) => edit.text_edit.clone(),
        })
        .collect()
}

fn resource_operation_name(operation: &lsp::ResourceOp) -> &'static str {
    match operation {
        lsp::ResourceOp::Create(_) => "create",
        lsp::ResourceOp::Rename(_) => "rename",
        lsp::ResourceOp::Delete(_) => "delete",
    }
}

fn canonical_open_uri(store: &DocumentStore, raw_uri: &str) -> Result<String, FeatureError> {
    let uri = Url::parse(raw_uri).map_err(|_| FeatureError::InvalidUri(raw_uri.to_owned()))?;
    let path = store
        .workspace()
        .resolve_uri(&uri)
        .map_err(|error| match error {
            DocumentError::PathOutsideWorkspace(_) => {
                FeatureError::OutsideWorkspace(raw_uri.to_owned())
            }
            other => FeatureError::Document(other.to_string()),
        })?;
    let canonical = super::documents::file_uri_from_absolute_path(&path)?;
    let value = canonical.as_str().to_owned();
    if store.snapshot(&value).is_none() {
        return Err(FeatureError::DocumentNotOpen(value));
    }
    Ok(value)
}

fn required_uri(value: Option<&Value>) -> Result<String, FeatureError> {
    let uri = value
        .and_then(Value::as_str)
        .filter(|uri| !uri.trim().is_empty())
        .ok_or_else(|| FeatureError::InvalidParams("uri must be a non-empty string".into()))?;
    // Parse here so malformed response URIs cannot reach a frontend even when
    // the caller chooses not to apply workspace filtering.
    uri.parse::<lsp::Uri>()
        .map_err(|_| FeatureError::InvalidUri(uri.to_owned()))?;
    Ok(uri.to_owned())
}

fn parse_string(value: &Value, field: &str) -> Result<String, FeatureError> {
    value
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| FeatureError::InvalidParams(format!("{field} must be a string")))
}

fn parse_optional_i32(value: Option<&Value>, field: &str) -> Result<Option<i32>, FeatureError> {
    value
        .map(|value| {
            let number = value.as_i64().ok_or_else(|| {
                FeatureError::InvalidParams(format!("{field} must be an integer"))
            })?;
            i32::try_from(number)
                .map(Some)
                .map_err(|_| FeatureError::InvalidParams(format!("{field} is outside i32")))
        })
        .transpose()
        .map(|value| value.flatten())
}

fn from_lsp_range(range: lsp::Range) -> LspRange {
    LspRange {
        start: to_lsp_position(range.start),
        end: to_lsp_position(range.end),
    }
}

fn to_lsp_position(position: lsp::Position) -> LspPosition {
    LspPosition {
        line: position.line,
        character: position.character,
    }
}

fn range_is_ordered(range: LspRange) -> bool {
    position_key(range.start) <= position_key(range.end)
}

fn position_key(position: LspPosition) -> (u32, u32) {
    (position.line, position.character)
}

fn position_between(position: LspPosition, start: lsp::Position, end: lsp::Position) -> bool {
    let position = position_key(position);
    position_key(to_lsp_position(start)) <= position
        && position <= position_key(to_lsp_position(end))
}

// `OptionalVersionedTextDocumentIdentifier` intentionally has no URI helper;
// keeping the conversion here avoids relying on its private representation.
trait TextDocumentUri {
    fn text_document_uri(&self) -> &str;
}

impl TextDocumentUri for lsp::OptionalVersionedTextDocumentIdentifier {
    fn text_document_uri(&self) -> &str {
        self.uri.as_str()
    }
}

impl From<&CompletionResult> for CompletionResult {
    fn from(value: &CompletionResult) -> Self {
        value.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn document_store(text: &str) -> (tempfile::TempDir, String, DocumentStore) {
        let directory = tempdir().unwrap();
        let path = directory.path().join("main.rs");
        fs::write(&path, text).unwrap();
        let workspace = WorkspaceRoot::new(directory.path()).unwrap();
        let mut store = DocumentStore::new(
            workspace,
            PositionEncoding::Utf16,
            super::super::documents::SyncKind::Full,
        );
        let opened = store.open(&path, "rust", text).unwrap();
        (directory, opened.uri, store)
    }

    fn text_edit(start: (u32, u32), end: (u32, u32), new_text: &str) -> lsp::TextEdit {
        lsp::TextEdit::new(
            lsp::Range::new(
                lsp::Position::new(start.0, start.1),
                lsp::Position::new(end.0, end.1),
            ),
            new_text.into(),
        )
    }

    #[test]
    fn diagnostics_accept_current_version_and_keep_stale_result_out_of_cache() {
        let (_directory, uri, _store) = document_store("fn main() {}\n");
        let mut diagnostics = DiagnosticStore::new();
        let current = RequestMetadata::new(uri.clone(), 2);
        let result = parse_publish_diagnostics(&json!({
            "uri": uri,
            "version": 2,
            "diagnostics": [{
                "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 2 } },
                "severity": 1,
                "message": "bad"
            }]
        })).unwrap();
        let accepted = diagnostics.accept_push(result, &current).unwrap();
        assert!(!accepted.stale);
        assert_eq!(
            diagnostics.latest(&current.uri).unwrap().diagnostics.len(),
            1
        );

        let stale = parse_publish_diagnostics(&json!({
            "uri": current.uri,
            "version": 1,
            "diagnostics": []
        }))
        .unwrap();
        let stale = diagnostics.accept_push(stale, &current).unwrap();
        assert!(stale.stale);
        assert_eq!(
            diagnostics.latest(&current.uri).unwrap().diagnostics.len(),
            1
        );

        let invalid = DiagnosticResult {
            uri: current.uri.clone(),
            version: Some(current.version),
            diagnostics: vec![ValidatedDiagnostic {
                range: LspRange {
                    start: LspPosition::new(1, 0),
                    end: LspPosition::new(0, 0),
                },
                severity: None,
                code: None,
                source: None,
                message: "invalid".into(),
            }],
            origin: DiagnosticOrigin::Push,
            result_id: None,
            unchanged: false,
            stale: false,
        };
        assert!(matches!(
            validate_push_diagnostics(invalid, &current),
            Err(FeatureError::InvalidRange { .. })
        ));
    }

    #[test]
    fn versionless_publish_diagnostics_is_rejected_fail_closed() {
        let current = RequestMetadata::new("file:///workspace/main.rs", 1);
        let result = parse_publish_diagnostics(&json!({
            "uri": current.uri,
            "diagnostics": []
        }))
        .unwrap();
        let error = validate_push_diagnostics(result, &current).unwrap_err();
        assert!(error.to_string().contains("versionless"));
    }

    #[test]
    fn pull_diagnostics_associate_request_version_and_support_unchanged() {
        let current = RequestMetadata::new("file:///workspace/main.rs", 7);
        let full = parse_pull_diagnostics(
            &json!({ "kind": "full", "resultId": "a", "items": [] }),
            &current,
        )
        .unwrap();
        assert!(!validate_pull_diagnostics(full, &current).unwrap().stale);
        let old_request = RequestMetadata::new(current.uri.clone(), 6);
        let old_full = parse_pull_diagnostics(
            &json!({ "kind": "full", "resultId": "old", "items": [] }),
            &old_request,
        )
        .unwrap();
        assert!(validate_pull_diagnostics(old_full, &current).unwrap().stale);
        let unchanged =
            parse_pull_diagnostics(&json!({ "kind": "unchanged", "resultId": "a" }), &current)
                .unwrap();
        assert!(unchanged.unchanged);
    }

    #[test]
    fn completion_and_hover_are_normalized_without_executing_commands() {
        let completion = parse_completion_response(&json!({
            "isIncomplete": true,
            "items": [{
                "label": "println!",
                "command": { "command": "unsafe", "title": "run" },
                "data": { "opaque": "must-not-cross-ui-boundary" }
            }]
        }))
        .unwrap();
        assert!(completion.is_incomplete);
        assert_eq!(completion.items[0].label, "println!");
        assert!(completion.items[0].command.is_none());
        assert!(completion.items[0].data.is_none());
        assert!(validate_completion_response(
            completion,
            "pri",
            LspPosition::new(0, 3),
            PositionEncoding::Utf16,
        )
        .is_ok());

        let hover = parse_hover_response(&json!({
            "contents": { "kind": "markdown", "value": "**ok** <script>alert(1)</script> [x](javascript:bad)" }
        })).unwrap().unwrap();
        let safe = sanitize_hover(hover);
        assert!(!safe.text.contains("script"));
        assert!(!safe.text.to_ascii_lowercase().contains("javascript:"));
    }

    #[test]
    fn definition_and_reference_filter_workspace_and_canonicalize_uri() {
        let (_directory, uri, store) = document_store("fn main() {}\n");
        let inside = LocationTarget {
            uri: uri.clone(),
            range: LspRange {
                start: LspPosition::new(0, 0),
                end: LspPosition::new(0, 2),
            },
            selection_range: None,
        };
        let outside = LocationTarget {
            uri: "file:///tmp/not-in-workspace.rs".into(),
            range: inside.range,
            selection_range: None,
        };
        let filtered = filter_definition_response(store.workspace(), &[inside, outside]);
        assert_eq!(filtered.locations.len(), 1);
        assert_eq!(filtered.rejected, 1);
        assert_eq!(filtered.locations[0].uri, uri);
    }

    #[test]
    fn workspace_edit_preflight_is_all_or_nothing_and_checks_versions_ranges() {
        let (_directory, uri, mut store) = document_store("one 😀\ntwo\n");
        let workspace_edit = lsp::WorkspaceEdit {
            changes: Some(HashMap::from([(
                uri.parse().unwrap(),
                vec![text_edit((0, 0), (0, 3), "ONE")],
            )])),
            document_changes: None,
            change_annotations: None,
        };
        let plan = preflight_workspace_edit(&store, &workspace_edit).unwrap();
        store.change(&uri, "edited before apply\n", true).unwrap();
        assert!(matches!(
            apply_workspace_edit(&mut store, &plan),
            Err(FeatureError::VersionConflict {
                expected: 1,
                actual: 2,
                ..
            })
        ));
        assert_eq!(store.snapshot(&uri).unwrap().text, "edited before apply\n");

        let workspace_edit = lsp::WorkspaceEdit {
            changes: Some(HashMap::from([(
                uri.parse().unwrap(),
                vec![text_edit((0, 0), (1, 0), "ONE\n")],
            )])),
            document_changes: None,
            change_annotations: None,
        };
        let plan = preflight_workspace_edit(&store, &workspace_edit).unwrap();
        let applied = apply_workspace_edit(&mut store, &plan).unwrap();
        assert_eq!(applied.changes[0].version, 3);
        assert_eq!(store.snapshot(&uri).unwrap().text, "ONE\n");

        let bad = lsp::WorkspaceEdit {
            changes: Some(HashMap::from([(
                uri.parse().unwrap(),
                vec![text_edit((0, 3), (0, 1), "bad")],
            )])),
            document_changes: None,
            change_annotations: None,
        };
        assert!(matches!(
            preflight_workspace_edit(&store, &bad),
            Err(FeatureError::InvalidRange { .. })
        ));
        assert_eq!(store.snapshot(&uri).unwrap().text, "ONE\n");
    }

    #[test]
    fn formatting_rejects_crlf_and_overlapping_edits_before_mutation() {
        let (_directory, uri, mut store) = document_store("abc\n");
        let edits = vec![
            text_edit((0, 0), (0, 2), "x"),
            text_edit((0, 1), (0, 3), "y"),
        ];
        assert!(matches!(
            apply_formatting_edits(&mut store, &uri, 1, &edits),
            Err(FeatureError::OverlappingEdits { .. })
        ));
        assert_eq!(store.snapshot(&uri).unwrap().text, "abc\n");
        assert!(matches!(
            apply_formatting_edits(&mut store, &uri, 1, vec![text_edit((0, 0), (0, 1), "\r\n")]),
            Err(FeatureError::NonNormalizedText { .. })
        ));
    }
}
