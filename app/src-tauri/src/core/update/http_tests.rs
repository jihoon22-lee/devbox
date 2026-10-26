use super::*;
use crate::core::{channel::AppChannel, update::storage::Root};
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

async fn server(reply: &'static [u8]) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buffer = [0; 4096];
        let _ = socket.read(&mut buffer).await;
        socket.write_all(reply).await.unwrap();
    });
    (format!("http://{address}/owned-fixture"), task)
}
fn local() -> Http {
    Http::new().unwrap();
    Http {
        deadline: Duration::from_secs(2),
        redirect_allowed: allowed_redirect,
        client: Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap(),
    }
}
fn candidate() -> Candidate {
    Candidate {
        version: "1.1.0".into(),
        id: 1,
        size: 3,
        digest: Sha256::digest(b"abc").into(),
    }
}
#[test]
fn tls_client_initializes_the_required_crypto_provider() {
    assert!(Http::new().is_ok());
    assert!(rustls::crypto::CryptoProvider::get_default().is_some());
}
#[tokio::test]
async fn real_http_200_streams_into_owned_file_and_verifies_digest() {
    let temp = tempfile::tempdir().unwrap();
    let root = Root::open(&temp.path().join("updates"), AppChannel::Prod).unwrap();
    let (url, task) =
        server(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\nabc").await;
    let proof = local()
        .download_at(
            &url,
            &candidate(),
            root.pending().unwrap(),
            &CancellationToken::new(),
            |_| {},
        )
        .await
        .unwrap();
    proof.revalidate().unwrap();
    drop(proof);
    task.await.unwrap();
    assert_eq!(
        std::fs::read_dir(temp.path().join("updates"))
            .unwrap()
            .count(),
        1
    );
}
#[tokio::test]
async fn real_http_302_to_untrusted_host_is_rejected_before_following() {
    let temp = tempfile::tempdir().unwrap();
    let root = Root::open(&temp.path().join("updates"), AppChannel::Prod).unwrap();
    let (url, task) = server(b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/not-authorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
    assert!(matches!(
        local()
            .download_at(
                &url,
                &candidate(),
                root.pending().unwrap(),
                &CancellationToken::new(),
                |_| {}
            )
            .await,
        Err(UpdateError::InvalidRelease)
    ));
    task.await.unwrap();
    assert_eq!(
        std::fs::read_dir(temp.path().join("updates"))
            .unwrap()
            .count(),
        1
    );
}
#[test]
fn redirects_require_exact_https_github_hosts_and_no_userinfo_or_port_tricks() {
    for url in [
        "https://release-assets.githubusercontent.com/github-production-release-asset/1",
        "https://objects.githubusercontent.com/a?token=fixture",
        "https://github.com/jihoon22-lee/devbox/a",
    ] {
        assert!(allowed_redirect(&Url::parse(url).unwrap()));
    }
    for url in [
        "http://github.com/x",
        "https://github.com.evil.invalid/x",
        "https://github.com@evil.invalid/x",
        "https://u:p@github.com/x",
        "https://api.github.com:444/x",
        "https://github.com/x#y",
        "file:///tmp/a",
    ] {
        assert!(!allowed_redirect(&Url::parse(url).unwrap()));
    }
}
#[tokio::test]
async fn real_http_length_and_chunk_overrun_and_digest_errors_leave_no_ready_file() {
    for (reply, expected) in [
        (&b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\nabcd"[..], UpdateError::Size),
        (&b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n4\r\nabcd\r\n0\r\n\r\n"[..], UpdateError::Size),
        (&b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\nxyz"[..], UpdateError::Digest),
    ] {
        let temp = tempfile::tempdir().unwrap(); let root = Root::open(&temp.path().join("updates"), AppChannel::Prod).unwrap();
        let (url, task) = server(reply).await;
        assert!(matches!(local().download_at(&url, &candidate(), root.pending().unwrap(), &CancellationToken::new(), |_| {}).await, Err(error) if error == expected));
        task.await.unwrap(); assert_eq!(std::fs::read_dir(temp.path().join("updates")).unwrap().count(), 1);
    }
}
#[tokio::test]
async fn cancel_interrupts_a_real_stalled_response_and_drops_its_writer() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/slow", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut b = [0; 1024];
        let _ = socket.read(&mut b).await;
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\na")
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_secs(5)).await;
    });
    let temp = tempfile::tempdir().unwrap();
    let root = Root::open(&temp.path().join("updates"), AppChannel::Prod).unwrap();
    let cancel = CancellationToken::new();
    let trigger = cancel.clone();
    let result = local()
        .download_at(
            &url,
            &candidate(),
            root.pending().unwrap(),
            &cancel,
            move |_| trigger.cancel(),
        )
        .await;
    assert!(matches!(result, Err(UpdateError::Cancelled)));
    server.abort();
    let _ = server.await;
    assert_eq!(
        std::fs::read_dir(temp.path().join("updates"))
            .unwrap()
            .count(),
        1
    );
}
#[tokio::test]
async fn metadata_is_bounded_and_404_is_no_release() {
    let (url, task) =
        server(b"HTTP/1.1 200 OK\r\nContent-Length: 1048577\r\nConnection: close\r\n\r\n").await;
    assert_eq!(
        local().metadata_at(&url, &CancellationToken::new()).await,
        Err(UpdateError::Size)
    );
    task.await.unwrap();
    let (url, task) =
        server(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
    let bytes = local()
        .metadata_at(&url, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(super::super::release::select(&bytes, "1.0.0"), Ok(None));
    task.await.unwrap();
}

#[tokio::test]
async fn real_302_follow_and_redirect_count_are_bounded_with_owned_loopback_transport() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let location = format!("{base}/body");
    let server = tokio::spawn(async move {
        for index in 0..2 {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut b = [0; 2048];
            let _ = socket.read(&mut b).await;
            let reply = if index == 0 {
                format!(
                    "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                )
            } else {
                "HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\nabc".into()
            };
            socket.write_all(reply.as_bytes()).await.unwrap();
        }
    });
    // Only this private test constructor substitutes HTTP loopback for the separately-tested HTTPS host policy.
    let mut http = local();
    http.redirect_allowed = |u| u.scheme() == "http" && u.host_str() == Some("127.0.0.1");
    let temp = tempfile::tempdir().unwrap();
    let root = Root::open(&temp.path().join("updates"), AppChannel::Prod).unwrap();
    let proof = http
        .download_at(
            &format!("{base}/asset"),
            &candidate(),
            root.pending().unwrap(),
            &CancellationToken::new(),
            |_| {},
        )
        .await
        .unwrap();
    proof.revalidate().unwrap();
    server.await.unwrap();
    drop(proof);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/loop", listener.local_addr().unwrap());
    let redirect = url.clone();
    let server = tokio::spawn(async move {
        for _ in 0..=REDIRECTS {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut b = [0; 2048];
            let _ = socket.read(&mut b).await;
            socket.write_all(format!("HTTP/1.1 302 Found\r\nLocation: {redirect}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
        }
    });
    assert!(matches!(
        http.download_at(
            &url,
            &candidate(),
            root.pending().unwrap(),
            &CancellationToken::new(),
            |_| {}
        )
        .await,
        Err(UpdateError::Network)
    ));
    server.await.unwrap();
}
#[tokio::test]
async fn whole_download_deadline_drops_a_live_writer_and_request_timeout_is_typed() {
    for whole_deadline in [true, false] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/stall", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut b = [0; 2048];
            let _ = socket.read(&mut b).await;
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\na")
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_secs(5)).await;
        });
        let mut http = local();
        if whole_deadline {
            http.deadline = Duration::from_millis(30);
        } else {
            http.client = Client::builder()
                .timeout(Duration::from_millis(30))
                .build()
                .unwrap();
        }
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("updates");
        let root = Root::open(&path, AppChannel::Prod).unwrap();
        assert!(matches!(
            http.download_at(
                &url,
                &candidate(),
                root.pending().unwrap(),
                &CancellationToken::new(),
                |_| {}
            )
            .await,
            Err(UpdateError::Timeout)
        ));
        server.abort();
        let _ = server.await;
        assert_eq!(std::fs::read_dir(path).unwrap().count(), 1);
    }
}
