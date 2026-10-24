//! S5 도구 섹션 등록 모음(`api.*`; 이후 `webhooks.*`·`tools.*`도 여기 더한다).
use super::state::Services;
use devbox_protocol::Router;

pub(super) fn register(router: Router<Services>) -> Router<Services> {
    router
        .register::<devbox_tools::ToolsInstallCommand, _, _>(|_, _, p| async move {
            devbox_tools::install::install_command(p)
        })
        .register::<devbox_api::ApiSend, _, _>(|s, ctx, p| async move { s.api.send(ctx, p).await })
        .register::<devbox_api::ApiWsOpen, _, _>(
            |s, ctx, p| async move { s.api.ws_open(ctx, p).await },
        )
        .register::<devbox_api::ApiSseOpen, _, _>(|s, ctx, p| async move {
            s.api.sse_open(ctx, p).await
        })
        .register::<devbox_api::ApiGrpcDescribe, _, _>(|s, _, p| async move {
            s.api.grpc_describe(p).await
        })
        .register::<devbox_api::ApiGrpcCall, _, _>(|s, ctx, p| async move {
            s.api.grpc_call(ctx, p).await
        })
        .register::<devbox_api::ApiMcpConnect, _, _>(|s, ctx, p| async move {
            s.api.mcp_connect(ctx, p, &s.api_collections).await
        })
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
        .register::<devbox_webhooks::WebhooksListeners, _, _>(|s, _, _| async move {
            s.webhooks.listeners().await
        })
        .register::<devbox_webhooks::WebhooksStart, _, _>(|s, _, p| async move {
            s.webhooks.start(p).await
        })
        .register::<devbox_webhooks::WebhooksStop, _, _>(|s, _, p| async move {
            s.webhooks.stop(p).await
        })
        .register::<devbox_webhooks::WebhooksRules, _, _>(|s, _, p| async move {
            s.webhooks.rules(p).await
        })
        .register::<devbox_webhooks::WebhooksRuleUpsert, _, _>(|s, _, p| async move {
            s.webhooks.rule_upsert(p).await
        })
        .register::<devbox_webhooks::WebhooksRuleDelete, _, _>(|s, _, p| async move {
            s.webhooks.rule_delete(p).await
        })
        .register::<devbox_webhooks::WebhooksHistory, _, _>(|s, _, p| async move {
            s.webhooks.history(p).await
        })
        .register::<devbox_webhooks::WebhooksHistoryGet, _, _>(|s, _, p| async move {
            s.webhooks.history_get(p).await
        })
        .register::<devbox_webhooks::WebhooksFixtureSave, _, _>(|s, _, p| async move {
            s.webhooks.fixture_save(p).await
        })
}

#[cfg(test)]
mod tests {
    use devbox_protocol::{Ctx, Method};
    #[tokio::test]
    async fn tools_install_command_routes_a_static_plan_without_execution() {
        let temp = tempfile::tempdir().expect("owned fixture");
        let router =
            super::super::routes::router(super::super::build_services_for_test(temp.path()));
        let result = router
            .dispatch(
                devbox_tools::ToolsInstallCommand::NAME,
                Ctx {
                    channel: 1,
                    cancel: tokio_util::sync::CancellationToken::new(),
                    streams: None,
                },
                serde_json::value::RawValue::from_string(r#"{"id":"tool.tmux"}"#.into())
                    .expect("fixture params"),
            )
            .await
            .expect("install suggestion query");
        let plan: devbox_tools::InstallPlan =
            serde_json::from_str(result.get()).expect("typed install plan");
        assert!(
            matches!(plan, devbox_tools::InstallPlan::Wsl { command, .. } if command == "sudo apt install tmux")
        );
    }
}
