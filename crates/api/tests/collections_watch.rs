//! S5-3 3b: 컬렉션 폴더 감시(`api.collections_changed`, 300ms 디바운스).
#![cfg(unix)]
use collection_fixture::*;
use devbox_api::*;
use devbox_core::HubMessage;
use std::time::Duration;

mod collection_fixture;

#[tokio::test]
async fn external_edit_emits_collections_changed() {
    let f = Fixture::new().await;
    let (root, project) = f.project("app").await;
    f.write(&root, "a.request.json", "{}", CollectionExpect::Absent)
        .await
        .expect("create");
    f.list(&root).await;
    // 자기 쓰기에서 나온 이벤트가 지나가도록 디바운스 시간보다 길게 기다린 뒤 구독한다.
    tokio::time::sleep(Duration::from_millis(700)).await;
    let mut events = f.hub.subscribe("api.collections_changed");
    std::fs::write(project.join(".devbox/api/a.request.json"), "{\"x\":1}").expect("external edit");
    let message = tokio::time::timeout(Duration::from_secs(5), events.recv())
        .await
        .expect("change within 5s")
        .expect("hub open");
    let HubMessage::Event { payload, .. } = message else {
        panic!("unexpected resync");
    };
    let change: CollectionsChange = serde_json::from_str(&payload).expect("payload");
    assert_eq!(change.root, root);
    // 300ms 디바운스: 100ms 간격으로 이어진 편집은 한 번으로 합친다.
    for n in 0..5 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        std::fs::write(
            project.join(".devbox/api/a.request.json"),
            format!("{{\"n\":{n}}}"),
        )
        .expect("burst");
    }
    let burst = tokio::time::timeout(Duration::from_secs(5), events.recv())
        .await
        .expect("burst change");
    assert!(burst.is_some());
    let extra = tokio::time::timeout(Duration::from_millis(900), events.recv()).await;
    assert!(extra.is_err(), "burst must be debounced into one event");
}

#[tokio::test]
async fn recreated_collection_folder_is_watched_again() {
    let f = Fixture::new().await;
    let (root, project) = f.project("app").await;
    f.write(&root, "a.request.json", "{}", CollectionExpect::Absent)
        .await
        .expect("create");
    f.list(&root).await;
    // git checkout처럼 폴더를 지웠다가 다시 만든다(같은 경로, 다른 inode).
    std::fs::remove_dir_all(project.join(".devbox/api")).expect("remove");
    f.write(&root, "a.request.json", "{}", CollectionExpect::Absent)
        .await
        .expect("recreate");
    f.list(&root).await;
    tokio::time::sleep(Duration::from_millis(700)).await;
    let mut events = f.hub.subscribe("api.collections_changed");
    std::fs::write(project.join(".devbox/api/a.request.json"), "{\"y\":1}").expect("external edit");
    let message = tokio::time::timeout(Duration::from_secs(5), events.recv())
        .await
        .expect("change after the folder was recreated");
    assert!(matches!(message, Some(HubMessage::Event { .. })));
}

/// 리뷰 권장: 계속 편집돼도 이벤트가 끝없이 미뤄지지 않게 최대 대기(1초)를 둔다.
#[tokio::test]
async fn continuous_edits_still_emit_within_the_maximum_wait() {
    let f = Fixture::new().await;
    let (root, project) = f.project("app").await;
    f.write(&root, "a.request.json", "{}", CollectionExpect::Absent)
        .await
        .expect("create");
    f.list(&root).await;
    tokio::time::sleep(Duration::from_millis(700)).await;
    let mut events = f.hub.subscribe("api.collections_changed");
    let file = project.join(".devbox/api/a.request.json");
    let editor = tokio::spawn(async move {
        for n in 0..30 {
            std::fs::write(&file, format!("{{\"n\":{n}}}")).expect("edit");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    });
    let first = tokio::time::timeout(Duration::from_millis(1_800), events.recv()).await;
    assert!(
        !editor.is_finished(),
        "edits must still be running when the event arrives"
    );
    assert!(matches!(first, Ok(Some(HubMessage::Event { .. }))));
    editor.await.expect("editor");
}
