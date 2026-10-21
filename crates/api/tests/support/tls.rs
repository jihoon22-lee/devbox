//! 시험 안에서 rcgen으로 만든 소유 CA·서버·클라이언트 인증서(PEM을 커밋하지 않는다).
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use std::path::PathBuf;
use std::sync::Arc;

pub struct Pki {
    pub dir: tempfile::TempDir,
    pub ca_pem: String,
    pub server_cert_pem: String,
    pub server_key_pem: String,
    pub client_cert_pem: String,
    pub client_key_pem: String,
}
impl Pki {
    pub fn new() -> Self {
        let ca_key = rcgen::KeyPair::generate().expect("ca key");
        let mut ca_params = rcgen::CertificateParams::new(Vec::<String>::new()).expect("ca");
        ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        ca_params.key_usages = vec![
            rcgen::KeyUsagePurpose::KeyCertSign,
            rcgen::KeyUsagePurpose::CrlSign,
        ];
        ca_params
            .distinguished_name
            .push(rcgen::DnType::CommonName, "devbox fixture CA");
        let ca = ca_params.self_signed(&ca_key).expect("self signed CA");
        let issuer = rcgen::Issuer::new(ca_params, ca_key);
        let leaf = |names: Vec<String>, client: bool| {
            let key = rcgen::KeyPair::generate().expect("leaf key");
            let mut params = rcgen::CertificateParams::new(names).expect("leaf");
            params.extended_key_usages = vec![if client {
                rcgen::ExtendedKeyUsagePurpose::ClientAuth
            } else {
                rcgen::ExtendedKeyUsagePurpose::ServerAuth
            }];
            let cert = params.signed_by(&key, &issuer).expect("signed leaf");
            (cert.pem(), key.serialize_pem())
        };
        let (server_cert_pem, server_key_pem) =
            leaf(vec!["127.0.0.1".into(), "localhost".into()], false);
        let (client_cert_pem, client_key_pem) = leaf(vec!["devbox-client".into()], true);
        Self {
            dir: tempfile::tempdir().expect("owned pki dir"),
            ca_pem: ca.pem(),
            server_cert_pem,
            server_key_pem,
            client_cert_pem,
            client_key_pem,
        }
    }
    pub fn write(&self, name: &str, text: &str) -> String {
        let path: PathBuf = self.dir.path().join(name);
        std::fs::write(&path, text).expect("write fixture pem");
        path.display().to_string()
    }
    pub fn server(&self, require_client: bool) -> ServerTls {
        ServerTls {
            cert_pem: self.server_cert_pem.clone(),
            key_pem: self.server_key_pem.clone(),
            client_ca_pem: Some(self.ca_pem.clone()),
            require_client,
        }
    }
}

pub struct ServerTls {
    pub cert_pem: String,
    pub key_pem: String,
    pub client_ca_pem: Option<String>,
    pub require_client: bool,
}

pub fn acceptor(tls: ServerTls) -> tokio_rustls::TlsAcceptor {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let certs = CertificateDer::pem_slice_iter(tls.cert_pem.as_bytes())
        .collect::<Result<Vec<_>, _>>()
        .expect("server cert");
    let key = PrivateKeyDer::from_pem_slice(tls.key_pem.as_bytes()).expect("server key");
    let builder = rustls::ServerConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .expect("tls versions");
    let builder = match tls.client_ca_pem {
        Some(ca) => {
            let mut roots = rustls::RootCertStore::empty();
            for cert in CertificateDer::pem_slice_iter(ca.as_bytes()) {
                roots.add(cert.expect("ca cert")).expect("ca root");
            }
            let verifier = rustls::server::WebPkiClientVerifier::builder_with_provider(
                Arc::new(roots),
                provider,
            );
            let verifier = if tls.require_client {
                verifier
            } else {
                verifier.allow_unauthenticated()
            };
            builder.with_client_cert_verifier(verifier.build().expect("client verifier"))
        }
        None => builder.with_no_client_auth(),
    };
    let mut config = builder.with_single_cert(certs, key).expect("server config");
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    tokio_rustls::TlsAcceptor::from(Arc::new(config))
}
