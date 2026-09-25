//! 용도별 entropy를 정하는 secret 경계. DPAPI와 envelope는 공유 secrets crate가 소유한다.

use devbox_secrets::SealError;
use zeroize::Zeroizing;

/// 플랫폼에 맞는 `Sealer`를 반환한다. Windows가 아니면 봉인 불가(명확한 오류).
pub fn platform_sealer() -> Box<dyn devbox_secrets::Sealer> {
    #[cfg(target_os = "windows")]
    {
        Box::new(devbox_secrets::dpapi::DpapiSealer::new(ENVIRONMENT_ENTROPY))
    }
    #[cfg(not(target_os = "windows"))]
    {
        Box::new(UnsupportedSealer)
    }
}

/// gRPC TLS material uses a distinct DPAPI entropy domain so its sealed blobs
/// cannot be replayed as ordinary request-environment secrets (or vice versa).
pub fn platform_grpc_sealer() -> Box<dyn devbox_secrets::Sealer> {
    #[cfg(target_os = "windows")]
    {
        Box::new(devbox_secrets::dpapi::DpapiSealer::new(GRPC_TLS_ENTROPY))
    }
    #[cfg(not(target_os = "windows"))]
    {
        Box::new(UnsupportedSealer)
    }
}

// Persisted purpose domains: changing these would strand existing secrets.
#[cfg(windows)]
const ENVIRONMENT_ENTROPY: &[u8] = b"devbox.api-playground.secrets.v1";
#[cfg(windows)]
const GRPC_TLS_ENTROPY: &[u8] = b"devbox.api-playground.grpc-tls-credentials.v1";

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    fn dpapi_entropy_domains_do_not_cross_unseal() {
        let environment = platform_sealer();
        let grpc = platform_grpc_sealer();
        let environment_blob = environment.seal("environment-secret").unwrap();
        let grpc_blob = grpc.seal("grpc-private-key").unwrap();
        assert_eq!(
            environment.unseal(&environment_blob).unwrap().as_str(),
            "environment-secret"
        );
        assert_eq!(
            grpc.unseal(&grpc_blob).unwrap().as_str(),
            "grpc-private-key"
        );
        assert!(grpc.unseal(&environment_blob).is_err());
        assert!(environment.unseal(&grpc_blob).is_err());
    }
}

#[cfg_attr(target_os = "windows", allow(dead_code))]
struct UnsupportedSealer;

impl devbox_secrets::Sealer for UnsupportedSealer {
    fn seal(&self, _plaintext: &str) -> Result<Vec<u8>, SealError> {
        Err(SealError::CryptoFailure)
    }
    fn unseal(&self, _blob: &[u8]) -> Result<Zeroizing<String>, SealError> {
        Err(SealError::CryptoFailure)
    }
}
