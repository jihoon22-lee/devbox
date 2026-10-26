//! 서술자 얻기: server reflection(v1, `Unimplemented`일 때만 v1alpha) 또는 protox(로컬 `.proto`).
//! 입력 상한: proto 파일 수·크기·가져오기 수·서술자 바이트 합·서비스·메서드 수. protox·서술자 오류 문장은
//! 파일 내용·경로를 담을 수 있으므로 고정 사유로만 바꾼다.
use super::{CatalogSource, GrpcCatalog, GrpcMethod, GrpcService, Prepared};
use crate::ApiError;
use prost::Message;
use prost_reflect::prost_types::{FileDescriptorProto, FileDescriptorSet};
use prost_reflect::{DescriptorPool, DynamicMessage, MessageDescriptor};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub(crate) const MAX_PROTO_FILES: usize = 32;
pub(crate) const MAX_PROTO_BYTES: u64 = 1024 * 1024;
/// 가져오기를 포함해 protox가 여는 파일 수와 바이트 합.
const MAX_OPENED_FILES: usize = 256;
const MAX_OPENED_BYTES: u64 = 8 * 1024 * 1024;
/// reflection으로 받는 서술자 파일 수와 바이트 합.
const MAX_DESCRIPTOR_FILES: usize = 512;
const MAX_DESCRIPTOR_BYTES: usize = 8 * 1024 * 1024;
const MAX_SERVICES: usize = 256;
const MAX_METHODS: usize = 4096;
const MAX_DEFAULT_JSON: usize = 64 * 1024;

pub(crate) fn check_proto_paths(files: &[String]) -> Result<(), ApiError> {
    if files.is_empty() {
        return Err(ApiError::grpc("proto_path"));
    }
    if files.len() > MAX_PROTO_FILES {
        return Err(ApiError::grpc("proto_too_many"));
    }
    for file in files {
        let path = Path::new(file);
        if !path.is_absolute() || !file.ends_with(".proto") || file.contains('\0') {
            return Err(ApiError::grpc("proto_path"));
        }
        let meta = std::fs::metadata(path).map_err(|_| ApiError::grpc("proto_missing"))?;
        if !meta.is_file() {
            return Err(ApiError::grpc("proto_missing"));
        }
        if meta.len() > MAX_PROTO_BYTES {
            return Err(ApiError::grpc("proto_too_large"));
        }
    }
    Ok(())
}

/// 각 파일의 부모 폴더를 include 경로로 쓰고, 여는 파일마다 크기·수·합을 먼저 확인한다.
struct Bounded {
    includes: Vec<PathBuf>,
    inner: protox::file::ChainFileResolver,
    opened: Mutex<(usize, u64)>,
}

impl protox::file::FileResolver for Bounded {
    fn resolve_path(&self, path: &Path) -> Option<String> {
        self.inner.resolve_path(path)
    }
    fn open_file(&self, name: &str) -> Result<protox::file::File, protox::Error> {
        let limit = || protox::Error::new(std::io::Error::other("limit"));
        for include in &self.includes {
            let candidate = include.join(name);
            if let Ok(meta) = std::fs::metadata(&candidate) {
                let mut opened = self.opened.lock().map_err(|_| limit())?;
                opened.0 += 1;
                opened.1 += meta.len();
                if meta.len() > MAX_PROTO_BYTES
                    || opened.0 > MAX_OPENED_FILES
                    || opened.1 > MAX_OPENED_BYTES
                {
                    return Err(limit());
                }
                break;
            }
        }
        self.inner.open_file(name)
    }
}

pub(crate) fn from_proto_files(files: &[String]) -> Result<DescriptorPool, ApiError> {
    check_proto_paths(files)?;
    let mut includes: Vec<PathBuf> = vec![];
    for file in files {
        let parent = Path::new(file)
            .parent()
            .ok_or_else(|| ApiError::grpc("proto_path"))?;
        if !includes.iter().any(|i| i == parent) {
            includes.push(parent.to_owned());
        }
    }
    let mut inner = protox::file::ChainFileResolver::new();
    for include in &includes {
        inner.add(protox::file::IncludeFileResolver::new(include.clone()));
    }
    inner.add(protox::file::GoogleFileResolver::new());
    let resolver = Bounded {
        includes,
        inner,
        opened: Mutex::new((0, 0)),
    };
    let mut compiler = protox::Compiler::with_file_resolver(resolver);
    compiler.include_imports(true);
    for file in files {
        compiler
            .open_file(file)
            .map_err(|_| ApiError::grpc("proto_parse"))?;
    }
    Ok(compiler.descriptor_pool())
}

type Client = tonic_reflection::pb::v1::server_reflection_client::ServerReflectionClient<
    tonic::transport::Channel,
>;
type AlphaClient = tonic_reflection::pb::v1alpha::server_reflection_client::ServerReflectionClient<
    tonic::transport::Channel,
>;

/// reflection 질의 하나의 결과(v1·v1alpha 공통 모양).
enum Answer {
    Services(Vec<String>),
    Files(Vec<Vec<u8>>),
    Missing,
}

enum Reflection {
    V1(Client),
    Alpha(AlphaClient),
}

fn request<T>(
    message: T,
    metadata: &tonic::metadata::MetadataMap,
) -> tonic::Request<futures::stream::Iter<std::array::IntoIter<T, 1>>> {
    let mut request = tonic::Request::new(futures::stream::iter([message]));
    *request.metadata_mut() = metadata.clone();
    request
}

enum ReflectionError {
    Status(tonic::Status),
    Redaction(ApiError),
}
impl From<ApiError> for ReflectionError {
    fn from(error: ApiError) -> Self {
        Self::Redaction(error)
    }
}
fn reflection_status(
    status: tonic::Status,
    redaction: &mut crate::redact::Redaction,
) -> ReflectionError {
    match redaction.response_headers(super::metadata::headers(status.metadata())) {
        Ok(()) => ReflectionError::Status(status),
        Err(error) => ReflectionError::Redaction(error),
    }
}
async fn reflected<T>(
    response: tonic::Response<tonic::Streaming<T>>,
    redaction: &mut crate::redact::Redaction,
) -> Result<Option<T>, ReflectionError> {
    redaction.response_headers(super::metadata::headers(response.metadata()))?;
    // Preserve first-response semantics: this call drops the reflection stream after one answer.
    // Unconsumed trailers are not accepted metadata and cannot retroactively change that answer.
    response
        .into_inner()
        .message()
        .await
        .map_err(|status| reflection_status(status, redaction))
}

impl Reflection {
    /// 요청 하나를 보내고 답 하나를 받는다(양방향 스트림을 요청마다 새로 연다: 단순·상한이 쉬움).
    async fn ask(
        &mut self,
        metadata: &tonic::metadata::MetadataMap,
        query: Query,
        redaction: &mut crate::redact::Redaction,
    ) -> Result<Answer, ReflectionError> {
        Ok(match self {
            Reflection::V1(c) => {
                use pb::server_reflection_response::MessageResponse as R;
                use tonic_reflection::pb::v1 as pb;
                let message = pb::ServerReflectionRequest {
                    host: String::new(),
                    message_request: Some(query.v1()),
                };
                let response = c
                    .server_reflection_info(request(message, metadata))
                    .await
                    .map_err(|status| reflection_status(status, redaction))?;
                match reflected(response, redaction)
                    .await?
                    .and_then(|r| r.message_response)
                {
                    Some(R::ListServicesResponse(list)) => {
                        Answer::Services(list.service.into_iter().map(|s| s.name).collect())
                    }
                    Some(R::FileDescriptorResponse(f)) => Answer::Files(f.file_descriptor_proto),
                    _ => Answer::Missing,
                }
            }
            Reflection::Alpha(c) => {
                use pb::server_reflection_response::MessageResponse as R;
                use tonic_reflection::pb::v1alpha as pb;
                let message = pb::ServerReflectionRequest {
                    host: String::new(),
                    message_request: Some(query.alpha()),
                };
                let response = c
                    .server_reflection_info(request(message, metadata))
                    .await
                    .map_err(|status| reflection_status(status, redaction))?;
                match reflected(response, redaction)
                    .await?
                    .and_then(|r| r.message_response)
                {
                    Some(R::ListServicesResponse(list)) => {
                        Answer::Services(list.service.into_iter().map(|s| s.name).collect())
                    }
                    Some(R::FileDescriptorResponse(f)) => Answer::Files(f.file_descriptor_proto),
                    _ => Answer::Missing,
                }
            }
        })
    }
}

#[derive(Clone)]
enum Query {
    List,
    Symbol(String),
    File(String),
}

impl Query {
    fn v1(self) -> tonic_reflection::pb::v1::server_reflection_request::MessageRequest {
        use tonic_reflection::pb::v1::server_reflection_request::MessageRequest as M;
        match self {
            Query::List => M::ListServices(String::new()),
            Query::Symbol(s) => M::FileContainingSymbol(s),
            Query::File(f) => M::FileByFilename(f),
        }
    }
    fn alpha(self) -> tonic_reflection::pb::v1alpha::server_reflection_request::MessageRequest {
        use tonic_reflection::pb::v1alpha::server_reflection_request::MessageRequest as M;
        match self {
            Query::List => M::ListServices(String::new()),
            Query::Symbol(s) => M::FileContainingSymbol(s),
            Query::File(f) => M::FileByFilename(f),
        }
    }
}

/// 전송 실패(연결 계층)와 서버 상태를 나눈다. 연결 실패 상태는 출처(source) 오류를 가진다.
pub(crate) fn transport(status: &tonic::Status) -> Option<ApiError> {
    use std::error::Error;
    let mut next = status.source();
    let mut kind = "protocol";
    next?;
    while let Some(error) = next {
        if let Some(failed) = error.downcast_ref::<super::connect::ConnectFailed>() {
            kind = failed.0;
            break;
        }
        if error
            .downcast_ref::<std::io::Error>()
            .is_some_and(|e| e.kind() == std::io::ErrorKind::TimedOut)
        {
            kind = "timeout";
        }
        next = error.source();
    }
    Some(ApiError::network(kind))
}

fn status_error(error: ReflectionError) -> ApiError {
    match error {
        ReflectionError::Status(status) => {
            transport(&status).unwrap_or_else(|| ApiError::grpc("reflection_failed"))
        }
        ReflectionError::Redaction(error) => error,
    }
}

/// 서술자 바이트를 모으고 의존 파일을 차례로 받는다(이름이 같은데 내용이 다르면 거절).
struct Accumulator {
    files: BTreeMap<String, FileDescriptorProto>,
    bytes: usize,
}

impl Accumulator {
    fn add(&mut self, raw: Vec<Vec<u8>>) -> Result<(), ApiError> {
        for bytes in raw {
            self.bytes += bytes.len();
            if self.bytes > MAX_DESCRIPTOR_BYTES || self.files.len() >= MAX_DESCRIPTOR_FILES {
                return Err(ApiError::grpc("descriptor_too_large"));
            }
            let file = FileDescriptorProto::decode(bytes.as_slice())
                .map_err(|_| ApiError::grpc("descriptor"))?;
            let name = file.name().to_owned();
            match self.files.get(&name) {
                Some(existing) if *existing != file => return Err(ApiError::grpc("descriptor")),
                Some(_) => {}
                None => {
                    self.files.insert(name, file);
                }
            }
        }
        Ok(())
    }
    fn missing(&self) -> Option<String> {
        self.files
            .values()
            .flat_map(|f| f.dependency.iter())
            .find(|d| !self.files.contains_key(*d))
            .cloned()
    }
    /// 의존 순서로 정렬해 pool을 만든다.
    fn pool(self) -> Result<DescriptorPool, ApiError> {
        let mut ordered = vec![];
        let mut done = BTreeSet::new();
        fn visit(
            name: &str,
            files: &BTreeMap<String, FileDescriptorProto>,
            done: &mut BTreeSet<String>,
            stack: &mut BTreeSet<String>,
            out: &mut Vec<FileDescriptorProto>,
        ) -> Result<(), ApiError> {
            if done.contains(name) {
                return Ok(());
            }
            if !stack.insert(name.to_owned()) {
                return Err(ApiError::grpc("descriptor"));
            }
            let file = files
                .get(name)
                .ok_or_else(|| ApiError::grpc("descriptor"))?;
            for dep in &file.dependency {
                visit(dep, files, done, stack, out)?;
            }
            stack.remove(name);
            done.insert(name.to_owned());
            out.push(file.clone());
            Ok(())
        }
        for name in self.files.keys() {
            visit(
                name,
                &self.files,
                &mut done,
                &mut BTreeSet::new(),
                &mut ordered,
            )?;
        }
        DescriptorPool::from_file_descriptor_set(FileDescriptorSet { file: ordered })
            .map_err(|_| ApiError::grpc("descriptor"))
    }
}

pub(crate) async fn from_reflection(p: &mut Prepared) -> Result<DescriptorPool, ApiError> {
    let channel = super::connect::channel(p)?;
    let size = super::MAX_MESSAGE_BYTES;
    let mut client = Reflection::V1(
        Client::new(channel.clone())
            .max_decoding_message_size(size)
            .max_encoding_message_size(size),
    );
    let services = match client.ask(&p.metadata, Query::List, &mut p.redaction).await {
        Ok(answer) => answer,
        Err(ReflectionError::Status(status)) if status.code() == tonic::Code::Unimplemented => {
            client = Reflection::Alpha(
                AlphaClient::new(channel)
                    .max_decoding_message_size(size)
                    .max_encoding_message_size(size),
            );
            match client.ask(&p.metadata, Query::List, &mut p.redaction).await {
                Ok(answer) => answer,
                Err(ReflectionError::Status(status))
                    if status.code() == tonic::Code::Unimplemented =>
                {
                    return Err(ApiError::grpc("reflection_unavailable"));
                }
                Err(status) => return Err(status_error(status)),
            }
        }
        Err(status) => return Err(status_error(status)),
    };
    let Answer::Services(mut names) = services else {
        return Err(ApiError::grpc("reflection_failed"));
    };
    names.retain(|n| !n.starts_with("grpc.reflection."));
    if names.len() > MAX_SERVICES {
        return Err(ApiError::grpc("descriptor_too_large"));
    }
    let mut acc = Accumulator {
        files: BTreeMap::new(),
        bytes: 0,
    };
    for name in names {
        match client
            .ask(&p.metadata, Query::Symbol(name), &mut p.redaction)
            .await
        {
            Ok(Answer::Files(raw)) => acc.add(raw)?,
            Ok(_) => return Err(ApiError::grpc("reflection_failed")),
            Err(status) => return Err(status_error(status)),
        }
    }
    while let Some(dep) = acc.missing() {
        match client
            .ask(&p.metadata, Query::File(dep), &mut p.redaction)
            .await
        {
            Ok(Answer::Files(raw)) if !raw.is_empty() => acc.add(raw)?,
            Ok(_) => return Err(ApiError::grpc("descriptor")),
            Err(status) => return Err(status_error(status)),
        }
    }
    acc.pool()
}

pub(crate) async fn pool(p: &mut Prepared) -> Result<(DescriptorPool, CatalogSource), ApiError> {
    match &p.proto_files {
        Some(files) => {
            let files = files.clone();
            let pool = tokio::task::spawn_blocking(move || from_proto_files(&files))
                .await
                .map_err(|_| ApiError::grpc("proto_parse"))??;
            Ok((pool, CatalogSource::ProtoFiles))
        }
        None => Ok((from_reflection(p).await?, CatalogSource::Reflection)),
    }
}

/// 입력 메시지 기본값(모든 필드를 기본값으로 보인 JSON).
pub(crate) fn default_json(message: &MessageDescriptor) -> String {
    let options = prost_reflect::SerializeOptions::new().skip_default_fields(false);
    let mut out = serde_json::Serializer::new(Vec::new());
    let text = DynamicMessage::new(message.clone())
        .serialize_with_options(&mut out, &options)
        .ok()
        .and_then(|_| String::from_utf8(out.into_inner()).ok())
        .unwrap_or_else(|| "{}".into());
    if text.len() > MAX_DEFAULT_JSON {
        "{}".into()
    } else {
        text
    }
}

pub(crate) fn catalog_of(
    pool: &DescriptorPool,
    source: CatalogSource,
) -> Result<GrpcCatalog, ApiError> {
    let mut services = vec![];
    let mut methods_total = 0;
    for service in pool.services() {
        if service.full_name().starts_with("grpc.reflection.") {
            continue;
        }
        let methods: Vec<_> = service
            .methods()
            .map(|m| GrpcMethod {
                name: m.name().to_owned(),
                full_name: format!("{}/{}", service.full_name(), m.name()),
                client_streaming: m.is_client_streaming(),
                server_streaming: m.is_server_streaming(),
                input_default: default_json(&m.input()),
            })
            .collect();
        methods_total += methods.len();
        services.push(GrpcService {
            name: service.full_name().to_owned(),
            methods,
        });
    }
    if services.len() > MAX_SERVICES || methods_total > MAX_METHODS {
        return Err(ApiError::grpc("descriptor_too_large"));
    }
    services.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(GrpcCatalog { source, services })
}

pub(crate) async fn catalog(p: &mut Prepared) -> Result<GrpcCatalog, ApiError> {
    let (pool, source) = pool(p).await?;
    let catalog = catalog_of(&pool, source)?;
    let contains = |text: &str| p.redaction.redactor.contains(text);
    if catalog.services.iter().any(|service| {
        contains(&service.name)
            || service.methods.iter().any(|method| {
                contains(&method.name)
                    || contains(&method.full_name)
                    || contains(&method.input_default)
            })
    }) {
        // Catalog strings are dispatch identities too: refuse instead of changing their meaning.
        return Err(ApiError::RedactionFailed {
            reason: "input".into(),
        });
    }
    Ok(catalog)
}
