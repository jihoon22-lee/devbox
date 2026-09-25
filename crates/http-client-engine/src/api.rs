//! Typed engine API; session ownership and admission remain in the product host.
use product_ipc::ExecutionClass;
use serde::Deserialize;
use tauri::Manager as _;
#[derive(Deserialize, ts_rs::TS)]
#[serde(
    tag = "method",
    content = "args",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ApiCall {
    TakePendingOpen {},
    FetchOpenapiSource {
        url: String,
    },
    PickMcpStdioExecutable {},
    PickMcpStdioCwd {},
    ConnectMcpStdio {
        profile: crate::commands::mcp_stdio::McpStdioProfile,
        environment: Vec<crate::commands::request::EnvironmentVariable>,
    },
    InvokeMcpStdio {
        connection_id: String,
        request_id: String,
        method: String,
        params: serde_json::Value,
    },
    CancelMcpStdio {
        connection_id: String,
        request_id: String,
    },
    DisconnectMcpStdio {
        connection_id: String,
    },
    ReadJsonFile {},
    SaveJsonFile {
        content: String,
        default_name: String,
    },
    SealSecret {
        value: String,
    },
    PickGrpcCa {},
    PickGrpcClientCertificate {},
    PickGrpcClientKey {},
    ImportGrpcTlsCredential {
        label: String,
        ca_selection_id: Option<String>,
        client_certificate_selection_id: Option<String>,
        client_key_selection_id: Option<String>,
    },
    ListGrpcTlsCredentials {},
    DeleteGrpcTlsCredential {
        credential_id: String,
    },
    ClaimApiRequest {
        handoff_id: String,
    },
    RenewApiRequest {
        handoff_id: String,
    },
    AckApiRequest {
        handoff_id: String,
    },
    RestoreApiRequest {
        handoff_id: String,
    },
    StartSseStream {
        req: crate::commands::request::RequestTemplate,
        environment: Vec<crate::commands::request::EnvironmentVariable>,
        options: crate::commands::sse::SseOptions,
    },
    StopSseStream {
        session_id: String,
    },
    AuthorizeMcpHttp {
        request_id: String,
        endpoint: String,
        issuer: Option<String>,
        client_id: String,
        scopes: Vec<String>,
    },
    CancelMcpOauth {
        request_id: String,
    },
    ListMcpOauthGrants {},
    RevokeMcpOauthGrant {
        grant_id: String,
        remove_local_on_remote_failure: bool,
    },
    PickGrpcProto {},
    PickGrpcImportRoot {},
    ConnectGrpc {
        profile: crate::commands::grpc::GrpcConnectProfile,
    },
    InvokeGrpc {
        connection_id: String,
        request_id: String,
        method: String,
        messages: Vec<String>,
    },
    CancelGrpc {
        connection_id: String,
        request_id: String,
    },
    DisconnectGrpc {
        connection_id: String,
    },
    ExportGrpcSummary {
        summary: crate::commands::grpc::GrpcExchangeSummary,
    },
    SendRequest {
        req: crate::commands::request::RequestTemplate,
        environment: Vec<crate::commands::request::EnvironmentVariable>,
        request_id: String,
    },
    CancelRequest {
        request_id: String,
    },
    DiscardCurrentResponse {},
    BuildRevealedCurl {
        req: crate::commands::request::RequestTemplate,
        environment: Vec<crate::commands::request::EnvironmentVariable>,
    },
    CopyRawResponseHeaders {
        response_id: String,
    },
    CopyRawResponseCookies {
        response_id: String,
    },
    SaveResponseBinary {
        response_id: String,
    },
    SanitizePersistedJson {
        serialized: String,
        environment: Vec<crate::commands::request::EnvironmentVariable>,
    },
    StartWebsocket {
        req: crate::commands::request::RequestTemplate,
        environment: Vec<crate::commands::request::EnvironmentVariable>,
    },
    SendWebsocketMessage {
        session_id: String,
        message: crate::commands::websocket::WebSocketMessageInput,
    },
    PingWebsocket {
        session_id: String,
        data: String,
    },
    CloseWebsocket {
        session_id: String,
        close: crate::commands::websocket::WebSocketCloseInput,
    },
    DisconnectWebsocket {
        session_id: String,
    },
    SaveWebsocketBinary {
        session_id: String,
        message_id: u64,
    },
    ConnectMcpHttp {
        profile: crate::commands::mcp::McpHttpProfile,
        environment: Vec<crate::commands::request::EnvironmentVariable>,
    },
    InvokeMcpHttp {
        connection_id: String,
        request_id: String,
        method: String,
        params: serde_json::Value,
    },
    CancelMcpHttp {
        connection_id: String,
        request_id: String,
    },
    DisconnectMcpHttp {
        connection_id: String,
    },
}
pub const API_METHODS: &[&str] = &[
    "take_pending_open",
    "fetch_openapi_source",
    "pick_mcp_stdio_executable",
    "pick_mcp_stdio_cwd",
    "connect_mcp_stdio",
    "invoke_mcp_stdio",
    "cancel_mcp_stdio",
    "disconnect_mcp_stdio",
    "read_json_file",
    "save_json_file",
    "seal_secret",
    "pick_grpc_ca",
    "pick_grpc_client_certificate",
    "pick_grpc_client_key",
    "import_grpc_tls_credential",
    "list_grpc_tls_credentials",
    "delete_grpc_tls_credential",
    "claim_api_request",
    "renew_api_request",
    "ack_api_request",
    "restore_api_request",
    "start_sse_stream",
    "stop_sse_stream",
    "authorize_mcp_http",
    "cancel_mcp_oauth",
    "list_mcp_oauth_grants",
    "revoke_mcp_oauth_grant",
    "pick_grpc_proto",
    "pick_grpc_import_root",
    "connect_grpc",
    "invoke_grpc",
    "cancel_grpc",
    "disconnect_grpc",
    "export_grpc_summary",
    "send_request",
    "cancel_request",
    "discard_current_response",
    "build_revealed_curl",
    "copy_raw_response_headers",
    "copy_raw_response_cookies",
    "save_response_binary",
    "sanitize_persisted_json",
    "start_websocket",
    "send_websocket_message",
    "ping_websocket",
    "close_websocket",
    "disconnect_websocket",
    "save_websocket_binary",
    "connect_mcp_http",
    "invoke_mcp_http",
    "cancel_mcp_http",
    "disconnect_mcp_http",
];
impl ApiCall {
    pub fn method(&self) -> &'static str {
        match self {
            Self::TakePendingOpen { .. } => "take_pending_open",
            Self::FetchOpenapiSource { .. } => "fetch_openapi_source",
            Self::PickMcpStdioExecutable { .. } => "pick_mcp_stdio_executable",
            Self::PickMcpStdioCwd { .. } => "pick_mcp_stdio_cwd",
            Self::ConnectMcpStdio { .. } => "connect_mcp_stdio",
            Self::InvokeMcpStdio { .. } => "invoke_mcp_stdio",
            Self::CancelMcpStdio { .. } => "cancel_mcp_stdio",
            Self::DisconnectMcpStdio { .. } => "disconnect_mcp_stdio",
            Self::ReadJsonFile { .. } => "read_json_file",
            Self::SaveJsonFile { .. } => "save_json_file",
            Self::SealSecret { .. } => "seal_secret",
            Self::PickGrpcCa { .. } => "pick_grpc_ca",
            Self::PickGrpcClientCertificate { .. } => "pick_grpc_client_certificate",
            Self::PickGrpcClientKey { .. } => "pick_grpc_client_key",
            Self::ImportGrpcTlsCredential { .. } => "import_grpc_tls_credential",
            Self::ListGrpcTlsCredentials { .. } => "list_grpc_tls_credentials",
            Self::DeleteGrpcTlsCredential { .. } => "delete_grpc_tls_credential",
            Self::ClaimApiRequest { .. } => "claim_api_request",
            Self::RenewApiRequest { .. } => "renew_api_request",
            Self::AckApiRequest { .. } => "ack_api_request",
            Self::RestoreApiRequest { .. } => "restore_api_request",
            Self::StartSseStream { .. } => "start_sse_stream",
            Self::StopSseStream { .. } => "stop_sse_stream",
            Self::AuthorizeMcpHttp { .. } => "authorize_mcp_http",
            Self::CancelMcpOauth { .. } => "cancel_mcp_oauth",
            Self::ListMcpOauthGrants { .. } => "list_mcp_oauth_grants",
            Self::RevokeMcpOauthGrant { .. } => "revoke_mcp_oauth_grant",
            Self::PickGrpcProto { .. } => "pick_grpc_proto",
            Self::PickGrpcImportRoot { .. } => "pick_grpc_import_root",
            Self::ConnectGrpc { .. } => "connect_grpc",
            Self::InvokeGrpc { .. } => "invoke_grpc",
            Self::CancelGrpc { .. } => "cancel_grpc",
            Self::DisconnectGrpc { .. } => "disconnect_grpc",
            Self::ExportGrpcSummary { .. } => "export_grpc_summary",
            Self::SendRequest { .. } => "send_request",
            Self::CancelRequest { .. } => "cancel_request",
            Self::DiscardCurrentResponse { .. } => "discard_current_response",
            Self::BuildRevealedCurl { .. } => "build_revealed_curl",
            Self::CopyRawResponseHeaders { .. } => "copy_raw_response_headers",
            Self::CopyRawResponseCookies { .. } => "copy_raw_response_cookies",
            Self::SaveResponseBinary { .. } => "save_response_binary",
            Self::SanitizePersistedJson { .. } => "sanitize_persisted_json",
            Self::StartWebsocket { .. } => "start_websocket",
            Self::SendWebsocketMessage { .. } => "send_websocket_message",
            Self::PingWebsocket { .. } => "ping_websocket",
            Self::CloseWebsocket { .. } => "close_websocket",
            Self::DisconnectWebsocket { .. } => "disconnect_websocket",
            Self::SaveWebsocketBinary { .. } => "save_websocket_binary",
            Self::ConnectMcpHttp { .. } => "connect_mcp_http",
            Self::InvokeMcpHttp { .. } => "invoke_mcp_http",
            Self::CancelMcpHttp { .. } => "cancel_mcp_http",
            Self::DisconnectMcpHttp { .. } => "disconnect_mcp_http",
        }
    }
    pub fn class(&self) -> ExecutionClass {
        match self {
            Self::CancelMcpStdio { .. }
            | Self::DisconnectMcpStdio { .. }
            | Self::StopSseStream { .. }
            | Self::CancelMcpOauth { .. }
            | Self::CancelGrpc { .. }
            | Self::DisconnectGrpc { .. }
            | Self::CancelRequest { .. }
            | Self::CloseWebsocket { .. }
            | Self::DisconnectWebsocket { .. }
            | Self::CancelMcpHttp { .. }
            | Self::DisconnectMcpHttp { .. } => ExecutionClass::Control,
            _ => ExecutionClass::Normal,
        }
    }
}
pub async fn dispatch(
    component_app: &tauri::AppHandle,
    call: ApiCall,
) -> Result<serde_json::Value, String> {
    match call {
        ApiCall::TakePendingOpen {} => {
            use crate::applink::*;
            let value = take_pending_open(component_app.state());
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        ApiCall::FetchOpenapiSource { url } => {
            use crate::commands::openapi::*;
            let value = fetch_openapi_source(url).await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        ApiCall::PickMcpStdioExecutable {} => {
            use crate::commands::mcp_stdio::*;
            let value =
                pick_mcp_stdio_executable(component_app.clone(), component_app.state()).await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        ApiCall::PickMcpStdioCwd {} => {
            use crate::commands::mcp_stdio::*;
            let value = pick_mcp_stdio_cwd(component_app.clone(), component_app.state()).await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        ApiCall::ConnectMcpStdio {
            profile,
            environment,
        } => {
            use crate::commands::mcp_stdio::*;
            let value = connect_mcp_stdio(component_app.state(), profile, environment).await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        ApiCall::InvokeMcpStdio {
            connection_id,
            request_id,
            method,
            params,
        } => {
            use crate::commands::mcp_stdio::*;
            let value = invoke_mcp_stdio(
                component_app.state(),
                connection_id,
                request_id,
                method,
                params,
            )
            .await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        ApiCall::CancelMcpStdio {
            connection_id,
            request_id,
        } => {
            use crate::commands::mcp_stdio::*;
            let value = cancel_mcp_stdio(component_app.state(), connection_id, request_id)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        ApiCall::DisconnectMcpStdio { connection_id } => {
            use crate::commands::mcp_stdio::*;
            disconnect_mcp_stdio(component_app.state(), connection_id).await?;
            serde_json::to_value(()).map_err(|_| "component_response_invalid".to_owned())
        }
        ApiCall::ReadJsonFile {} => {
            use crate::commands::transfer::*;
            let value = read_json_file(component_app.clone()).await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        ApiCall::SaveJsonFile {
            content,
            default_name,
        } => {
            use crate::commands::transfer::*;
            let value = save_json_file(component_app.clone(), content, default_name).await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        ApiCall::SealSecret { value } => {
            use crate::commands::secrets::*;
            let value = seal_secret(value)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        ApiCall::PickGrpcCa {} => {
            use crate::commands::grpc_credentials::*;
            let value = pick_grpc_ca(component_app.clone(), component_app.state()).await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        ApiCall::PickGrpcClientCertificate {} => {
            use crate::commands::grpc_credentials::*;
            let value =
                pick_grpc_client_certificate(component_app.clone(), component_app.state()).await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        ApiCall::PickGrpcClientKey {} => {
            use crate::commands::grpc_credentials::*;
            let value = pick_grpc_client_key(component_app.clone(), component_app.state()).await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        ApiCall::ImportGrpcTlsCredential {
            label,
            ca_selection_id,
            client_certificate_selection_id,
            client_key_selection_id,
        } => {
            use crate::commands::grpc_credentials::*;
            let value = import_grpc_tls_credential(
                component_app.clone(),
                component_app.state(),
                component_app.state(),
                label,
                ca_selection_id,
                client_certificate_selection_id,
                client_key_selection_id,
            )
            .await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        ApiCall::ListGrpcTlsCredentials {} => {
            use crate::commands::grpc_credentials::*;
            let value =
                list_grpc_tls_credentials(component_app.clone(), component_app.state()).await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        ApiCall::DeleteGrpcTlsCredential { credential_id } => {
            use crate::commands::grpc_credentials::*;
            let value = delete_grpc_tls_credential(
                component_app.clone(),
                component_app.state(),
                credential_id,
            )
            .await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        ApiCall::ClaimApiRequest { handoff_id } => {
            use crate::commands::handoff::*;
            let result = claim_api_request(component_app.state(), handoff_id.clone());
            if !component_app
                .state::<ApiHandoffState>()
                .has_claim(&handoff_id)
            {
                component_app
                    .state::<crate::applink::PendingOpen>()
                    .release(&handoff_id);
            }
            let value = result?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        ApiCall::RenewApiRequest { handoff_id } => {
            use crate::commands::handoff::*;
            let result = renew_api_request(component_app.state(), handoff_id.clone());
            if !component_app
                .state::<ApiHandoffState>()
                .has_claim(&handoff_id)
            {
                component_app
                    .state::<crate::applink::PendingOpen>()
                    .release(&handoff_id);
            }
            let value = result?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        ApiCall::AckApiRequest { handoff_id } => {
            use crate::commands::handoff::*;
            let result = ack_api_request(component_app.state(), handoff_id.clone());
            if !component_app
                .state::<ApiHandoffState>()
                .has_claim(&handoff_id)
            {
                component_app
                    .state::<crate::applink::PendingOpen>()
                    .release(&handoff_id);
            }
            let value = result?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        ApiCall::RestoreApiRequest { handoff_id } => {
            use crate::commands::handoff::*;
            let result = restore_api_request(component_app.state(), handoff_id.clone());
            if !component_app
                .state::<ApiHandoffState>()
                .has_claim(&handoff_id)
            {
                component_app
                    .state::<crate::applink::PendingOpen>()
                    .release(&handoff_id);
            }
            result?;
            serde_json::to_value(()).map_err(|_| "component_response_invalid".to_owned())
        }
        ApiCall::StartSseStream {
            req,
            environment,
            options,
        } => {
            use crate::commands::sse::*;
            let value = start_sse_stream(
                component_app.clone(),
                component_app.state(),
                req,
                environment,
                options,
            )?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        ApiCall::StopSseStream { session_id } => {
            use crate::commands::sse::*;
            stop_sse_stream(component_app.state(), session_id)?;
            serde_json::to_value(()).map_err(|_| "component_response_invalid".to_owned())
        }
        ApiCall::AuthorizeMcpHttp {
            request_id,
            endpoint,
            issuer,
            client_id,
            scopes,
        } => {
            use crate::commands::mcp_oauth::*;
            let value = authorize_mcp_http(
                component_app.clone(),
                component_app.state(),
                request_id,
                endpoint,
                issuer,
                client_id,
                scopes,
            )
            .await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        ApiCall::CancelMcpOauth { request_id } => {
            use crate::commands::mcp_oauth::*;
            let value = cancel_mcp_oauth(component_app.state(), request_id)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        ApiCall::ListMcpOauthGrants {} => {
            use crate::commands::mcp_oauth::*;
            let value = list_mcp_oauth_grants(component_app.clone(), component_app.state()).await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        ApiCall::RevokeMcpOauthGrant {
            grant_id,
            remove_local_on_remote_failure,
        } => {
            use crate::commands::mcp_oauth::*;
            let value = revoke_mcp_oauth_grant(
                component_app.clone(),
                component_app.state(),
                grant_id,
                remove_local_on_remote_failure,
            )
            .await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        ApiCall::PickGrpcProto {} => {
            use crate::commands::grpc::*;
            let value = pick_grpc_proto(component_app.clone(), component_app.state()).await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        ApiCall::PickGrpcImportRoot {} => {
            use crate::commands::grpc::*;
            let value = pick_grpc_import_root(component_app.clone(), component_app.state()).await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        ApiCall::ConnectGrpc { profile } => {
            use crate::commands::grpc::*;
            let value = connect_grpc(
                component_app.clone(),
                component_app.state(),
                component_app.state(),
                component_app.state(),
                profile,
            )
            .await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        ApiCall::InvokeGrpc {
            connection_id,
            request_id,
            method,
            messages,
        } => {
            use crate::commands::grpc::*;
            let value = invoke_grpc(
                component_app.state(),
                connection_id,
                request_id,
                method,
                messages,
            )
            .await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        ApiCall::CancelGrpc {
            connection_id,
            request_id,
        } => {
            use crate::commands::grpc::*;
            let value = cancel_grpc(component_app.state(), connection_id, request_id)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        ApiCall::DisconnectGrpc { connection_id } => {
            use crate::commands::grpc::*;
            disconnect_grpc(component_app.state(), connection_id)?;
            serde_json::to_value(()).map_err(|_| "component_response_invalid".to_owned())
        }
        ApiCall::ExportGrpcSummary { summary } => {
            use crate::commands::grpc::*;
            let value = export_grpc_summary(component_app.clone(), summary).await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        ApiCall::SendRequest {
            req,
            environment,
            request_id,
        } => {
            use crate::commands::request::*;
            let value = send_request(
                req,
                environment,
                request_id,
                component_app.state(),
                component_app.state(),
            )
            .await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        ApiCall::CancelRequest { request_id } => {
            use crate::commands::request::*;
            cancel_request(component_app.state(), request_id);
            serde_json::to_value(()).map_err(|_| "component_response_invalid".to_owned())
        }
        ApiCall::DiscardCurrentResponse {} => {
            use crate::commands::request::*;
            discard_current_response(component_app.state())?;
            serde_json::to_value(()).map_err(|_| "component_response_invalid".to_owned())
        }
        ApiCall::BuildRevealedCurl { req, environment } => {
            use crate::commands::request::*;
            let value = build_revealed_curl(req, environment)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        ApiCall::CopyRawResponseHeaders { response_id } => {
            use crate::commands::request::*;
            let value = copy_raw_response_headers(component_app.state(), response_id)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        ApiCall::CopyRawResponseCookies { response_id } => {
            use crate::commands::request::*;
            let value = copy_raw_response_cookies(component_app.state(), response_id)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        ApiCall::SaveResponseBinary { response_id } => {
            use crate::commands::request::*;
            let value =
                save_response_binary(component_app.clone(), component_app.state(), response_id)
                    .await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        ApiCall::SanitizePersistedJson {
            serialized,
            environment,
        } => {
            use crate::commands::request::*;
            let value = sanitize_persisted_json(serialized, environment)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        ApiCall::StartWebsocket { req, environment } => {
            use crate::commands::websocket::*;
            let value = start_websocket(
                component_app.clone(),
                component_app.state(),
                req,
                environment,
            )?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        ApiCall::SendWebsocketMessage {
            session_id,
            message,
        } => {
            use crate::commands::websocket::*;
            send_websocket_message(component_app.state(), session_id, message).await?;
            serde_json::to_value(()).map_err(|_| "component_response_invalid".to_owned())
        }
        ApiCall::PingWebsocket { session_id, data } => {
            use crate::commands::websocket::*;
            ping_websocket(component_app.state(), session_id, data).await?;
            serde_json::to_value(()).map_err(|_| "component_response_invalid".to_owned())
        }
        ApiCall::CloseWebsocket { session_id, close } => {
            use crate::commands::websocket::*;
            close_websocket(component_app.state(), session_id, close).await?;
            serde_json::to_value(()).map_err(|_| "component_response_invalid".to_owned())
        }
        ApiCall::DisconnectWebsocket { session_id } => {
            use crate::commands::websocket::*;
            disconnect_websocket(component_app.state(), session_id).await?;
            serde_json::to_value(()).map_err(|_| "component_response_invalid".to_owned())
        }
        ApiCall::SaveWebsocketBinary {
            session_id,
            message_id,
        } => {
            use crate::commands::websocket::*;
            let value = save_websocket_binary(
                component_app.clone(),
                component_app.state(),
                session_id,
                message_id,
            )
            .await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        ApiCall::ConnectMcpHttp {
            profile,
            environment,
        } => {
            use crate::commands::mcp::*;
            let value = connect_mcp_http(
                component_app.clone(),
                component_app.state(),
                component_app.state(),
                profile,
                environment,
            )
            .await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        ApiCall::InvokeMcpHttp {
            connection_id,
            request_id,
            method,
            params,
        } => {
            use crate::commands::mcp::*;
            let value = invoke_mcp_http(
                component_app.clone(),
                component_app.state(),
                component_app.state(),
                connection_id,
                request_id,
                method,
                params,
            )
            .await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        ApiCall::CancelMcpHttp {
            connection_id,
            request_id,
        } => {
            use crate::commands::mcp::*;
            let value = cancel_mcp_http(component_app.state(), connection_id, request_id)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        ApiCall::DisconnectMcpHttp { connection_id } => {
            use crate::commands::mcp::*;
            disconnect_mcp_http(component_app.state(), connection_id).await?;
            serde_json::to_value(()).map_err(|_| "component_response_invalid".to_owned())
        }
    }
}
pub fn result_types(
    export: &mut product_ipc::TypeExporter<'_>,
) -> Result<Vec<(&'static str, String)>, String> {
    Ok(vec![
        (
            "take_pending_open",
            export.register::<Option<devbox_applink::OpenRequest>>()?,
        ),
        (
            "fetch_openapi_source",
            export.register::<crate::commands::openapi::RemoteOpenApiSource>()?,
        ),
        (
            "pick_mcp_stdio_executable",
            export.register::<Option<crate::commands::mcp_stdio::McpNativeSelection>>()?,
        ),
        (
            "pick_mcp_stdio_cwd",
            export.register::<Option<crate::commands::mcp_stdio::McpNativeSelection>>()?,
        ),
        (
            "connect_mcp_stdio",
            export.register::<crate::commands::mcp::McpConnectResult>()?,
        ),
        (
            "invoke_mcp_stdio",
            export.register::<crate::commands::mcp::McpInvokeResult>()?,
        ),
        ("cancel_mcp_stdio", export.register::<bool>()?),
        ("disconnect_mcp_stdio", export.register::<()>()?),
        ("read_json_file", export.register::<Option<String>>()?),
        ("save_json_file", export.register::<bool>()?),
        ("seal_secret", export.register::<String>()?),
        (
            "pick_grpc_ca",
            export.register::<Option<crate::commands::grpc_selection::GrpcNativeSelection>>()?,
        ),
        (
            "pick_grpc_client_certificate",
            export.register::<Option<crate::commands::grpc_selection::GrpcNativeSelection>>()?,
        ),
        (
            "pick_grpc_client_key",
            export.register::<Option<crate::commands::grpc_selection::GrpcNativeSelection>>()?,
        ),
        (
            "import_grpc_tls_credential",
            export.register::<crate::commands::grpc_credentials::GrpcCredentialProjection>()?,
        ),
        (
            "list_grpc_tls_credentials",
            export
                .register::<Vec<crate::commands::grpc_credentials::GrpcCredentialProjection>>()?,
        ),
        ("delete_grpc_tls_credential", export.register::<bool>()?),
        (
            "claim_api_request",
            export.register::<crate::commands::handoff::ApiRequestHandoffPreview>()?,
        ),
        (
            "renew_api_request",
            export.register::<crate::commands::handoff::RenewApiRequestResult>()?,
        ),
        (
            "ack_api_request",
            export.register::<crate::commands::request::RequestTemplate>()?,
        ),
        ("restore_api_request", export.register::<()>()?),
        ("start_sse_stream", export.register::<String>()?),
        ("stop_sse_stream", export.register::<()>()?),
        (
            "authorize_mcp_http",
            export.register::<crate::commands::mcp_oauth::McpOAuthGrantProjection>()?,
        ),
        ("cancel_mcp_oauth", export.register::<bool>()?),
        (
            "list_mcp_oauth_grants",
            export.register::<Vec<crate::commands::mcp_oauth::McpOAuthGrantProjection>>()?,
        ),
        (
            "revoke_mcp_oauth_grant",
            export.register::<crate::commands::mcp_oauth::McpOAuthRevokeResult>()?,
        ),
        (
            "pick_grpc_proto",
            export.register::<Option<crate::commands::grpc_selection::GrpcNativeSelection>>()?,
        ),
        (
            "pick_grpc_import_root",
            export.register::<Option<crate::commands::grpc_selection::GrpcNativeSelection>>()?,
        ),
        (
            "connect_grpc",
            export.register::<crate::commands::grpc::GrpcConnectResult>()?,
        ),
        (
            "invoke_grpc",
            export.register::<crate::commands::grpc::GrpcInvokeResult>()?,
        ),
        ("cancel_grpc", export.register::<bool>()?),
        ("disconnect_grpc", export.register::<()>()?),
        ("export_grpc_summary", export.register::<bool>()?),
        (
            "send_request",
            export.register::<crate::commands::request::ApiResponse>()?,
        ),
        ("cancel_request", export.register::<()>()?),
        ("discard_current_response", export.register::<()>()?),
        ("build_revealed_curl", export.register::<String>()?),
        ("copy_raw_response_headers", export.register::<String>()?),
        ("copy_raw_response_cookies", export.register::<String>()?),
        ("save_response_binary", export.register::<bool>()?),
        ("sanitize_persisted_json", export.register::<String>()?),
        ("start_websocket", export.register::<String>()?),
        ("send_websocket_message", export.register::<()>()?),
        ("ping_websocket", export.register::<()>()?),
        ("close_websocket", export.register::<()>()?),
        ("disconnect_websocket", export.register::<()>()?),
        ("save_websocket_binary", export.register::<bool>()?),
        (
            "connect_mcp_http",
            export.register::<crate::commands::mcp::McpConnectResult>()?,
        ),
        (
            "invoke_mcp_http",
            export.register::<crate::commands::mcp::McpInvokeResult>()?,
        ),
        ("cancel_mcp_http", export.register::<bool>()?),
        ("disconnect_mcp_http", export.register::<()>()?),
    ])
}
product_ipc::issue_codes! {pub enum ApiIssue {
ApiWorkspaceInvalid="api_workspace_invalid",
ApiWorkspaceProjectUnavailable="api_workspace_project_unavailable",
ApiWorkspaceStale="api_workspace_stale",
ApiWorkspaceUnavailable="api_workspace_unavailable",
ComponentArgsInvalid="component_args_invalid",
ComponentDeliveryBusy="component_delivery_busy",
ComponentDeliveryInvalid="component_delivery_invalid",
ComponentDeliveryUnavailable="component_delivery_unavailable",
ComponentResponseInvalid="component_response_invalid",
ComponentStateConflict="component_state_conflict",
ComponentStorageUnavailable="component_storage_unavailable",
ComponentUnavailable="component_unavailable",
GrpcConnectTimeout="grpc_connect_timeout",
GrpcConnectionLimit="grpc_connection_limit",
GrpcConnectionStale="grpc_connection_stale",
GrpcCredentialInvalid="grpc_credential_invalid",
GrpcCredentialStorageFailed="grpc_credential_storage_failed",
GrpcCredentialStorageUnavailable="grpc_credential_storage_unavailable",
GrpcDescriptorInvalid="grpc_descriptor_invalid",
GrpcExportFailed="grpc_export_failed",
GrpcInvalidProfile="grpc_invalid_profile",
GrpcMethodUnavailable="grpc_method_unavailable",
GrpcNativeRequired="grpc_native_required",
GrpcProtocolFailed="grpc_protocol_failed",
GrpcReflectionUnavailable="grpc_reflection_unavailable",
GrpcRequestCancelled="grpc_request_cancelled",
GrpcRequestInvalid="grpc_request_invalid",
GrpcRequestLimit="grpc_request_limit",
GrpcRequestTimeout="grpc_request_timeout",
GrpcRequestTooLarge="grpc_request_too_large",
GrpcResponseTooLarge="grpc_response_too_large",
GrpcSourceInvalid="grpc_source_invalid",
GrpcSourceSelectionInvalid="grpc_source_selection_invalid",
GrpcSourceTooLarge="grpc_source_too_large",
GrpcTlsFailed="grpc_tls_failed",
KnowledgeDraftInvalid="knowledge_draft_invalid",
KnowledgeOwnerInvalid="knowledge_owner_invalid",
KnowledgeStorageFull="knowledge_storage_full",
KnowledgeStorageUnavailable="knowledge_storage_unavailable",
LegacyApiStorageInvalid="legacy_api_storage_invalid",
McpCapabilityUnavailable="mcp_capability_unavailable",
McpConnectTimeout="mcp_connect_timeout",
McpConnectionLimit="mcp_connection_limit",
McpConnectionStale="mcp_connection_stale",
McpCursorInvalid="mcp_cursor_invalid",
McpInvalidProfile="mcp_invalid_profile",
McpLegacyFallback="mcp_legacy_fallback",
McpLegacyVersionNegotiated="mcp_legacy_version_negotiated",
McpMessageInvalid="mcp_message_invalid",
McpOauthCallbackFailed="mcp_oauth_callback_failed",
McpOauthCancelled="mcp_oauth_cancelled",
McpOauthClientUnsupported="mcp_oauth_client_unsupported",
McpOauthDiscoveryFailed="mcp_oauth_discovery_failed",
McpOauthIssuerMismatch="mcp_oauth_issuer_mismatch",
McpOauthPkceRequired="mcp_oauth_pkce_required",
McpOauthReauthorizationRequired="mcp_oauth_reauthorization_required",
McpOauthRequestInvalid="mcp_oauth_request_invalid",
McpOauthRequired="mcp_oauth_required",
McpOauthResourceMismatch="mcp_oauth_resource_mismatch",
McpOauthRevokeFailed="mcp_oauth_revoke_failed",
McpOauthStorageFailed="mcp_oauth_storage_failed",
McpOauthTokenFailed="mcp_oauth_token_failed",
McpRedirectBlocked="mcp_redirect_blocked",
McpRequestCancelled="mcp_request_cancelled",
McpRequestLimit="mcp_request_limit",
McpRequestTimeout="mcp_request_timeout",
McpRequestTooLarge="mcp_request_too_large",
McpResponseTooLarge="mcp_response_too_large",
McpResponseTypeInvalid="mcp_response_type_invalid",
McpSchemaUnsupported="mcp_schema_unsupported",
McpSecretUnavailable="mcp_secret_unavailable",
McpServerError="mcp_server_error",
McpStdioCleanupFailed="mcp_stdio_cleanup_failed",
McpStdioConnectionLimit="mcp_stdio_connection_limit",
McpStdioConnectionStale="mcp_stdio_connection_stale",
McpStdioEnvironmentInvalid="mcp_stdio_environment_invalid",
McpStdioMessageTooLarge="mcp_stdio_message_too_large",
McpStdioProfileInvalid="mcp_stdio_profile_invalid",
McpStdioProtocolInvalid="mcp_stdio_protocol_invalid",
McpStdioRequestCancelled="mcp_stdio_request_cancelled",
McpStdioRequestLimit="mcp_stdio_request_limit",
McpStdioRequestTimeout="mcp_stdio_request_timeout",
McpStdioSelectionInvalid="mcp_stdio_selection_invalid",
McpStdioSpawnFailed="mcp_stdio_spawn_failed",
McpStdioTransportFailed="mcp_stdio_transport_failed",
McpTransportFailed="mcp_transport_failed",
McpVersionUnsupported="mcp_version_unsupported",
MockDraftBusy="mock_draft_busy",
MockDraftExpired="mock_draft_expired",
MockDraftInvalid="mock_draft_invalid",
MockDraftUnavailable="mock_draft_unavailable",
NativeError0Ffa141E36F0="native_error_0ffa141e36f0",
NativeError13A4255931Ab="native_error_13a4255931ab",
NativeError149741B17051="native_error_149741b17051",
NativeError17A72638B389="native_error_17a72638b389",
NativeError1F76B55D6589="native_error_1f76b55d6589",
NativeError252899A04E88="native_error_252899a04e88",
NativeError292B12C17C95="native_error_292b12c17c95",
NativeError33073648Cd6E="native_error_33073648cd6e",
NativeError349C6Ee3Df7F="native_error_349c6ee3df7f",
NativeError390B54C32F85="native_error_390b54c32f85",
NativeError3C1177Ccc6E5="native_error_3c1177ccc6e5",
NativeError3C8Be37Bf5E0="native_error_3c8be37bf5e0",
NativeError44Dde421C35C="native_error_44dde421c35c",
NativeError45C2Dd487777="native_error_45c2dd487777",
NativeError463Ac8Cf790C="native_error_463ac8cf790c",
NativeError4662419A3D49="native_error_4662419a3d49",
NativeError47D5Dd07796B="native_error_47d5dd07796b",
NativeError4A6C32Ea00A9="native_error_4a6c32ea00a9",
NativeError4E6D6D3Ae546="native_error_4e6d6d3ae546",
NativeError4F6139E6D8B2="native_error_4f6139e6d8b2",
NativeError52A4610Ed754="native_error_52a4610ed754",
NativeError57Dcc8E71E2B="native_error_57dcc8e71e2b",
NativeError5953C08A5925="native_error_5953c08a5925",
NativeError5A2568Ab5822="native_error_5a2568ab5822",
NativeError62C63840F5D9="native_error_62c63840f5d9",
NativeError64E46Eeb3623="native_error_64e46eeb3623",
NativeError6E4890Df1Afe="native_error_6e4890df1afe",
NativeError6Fe6A4E06Bfc="native_error_6fe6a4e06bfc",
NativeError700Ed54Fa2A1="native_error_700ed54fa2a1",
NativeError7065D6B493Cf="native_error_7065d6b493cf",
NativeError72Fc93Dbb3Bb="native_error_72fc93dbb3bb",
NativeError78Dd672D9Aae="native_error_78dd672d9aae",
NativeError7Af63Ead853B="native_error_7af63ead853b",
NativeError7E5C23216Bc5="native_error_7e5c23216bc5",
NativeError809Ec96B5769="native_error_809ec96b5769",
NativeError82F0B8E19A03="native_error_82f0b8e19a03",
NativeError86989F4B8D10="native_error_86989f4b8d10",
NativeError898930572Fbb="native_error_898930572fbb",
NativeError8B2107214Da0="native_error_8b2107214da0",
NativeError8B80C129C169="native_error_8b80c129c169",
NativeError8F2C5Cd08252="native_error_8f2c5cd08252",
NativeError900C775Dc059="native_error_900c775dc059",
NativeError9187Bbd3128C="native_error_9187bbd3128c",
NativeError9464Ae2B95A2="native_error_9464ae2b95a2",
NativeError9823300E2Dfd="native_error_9823300e2dfd",
NativeError9Ace2E8957Bf="native_error_9ace2e8957bf",
NativeError9B450Fbd09Ea="native_error_9b450fbd09ea",
NativeError9D044E02E4F2="native_error_9d044e02e4f2",
NativeErrorA40C623483B7="native_error_a40c623483b7",
NativeErrorA5D65A7C9Ad2="native_error_a5d65a7c9ad2",
NativeErrorB535F1F97A14="native_error_b535f1f97a14",
NativeErrorBa228492Dc19="native_error_ba228492dc19",
NativeErrorBe2D74B67A72="native_error_be2d74b67a72",
NativeErrorBf0590B1Cc53="native_error_bf0590b1cc53",
NativeErrorC2D036E22480="native_error_c2d036e22480",
NativeErrorC5295C1Dc259="native_error_c5295c1dc259",
NativeErrorCad5D5Fa7Bee="native_error_cad5d5fa7bee",
NativeErrorCbf814D599C5="native_error_cbf814d599c5",
NativeErrorCf85B26D9D44="native_error_cf85b26d9d44",
NativeErrorD4B6F4Fa1184="native_error_d4b6f4fa1184",
NativeErrorD7F9Bc586Cd9="native_error_d7f9bc586cd9",
NativeErrorE1Fe1C6953Db="native_error_e1fe1c6953db",
NativeErrorE3207A0B0Bb5="native_error_e3207a0b0bb5",
NativeErrorE49092C0Ff02="native_error_e49092c0ff02",
NativeErrorE4Fb51C5A26A="native_error_e4fb51c5a26a",
NativeErrorE60C1973Ba50="native_error_e60c1973ba50",
NativeErrorE8E3Aa74C6Ca="native_error_e8e3aa74c6ca",
NativeErrorEb904156E3Ae="native_error_eb904156e3ae",
NativeErrorEc9552Cfa292="native_error_ec9552cfa292",
NativeErrorF3Cfab0036Ec="native_error_f3cfab0036ec",
NativeErrorF420F083093A="native_error_f420f083093a",
NativeErrorF65026518D74="native_error_f65026518d74",
NativeErrorFc3030266031="native_error_fc3030266031",
NativeErrorFd29F5Ca70Fe="native_error_fd29f5ca70fe",
NativeErrorFe3411A5D8C8="native_error_fe3411a5d8c8",
OpenapiDefinitionFull="openapi_definition_full",
OpenapiDefinitionInvalid="openapi_definition_invalid",
OpenapiDefinitionUnavailable="openapi_definition_unavailable",
OwnedWorkerAssignmentFailed="owned_worker_assignment_failed",
Unavailable="unavailable",
}}
/// Fixed pre-existing messages are matched exactly, never by substrings.
/// Content-derived IDs keep these legacy translations stable across reordering.
pub fn classify(error: &str) -> &'static str {
    match error {
"Cookie header와 구조화 Cookie를 동시에 전송할 수 없습니다."=>ApiIssue::NativeError17A72638B389.code(),
"Cookie는 최대 100행까지 사용할 수 있습니다."=>ApiIssue::NativeError62C63840F5D9.code(),
"Developer Toolbox로 선택 영역을 전달하지 못했습니다. 클립보드로 자동 전환하지 않습니다"=>ApiIssue::NativeErrorB535F1F97A14.code(),
"Developer Toolbox를 사용할 수 없습니다. 클립보드로 자동 전환하지 않습니다"=>ApiIssue::NativeError4F6139E6D8B2.code(),
"GET SSE 요청에는 본문을 사용할 수 없습니다"=>ApiIssue::NativeError898930572Fbb.code(),
"GraphQL endpoint URL이 올바르지 않습니다"=>ApiIssue::NativeError9Ace2E8957Bf.code(),
"GraphQL endpoint query에 credential을 넣을 수 없습니다"=>ApiIssue::NativeError149741B17051.code(),
"GraphQL header 크기가 허용된 한계를 초과했습니다"=>ApiIssue::NativeError6E4890Df1Afe.code(),
"GraphQL header 행 수가 허용된 한계를 초과했습니다"=>ApiIssue::NativeErrorBf0590B1Cc53.code(),
"JSON 파일을 안전하게 처리하지 못했습니다"=>ApiIssue::NativeError52A4610Ed754.code(),
"OpenAPI URL 가져오기 시간이 초과되었습니다"=>ApiIssue::NativeErrorCad5D5Fa7Bee.code(),
"OpenAPI URL에 연결하지 못했습니다"=>ApiIssue::NativeError252899A04E88.code(),
"OpenAPI URL의 안전한 redirect 범위를 벗어났습니다"=>ApiIssue::NativeError57Dcc8E71E2B.code(),
"OpenAPI URL이 성공 응답을 반환하지 않았습니다"=>ApiIssue::NativeError9823300E2Dfd.code(),
"OpenAPI 문서는 4 MiB 이하만 가져올 수 있습니다"=>ApiIssue::NativeError292B12C17C95.code(),
"OpenAPI 문서를 안전하게 읽지 못했습니다"=>ApiIssue::NativeError7065D6B493Cf.code(),
"SSE idle timeout 범위가 올바르지 않습니다"=>ApiIssue::NativeError390B54C32F85.code(),
"SSE multipart part별 Content-Type은 데스크톱 앱에서만 사용할 수 있습니다."=>ApiIssue::NativeErrorA40C623483B7.code(),
"SSE multipart 파일 경로가 너무 깁니다"=>ApiIssue::NativeError3C1177Ccc6E5.code(),
"SSE multipart 파일 전송은 데스크톱 앱에서만 사용할 수 있습니다."=>ApiIssue::NativeError7Af63Ead853B.code(),
"SSE stream은 GET 또는 POST만 지원합니다"=>ApiIssue::NativeErrorF65026518D74.code(),
"SSE 리다이렉트 정책으로 요청을 차단했습니다"=>ApiIssue::NativeError7E5C23216Bc5.code(),
"SSE 연결 timeout 범위가 올바르지 않습니다"=>ApiIssue::NativeError9187Bbd3128C.code(),
"SSE 요청 Cookie가 너무 깁니다"=>ApiIssue::NativeError809Ec96B5769.code(),
"SSE 요청 URL이 너무 깁니다"=>ApiIssue::NativeError8F2C5Cd08252.code(),
"SSE 요청 URL이 올바르지 않습니다"=>ApiIssue::NativeError9D044E02E4F2.code(),
"SSE 요청 header가 너무 깁니다"=>ApiIssue::NativeErrorE3207A0B0Bb5.code(),
"SSE 요청 header가 올바르지 않습니다"=>ApiIssue::NativeError5A2568Ab5822.code(),
"SSE 요청 parameter가 너무 깁니다"=>ApiIssue::NativeError349C6Ee3Df7F.code(),
"SSE 요청 본문 형식이 올바르지 않습니다"=>ApiIssue::NativeError4E6D6D3Ae546.code(),
"SSE 요청 본문이 너무 큽니다"=>ApiIssue::NativeError1F76B55D6589.code(),
"SSE 요청 항목 수 또는 URL이 제한을 초과했습니다"=>ApiIssue::NativeError47D5Dd07796B.code(),
"SSE 요청 항목 수가 제한을 초과했습니다"=>ApiIssue::NativeErrorA5D65A7C9Ad2.code(),
"SSE 요청을 보낼 수 없습니다"=>ApiIssue::NativeError9B450Fbd09Ea.code(),
"SSE 응답 형식이 아닙니다"=>ApiIssue::NativeError900C775Dc059.code(),
"SSE 인증 설정이 너무 깁니다"=>ApiIssue::NativeErrorCbf814D599C5.code(),
"SSE 인증 설정이 올바르지 않습니다"=>ApiIssue::NativeError3C8Be37Bf5E0.code(),
"SSE 전체 timeout 범위가 올바르지 않습니다"=>ApiIssue::NativeError64E46Eeb3623.code(),
"SSE 환경 변수 형식이 올바르지 않습니다"=>ApiIssue::NativeError33073648Cd6E.code(),
"SSE 환경 변수는 최대 100개까지 사용할 수 있습니다"=>ApiIssue::NativeError5953C08A5925.code(),
"WebSocket binary payload가 올바르지 않습니다"=>ApiIssue::NativeError44Dde421C35C.code(),
"WebSocket binary를 안전하게 저장할 수 없습니다"=>ApiIssue::NativeError6Fe6A4E06Bfc.code(),
"WebSocket close code가 올바르지 않습니다"=>ApiIssue::NativeError0Ffa141E36F0.code(),
"WebSocket close reason이 올바르지 않습니다"=>ApiIssue::NativeErrorEc9552Cfa292.code(),
"WebSocket endpoint URL이 올바르지 않습니다"=>ApiIssue::NativeError8B80C129C169.code(),
"WebSocket endpoint query에 credential을 넣을 수 없습니다"=>ApiIssue::NativeError9464Ae2B95A2.code(),
"WebSocket message가 허용된 크기를 초과했습니다"=>ApiIssue::NativeError45C2Dd487777.code(),
"WebSocket message를 보낼 수 없습니다"=>ApiIssue::NativeErrorE8E3Aa74C6Ca.code(),
"WebSocket ping을 보낼 수 없습니다"=>ApiIssue::NativeError86989F4B8D10.code(),
"WebSocket session 상태를 읽을 수 없습니다"=>ApiIssue::NativeError4A6C32Ea00A9.code(),
"WebSocket 연결 timeout 범위가 올바르지 않습니다"=>ApiIssue::NativeErrorFd29F5Ca70Fe.code(),
"WebSocket 연결 시간이 초과되었습니다"=>ApiIssue::NativeErrorBa228492Dc19.code(),
"WebSocket 연결에 실패했습니다"=>ApiIssue::NativeError463Ac8Cf790C.code(),
"WebSocket 연결을 닫을 수 없습니다"=>ApiIssue::NativeErrorC2D036E22480.code(),
"WebSocket 연결을 시작할 수 없습니다"=>ApiIssue::NativeError13A4255931Ab.code(),
"WebSocket 연결이 끊어졌습니다"=>ApiIssue::NativeErrorF3Cfab0036Ec.code(),
"WebSocket 연결이 열려 있지 않습니다"=>ApiIssue::NativeErrorE4Fb51C5A26A.code(),
"WebSocket 요청 URL이 너무 깁니다"=>ApiIssue::NativeErrorE49092C0Ff02.code(),
"WebSocket 요청 header가 너무 깁니다"=>ApiIssue::NativeError82F0B8E19A03.code(),
"WebSocket 요청 header가 올바르지 않습니다"=>ApiIssue::NativeErrorD4B6F4Fa1184.code(),
"WebSocket 요청 parameter가 올바르지 않습니다"=>ApiIssue::NativeErrorF420F083093A.code(),
"WebSocket 요청 항목 수가 제한을 초과했습니다"=>ApiIssue::NativeErrorFe3411A5D8C8.code(),
"WebSocket 인증 설정이 올바르지 않습니다"=>ApiIssue::NativeErrorC5295C1Dc259.code(),
"binary 응답을 안전하게 저장할 수 없습니다"=>ApiIssue::NativeErrorE60C1973Ba50.code(),
"handoff 미리보기가 만료되었습니다. 다시 전달하세요"=>ApiIssue::NativeErrorCf85B26D9D44.code(),
"handoff 요청을 사용할 수 없습니다"=>ApiIssue::NativeErrorD7F9Bc586Cd9.code(),
"handoff 요청이 다른 작업에서 사용 중입니다. 잠시 후 다시 시도하세요"=>ApiIssue::NativeError78Dd672D9Aae.code(),
"handoff 요청이 만료되었거나 더 이상 사용할 수 없습니다"=>ApiIssue::NativeError8B2107214Da0.code(),
"handoff 저장소를 사용할 수 없습니다"=>ApiIssue::NativeError700Ed54Fa2A1.code(),
"secret 포함 SSE stream은 데스크톱 앱에서만 사용할 수 있습니다."=>ApiIssue::NativeErrorFc3030266031.code(),
"값에 공백, 세미콜론, 따옴표 또는 제어 문자를 사용할 수 없습니다."=>ApiIssue::NativeError72Fc93Dbb3Bb.code(),
"요청이 취소되었습니다"=>ApiIssue::NativeError4662419A3D49.code(),
"이름에 Cookie token으로 쓸 수 없는 문자가 있습니다."=>ApiIssue::NativeErrorEb904156E3Ae.code(),
"이름이 필요합니다."=>ApiIssue::NativeErrorBe2D74B67A72.code(),
"이미 실행 중인 WebSocket 연결이 있습니다"=>ApiIssue::NativeErrorE1Fe1C6953Db.code(),
_=>ApiIssue::from_code(error).unwrap_or(ApiIssue::Unavailable).code(),
}
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_methods_are_unique_and_unknown_errors_are_private() {
        let mut names = API_METHODS.to_vec();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), 52);
        assert_eq!(classify("synthetic remote secret"), "unavailable");
    }
    #[test]
    fn cancel_and_disconnect_keep_the_control_pool() {
        for value in [
            r#"{"method":"cancel_request","args":{"requestId":"r"}}"#,
            r#"{"method":"disconnect_mcp_stdio","args":{"connectionId":"s"}}"#,
        ] {
            let call: ApiCall = serde_json::from_str(value).unwrap();
            assert_eq!(call.class(), ExecutionClass::Control);
        }
        let read: ApiCall =
            serde_json::from_str(r#"{"method":"list_mcp_oauth_grants","args":{}}"#).unwrap();
        assert_eq!(read.class(), ExecutionClass::Normal);
    }
}
