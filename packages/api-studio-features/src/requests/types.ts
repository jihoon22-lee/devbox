export interface KeyValue {
  key: string;
  value: string;
}

/** 이전 v2 저장본에는 enabled가 없으며 이 경우 활성 상태로 해석한다. */
export interface RequestHeader extends KeyValue {
  enabled?: boolean;
}

/** request Cookie header로 조립되는 단일 name/value 행. domain cookie jar가 아니다. */
export interface RequestCookie {
  name: string;
  value: string;
  /** 이전 저장본과의 일관성을 위해 누락 시 활성으로 해석한다. */
  enabled?: boolean;
}

export interface MultipartPart {
  kind: "text" | "file";
  name: string;
  /** text part의 값. file part에서는 항상 빈 문자열이다. */
  value: string;
  /** file picker가 선택한 현재 실행의 경로. persistence에서는 항상 제거한다. */
  file_path: string;
  /** 경로 없이 다시 선택할 파일을 알려 주는 basename 표시 metadata. */
  file_name: string;
  /** 비어 있으면 backend가 기본 content type을 사용한다. */
  content_type: string;
  enabled?: boolean;
}

export interface AuthConfig {
  kind: string;
  username: string;
  password: string;
  token: string;
  api_key: string;
  api_value: string;
}

export interface GraphqlRequest {
  query: string;
  variables: string;
  operation_name: string;
}

export interface GraphqlLocation {
  line: number;
  column: number;
}

export interface GraphqlError {
  message: string;
  locations: GraphqlLocation[];
  path: string[];
}

export interface GraphqlResponse {
  envelope: "valid" | "not_json" | "invalid" | "oversized";
  data: unknown | null;
  errors: GraphqlError[];
  errors_truncated: boolean;
}

/** 사용자가 편집하고 저장하는 요청 원본. 환경 변수 참조는 해석하지 않은 채 유지한다. */
export interface RequestTemplate {
  method: string;
  url: string;
  headers: RequestHeader[];
  cookies: RequestCookie[];
  multipart: MultipartPart[];
  params: KeyValue[];
  body_kind: string;
  body: string;
  auth: AuthConfig | null;
  timeout_ms: number;
  graphql?: GraphqlRequest | null;
}

export type OpenTarget =
  | { kind: "path"; path: string; line: number | null; column: number | null }
  | { kind: "profile"; id: string }
  | { kind: "workspace"; path: string }
  | { kind: "query"; text: string }
  | { kind: "handoff"; handoffKind: string; id: string };

export interface OpenRequest {
  target: OpenTarget;
  from: string | null;
}

export interface ApiRequestHandoffPreview {
  handoffId: string;
  kind: string;
  producerId: string;
  consumerId: string;
  expiresAtMs: number;
  request: RequestTemplate;
}

/**
 * 저장 직전에 민감한 직접 입력값을 제거한 요청.
 * 실제 전송에 쓰이는 ResolvedRequest는 Rust 내부에만 존재한다.
 */
export interface PersistedHistoryRequest extends RequestTemplate {
  requiresSecretReview: boolean;
}

export interface ApiResponse {
  status: number;
  status_text: string;
  headers: KeyValue[];
  duration_ms: number;
  size_bytes: number;
  body: string;
  is_json: boolean;
  final_url: string;
  redirects: RedirectHop[];
  cookies: ResponseCookie[];
  response_id: string | null;
  raw_headers_available: boolean;
  headers_truncated: boolean;
  binary?: BinaryResponse | null;
  graphql?: GraphqlResponse | null;
}

/** Safe result returned after the native response selection handoff is queued. */
export interface ToolboxDispatch {
  handoffId: string;
  redacted: boolean;
}

/** Safe projection of a binary HTTP response. Raw bytes stay in native memory until explicit save. */
export interface BinaryResponse {
  media_type: string;
  size_bytes: number;
  hex_preview: string;
  text_preview?: string | null;
  hex_truncated: boolean;
  text_truncated: boolean;
  save_available: boolean;
}

export interface ResponseCookie {
  name: string;
  /** 응답 DTO에서는 항상 마스킹된 값이다. */
  value: string;
  /** 알려진 안전 attribute만 제한적으로 표시하며, 미지 값은 마스킹한다. */
  attributes: KeyValue[];
}

export interface RedirectHop {
  status: number;
  location: string;
}

export interface HistoryItem {
  id: string;
  /** 사용자가 지정한 표시 이름. 기존 v2 항목은 URL을 fallback으로 사용한다. */
  name?: string;
  saved_at: number;
  request: PersistedHistoryRequest;
  status?: number;
}

export interface SseOptions {
  connectTimeoutMs: number;
  idleTimeoutMs: number;
  totalTimeoutMs: number;
  /** Native reconnect is deliberately opt-in and is capped by the transport. */
  reconnect: boolean;
}

export type SseUpdateKind = "connected" | "event" | "closed" | "error";

/** Safe event envelope emitted by the native task or browser preview. */
export interface SseUpdate {
  sessionId: string;
  kind: SseUpdateKind;
  event?: string;
  data?: string;
  id?: string;
  retryMs?: number;
  sequence: number;
  dropped: number;
  message?: string;
  attempt?: number;
}

export type WebSocketConnectionState = "idle" | "connecting" | "open" | "closing" | "closed" | "error";
export type WebSocketMessageKind = "text" | "binary" | "ping" | "pong" | "close";
export type WebSocketMessageDirection = "sent" | "received";

/** Masked message projection used by both native events and browser preview. */
export interface WebSocketMessage {
  id: number;
  direction: WebSocketMessageDirection;
  kind: WebSocketMessageKind;
  text?: string;
  textTruncated?: boolean;
  binaryHex?: string;
  binaryText?: string;
  binarySize?: number;
  binaryTruncated?: boolean;
  closeCode?: number;
  closeReason?: string;
}

export interface WebSocketMessageInput {
  kind: "text" | "binary" | "ping" | "pong";
  text: string;
  /** Base64 payload for binary/control messages. */
  data: string;
}

/** Fixed event envelope. It intentionally contains no URL, headers, raw error, or path. */
export interface WebSocketUpdate {
  sessionId: string;
  kind: "state" | "message";
  state?: WebSocketConnectionState;
  direction?: WebSocketMessageDirection;
  messageId?: number;
  messageType?: WebSocketMessageKind;
  text?: string;
  textTruncated?: boolean;
  binaryHex?: string;
  binaryText?: string;
  binarySize?: number;
  binaryTruncated?: boolean;
  closeCode?: number;
  closeReason?: string;
  sequence: number;
  dropped: number;
  message?: string;
}

export type McpEraPreference = "auto" | "modern" | "legacy";
export type McpEra = Exclude<McpEraPreference, "auto">;

export type McpTransport = "http" | "stdio";

export interface McpNativeSelection {
  selectionId: string;
  kind: "executable" | "directory";
  label: string;
  expiresAtMs: number;
}

export interface McpStdioEnvironmentBinding {
  childName: string;
  sourceName: string;
}

export interface McpStdioProfile {
  executableSelectionId: string;
  cwdSelectionId?: string;
  era: McpEraPreference;
  args: string[];
  environment: McpStdioEnvironmentBinding[];
  timeoutMs: number;
}

export interface McpHttpProfile {
  endpoint: string;
  era: McpEraPreference;
  headers: RequestHeader[];
  timeoutMs: number;
  oauthGrantId?: string;
}

export type McpOAuthGrantStatus = "active" | "expired";

export interface McpOAuthGrantProjection {
  grantId: string;
  issuer: string;
  resource: string;
  clientId: string;
  scopes: string[];
  expiresAtMs: number | null;
  status: McpOAuthGrantStatus;
}

export interface McpOAuthRevokeResult {
  remoteRevoked: boolean;
  removedLocal: boolean;
}

export interface McpServerProjection {
  era: McpEra;
  protocolVersion: string;
  serverName: string;
  serverVersion: string;
  capabilities: Record<string, unknown>;
  supportedVersions: string[];
}

export interface McpTimelineEntry {
  sequence: number;
  offsetMs: number;
  direction: "outgoing" | "incoming";
  kind: "request" | "notification" | "response" | "error";
  method: string | null;
  requestId: string | null;
  payload: unknown | null;
}

export interface McpConnectResult {
  connectionId: string;
  server: McpServerProjection;
  sessionManaged: boolean;
  timeline: McpTimelineEntry[];
}

export interface McpInvokeResult {
  result: unknown | null;
  errorCode: string | null;
  rpcErrorCode: number | null;
  nextCursor: string | null;
  timeline: McpTimelineEntry[];
}
