//! S5 도구 섹션 등록 모음(`api.*`; 이후 `webhooks.*`·`tools.*`도 여기 더한다).
use super::state::Services;
use devbox_protocol::Router;

pub(super) fn register(router: Router<Services>) -> Router<Services> {
    router
        .register::<devbox_api::ApiSend, _, _>(|s, ctx, p| async move { s.api.send(ctx, p).await })
        .register::<devbox_api::ApiHistory, _, _>(|s, _, p| async move { s.api.history(p).await })
        .register::<devbox_api::ApiHistoryGet, _, _>(
            |s, _, p| async move { s.api.history_get(p).await },
        )
        .register::<devbox_api::ApiCookies, _, _>(|s, _, p| async move { s.api.cookies(p).await })
        .register::<devbox_api::ApiCookiesClear, _, _>(|s, _, p| async move {
            s.api.cookies_clear(p).await
        })
        .register::<devbox_api::ApiCollections, _, _>(|s, _, p| async move {
            s.api_collections.list(p).await
        })
        .register::<devbox_api::ApiCollectionRead, _, _>(|s, _, p| async move {
            s.api_collections.read(p).await
        })
        .register::<devbox_api::ApiCollectionWrite, _, _>(|s, _, p| async move {
            s.api_collections.write(p).await
        })
        .register::<devbox_api::ApiCollectionDelete, _, _>(|s, _, p| async move {
            s.api_collections.delete(p).await
        })
        .register::<devbox_api::ApiCollectionReview, _, _>(|s, _, p| async move {
            s.api_collections.review(p).await
        })
}
