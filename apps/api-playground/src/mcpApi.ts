import { invoke } from "@tauri-apps/api/core";
import { isTauri } from "./lib/isTauri";
import type { EnvVariable } from "./lib/environments";
import type {
  McpConnectResult,
  McpHttpProfile,
  McpInvokeResult,
  McpNativeSelection,
  McpOAuthGrantProjection,
  McpOAuthGrantStatus,
  McpOAuthRevokeResult,
  McpStdioProfile,
  McpServerProjection,
  McpTimelineEntry,
} from "./types";

const NATIVE_REQUIRED = "native_required";
const MAX_TIMELINE_EVENTS = 1_000;
const MAX_TIMELINE_BYTES = 4 * 1024 * 1024;
const MAX_RESULT_BYTES = 4 * 1024 * 1024;
const CONNECTION_ID = /^[a-f0-9]{32}$/u;
const REQUEST_ID = /^[A-Za-z0-9_-]{1,128}$/u;
const OAUTH_REQUEST_ID = /^[A-Za-z0-9_.-]{1,128}$/u;
const GRANT_ID = /^[a-f0-9]{32}$/u;
const PROTOCOL_VERSION = /^\d{4}-\d{2}-\d{2}$/u;
const SAFE_ERROR_CODES = new Set([
  "mcp_invalid_profile",
  "mcp_secret_unavailable",
  "mcp_connection_limit",
  "mcp_connect_timeout",
  "mcp_transport_failed",
  "mcp_redirect_blocked",
  "mcp_response_type_invalid",
  "mcp_request_too_large",
  "mcp_response_too_large",
  "mcp_message_invalid",
  "mcp_version_unsupported",
  "mcp_capability_unavailable",
  "mcp_request_limit",
  "mcp_request_timeout",
  "mcp_request_cancelled",
  "mcp_cursor_invalid",
  "mcp_schema_unsupported",
  "mcp_connection_stale",
  "mcp_server_error",
  "mcp_stdio_selection_invalid",
  "mcp_stdio_profile_invalid",
  "mcp_stdio_environment_invalid",
  "mcp_stdio_spawn_failed",
  "mcp_stdio_transport_failed",
  "mcp_stdio_protocol_invalid",
  "mcp_stdio_message_too_large",
  "mcp_stdio_request_timeout",
  "mcp_stdio_request_cancelled",
  "mcp_stdio_connection_stale",
  "mcp_stdio_cleanup_failed",
  "mcp_stdio_connection_limit",
  "mcp_stdio_request_limit",
  "mcp_oauth_required",
  "mcp_oauth_request_invalid",
  "mcp_oauth_discovery_failed",
  "mcp_oauth_resource_mismatch",
  "mcp_oauth_issuer_mismatch",
  "mcp_oauth_pkce_required",
  "mcp_oauth_client_unsupported",
  "mcp_oauth_callback_failed",
  "mcp_oauth_token_failed",
  "mcp_oauth_storage_failed",
  "mcp_oauth_reauthorization_required",
  "mcp_oauth_cancelled",
  "mcp_oauth_revoke_failed",
]);

let requestSequence = 0;

export function nextMcpRequestId(): string {
  requestSequence = (requestSequence + 1) % Number.MAX_SAFE_INTEGER;
  const random = globalThis.crypto?.randomUUID?.().replace(/-/gu, "");
  return random
    ? `mcp-${random}`
    : `mcp-${Date.now().toString(36)}-${requestSequence.toString(36)}`;
}

export function safeMcpErrorCode(cause: unknown): string {
  const message = typeof cause === "string"
    ? cause
    : cause instanceof Error ? cause.message : "";
  if (message === NATIVE_REQUIRED || SAFE_ERROR_CODES.has(message)) return message;
  return "mcp_transport_failed";
}

export async function connectMcpHttp(
  profile: McpHttpProfile,
  environment: readonly EnvVariable[],
): Promise<McpConnectResult> {
  requireNative();
  const value = await invoke<unknown>("connect_mcp_http", { profile, environment });
  try {
    return validateConnectResult(value);
  } catch (cause) {
    const connectionId = isRecord(value) && typeof value.connectionId === "string"
      && CONNECTION_ID.test(value.connectionId)
      ? value.connectionId
      : null;
    if (connectionId) {
      try {
        await invoke<void>("disconnect_mcp_http", { connectionId });
      } catch {
        // Preserve the validation error even if best-effort native cleanup fails.
      }
    }
    throw cause;
  }
}

export async function invokeMcpHttp(
  connectionId: string,
  requestId: string,
  method: string,
  params: Record<string, unknown>,
): Promise<McpInvokeResult> {
  requireNative();
  if (!CONNECTION_ID.test(connectionId) || !REQUEST_ID.test(requestId)) {
    throw new Error("mcp_connection_stale");
  }
  const value = await invoke<unknown>("invoke_mcp_http", {
    connectionId,
    requestId,
    method,
    params,
  });
  return validateInvokeResult(value, method, requestId);
}

export async function cancelMcpHttp(
  connectionId: string,
  requestId: string,
): Promise<boolean> {
  requireNative();
  if (!CONNECTION_ID.test(connectionId) || !REQUEST_ID.test(requestId)) {
    throw new Error("mcp_connection_stale");
  }
  const value = await invoke<unknown>("cancel_mcp_http", { connectionId, requestId });
  if (typeof value !== "boolean") throw new Error("mcp_message_invalid");
  return value;
}

export async function disconnectMcpHttp(connectionId: string): Promise<void> {
  requireNative();
  if (!CONNECTION_ID.test(connectionId)) throw new Error("mcp_connection_stale");
  await invoke<void>("disconnect_mcp_http", { connectionId });
}

export async function pickMcpStdioExecutable(): Promise<McpNativeSelection | null> {
  return pickMcpStdioSelection("pick_mcp_stdio_executable", "executable");
}

export async function pickMcpStdioCwd(): Promise<McpNativeSelection | null> {
  return pickMcpStdioSelection("pick_mcp_stdio_cwd", "directory");
}

async function pickMcpStdioSelection(
  command: "pick_mcp_stdio_executable" | "pick_mcp_stdio_cwd",
  kind: McpNativeSelection["kind"],
): Promise<McpNativeSelection | null> {
  requireNative();
  const value = await invoke<unknown>(command);
  if (value === null) return null;
  const record = asRecord(value, "mcp_stdio_selection_invalid");
  const selectionId = record.selectionId;
  const label = record.label;
  const expiresAtMs = record.expiresAtMs;
  if (
    typeof selectionId !== "string"
    || !CONNECTION_ID.test(selectionId)
    || record.kind !== kind
    || typeof label !== "string"
    || utf8Bytes(label) > 256
    || hasControl(label)
    || label.includes("/")
    || label.includes("\\")
    || label === "."
    || label === ".."
    || typeof expiresAtMs !== "number"
    || !Number.isSafeInteger(expiresAtMs)
    || expiresAtMs < 0
  ) {
    throw new Error("mcp_stdio_selection_invalid");
  }
  // Only this safe projection crosses into renderer state. In particular, a
  // native dialog path (even if a malformed native payload includes one) is
  // never returned to the caller.
  return {
    selectionId,
    kind,
    label,
    expiresAtMs,
  };
}

export async function connectMcpStdio(
  profile: McpStdioProfile,
  environment: readonly EnvVariable[],
): Promise<McpConnectResult> {
  requireNative();
  const safeProfile: McpStdioProfile = {
    executableSelectionId: profile.executableSelectionId,
    ...(profile.cwdSelectionId === undefined ? {} : { cwdSelectionId: profile.cwdSelectionId }),
    era: profile.era,
    args: [...profile.args],
    environment: profile.environment.map(({ childName, sourceName }) => ({ childName, sourceName })),
    timeoutMs: profile.timeoutMs,
  };
  const value = await invoke<unknown>("connect_mcp_stdio", { profile: safeProfile, environment });
  try {
    return validateConnectResult(value);
  } catch (cause) {
    const connectionId = isRecord(value) && typeof value.connectionId === "string"
      && CONNECTION_ID.test(value.connectionId)
      ? value.connectionId
      : null;
    if (connectionId) {
      try {
        await invoke<void>("disconnect_mcp_stdio", { connectionId });
      } catch {
        // Preserve the validation error even if best-effort native cleanup fails.
      }
    }
    throw cause;
  }
}

export async function invokeMcpStdio(
  connectionId: string,
  requestId: string,
  method: string,
  params: Record<string, unknown>,
): Promise<McpInvokeResult> {
  requireNative();
  if (!CONNECTION_ID.test(connectionId) || !REQUEST_ID.test(requestId)) {
    throw new Error("mcp_stdio_connection_stale");
  }
  const value = await invoke<unknown>("invoke_mcp_stdio", {
    connectionId,
    requestId,
    method,
    params,
  });
  return validateInvokeResult(value, method, requestId);
}

export async function cancelMcpStdio(
  connectionId: string,
  requestId: string,
): Promise<boolean> {
  requireNative();
  if (!CONNECTION_ID.test(connectionId) || !REQUEST_ID.test(requestId)) {
    throw new Error("mcp_stdio_connection_stale");
  }
  const value = await invoke<unknown>("cancel_mcp_stdio", { connectionId, requestId });
  if (typeof value !== "boolean") throw new Error("mcp_stdio_protocol_invalid");
  return value;
}

export async function disconnectMcpStdio(connectionId: string): Promise<void> {
  requireNative();
  if (!CONNECTION_ID.test(connectionId)) throw new Error("mcp_stdio_connection_stale");
  await invoke<void>("disconnect_mcp_stdio", { connectionId });
}

export async function authorizeMcpHttp(
  requestId: string,
  endpoint: string,
  issuer: string | null | undefined,
  clientId: string,
  scopes: string[],
): Promise<McpOAuthGrantProjection> {
  requireNative();
  if (
    !OAUTH_REQUEST_ID.test(requestId)
    || !safeOAuthText(endpoint, 8 * 1024)
    || (issuer !== null && issuer !== undefined && !safeOAuthText(issuer, 8 * 1024))
    || !safeOAuthText(clientId, 8 * 1024)
    || scopes.length > 32
    || !scopes.every((scope) => isSafeOAuthScope(scope))
    || new Set(scopes).size !== scopes.length
  ) {
    throw new Error("mcp_oauth_request_invalid");
  }
  const payload = {
    requestId,
    endpoint,
    ...(issuer === undefined ? {} : { issuer }),
    clientId,
    scopes: [...scopes],
  };
  const value = await invoke<unknown>("authorize_mcp_http", payload);
  return validateOAuthGrantProjection(value);
}

export async function cancelMcpOAuth(requestId: string): Promise<boolean> {
  requireNative();
  if (!OAUTH_REQUEST_ID.test(requestId)) throw new Error("mcp_oauth_request_invalid");
  const value = await invoke<unknown>("cancel_mcp_oauth", { requestId });
  if (typeof value !== "boolean") throw new Error("mcp_oauth_request_invalid");
  return value;
}

export async function listMcpOAuthGrants(): Promise<McpOAuthGrantProjection[]> {
  requireNative();
  const value = await invoke<unknown>("list_mcp_oauth_grants");
  if (!Array.isArray(value) || value.length > 32) throw new Error("mcp_oauth_storage_failed");
  const grants = value.map(validateOAuthGrantProjection);
  if (new Set(grants.map((grant) => grant.grantId)).size !== grants.length) {
    throw new Error("mcp_oauth_storage_failed");
  }
  return grants;
}

export async function revokeMcpOAuthGrant(
  grantId: string,
  removeLocalOnRemoteFailure: boolean,
): Promise<McpOAuthRevokeResult> {
  requireNative();
  if (!GRANT_ID.test(grantId)) throw new Error("mcp_oauth_required");
  const value = await invoke<unknown>("revoke_mcp_oauth_grant", {
    grantId,
    removeLocalOnRemoteFailure,
  });
  const record = asRecord(value, "mcp_oauth_revoke_failed");
  if (typeof record.remoteRevoked !== "boolean" || typeof record.removedLocal !== "boolean") {
    throw new Error("mcp_oauth_revoke_failed");
  }
  return {
    remoteRevoked: record.remoteRevoked,
    removedLocal: record.removedLocal,
  };
}

function requireNative(): void {
  if (!isTauri()) throw new Error(NATIVE_REQUIRED);
}

function validateOAuthGrantProjection(value: unknown): McpOAuthGrantProjection {
  const record = asRecord(value, "mcp_oauth_request_invalid");
  const grantId = record.grantId;
  const issuer = record.issuer;
  const resource = record.resource;
  const clientId = record.clientId;
  const scopes = record.scopes;
  const expiresAtMs = record.expiresAtMs;
  const status = record.status;
  if (
    typeof grantId !== "string"
    || !GRANT_ID.test(grantId)
    || !safeOAuthText(issuer, 8 * 1024)
    || !safeOAuthText(resource, 8 * 1024)
    || !safeOAuthText(clientId, 8 * 1024)
    || !Array.isArray(scopes)
    || scopes.length > 32
    || !scopes.every((scope) => isSafeOAuthScope(scope))
    || new Set(scopes).size !== scopes.length
    || (expiresAtMs !== null && (
      typeof expiresAtMs !== "number"
      || !Number.isSafeInteger(expiresAtMs)
      || expiresAtMs < 0
    ))
    || (status !== "active" && status !== "expired")
  ) {
    throw new Error("mcp_oauth_request_invalid");
  }
  return {
    grantId,
    issuer: issuer as string,
    resource: resource as string,
    clientId: clientId as string,
    scopes: [...(scopes as string[])],
    expiresAtMs: expiresAtMs as number | null,
    status: status as McpOAuthGrantStatus,
  };
}

function safeOAuthText(value: unknown, maxBytes: number): value is string {
  return typeof value === "string"
    && value.length > 0
    && utf8Bytes(value) <= maxBytes
    && !hasControl(value);
}

function isSafeOAuthScope(value: unknown): value is string {
  return typeof value === "string"
    && value.length > 0
    && value.length <= 256
    && [...value].every((character) => {
      const code = character.codePointAt(0) ?? 0;
      return code >= 0x21 && code <= 0x7e;
    });
}

function validateConnectResult(value: unknown): McpConnectResult {
  const record = asRecord(value, "mcp_message_invalid");
  if (!CONNECTION_ID.test(asString(record.connectionId))) {
    throw new Error("mcp_message_invalid");
  }
  if (typeof record.sessionManaged !== "boolean") throw new Error("mcp_message_invalid");
  const server = validateServer(record.server);
  if (server.era === "modern" && record.sessionManaged) throw new Error("mcp_message_invalid");
  const timeline = validateTimeline(record.timeline);
  const first = timeline[0];
  if (
    first.direction !== "outgoing"
    || first.kind !== "request"
    || first.method !== (server.era === "modern" ? "server/discover" : "initialize")
    || first.requestId !== (server.era === "modern" ? "discover-1" : "initialize-1")
  ) {
    throw new Error("mcp_message_invalid");
  }
  if (server.era === "modern") {
    const last = timeline[timeline.length - 1];
    if (
      last.direction !== "incoming"
      || last.kind !== "response"
      || last.requestId !== "discover-1"
      || !timeline.slice(1, -1).every(isIncomingNotification)
    ) {
      throw new Error("mcp_message_invalid");
    }
  } else {
    const last = timeline[timeline.length - 1];
    const handshake = timeline.slice(1, -1);
    const initializeResponse = handshake[handshake.length - 1];
    if (
      !initializeResponse
      || initializeResponse.direction !== "incoming"
      || initializeResponse.kind !== "response"
      || initializeResponse.requestId !== "initialize-1"
      || !handshake.slice(0, -1).every(isIncomingNotification)
      || last.direction !== "outgoing"
      || last.kind !== "notification"
      || last.method !== "notifications/initialized"
      || last.requestId !== null
    ) {
      throw new Error("mcp_message_invalid");
    }
  }
  return {
    connectionId: record.connectionId as string,
    server,
    sessionManaged: record.sessionManaged,
    timeline,
  };
}

function validateServer(value: unknown): McpServerProjection {
  const record = asRecord(value, "mcp_message_invalid");
  const era = record.era;
  const protocolVersion = asString(record.protocolVersion);
  const serverName = asString(record.serverName);
  const serverVersion = asString(record.serverVersion);
  const capabilities = asRecord(record.capabilities, "mcp_message_invalid");
  if (
    (era !== "modern" && era !== "legacy")
    || !PROTOCOL_VERSION.test(protocolVersion)
    || (era === "modern" && protocolVersion !== "2026-07-28")
    || (era === "legacy" && protocolVersion !== "2025-11-25")
    || utf8Bytes(serverName) > 512
    || utf8Bytes(serverVersion) > 512
    || hasControl(serverName)
    || hasControl(serverVersion)
    || jsonBytes(capabilities) > 256 * 1024
    || !Array.isArray(record.supportedVersions)
    || record.supportedVersions.length === 0
    || record.supportedVersions.length > 16
  ) {
    throw new Error("mcp_message_invalid");
  }
  const supportedVersions = record.supportedVersions.map((item) => {
    const version = asString(item);
    if (!PROTOCOL_VERSION.test(version)) throw new Error("mcp_message_invalid");
    return version;
  });
  if (
    !supportedVersions.includes(protocolVersion)
    || new Set(supportedVersions).size !== supportedVersions.length
  ) throw new Error("mcp_message_invalid");
  return {
    era,
    protocolVersion,
    serverName,
    serverVersion,
    capabilities,
    supportedVersions,
  };
}

function validateInvokeResult(value: unknown, method: string, requestId: string): McpInvokeResult {
  const record = asRecord(value, "mcp_message_invalid");
  if (!["result", "errorCode", "rpcErrorCode", "nextCursor", "timeline"].every(
    (key) => key in record,
  )) throw new Error("mcp_message_invalid");
  const result = record.result ?? null;
  const errorCode = record.errorCode ?? null;
  const rpcErrorCode = record.rpcErrorCode ?? null;
  const nextCursor = record.nextCursor ?? null;
  if (
    (errorCode !== null && (typeof errorCode !== "string" || !SAFE_ERROR_CODES.has(errorCode)))
    || (rpcErrorCode !== null && (!Number.isSafeInteger(rpcErrorCode) || typeof rpcErrorCode !== "number"))
    || (nextCursor !== null && (
      typeof nextCursor !== "string"
      || nextCursor.length === 0
      || nextCursor.length > 4 * 1024
      || hasControl(nextCursor)
    ))
    || (result === null) !== (errorCode !== null)
    || (rpcErrorCode === null) !== (errorCode === null)
    || (errorCode !== null && nextCursor !== null)
    || (result !== null && !isRecord(result))
    || (result !== null && jsonBytes(result) > MAX_RESULT_BYTES)
  ) {
    throw new Error("mcp_message_invalid");
  }
  if (result !== null) {
    const resultCursor = isRecord(result) && result.nextCursor === "[PRESENT]"
      ? "[PRESENT]"
      : null;
    const listMethod = [
      "tools/list",
      "resources/list",
      "resources/templates/list",
      "prompts/list",
    ].includes(method);
    const expectedProjection = listMethod && nextCursor !== null ? "[PRESENT]" : null;
    if (
      (listMethod && resultCursor !== expectedProjection)
      || (!listMethod && (resultCursor !== null || nextCursor !== null))
    ) {
      throw new Error("mcp_message_invalid");
    }
  }
  const timeline = validateTimeline(record.timeline);
  const first = timeline[0];
  const last = timeline[timeline.length - 1];
  if (
    first.direction !== "outgoing"
    || first.kind !== "request"
    || first.method !== method
    || first.requestId !== requestId
    || last.direction !== "incoming"
    || last.kind !== (errorCode === null ? "response" : "error")
    || last.requestId !== requestId
    || !timeline.slice(1, -1).every(isIncomingNotification)
  ) {
    throw new Error("mcp_message_invalid");
  }
  return {
    result,
    errorCode,
    rpcErrorCode,
    nextCursor,
    timeline,
  };
}

function validateTimeline(value: unknown): McpTimelineEntry[] {
  if (!Array.isArray(value) || value.length === 0 || value.length > MAX_TIMELINE_EVENTS) {
    throw new Error("mcp_message_invalid");
  }
  if (jsonBytes(value) > MAX_TIMELINE_BYTES) throw new Error("mcp_response_too_large");
  return value.map((candidate, index) => {
    const entry = asRecord(candidate, "mcp_message_invalid");
    const kind = asString(entry.kind) as McpTimelineEntry["kind"];
    const method = entry.method ?? null;
    const requestId = entry.requestId ?? null;
    const payload = entry.payload ?? null;
    if (
      entry.sequence !== index + 1
      || !Number.isSafeInteger(entry.offsetMs)
      || typeof entry.offsetMs !== "number"
      || entry.offsetMs < 0
      || (entry.direction !== "outgoing" && entry.direction !== "incoming")
      || !["request", "notification", "response", "error"].includes(kind)
      || (method !== null && (
        typeof method !== "string"
        || method.length === 0
        || method.length > 256
        || hasControl(method)
      ))
      || (requestId !== null && (typeof requestId !== "string" || !REQUEST_ID.test(requestId)))
      || (payload !== null && jsonBytes(payload) > MAX_RESULT_BYTES)
      || (kind === "request" && (
        entry.direction !== "outgoing" || method === null || requestId === null
      ))
      || (kind === "notification" && (method === null || requestId !== null))
      || ((kind === "response" || kind === "error") && (
        entry.direction !== "incoming" || method !== null || requestId === null
      ))
    ) {
      throw new Error("mcp_message_invalid");
    }
    return {
      sequence: entry.sequence as number,
      offsetMs: entry.offsetMs,
      direction: entry.direction,
      kind,
      method,
      requestId,
      payload,
    } as McpTimelineEntry;
  });
}

function isIncomingNotification(entry: McpTimelineEntry): boolean {
  return entry.direction === "incoming"
    && entry.kind === "notification"
    && entry.method !== null
    && entry.requestId === null;
}

function asRecord(value: unknown, code: string): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error(code);
  return value as Record<string, unknown>;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}

function asString(value: unknown): string {
  if (typeof value !== "string") throw new Error("mcp_message_invalid");
  return value;
}

function hasControl(value: string): boolean {
  return /[\u0000-\u001f\u007f]/u.test(value);
}

function utf8Bytes(value: string): number {
  return new TextEncoder().encode(value).byteLength;
}

function jsonBytes(value: unknown): number {
  try {
    return new TextEncoder().encode(JSON.stringify(value)).byteLength;
  } catch {
    throw new Error("mcp_message_invalid");
  }
}
