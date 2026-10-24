use crate::fixture::*;
use crate::*;
use devbox_core::HubMessage;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

fn leased_elsewhere(c: &Claimed) -> bool {
    c.lease_id.is_none()
        && c.read_only
            == Some(ReadOnly {
                reason: ReadOnlyReason::LeasedElsewhere,
            })
}

#[tokio::test]
async fn second_window_gets_read_only_until_take() -> T {
    let fx = Fx::new();
    fx.write("a.md", "v1\n");
    let mut lost = fx.hub.subscribe("documents.lease_lost");
    let w1 = claimed(&fx.svc, "a.md", "w1").await;
    let w2 = claimed(&fx.svc, "a.md", "w2").await;
    assert!(leased_elsewhere(&w2));
    assert_eq!(
        w2.text.as_deref(),
        Some("v1\n"),
        "read-only view still shows the text"
    );
    assert!(w2.journal.is_none());
    let taken = fx
        .svc
        .take(
            CancellationToken::new(),
            TakeDocument {
                key: key("a.md"),
                window_id: "w2".into(),
                policy: None,
            },
        )
        .await
        .map_err(|e| format!("{e:?}"))?
        .expect("live owner");
    assert!(taken.lease_id.is_some() && taken.read_only.is_none());
    assert_ne!(taken.lease_id, w1.lease_id);
    let Some(HubMessage::Event { payload, .. }) =
        tokio::time::timeout(Duration::from_secs(1), lost.recv()).await?
    else {
        panic!("lease_lost is published");
    };
    let event: LeaseLost = serde_json::from_str(&payload)?;
    assert_eq!(
        (event.key, Some(event.lease_id)),
        (key("a.md"), w1.lease_id.clone())
    );
    let err = domain(fx.svc.save(save(&w1, w1.rev.clone(), "w1\n")).await);
    assert_eq!(
        err,
        DocumentsError::LeaseLost {
            reason: LeaseLoss::Taken
        }
    );
    fx.svc
        .save(save(&taken, taken.rev.clone(), "w2\n"))
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(fx.read("a.md"), "w2\n");
    Ok(())
}

#[tokio::test]
async fn owner_cancel_releases_only_that_lease() -> T {
    let fx = Fx::new();
    fx.write("a.md", "a\n");
    fx.write("b.md", "b\n");
    let (a, b, c) = (
        CancellationToken::new(),
        CancellationToken::new(),
        CancellationToken::new(),
    );
    claim_as(&fx.svc, a.clone(), "a.md", "w1").await;
    let first_b = claim_as(&fx.svc, b.clone(), "b.md", "w1").await;
    a.cancel();
    let next = claimed(&fx.svc, "a.md", "w2").await;
    assert!(
        next.lease_id.is_some(),
        "a cancelled owner frees its lease at once"
    );
    assert!(leased_elsewhere(&claimed(&fx.svc, "b.md", "w2").await));
    let moved = claim_as(&fx.svc, c.clone(), "b.md", "w1").await;
    assert_eq!(
        moved.lease_id, first_b.lease_id,
        "same window keeps its lease"
    );
    b.cancel();
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        leased_elsewhere(&claimed(&fx.svc, "b.md", "w2").await),
        "the old owner no longer holds the moved lease"
    );
    c.cancel();
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(claimed(&fx.svc, "b.md", "w2").await.lease_id.is_some());
    Ok(())
}

#[tokio::test]
async fn cancelled_owner_never_inserts_a_lease() -> T {
    let fx = Fx::new();
    fx.write("a.md", "a\n");
    let owner = CancellationToken::new();
    owner.cancel();
    let result = fx
        .svc
        .claim(owner, claim("a.md", "w1"))
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert!(
        result.is_none(),
        "a closing channel's claim reports cancellation"
    );
    assert!(claimed(&fx.svc, "a.md", "w2").await.lease_id.is_some());
    Ok(())
}

#[tokio::test]
async fn lease_survives_reconnect_with_same_window_and_expires_without_renewal() -> T {
    let fx = Fx::new();
    fx.write("a.md", "a\n");
    let first = claimed(&fx.svc, "a.md", "w1").await;
    let id = first.lease_id.clone().expect("lease");
    fx.advance(50_000);
    let renewed = fx
        .svc
        .renew(RenewLeases {
            lease_ids: vec![id.clone(), "unknown".into()],
        })
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(renewed.lost, ["unknown"]);
    fx.advance(50_000);
    let again = claimed(&fx.svc, "a.md", "w1").await;
    assert_eq!(again.lease_id.as_deref(), Some(id.as_str()));
    fx.advance(59_000);
    assert!(leased_elsewhere(&claimed(&fx.svc, "a.md", "w2").await));
    fx.advance(2_000);
    let other = claimed(&fx.svc, "a.md", "w2").await;
    assert!(
        other.lease_id.is_some(),
        "61 s without renewal expires the lease"
    );
    let err = domain(fx.svc.save(save(&first, first.rev.clone(), "x\n")).await);
    assert!(matches!(err, DocumentsError::LeaseLost { .. }));
    Ok(())
}

#[tokio::test]
async fn release_frees_the_document_for_another_window() -> T {
    let fx = Fx::new();
    fx.write("a.md", "a\n");
    let w1 = claimed(&fx.svc, "a.md", "w1").await;
    fx.svc
        .release(LeaseRef {
            lease_id: w1.lease_id.clone().expect("lease"),
        })
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert!(claimed(&fx.svc, "a.md", "w2").await.lease_id.is_some());
    let err = domain(fx.svc.save(save(&w1, w1.rev.clone(), "x\n")).await);
    assert!(matches!(err, DocumentsError::LeaseLost { .. }));
    Ok(())
}

#[tokio::test]
async fn an_owner_closing_during_a_save_leaves_no_watch_behind() -> T {
    use std::sync::Arc;
    use tokio::sync::Notify;
    let (reached, release) = (Arc::new(Notify::new()), Arc::new(Notify::new()));
    let (r2, g2) = (reached.clone(), release.clone());
    let fx = Fx::with(move |c| {
        let (reached, release) = (r2.clone(), g2.clone());
        c.hooks.before_save_watch = Some(Arc::new(move || {
            let (reached, release) = (reached.clone(), release.clone());
            Box::pin(async move {
                reached.notify_one();
                release.notified().await;
            })
        }));
    });
    fx.write("a.md", "a\n");
    let owner = CancellationToken::new();
    let c = claim_as(&fx.svc, owner.clone(), "a.md", "w1").await;
    assert!(fx.svc.s.watch.watching(&c.key));
    let svc = fx.svc.clone();
    let request = save(&c, c.rev.clone(), "b\n");
    let saving = tokio::spawn(async move { svc.save(request).await });
    tokio::time::timeout(Duration::from_secs(5), reached.notified()).await?;
    owner.cancel();
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        !fx.svc.s.watch.watching(&c.key),
        "eviction removed the watch"
    );
    release.notify_one();
    saving.await?.map_err(|e| format!("{e:?}"))?;
    assert!(
        !fx.svc.s.watch.watching(&c.key),
        "a save finishing after eviction does not re-watch a lease-less key"
    );
    Ok(())
}
