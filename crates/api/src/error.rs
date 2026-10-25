//! `api.*` 도메인 오류. 세부에는 값·URL·경로·비밀을 넣지 않고 행 번호와 고정 사유만 둔다.
devbox_protocol::domain_error! {
    pub enum ApiError {
        InvalidHeader { index: u32, field: String, reason: String } = "api.invalid_header",
        InvalidCookie { index: u32, reason: String } = "api.invalid_cookie",
        InvalidUrl { reason: String } = "api.invalid_url",
        InvalidMethod = "api.invalid_method",
        InvalidBody { reason: String } = "api.invalid_body",
        InvalidInput { field: String, reason: String } = "api.invalid_input",
        VariableMissing { names: Vec<String> } = "api.variable_missing",
        SecretMissing { names: Vec<String> } = "api.secret_missing",
        SecretsLocked = "secrets_locked",
        SecretKeyMismatch = "secrets.key_mismatch",
        FileMissing { part: u32 } = "api.file_missing",
        TooLarge { limit: u64 } = "api.too_large",
        RedactionFailed { reason: String } = "api.redaction_failed",
        TlsInvalid { reason: String } = "api.tls_invalid",
        OauthFailed { stage: String } = "api.oauth_failed",
        Network { kind: String } = "api.network",
        StreamUnsupported = "api.stream_unsupported",
        StreamRejected { status: u16, reason: String } = "api.stream_rejected",
        TooManyStreams { limit: u32 } = "api.too_many_streams",
        HistoryNotFound = "api.history_not_found",
        CollectionPathInvalid = "api.collection_path_invalid",
        CollectionConflict { digest: Option<String> } = "api.collection_conflict",
        CollectionTooLarge { limit: u64 } = "api.collection_too_large",
        CollectionNotFound = "api.collection_not_found",
        StorageSensitiveBody = "api.storage_sensitive_body",
        ProjectNotFound = "api.project_not_found",
        Grpc { reason: String } = "api.grpc",
        ReviewRequired = "api.review_required",
        Mcp { reason: String } = "api.mcp",
    }
}

impl ApiError {
    pub(crate) fn header(index: u32, field: &str, reason: &str) -> Self {
        Self::InvalidHeader {
            index,
            field: field.into(),
            reason: reason.into(),
        }
    }
    pub(crate) fn url(reason: &str) -> Self {
        Self::InvalidUrl {
            reason: reason.into(),
        }
    }
    pub(crate) fn body(reason: &str) -> Self {
        Self::InvalidBody {
            reason: reason.into(),
        }
    }
    pub(crate) fn input(field: &str, reason: &str) -> Self {
        Self::InvalidInput {
            field: field.into(),
            reason: reason.into(),
        }
    }
    pub(crate) fn tls(reason: &str) -> Self {
        Self::TlsInvalid {
            reason: reason.into(),
        }
    }
    pub(crate) fn network(kind: &str) -> Self {
        Self::Network { kind: kind.into() }
    }
    pub(crate) fn grpc(reason: &str) -> Self {
        Self::Grpc {
            reason: reason.into(),
        }
    }
    pub(crate) fn mcp(reason: &str) -> Self {
        Self::Mcp {
            reason: reason.into(),
        }
    }
    pub(crate) fn oauth(stage: &str) -> Self {
        Self::OauthFailed {
            stage: stage.into(),
        }
    }
}

/// secrets 오류를 기존 코드 문자열로 옮긴다(runtime `mapped`와 같은 모양).
pub(crate) fn mapped(
    error: devbox_protocol::Fail<devbox_secrets::SecretsError>,
) -> devbox_protocol::Fail<ApiError> {
    use devbox_protocol::Fail;
    use devbox_secrets::SecretsError;
    match error {
        Fail::Domain(SecretsError::Missing { names }) => ApiError::SecretMissing { names }.into(),
        Fail::Domain(SecretsError::KeyMismatch) => ApiError::SecretKeyMismatch.into(),
        Fail::Domain(SecretsError::InvalidInput) => ApiError::input("secret", "reference").into(),
        Fail::Domain(_) | Fail::Internal(_) => ApiError::SecretsLocked.into(),
    }
}
