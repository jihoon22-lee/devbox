//! Pure gRPC descriptor, ProtoJSON, and dynamic codec boundaries.
//!
//! Native dialogs, DPAPI, and connection ownership live in the command layer.
//! This module deliberately accepts already-authorized filesystem identities
//! and never returns parser, filesystem, network, or descriptor text.

use crate::core::{mcp, oauth};
use bytes::Buf;
use devbox_filesystem::{
    ensure_no_links, filesystem_identity, open_filesystem_object, FilesystemIdentity,
};
use prost::Message;
use prost_reflect::{
    DescriptorPool, DynamicMessage, MessageDescriptor, MethodDescriptor, ReflectMessage,
    SerializeOptions,
};
use protox::file::{ChainFileResolver, File, FileResolver, GoogleFileResolver};
use serde::de::IntoDeserializer;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use tonic::codec::{Codec, DecodeBuf, Decoder, EncodeBuf, Encoder};
use tonic::Status;
use url::{Host, Url};

pub const INVALID_PROFILE: &str = "grpc_invalid_profile";
pub const SOURCE_SELECTION_INVALID: &str = "grpc_source_selection_invalid";
pub const SOURCE_INVALID: &str = "grpc_source_invalid";
pub const SOURCE_TOO_LARGE: &str = "grpc_source_too_large";
pub const DESCRIPTOR_INVALID: &str = "grpc_descriptor_invalid";
pub const REFLECTION_UNAVAILABLE: &str = "grpc_reflection_unavailable";
pub const CONNECTION_LIMIT: &str = "grpc_connection_limit";
pub const CONNECT_TIMEOUT: &str = "grpc_connect_timeout";
pub const TLS_FAILED: &str = "grpc_tls_failed";
pub const CREDENTIAL_STORAGE_UNAVAILABLE: &str = "grpc_credential_storage_unavailable";
pub const CREDENTIAL_STORAGE_FAILED: &str = "grpc_credential_storage_failed";
pub const CREDENTIAL_INVALID: &str = "grpc_credential_invalid";
pub const CONNECTION_STALE: &str = "grpc_connection_stale";
pub const METHOD_UNAVAILABLE: &str = "grpc_method_unavailable";
pub const REQUEST_INVALID: &str = "grpc_request_invalid";
pub const REQUEST_TOO_LARGE: &str = "grpc_request_too_large";
pub const REQUEST_LIMIT: &str = "grpc_request_limit";
pub const REQUEST_TIMEOUT: &str = "grpc_request_timeout";
pub const REQUEST_CANCELLED: &str = "grpc_request_cancelled";
pub const RESPONSE_TOO_LARGE: &str = "grpc_response_too_large";
pub const PROTOCOL_FAILED: &str = "grpc_protocol_failed";
pub const EXPORT_FAILED: &str = "grpc_export_failed";

pub const MAX_ENDPOINT_BYTES: usize = 8 * 1024;
pub const MAX_NAME_BYTES: usize = 1024;
pub const MAX_SOURCE_FILES: usize = 256;
pub const MAX_SOURCE_FILE_BYTES: usize = 1024 * 1024;
pub const MAX_SOURCE_TOTAL_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_DESCRIPTOR_FILES: usize = 256;
pub const MAX_DESCRIPTOR_FILE_BYTES: usize = 1024 * 1024;
pub const MAX_DESCRIPTOR_TOTAL_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_SERVICES: usize = 256;
pub const MAX_METHODS: usize = 2_000;
pub const MAX_TYPES: usize = 5_000;
pub const MAX_TEMPLATE_BYTES: usize = 256 * 1024;
pub const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
pub const MAX_REQUEST_TOTAL_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_RESPONSE_TOTAL_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_STREAM_MESSAGES: usize = 100;

#[derive(Debug, Clone, Copy, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum GrpcRootMode {
    Native,
    Custom,
    NativeAndCustom,
}

impl GrpcRootMode {
    pub fn uses_native(self) -> bool {
        matches!(self, Self::Native | Self::NativeAndCustom)
    }

    pub fn uses_custom(self) -> bool {
        matches!(self, Self::Custom | Self::NativeAndCustom)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::Custom => "custom",
            Self::NativeAndCustom => "native+custom",
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum GrpcRpcKind {
    Unary,
    ServerStreaming,
    ClientStreaming,
    BidirectionalStreaming,
}

impl GrpcRpcKind {
    pub fn from_method(method: &MethodDescriptor) -> Self {
        match (method.is_client_streaming(), method.is_server_streaming()) {
            (false, false) => Self::Unary,
            (false, true) => Self::ServerStreaming,
            (true, false) => Self::ClientStreaming,
            (true, true) => Self::BidirectionalStreaming,
        }
    }

    pub fn accepts_multiple_requests(self) -> bool {
        matches!(self, Self::ClientStreaming | Self::BidirectionalStreaming)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrpcMethodProjection {
    pub service: String,
    pub method: String,
    pub full_name: String,
    pub input_type: String,
    pub output_type: String,
    pub rpc_kind: GrpcRpcKind,
    pub input_template: Value,
}

#[derive(Debug, Clone)]
pub struct DescriptorProjection {
    pub methods: Vec<GrpcMethodProjection>,
    pub descriptor_file_count: usize,
    pub service_count: usize,
}

#[derive(Debug, Clone)]
pub struct NormalizedGrpcEndpoint {
    pub uri: String,
    pub tls: bool,
    pub authority: String,
}

pub fn normalize_endpoint(value: &str) -> Result<NormalizedGrpcEndpoint, &'static str> {
    if value.is_empty() || value.len() > MAX_ENDPOINT_BYTES || value.chars().any(char::is_control) {
        return Err(INVALID_PROFILE);
    }
    let mut url = Url::parse(value).map_err(|_| INVALID_PROFILE)?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "" | "/")
        || url.port() == Some(0)
    {
        return Err(INVALID_PROFILE);
    }
    url.set_path("/");
    let host = match url.host().ok_or(INVALID_PROFILE)? {
        Host::Domain(value) => value.to_string(),
        Host::Ipv4(value) => value.to_string(),
        Host::Ipv6(value) => format!("[{value}]"),
    };
    let authority = match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host,
    };
    if authority.len() > MAX_NAME_BYTES || authority.chars().any(char::is_control) {
        return Err(INVALID_PROFILE);
    }
    Ok(NormalizedGrpcEndpoint {
        tls: url.scheme() == "https",
        uri: url.to_string(),
        authority,
    })
}

pub fn validate_server_name(value: Option<&str>) -> Result<Option<String>, &'static str> {
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    if value.len() > 253
        || !value.is_ascii()
        || value.chars().any(char::is_control)
        || value
            .bytes()
            .any(|byte| matches!(byte, b'/' | b'\\' | b'@' | b'?' | b'#' | b'[' | b']'))
        || value.bytes().any(|byte| byte.is_ascii_whitespace())
    {
        return Err(INVALID_PROFILE);
    }
    Ok(Some(value.to_string()))
}

pub fn validate_descriptor_pool(
    pool: &DescriptorPool,
) -> Result<DescriptorProjection, &'static str> {
    let descriptor_file_count = pool.files().len();
    let service_count = pool.services().len();
    let type_count = pool
        .all_messages()
        .len()
        .checked_add(pool.all_enums().len())
        .ok_or(DESCRIPTOR_INVALID)?;
    if descriptor_file_count == 0
        || descriptor_file_count > MAX_DESCRIPTOR_FILES
        || service_count > MAX_SERVICES
        || type_count > MAX_TYPES
    {
        return Err(DESCRIPTOR_INVALID);
    }

    let mut methods = Vec::new();
    let mut identities = BTreeSet::new();
    for service in pool.services() {
        validate_descriptor_name(service.full_name())?;
        if service.full_name().starts_with("grpc.reflection.") {
            continue;
        }
        for method in service.methods() {
            if methods.len() >= MAX_METHODS {
                return Err(DESCRIPTOR_INVALID);
            }
            validate_descriptor_name(method.full_name())?;
            validate_descriptor_name(method.name())?;
            validate_descriptor_name(method.input().full_name())?;
            validate_descriptor_name(method.output().full_name())?;
            if !identities.insert(method.full_name().to_string()) {
                return Err(DESCRIPTOR_INVALID);
            }
            methods.push(GrpcMethodProjection {
                service: service.full_name().to_string(),
                method: method.name().to_string(),
                full_name: method.full_name().to_string(),
                input_type: method.input().full_name().to_string(),
                output_type: method.output().full_name().to_string(),
                rpc_kind: GrpcRpcKind::from_method(&method),
                input_template: message_template(method.input())?,
            });
        }
    }
    methods.sort_by(|left, right| left.full_name.cmp(&right.full_name));
    if methods.is_empty() {
        return Err(DESCRIPTOR_INVALID);
    }
    Ok(DescriptorProjection {
        methods,
        descriptor_file_count,
        service_count,
    })
}

pub fn method_map(
    pool: &DescriptorPool,
) -> Result<HashMap<String, MethodDescriptor>, &'static str> {
    let projection = validate_descriptor_pool(pool)?;
    let allowed = projection
        .methods
        .into_iter()
        .map(|method| method.full_name)
        .collect::<BTreeSet<_>>();
    let mut methods = HashMap::with_capacity(allowed.len());
    for service in pool.services() {
        for method in service.methods() {
            if allowed.contains(method.full_name()) {
                methods.insert(method.full_name().to_string(), method);
            }
        }
    }
    if methods.len() != allowed.len() {
        return Err(DESCRIPTOR_INVALID);
    }
    Ok(methods)
}

fn validate_descriptor_name(value: &str) -> Result<(), &'static str> {
    if value.is_empty()
        || value.len() > MAX_NAME_BYTES
        || !value.is_ascii()
        || value.chars().any(char::is_control)
    {
        Err(DESCRIPTOR_INVALID)
    } else {
        Ok(())
    }
}

fn message_template(descriptor: MessageDescriptor) -> Result<Value, &'static str> {
    let message = DynamicMessage::new(descriptor);
    let mut serializer = serde_json::Serializer::new(Vec::new());
    message
        .serialize_with_options(
            &mut serializer,
            &SerializeOptions::new().skip_default_fields(false),
        )
        .map_err(|_| DESCRIPTOR_INVALID)?;
    let bytes = serializer.into_inner();
    if bytes.len() > MAX_TEMPLATE_BYTES {
        return Err(DESCRIPTOR_INVALID);
    }
    let value = oauth::parse_unique_json(&bytes, MAX_TEMPLATE_BYTES, DESCRIPTOR_INVALID)?;
    mcp::validate_json(&value, MAX_TEMPLATE_BYTES).map_err(|_| DESCRIPTOR_INVALID)?;
    Ok(value)
}

pub fn parse_request_messages(
    method: &MethodDescriptor,
    raw_messages: &[String],
) -> Result<Vec<DynamicMessage>, &'static str> {
    let kind = GrpcRpcKind::from_method(method);
    let valid_count = if kind.accepts_multiple_requests() {
        !raw_messages.is_empty() && raw_messages.len() <= MAX_STREAM_MESSAGES
    } else {
        raw_messages.len() == 1
    };
    if !valid_count {
        return Err(REQUEST_INVALID);
    }

    let mut raw_total = 0usize;
    let mut encoded_total = 0usize;
    let mut output = Vec::with_capacity(raw_messages.len());
    for raw in raw_messages {
        if raw.is_empty() || raw.len() > MAX_MESSAGE_BYTES {
            return Err(REQUEST_TOO_LARGE);
        }
        raw_total = raw_total.checked_add(raw.len()).ok_or(REQUEST_TOO_LARGE)?;
        if raw_total > MAX_REQUEST_TOTAL_BYTES {
            return Err(REQUEST_TOO_LARGE);
        }
        let value = oauth::parse_unique_json(raw.as_bytes(), MAX_MESSAGE_BYTES, REQUEST_INVALID)?;
        mcp::validate_json(&value, MAX_MESSAGE_BYTES).map_err(|code| {
            if code == mcp::REQUEST_TOO_LARGE || code == mcp::RESPONSE_TOO_LARGE {
                REQUEST_TOO_LARGE
            } else {
                REQUEST_INVALID
            }
        })?;
        let message = DynamicMessage::deserialize(method.input(), value.into_deserializer())
            .map_err(|_| REQUEST_INVALID)?;
        let encoded = message.encoded_len();
        if encoded > MAX_MESSAGE_BYTES {
            return Err(REQUEST_TOO_LARGE);
        }
        encoded_total = encoded_total
            .checked_add(encoded)
            .ok_or(REQUEST_TOO_LARGE)?;
        if encoded_total > MAX_REQUEST_TOTAL_BYTES {
            return Err(REQUEST_TOO_LARGE);
        }
        output.push(message);
    }
    Ok(output)
}

pub fn serialize_response_message(message: &DynamicMessage) -> Result<Value, &'static str> {
    if message.encoded_len() > MAX_MESSAGE_BYTES {
        return Err(RESPONSE_TOO_LARGE);
    }
    let mut serializer = serde_json::Serializer::new(Vec::new());
    message
        .serialize_with_options(&mut serializer, &SerializeOptions::new())
        .map_err(|_| PROTOCOL_FAILED)?;
    let bytes = serializer.into_inner();
    if bytes.len() > MAX_MESSAGE_BYTES {
        return Err(RESPONSE_TOO_LARGE);
    }
    let value = oauth::parse_unique_json(&bytes, MAX_MESSAGE_BYTES, PROTOCOL_FAILED)?;
    mcp::validate_json(&value, MAX_MESSAGE_BYTES).map_err(|code| {
        if code == mcp::REQUEST_TOO_LARGE || code == mcp::RESPONSE_TOO_LARGE {
            RESPONSE_TOO_LARGE
        } else {
            PROTOCOL_FAILED
        }
    })?;
    Ok(value)
}

pub fn method_path(method: &MethodDescriptor) -> Result<String, &'static str> {
    validate_descriptor_name(method.parent_service().full_name())?;
    validate_descriptor_name(method.name())?;
    let path = format!("/{}/{}", method.parent_service().full_name(), method.name());
    if path.len() > MAX_NAME_BYTES.saturating_mul(2).saturating_add(2) {
        Err(DESCRIPTOR_INVALID)
    } else {
        Ok(path)
    }
}

#[derive(Clone)]
pub struct DynamicGrpcCodec {
    input: MessageDescriptor,
    output: MessageDescriptor,
}

impl DynamicGrpcCodec {
    pub fn new(method: &MethodDescriptor) -> Self {
        Self {
            input: method.input(),
            output: method.output(),
        }
    }
}

pub struct DynamicGrpcEncoder {
    expected: MessageDescriptor,
}

pub struct DynamicGrpcDecoder {
    descriptor: MessageDescriptor,
}

impl Codec for DynamicGrpcCodec {
    type Encode = DynamicMessage;
    type Decode = DynamicMessage;
    type Encoder = DynamicGrpcEncoder;
    type Decoder = DynamicGrpcDecoder;

    fn encoder(&mut self) -> Self::Encoder {
        DynamicGrpcEncoder {
            expected: self.input.clone(),
        }
    }

    fn decoder(&mut self) -> Self::Decoder {
        DynamicGrpcDecoder {
            descriptor: self.output.clone(),
        }
    }
}

impl Encoder for DynamicGrpcEncoder {
    type Item = DynamicMessage;
    type Error = Status;

    fn encode(
        &mut self,
        item: Self::Item,
        destination: &mut EncodeBuf<'_>,
    ) -> Result<(), Self::Error> {
        if item.descriptor().full_name() != self.expected.full_name() {
            return Err(Status::internal("dynamic request type mismatch"));
        }
        item.encode(destination)
            .map_err(|_| Status::internal("dynamic request encoding failed"))
    }
}

impl Decoder for DynamicGrpcDecoder {
    type Item = DynamicMessage;
    type Error = Status;

    fn decode(&mut self, source: &mut DecodeBuf<'_>) -> Result<Option<Self::Item>, Self::Error> {
        let message = DynamicMessage::decode(self.descriptor.clone(), &mut *source)
            .map_err(|_| Status::internal("dynamic response decoding failed"))?;
        if source.has_remaining() {
            return Err(Status::internal("dynamic response has trailing bytes"));
        }
        Ok(Some(message))
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum ResolverFailure {
    Invalid,
    TooLarge,
}

#[derive(Default)]
struct ResolverState {
    sources: BTreeMap<String, String>,
    objects: BTreeMap<String, (PathBuf, FilesystemIdentity)>,
    total_bytes: usize,
    failure: Option<ResolverFailure>,
}

struct ControlledProtoResolver {
    import_root: PathBuf,
    root_path: PathBuf,
    root_name: String,
    state: Arc<Mutex<ResolverState>>,
}

impl FileResolver for ControlledProtoResolver {
    fn resolve_path(&self, path: &Path) -> Option<String> {
        (path == self.root_path).then(|| self.root_name.clone())
    }

    fn open_file(&self, name: &str) -> Result<File, protox::Error> {
        match self.open_controlled(name) {
            Ok(file) => Ok(file),
            Err(failure) => {
                if let Ok(mut state) = self.state.lock() {
                    state.failure = Some(failure);
                }
                Err(protox::Error::file_not_found(name))
            }
        }
    }
}

impl ControlledProtoResolver {
    fn open_controlled(&self, name: &str) -> Result<File, ResolverFailure> {
        if !valid_import_name(name) {
            return Err(ResolverFailure::Invalid);
        }
        if let Some(source) = self
            .state
            .lock()
            .map_err(|_| ResolverFailure::Invalid)?
            .sources
            .get(name)
            .cloned()
        {
            return File::from_source(name, &source).map_err(|_| ResolverFailure::Invalid);
        }
        let path = self.import_root.join(name);
        ensure_no_links(&path).map_err(|_| ResolverFailure::Invalid)?;
        let canonical = path.canonicalize().map_err(|_| ResolverFailure::Invalid)?;
        if !canonical.starts_with(&self.import_root) {
            return Err(ResolverFailure::Invalid);
        }
        let (mut handle, identity) =
            open_filesystem_object(&canonical, false).map_err(|_| ResolverFailure::Invalid)?;
        let length = usize::try_from(
            handle
                .metadata()
                .map_err(|_| ResolverFailure::Invalid)?
                .len(),
        )
        .map_err(|_| ResolverFailure::TooLarge)?;
        if length == 0 || length > MAX_SOURCE_FILE_BYTES {
            return Err(ResolverFailure::TooLarge);
        }
        let mut source = String::with_capacity(length);
        handle
            .by_ref()
            .take((MAX_SOURCE_FILE_BYTES + 1) as u64)
            .read_to_string(&mut source)
            .map_err(|_| ResolverFailure::Invalid)?;
        if source.len() != length || source.len() > MAX_SOURCE_FILE_BYTES {
            return Err(ResolverFailure::TooLarge);
        }
        if filesystem_identity(&canonical, false).map_err(|_| ResolverFailure::Invalid)? != identity
        {
            return Err(ResolverFailure::Invalid);
        }
        let parsed = File::from_source(name, &source).map_err(|_| ResolverFailure::Invalid)?;
        let mut state = self.state.lock().map_err(|_| ResolverFailure::Invalid)?;
        if state.sources.len() >= MAX_SOURCE_FILES {
            return Err(ResolverFailure::TooLarge);
        }
        state.total_bytes = state
            .total_bytes
            .checked_add(source.len())
            .ok_or(ResolverFailure::TooLarge)?;
        if state.total_bytes > MAX_SOURCE_TOTAL_BYTES {
            return Err(ResolverFailure::TooLarge);
        }
        state.sources.insert(name.to_string(), source);
        state
            .objects
            .insert(name.to_string(), (canonical, identity));
        Ok(parsed)
    }
}

fn valid_import_name(name: &str) -> bool {
    if name.is_empty() || name.len() > MAX_NAME_BYTES.saturating_mul(4) || !name.ends_with(".proto")
    {
        return false;
    }
    Path::new(name)
        .components()
        .all(|component| matches!(component, Component::Normal(_)))
}

fn relative_proto_name(root: &Path, file: &Path) -> Result<String, &'static str> {
    let relative = file
        .strip_prefix(root)
        .map_err(|_| SOURCE_SELECTION_INVALID)?;
    if !valid_import_name(relative.to_str().ok_or(SOURCE_SELECTION_INVALID)?) {
        return Err(SOURCE_SELECTION_INVALID);
    }
    let mut output = String::new();
    for component in relative.components() {
        let Component::Normal(value) = component else {
            return Err(SOURCE_SELECTION_INVALID);
        };
        let value = value.to_str().ok_or(SOURCE_SELECTION_INVALID)?;
        if !output.is_empty() {
            output.push('/');
        }
        output.push_str(value);
    }
    Ok(output)
}

pub fn compile_local_proto(
    selected_file: &Path,
    expected_file: FilesystemIdentity,
    selected_import_root: &Path,
    expected_import_root: FilesystemIdentity,
) -> Result<DescriptorPool, &'static str> {
    ensure_no_links(selected_file).map_err(|_| SOURCE_SELECTION_INVALID)?;
    ensure_no_links(selected_import_root).map_err(|_| SOURCE_SELECTION_INVALID)?;
    let file = selected_file
        .canonicalize()
        .map_err(|_| SOURCE_SELECTION_INVALID)?;
    let import_root = selected_import_root
        .canonicalize()
        .map_err(|_| SOURCE_SELECTION_INVALID)?;
    if filesystem_identity(&file, false).map_err(|_| SOURCE_SELECTION_INVALID)? != expected_file
        || filesystem_identity(&import_root, true).map_err(|_| SOURCE_SELECTION_INVALID)?
            != expected_import_root
    {
        return Err(SOURCE_SELECTION_INVALID);
    }
    let root_name = relative_proto_name(&import_root, &file)?;
    let state = Arc::new(Mutex::new(ResolverState::default()));
    let resolver = ControlledProtoResolver {
        import_root: import_root.clone(),
        root_path: file.clone(),
        root_name,
        state: state.clone(),
    };
    let mut chain = ChainFileResolver::new();
    chain.add(GoogleFileResolver::new());
    chain.add(resolver);
    let mut compiler = protox::Compiler::with_file_resolver(chain);
    compiler.include_imports(true).include_source_info(false);
    let compile = compiler.open_file(&file);
    if compile.is_err() {
        let failure = state
            .lock()
            .ok()
            .and_then(|state| state.failure)
            .unwrap_or(ResolverFailure::Invalid);
        return Err(match failure {
            ResolverFailure::Invalid => SOURCE_INVALID,
            ResolverFailure::TooLarge => SOURCE_TOO_LARGE,
        });
    }
    if filesystem_identity(&file, false).map_err(|_| SOURCE_SELECTION_INVALID)? != expected_file
        || filesystem_identity(&import_root, true).map_err(|_| SOURCE_SELECTION_INVALID)?
            != expected_import_root
    {
        return Err(SOURCE_SELECTION_INVALID);
    }
    let tracked = state.lock().map_err(|_| SOURCE_INVALID)?;
    for (path, identity) in tracked.objects.values() {
        if filesystem_identity(path, false).map_err(|_| SOURCE_SELECTION_INVALID)? != *identity {
            return Err(SOURCE_SELECTION_INVALID);
        }
    }
    drop(tracked);
    let descriptor_set = compiler.file_descriptor_set();
    let pool =
        DescriptorPool::from_file_descriptor_set(descriptor_set).map_err(|_| DESCRIPTOR_INVALID)?;
    validate_descriptor_pool(&pool)?;
    Ok(pool)
}

pub fn stable_status_name(code: tonic::Code) -> &'static str {
    match code {
        tonic::Code::Ok => "OK",
        tonic::Code::Cancelled => "CANCELLED",
        tonic::Code::Unknown => "UNKNOWN",
        tonic::Code::InvalidArgument => "INVALID_ARGUMENT",
        tonic::Code::DeadlineExceeded => "DEADLINE_EXCEEDED",
        tonic::Code::NotFound => "NOT_FOUND",
        tonic::Code::AlreadyExists => "ALREADY_EXISTS",
        tonic::Code::PermissionDenied => "PERMISSION_DENIED",
        tonic::Code::ResourceExhausted => "RESOURCE_EXHAUSTED",
        tonic::Code::FailedPrecondition => "FAILED_PRECONDITION",
        tonic::Code::Aborted => "ABORTED",
        tonic::Code::OutOfRange => "OUT_OF_RANGE",
        tonic::Code::Unimplemented => "UNIMPLEMENTED",
        tonic::Code::Internal => "INTERNAL",
        tonic::Code::Unavailable => "UNAVAILABLE",
        tonic::Code::DataLoss => "DATA_LOSS",
        tonic::Code::Unauthenticated => "UNAUTHENTICATED",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use prost_reflect::Value as ReflectValue;
    use tempfile::TempDir;

    fn fixture() -> (TempDir, PathBuf, PathBuf) {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("proto");
        std::fs::create_dir_all(root.join("shared")).unwrap();
        std::fs::write(
            root.join("shared/types.proto"),
            r#"syntax = "proto3";
package demo;
message Reply { string message = 1; }
"#,
        )
        .unwrap();
        let file = root.join("echo.proto");
        std::fs::write(
            &file,
            r#"syntax = "proto3";
package demo;
import "shared/types.proto";
message Request { string name = 1; int64 count = 2; }
service Echo {
  rpc Unary(Request) returns (Reply);
  rpc Server(Request) returns (stream Reply);
  rpc Client(stream Request) returns (Reply);
  rpc Bidi(stream Request) returns (stream Reply);
}
"#,
        )
        .unwrap();
        (temp, root, file)
    }

    fn pool() -> DescriptorPool {
        let (_temp, root, file) = fixture();
        compile_local_proto(
            &file,
            filesystem_identity(&file, false).unwrap(),
            &root,
            filesystem_identity(&root, true).unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn endpoint_requires_authority_only_http_or_https() {
        assert_eq!(
            normalize_endpoint("https://example.test:7443").unwrap().uri,
            "https://example.test:7443/"
        );
        assert!(normalize_endpoint("http://127.0.0.1:50051").is_ok());
        assert_eq!(
            normalize_endpoint("http://[::1]:50051").unwrap().authority,
            "[::1]:50051"
        );
        for invalid in [
            "grpc://example.test",
            "https://user@example.test",
            "https://example.test/rpc",
            "https://example.test?token=x",
            "https://example.test#fragment",
            "https://example.test:0",
        ] {
            assert_eq!(normalize_endpoint(invalid).unwrap_err(), INVALID_PROFILE);
        }
    }

    #[test]
    fn server_name_is_bounded_and_has_no_uri_syntax() {
        assert_eq!(
            validate_server_name(Some("api.example.test")).unwrap(),
            Some("api.example.test".into())
        );
        assert_eq!(
            validate_server_name(Some("::1")).unwrap(),
            Some("::1".into())
        );
        assert_eq!(
            validate_server_name(Some("bad/name")).unwrap_err(),
            INVALID_PROFILE
        );
        assert_eq!(
            validate_server_name(Some("bad name")).unwrap_err(),
            INVALID_PROFILE
        );
    }

    #[test]
    fn local_proto_compiles_imports_and_projects_all_rpc_kinds() {
        let pool = pool();
        let projection = validate_descriptor_pool(&pool).unwrap();
        assert_eq!(projection.service_count, 1);
        assert_eq!(projection.methods.len(), 4);
        assert_eq!(projection.methods[0].full_name, "demo.Echo.Bidi");
        assert_eq!(
            projection.methods[0].rpc_kind,
            GrpcRpcKind::BidirectionalStreaming
        );
        assert!(projection
            .methods
            .iter()
            .any(|method| method.rpc_kind == GrpcRpcKind::Unary));
        assert!(projection
            .methods
            .iter()
            .any(|method| method.rpc_kind == GrpcRpcKind::ServerStreaming));
        assert!(projection
            .methods
            .iter()
            .any(|method| method.rpc_kind == GrpcRpcKind::ClientStreaming));
    }

    #[test]
    fn protojson_rejects_duplicates_unknown_fields_and_wrong_stream_count() {
        let pool = pool();
        let methods = method_map(&pool).unwrap();
        let unary = methods.get("demo.Echo.Unary").unwrap();
        assert_eq!(
            parse_request_messages(unary, &[r#"{"name":"a","name":"b"}"#.into()]).unwrap_err(),
            REQUEST_INVALID
        );
        assert_eq!(
            parse_request_messages(unary, &[r#"{"missing":true}"#.into()]).unwrap_err(),
            REQUEST_INVALID
        );
        assert_eq!(
            parse_request_messages(unary, &["{}".into(), "{}".into()]).unwrap_err(),
            REQUEST_INVALID
        );
        let parsed =
            parse_request_messages(unary, &[r#"{"name":"hello","count":"42"}"#.into()]).unwrap();
        assert_eq!(
            parsed[0].get_field_by_name("name").unwrap().as_ref(),
            &ReflectValue::String("hello".into())
        );
        assert_eq!(
            parsed[0].get_field_by_name("count").unwrap().as_ref(),
            &ReflectValue::I64(42)
        );

        let client = methods.get("demo.Echo.Client").unwrap();
        let padded = format!("{{}}{}", " ".repeat(900_000));
        assert_eq!(
            parse_request_messages(client, &vec![padded; 5]).unwrap_err(),
            REQUEST_TOO_LARGE
        );
    }

    #[test]
    fn protojson_supports_canonical_scalars_enums_bytes_and_well_known_types() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("proto");
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("types.proto");
        std::fs::write(
            &file,
            r#"syntax = "proto3";
package demo;
import "google/protobuf/timestamp.proto";
enum State { STATE_UNSPECIFIED = 0; STATE_READY = 1; }
message Request {
  int64 count = 1;
  bytes payload = 2;
  State state = 3;
  google.protobuf.Timestamp when = 4;
}
service Echo { rpc Unary(Request) returns (Request); }
"#,
        )
        .unwrap();
        let pool = compile_local_proto(
            &file,
            filesystem_identity(&file, false).unwrap(),
            &root,
            filesystem_identity(&root, true).unwrap(),
        )
        .unwrap();
        let method = method_map(&pool)
            .unwrap()
            .remove("demo.Echo.Unary")
            .unwrap();
        let mut parsed = parse_request_messages(
            &method,
            &[r#"{"count":"42","payload":"AQI=","state":"STATE_READY","when":"2023-11-14T22:13:20Z"}"#.into()],
        )
        .unwrap();

        assert_eq!(
            serialize_response_message(&parsed.remove(0)).unwrap(),
            serde_json::json!({
                "count": "42",
                "payload": "AQI=",
                "state": "STATE_READY",
                "when": "2023-11-14T22:13:20Z"
            })
        );
    }

    #[test]
    fn local_proto_rejects_file_outside_selected_import_root() {
        let (_temp, root, file) = fixture();
        let nested = root.join("shared");
        assert_eq!(
            compile_local_proto(
                &file,
                filesystem_identity(&file, false).unwrap(),
                &nested,
                filesystem_identity(&nested, true).unwrap(),
            )
            .unwrap_err(),
            SOURCE_SELECTION_INVALID
        );
    }

    #[test]
    fn local_proto_rejects_traversal_cycles_and_oversized_sources() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("proto");
        std::fs::create_dir_all(&root).unwrap();

        let outside = temp.path().join("outside.proto");
        std::fs::write(
            &outside,
            "syntax = \"proto3\"; package demo; message Outside {}",
        )
        .unwrap();
        let traversal = root.join("traversal.proto");
        std::fs::write(
            &traversal,
            "syntax = \"proto3\"; package demo; import \"../outside.proto\"; message Root { Outside value = 1; }",
        )
        .unwrap();
        assert_eq!(
            compile_local_proto(
                &traversal,
                filesystem_identity(&traversal, false).unwrap(),
                &root,
                filesystem_identity(&root, true).unwrap(),
            )
            .unwrap_err(),
            SOURCE_INVALID
        );

        let first = root.join("first.proto");
        let second = root.join("second.proto");
        std::fs::write(
            &first,
            "syntax = \"proto3\"; package demo; import \"second.proto\"; message First { Second value = 1; }",
        )
        .unwrap();
        std::fs::write(
            &second,
            "syntax = \"proto3\"; package demo; import \"first.proto\"; message Second { First value = 1; }",
        )
        .unwrap();
        assert_eq!(
            compile_local_proto(
                &first,
                filesystem_identity(&first, false).unwrap(),
                &root,
                filesystem_identity(&root, true).unwrap(),
            )
            .unwrap_err(),
            SOURCE_INVALID
        );

        let oversized = root.join("oversized.proto");
        std::fs::write(&oversized, vec![b' '; MAX_SOURCE_FILE_BYTES + 1]).unwrap();
        assert_eq!(
            compile_local_proto(
                &oversized,
                filesystem_identity(&oversized, false).unwrap(),
                &root,
                filesystem_identity(&root, true).unwrap(),
            )
            .unwrap_err(),
            SOURCE_TOO_LARGE
        );
    }

    #[cfg(unix)]
    #[test]
    fn local_proto_rejects_symlinked_import() {
        use std::os::unix::fs::symlink;

        let (temp, root, file) = fixture();
        let external = temp.path().join("external.proto");
        std::fs::write(
            &external,
            "syntax = \"proto3\"; package demo; message Reply { string value = 1; }",
        )
        .unwrap();
        std::fs::remove_file(root.join("shared/types.proto")).unwrap();
        symlink(&external, root.join("shared/types.proto")).unwrap();
        assert_eq!(
            compile_local_proto(
                &file,
                filesystem_identity(&file, false).unwrap(),
                &root,
                filesystem_identity(&root, true).unwrap(),
            )
            .unwrap_err(),
            SOURCE_INVALID
        );
    }

    #[test]
    fn response_serialization_is_canonical_and_bounded() {
        let pool = pool();
        let reply = pool.get_message_by_name("demo.Reply").unwrap();
        let mut message = DynamicMessage::new(reply);
        message
            .try_set_field_by_name("message", ReflectValue::String("ok".into()))
            .unwrap();
        assert_eq!(
            serialize_response_message(&message).unwrap(),
            serde_json::json!({"message":"ok"})
        );
    }
}
