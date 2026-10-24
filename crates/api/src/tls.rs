//! 요청 단위 TLS: CA·클라이언트 인증서·키 PEM(WSL 절대 경로), 검증 끔. 연결 전에 모두 검사한다.
use crate::{ApiError, TlsOptions};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
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
    /// WebSocket(rustls 직접)용 같은 재료. reqwest 형식에서는 다시 꺼낼 수 없어 따로 둔다.
    roots_der: Vec<CertificateDer<'static>>,
    identity_der: Option<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>)>,
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
    let mut roots_der = vec![];
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
            roots_der.push(der);
        }
    }
    let mut identity_der = None;
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
            .with_client_auth_cert(certs.clone(), parsed.clone_key())
            .map_err(|_| ApiError::tls("invalid_certificate"))?;
            identity_der = Some((certs, parsed));
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
        roots_der,
        identity_der,
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

/// WebSocket 연결용 rustls 설정(ring). reqwest와 같은 규칙: 검증 켬은 플랫폼 검증기(+ 지정 CA),
/// 검증 끔은 인증서를 받아들이고, 클라이언트 인증서가 있으면 보낸다. 어느 필드도 조용히 무시하지 않는다.
pub(crate) fn rustls_config(tls: &PreparedTls) -> Result<Arc<rustls::ClientConfig>, ApiError> {
    rustls_config_alpn(tls, &[b"http/1.1"])
}

/// 같은 규칙에 ALPN만 고른다(gRPC는 `h2`).
pub(crate) fn rustls_config_alpn(
    tls: &PreparedTls,
    alpn: &[&[u8]],
) -> Result<Arc<rustls::ClientConfig>, ApiError> {
    provider();
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .map_err(|_| ApiError::tls("client"))?
        .dangerous();
    let builder = if tls.verify {
        let verifier = if tls.roots_der.is_empty() {
            rustls_platform_verifier::Verifier::new(provider)
        } else {
            rustls_platform_verifier::Verifier::new_with_extra_roots(
                tls.roots_der.iter().cloned(),
                provider,
            )
        }
        .map_err(|_| ApiError::tls("client"))?;
        builder.with_custom_certificate_verifier(Arc::new(verifier))
    } else {
        builder.with_custom_certificate_verifier(Arc::new(AcceptAny(
            rustls::crypto::ring::default_provider().signature_verification_algorithms,
        )))
    };
    let mut config = match &tls.identity_der {
        Some((certs, key)) => builder
            .with_client_auth_cert(certs.clone(), key.clone_key())
            .map_err(|_| ApiError::tls("invalid_certificate"))?,
        None => builder.with_no_client_auth(),
    };
    config.alpn_protocols = alpn.iter().map(|p| p.to_vec()).collect();
    Ok(Arc::new(config))
}

/// 검증 끔(사용자 선택). 서명 확인 알고리즘 목록만 공급자 것을 쓴다.
#[derive(Debug)]
struct AcceptAny(rustls::crypto::WebPkiSupportedAlgorithms);

impl rustls::client::danger::ServerCertVerifier for AcceptAny {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.0)
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.0)
    }
    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.0.supported_schemes()
    }
}
