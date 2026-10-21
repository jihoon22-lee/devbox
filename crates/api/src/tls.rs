//! 요청 단위 TLS: CA·클라이언트 인증서·키 PEM(WSL 절대 경로), 검증 끔. 연결 전에 모두 검사한다.
use crate::{ApiError, TlsOptions};
use std::sync::{Arc, OnceLock};

const MAX_PEM: u64 = 1024 * 1024;

/// ring 공급자를 한 곳에서 설치한다(reqwest `rustls-no-provider`).
fn provider() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

pub(crate) struct PreparedTls {
    pub(crate) verify: bool,
    roots: Vec<reqwest::Certificate>,
    identity: Option<reqwest::Identity>,
}

impl PreparedTls {
    pub(crate) fn has_identity(&self) -> bool {
        self.identity.is_some()
    }
}

fn read_pem(path: &str) -> Result<Vec<u8>, ApiError> {
    let path = std::path::Path::new(path);
    if !path.is_absolute() {
        return Err(ApiError::tls("relative_path"));
    }
    let meta = std::fs::metadata(path).map_err(|_| ApiError::tls("missing_file"))?;
    if !meta.is_file() {
        return Err(ApiError::tls("missing_file"));
    }
    if meta.len() > MAX_PEM {
        return Err(ApiError::tls("too_large"));
    }
    std::fs::read(path).map_err(|_| ApiError::tls("missing_file"))
}

pub(crate) fn prepare(options: &TlsOptions) -> Result<PreparedTls, ApiError> {
    use rustls::pki_types::pem::PemObject;
    use rustls::pki_types::{CertificateDer, PrivateKeyDer};
    let ca = options.ca_path.as_deref().map(read_pem).transpose()?;
    let cert = options
        .client_cert_path
        .as_deref()
        .map(read_pem)
        .transpose()?;
    let key = options
        .client_key_path
        .as_deref()
        .map(read_pem)
        .transpose()?;
    let mut roots = vec![];
    if let Some(ca) = ca {
        let certs = CertificateDer::pem_slice_iter(&ca)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| ApiError::tls("invalid_ca"))?;
        if certs.is_empty() {
            return Err(ApiError::tls("invalid_ca"));
        }
        for der in certs {
            // 신뢰 앵커로 해석할 수 있는지 연결 전에 확인한다.
            rustls::RootCertStore::empty()
                .add(der.clone())
                .map_err(|_| ApiError::tls("invalid_ca"))?;
            roots.push(
                reqwest::Certificate::from_der(&der).map_err(|_| ApiError::tls("invalid_ca"))?,
            );
        }
    }
    let identity = match (cert, key) {
        (None, None) => None,
        (Some(cert), Some(key)) => {
            let certs = CertificateDer::pem_slice_iter(&cert)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| ApiError::tls("invalid_certificate"))?;
            let parsed =
                PrivateKeyDer::from_pem_slice(&key).map_err(|_| ApiError::tls("invalid_key"))?;
            if certs.is_empty() {
                return Err(ApiError::tls("invalid_certificate"));
            }
            provider();
            rustls::crypto::ring::sign::any_supported_type(&parsed)
                .map_err(|_| ApiError::tls("invalid_key"))?;
            // 인증서가 X.509로 해석되고 키와 짝이 맞는지 연결 전에 확인한다.
            rustls::ClientConfig::builder_with_provider(Arc::new(
                rustls::crypto::ring::default_provider(),
            ))
            .with_safe_default_protocol_versions()
            .map_err(|_| ApiError::tls("client"))?
            .with_root_certificates(rustls::RootCertStore::empty())
            .with_client_auth_cert(certs, parsed)
            .map_err(|_| ApiError::tls("invalid_certificate"))?;
            let mut pem = cert;
            pem.push(b'\n');
            pem.extend_from_slice(&key);
            Some(reqwest::Identity::from_pem(&pem).map_err(|_| ApiError::tls("invalid_key"))?)
        }
        _ => return Err(ApiError::tls("incomplete_identity")),
    };
    Ok(PreparedTls {
        verify: options.verify,
        roots,
        identity,
    })
}

pub(crate) type Resolver = Arc<dyn reqwest::dns::Resolve>;

/// 리디렉션은 직접 처리하므로 끈다. 쿠키도 jar가 직접 맡는다.
pub(crate) fn client(
    tls: &PreparedTls,
    identity: bool,
    resolver: Option<&Resolver>,
) -> Result<reqwest::Client, ApiError> {
    provider();
    let mut builder = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .tls_danger_accept_invalid_certs(!tls.verify);
    for root in &tls.roots {
        builder = builder.add_root_certificate(root.clone());
    }
    if identity && let Some(id) = &tls.identity {
        builder = builder.identity(id.clone());
    }
    if let Some(resolver) = resolver {
        builder = builder.dns_resolver(resolver.clone());
    }
    builder.build().map_err(|_| ApiError::tls("client"))
}
