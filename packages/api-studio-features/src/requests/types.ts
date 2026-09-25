export type KeyValue = import("../generated/KeyValue").KeyValue;

/** 이전 v2 저장본에는 enabled가 없으며 이 경우 활성 상태로 해석한다. */
export type RequestHeader = import("../generated/RequestHeader").RequestHeader;

/** request Cookie header로 조립되는 단일 name/value 행. domain cookie jar가 아니다. */
export type RequestCookie = import("../generated/RequestCookie").RequestCookie;

export type MultipartPart = import("../generated/MultipartPart").MultipartPart;

export type AuthConfig = import("../generated/AuthConfig").AuthConfig;

export type GraphqlRequest = import("../generated/GraphqlRequest").GraphqlRequest;

export type GraphqlLocation = import("../generated/GraphqlLocation").GraphqlLocation;

export type GraphqlError = import("../generated/GraphqlError").GraphqlError;

export type GraphqlResponse = import("../generated/GraphqlResponse").GraphqlResponse;

/** 사용자가 편집하고 저장하는 요청 원본. 환경 변수 참조는 해석하지 않은 채 유지한다. */
type NativeRequestTemplate = import("../generated/RequestTemplate").RequestTemplate;
/** Editor state materializes arrays that are optional only at the native input boundary. */
export type RequestTemplate = NativeRequestTemplate & { cookies: RequestCookie[]; multipart: MultipartPart[] };

export type OpenTarget =
  | { kind: "path"; path: string; line: number | null; column: number | null }
  | { kind: "profile"; id: string }
  | { kind: "workspace"; path: string }
  | { kind: "query"; text: string }
  | { kind: "handoff"; handoffKind: string; id: string };

export type OpenRequest = import("../generated/OpenRequest").OpenRequest;

export type ApiRequestHandoffPreview = import("../generated/ApiRequestHandoffPreview").ApiRequestHandoffPreview;

/**
 * 저장 직전에 민감한 직접 입력값을 제거한 요청.
 * 실제 전송에 쓰이는 ResolvedRequest는 Rust 내부에만 존재한다.
 */
export interface PersistedHistoryRequest extends RequestTemplate {
  requiresSecretReview: boolean;
}

export type ApiResponse = import("../generated/ApiResponse").ApiResponse;

/** Safe result returned after the native response selection handoff is queued. */
export type ToolboxDispatch = Pick<import("../generated/HandoffResult").HandoffResult, "handoffId" | "redacted">;

/** Safe projection of a binary HTTP response. Raw bytes stay in native memory until explicit save. */
export type BinaryResponse = import("../generated/BinaryResponse").BinaryResponse;

export type ResponseCookie = import("../generated/ResponseCookie").ResponseCookie;

export type RedirectHop = import("../generated/RedirectHop").RedirectHop;

export interface HistoryItem {
  id: string;
  /** 사용자가 지정한 표시 이름. 기존 v2 항목은 URL을 fallback으로 사용한다. */
  name?: string;
  saved_at: number;
  request: PersistedHistoryRequest;
  status?: number;
}

export type SseOptions = import("../generated/SseOptions").SseOptions;

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

export type WebSocketMessageInput = import("../generated/WebSocketMessageInput").WebSocketMessageInput;

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

export type McpNativeSelection = import("../generated/McpNativeSelection").McpNativeSelection;

export type McpStdioEnvironmentBinding = import("../generated/McpStdioEnvironmentBinding").McpStdioEnvironmentBinding;

export type McpStdioProfile = import("../generated/McpStdioProfile").McpStdioProfile;

export type McpHttpProfile = import("../generated/McpHttpProfile").McpHttpProfile;

export type McpOAuthGrantStatus = "active" | "expired";

export type McpOAuthGrantProjection = import("../generated/McpOAuthGrantProjection").McpOAuthGrantProjection;

export type McpOAuthRevokeResult = import("../generated/McpOAuthRevokeResult").McpOAuthRevokeResult;

export type McpServerProjection = import("../generated/ServerProjection").ServerProjection;

export type McpTimelineEntry = import("../generated/McpTimelineEntry").McpTimelineEntry;

export type McpConnectResult = import("../generated/McpConnectResult").McpConnectResult;

export type McpInvokeResult = import("../generated/McpInvokeResult").McpInvokeResult;
