use super::*;
use crate::service::now_ms;
use std::sync::Arc;
async fn fixture() -> (tempfile::TempDir, WebhooksService, String) {
    let dir = tempfile::tempdir().expect("webhook history fixture");
    let db = devbox_core::Db::open(
        &dir.path().join("db"),
        &dir.path().join("backups"),
        "1",
        MIGRATIONS,
    )
    .expect("webhook history fixture");
    let svc = WebhooksService::new(
        db,
        Arc::new(devbox_core::Hub::new()),
        dir.path().join("webhooks"),
    );
    let id = uuid::Uuid::new_v4().to_string();
    let saved = id.clone();
    svc.inner
        .db
        .write(move |tx| {
            tx.execute(
                "INSERT INTO webhooks_listeners VALUES(?1,1234,0,0)",
                [saved],
            )?;
            Ok(())
        })
        .await
        .expect("webhook history fixture");
    (dir, svc, id)
}
fn detail(listener: &str, body: &[u8], at: i64) -> ReceivedDetail {
    ReceivedDetail {
        received: Received {
            listener_id: listener.into(),
            id: uuid::Uuid::new_v4().to_string(),
            method: "POST".into(),
            path: "/hook".into(),
            size_bytes: body.len() as u32,
            at_ms: at as f64,
            truncated: false,
        },
        query: "".into(),
        headers: vec![],
        body_base64: "".into(),
        preview_truncated: false,
    }
}
#[tokio::test]
async fn history_bounded_and_removal_does_not_retarget_stale_ids() {
    let (dir, svc, listener) = fixture().await;
    let first = detail(&listener, b"first", now_ms());
    let old = first.received.id.clone();
    svc.inner
        .record(first, b"first")
        .await
        .expect("webhook history fixture");
    for _ in 0..200 {
        svc.inner
            .record(detail(&listener, b"x", now_ms()), b"x")
            .await
            .expect("webhook history fixture");
    }
    let list = svc
        .history(HistoryQuery {
            listener_id: listener.clone(),
            before: None,
            limit: 500,
        })
        .await
        .expect("webhook history fixture");
    assert_eq!(list.items.len(), 200);
    assert!(
        svc.history_get(ReceivedId { id: old.clone() })
            .await
            .is_err()
    );
    assert!(
        !dir.path()
            .join("webhooks")
            .join(listener)
            .join(format!("{old}.bin"))
            .exists()
    );
}
#[tokio::test]
async fn retention_removes_only_owned_old_bodies() {
    let (dir, svc, listener) = fixture().await;
    let old = detail(&listener, b"old", now_ms() - 31 * 86400000i64);
    let id = old.received.id.clone();
    svc.inner
        .record(old, b"old")
        .await
        .expect("webhook history fixture");
    assert!(
        svc.history_get(ReceivedId { id: id.clone() })
            .await
            .is_err()
    );
    assert!(
        !dir.path()
            .join("webhooks")
            .join(&listener)
            .join(format!("{id}.bin"))
            .exists()
    );
}
#[tokio::test]
async fn binary_fixtures_keep_bytes_and_empty_body_is_valid() {
    let (_dir, svc, listener) = fixture().await;
    for bytes in [vec![0, 255, 128], vec![]] {
        let d = detail(&listener, &bytes, now_ms());
        let id = d.received.id.clone();
        svc.inner
            .record(d, &bytes)
            .await
            .expect("webhook history fixture");
        let fixture = svc
            .fixture_save(SaveFixture {
                received_id: id,
                name: "fixture".into(),
            })
            .await
            .expect("webhook history fixture");
        assert_eq!(
            data_encoding::BASE64
                .decode(fixture.request.body_base64.as_bytes())
                .expect("webhook history fixture"),
            bytes
        );
    }
}
#[tokio::test]
async fn oversized_preview_fails_without_partial_fixture() {
    let (_dir, svc, listener) = fixture().await;
    let bytes = vec![42; 1024 * 1024 + 1];
    let d = detail(&listener, &bytes, now_ms());
    let id = d.received.id.clone();
    svc.inner
        .record(d, &bytes)
        .await
        .expect("webhook history fixture");
    let view = svc
        .history_get(ReceivedId { id: id.clone() })
        .await
        .expect("webhook history fixture");
    assert!(view.preview_truncated);
    assert_eq!(
        data_encoding::BASE64
            .decode(view.body_base64.as_bytes())
            .expect("webhook history fixture")
            .len(),
        1024 * 1024
    );
    assert!(
        svc.fixture_save(SaveFixture {
            received_id: id,
            name: "large".into()
        })
        .await
        .is_err()
    );
    let count: i64 = svc
        .inner
        .db
        .read(|tx| tx.query_row("SELECT count(*) FROM webhooks_fixtures", [], |r| r.get(0)))
        .await
        .expect("webhook history fixture");
    assert_eq!(count, 0);
}
#[tokio::test]
async fn history_cursor_is_listener_scoped_and_stable() {
    let (_dir, svc, listener) = fixture().await;
    for _ in 0..3 {
        svc.inner
            .record(detail(&listener, b"x", now_ms()), b"x")
            .await
            .expect("webhook history fixture");
    }
    let first = svc
        .history(HistoryQuery {
            listener_id: listener.clone(),
            before: None,
            limit: 2,
        })
        .await
        .expect("webhook history fixture");
    let last = svc
        .history(HistoryQuery {
            listener_id: listener.clone(),
            before: Some(first.items[1].id.clone()),
            limit: 2,
        })
        .await
        .expect("webhook history fixture");
    assert_eq!(last.items.len(), 1);
    assert!(!first.items.iter().any(|x| x.id == last.items[0].id));
    assert!(
        svc.history(HistoryQuery {
            listener_id: "other".into(),
            before: Some(first.items[1].id.clone()),
            limit: 2
        })
        .await
        .expect("webhook history fixture")
        .items
        .is_empty()
    );
}
#[tokio::test]
#[cfg(unix)]
async fn body_files_are_private_and_symlinks_are_not_followed() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let (dir, svc, listener) = fixture().await;
    let d = detail(&listener, b"x", now_ms());
    let id = d.received.id.clone();
    svc.inner
        .record(d, b"x")
        .await
        .expect("webhook history fixture");
    let path = dir
        .path()
        .join("webhooks")
        .join(listener)
        .join(format!("{id}.bin"));
    assert_eq!(
        std::fs::metadata(&path)
            .expect("webhook history fixture")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    std::fs::remove_file(&path).expect("webhook history fixture");
    let external = dir.path().join("user-data");
    std::fs::write(&external, b"private").expect("webhook history fixture");
    symlink(external, &path).expect("webhook history fixture");
    assert!(svc.history_get(ReceivedId { id }).await.is_err());
}
#[tokio::test]
async fn expired_records_are_pruned_without_new_network_requests() {
    let (dir, svc, listener) = fixture().await;
    let d = detail(&listener, b"old", now_ms());
    let id = d.received.id.clone();
    svc.inner
        .record(d, b"old")
        .await
        .expect("webhook history fixture");
    let old_id = id.clone();
    svc.inner
        .db
        .write(move |tx| {
            tx.execute(
                "UPDATE webhooks_received SET at_ms=?1 WHERE id=?2",
                rusqlite::params![now_ms() - 31 * 86400000i64, old_id],
            )?;
            Ok(())
        })
        .await
        .expect("webhook history fixture");
    assert!(
        svc.history(HistoryQuery {
            listener_id: listener.clone(),
            before: None,
            limit: 20
        })
        .await
        .expect("webhook history fixture")
        .items
        .is_empty()
    );
    assert!(
        !dir.path()
            .join("webhooks")
            .join(listener)
            .join(format!("{id}.bin"))
            .exists()
    );
}
#[tokio::test]
#[cfg(unix)]
async fn retention_and_reads_do_not_follow_replaced_listener_directory() {
    use std::os::unix::fs::symlink;
    let (dir, svc, listener) = fixture().await;
    let d = detail(&listener, b"owned", now_ms());
    let id = d.received.id.clone();
    svc.inner
        .record(d, b"owned")
        .await
        .expect("webhook history fixture");
    let original = dir.path().join("webhooks").join(&listener);
    let moved = dir.path().join("user-data");
    std::fs::rename(&original, &moved).expect("webhook history fixture");
    symlink(&moved, &original).expect("webhook history fixture");
    assert!(
        svc.history_get(ReceivedId { id: id.clone() })
            .await
            .is_err()
    );
    let old_id = id.clone();
    svc.inner
        .db
        .write(move |tx| {
            tx.execute("UPDATE webhooks_received SET at_ms=0 WHERE id=?1", [old_id])?;
            Ok(())
        })
        .await
        .expect("webhook history fixture");
    assert!(
        svc.history(HistoryQuery {
            listener_id: listener,
            before: None,
            limit: 20
        })
        .await
        .is_err()
    );
    assert_eq!(
        std::fs::read(moved.join(format!("{id}.bin"))).expect("webhook history fixture"),
        b"owned"
    );
}
