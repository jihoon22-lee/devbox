devbox_protocol::domain_error! {
    pub enum WebhooksError {
        InvalidInput = "webhooks.invalid_input",
        NotFound = "webhooks.not_found",
        BindFailed = "webhooks.bind_failed",
        LimitReached = "webhooks.limit_reached",
        Storage = "webhooks.storage",
    }
}
pub(crate) fn storage(_: impl std::fmt::Display) -> devbox_protocol::Fail<WebhooksError> {
    WebhooksError::Storage.into()
}
