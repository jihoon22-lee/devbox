export const ERROR_LABELS: Record<string, string> = {
  native_required: "Protocol Lab 네트워크 연결은 데스크톱 앱에서만 사용할 수 있습니다.",
  mcp_invalid_profile: "endpoint, timeout 또는 custom header 구성을 확인하세요.",
  mcp_secret_unavailable: "현재 Environment의 secret을 안전하게 해제할 수 없습니다.",
  mcp_connection_limit: "열 수 있는 MCP 연결 수를 초과했습니다.",
  mcp_connect_timeout: "MCP 연결 시간이 초과되었습니다.",
  mcp_transport_failed: "MCP transport 요청에 실패했습니다.",
  mcp_redirect_blocked: "credential 보호를 위해 redirect를 차단했습니다.",
  mcp_response_type_invalid: "서버가 JSON 또는 SSE가 아닌 응답을 반환했습니다.",
  mcp_request_too_large: "MCP 요청이 허용된 크기를 초과했습니다.",
  mcp_response_too_large: "MCP 응답이 허용된 크기를 초과했습니다.",
  mcp_message_invalid: "MCP message 또는 응답 구조가 올바르지 않습니다.",
  mcp_version_unsupported: "지원하는 MCP protocol version을 협상하지 못했습니다.",
  mcp_capability_unavailable: "서버가 이 capability를 제공하지 않습니다.",
  mcp_request_limit: "동시에 실행할 수 있는 MCP 요청 수를 초과했습니다.",
  mcp_request_timeout: "MCP 요청 시간이 초과되었습니다.",
  mcp_request_cancelled: "MCP 요청을 취소했습니다.",
  mcp_cursor_invalid: "pagination cursor가 올바르지 않습니다.",
  mcp_schema_unsupported: "이 tool schema는 안전한 호출 형식으로 해석할 수 없습니다.",
  mcp_connection_stale: "연결이 닫혔거나 오래되었습니다. 다시 연결하세요.",
  mcp_server_error: "MCP 서버가 JSON-RPC 오류를 반환했습니다.",
  mcp_stdio_selection_invalid: "선택한 native executable 또는 cwd를 다시 선택하세요.",
  mcp_stdio_profile_invalid: "stdio profile의 executable, 인자, environment 또는 timeout을 확인하세요.",
  mcp_stdio_environment_invalid: "stdio environment binding을 확인하세요.",
  mcp_stdio_spawn_failed: "native executable을 시작하지 못했습니다.",
  mcp_stdio_transport_failed: "native stdio transport 요청에 실패했습니다.",
  mcp_stdio_protocol_invalid: "native stdio MCP message가 올바르지 않습니다.",
  mcp_stdio_message_too_large: "native stdio MCP message가 허용된 크기를 초과했습니다.",
  mcp_stdio_request_timeout: "native stdio MCP 요청 시간이 초과되었습니다.",
  mcp_stdio_request_cancelled: "native stdio MCP 요청을 취소했습니다.",
  mcp_stdio_connection_stale: "native stdio 연결이 닫혔거나 오래되었습니다. 다시 연결하세요.",
  mcp_stdio_cleanup_failed: "native stdio process 정리를 완료하지 못했습니다.",
  mcp_stdio_connection_limit: "열 수 있는 native stdio 연결 수를 초과했습니다.",
  mcp_stdio_request_limit: "동시에 실행할 수 있는 native stdio 요청 수를 초과했습니다.",
  mcp_oauth_required: "선택한 OAuth grant를 다시 인증하세요.",
  mcp_oauth_request_invalid: "OAuth 요청 구성을 확인하세요.",
  mcp_oauth_discovery_failed: "OAuth 보호 resource 또는 authorization server를 확인하지 못했습니다.",
  mcp_oauth_resource_mismatch: "OAuth resource binding이 MCP endpoint와 일치하지 않습니다.",
  mcp_oauth_issuer_mismatch: "OAuth issuer binding이 선택한 issuer와 일치하지 않습니다.",
  mcp_oauth_pkce_required: "OAuth server가 필요한 PKCE S256을 지원하지 않습니다.",
  mcp_oauth_client_unsupported: "OAuth public client 구성을 지원하지 않습니다.",
  mcp_oauth_callback_failed: "OAuth browser callback을 확인하지 못했습니다.",
  mcp_oauth_token_failed: "OAuth token 교환에 실패했습니다.",
  mcp_oauth_storage_failed: "OAuth grant를 안전하게 저장하거나 읽지 못했습니다.",
  mcp_oauth_reauthorization_required: "OAuth grant가 만료되어 다시 인증해야 합니다.",
  mcp_oauth_cancelled: "OAuth authorization을 취소했습니다.",
  mcp_oauth_revoke_failed: "OAuth grant를 원격에서 revoke하지 못했습니다. 원하면 로컬에서 제거할 수 있습니다.",
};

export function isPromptArgument(value: unknown): value is { name: string; required?: boolean } {
  return (
    Boolean(value) &&
    typeof value === "object" &&
    typeof (value as { name?: unknown }).name === "string" &&
    ((value as { required?: unknown }).required === undefined ||
      typeof (value as { required?: unknown }).required === "boolean")
  );
}

export function boundedText(value: string, max: number): string {
  return value.length <= max ? value : `${value.slice(0, max)}…`;
}

export function formatOAuthExpiry(value: number | null): string {
  if (value === null) return "제공되지 않음";
  const formatted = new Date(value).toLocaleString();
  return formatted === "Invalid Date" ? "제공되지 않음" : formatted;
}

export function boundedJson(value: unknown, max: number): string {
  try {
    const serialized = JSON.stringify(value, null, 2);
    return serialized.length <= max ? serialized : `${serialized.slice(0, max)}\n… UI preview truncated …`;
  } catch {
    return "[표시할 수 없는 JSON]";
  }
}
