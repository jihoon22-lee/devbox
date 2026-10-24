use devbox_deps::{
    model::AuditFailure,
    osv::{Http, NetworkHttp},
};

// This integration-test binary has no API service initializer or other tests.
// An invalid URL exercises client construction without opening any socket.
#[tokio::test]
async fn first_network_client_initializes_tls_in_a_fresh_process_without_network() {
    assert!(rustls::crypto::CryptoProvider::get_default().is_none());
    let result = NetworkHttp.post("not a URL", b"{}".to_vec()).await;
    assert_eq!(result, Err(AuditFailure::Network));
    assert!(rustls::crypto::CryptoProvider::get_default().is_some());
}
