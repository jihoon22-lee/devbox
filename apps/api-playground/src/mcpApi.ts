import { invoke } from "@tauri-apps/api/core";
import { isTauri } from "./lib/isTauri";
import type { EnvVariable } from "./lib/environments";
import type {
  McpConnectResult,
  McpHttpProfile,
  McpInvokeResult,
  McpServerProjection,
  McpTimelineEntry,
} from "./types";

const NATIVE_REQUIRED = "native_required";
const MAX_TIMELINE_EVENTS = 1_000;
const MAX_TIMELINE_BYTES = 4 * 1024 * 1024;
const MAX_RESULT_BYTES = 4 * 1024 * 1024;
const CONNECTION_ID = /^[a-f0-9]{32}$/u;
const REQUEST_ID = /^[A-Za-z0-9_-]{1,128}$/u;
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

function requireNative(): void {
  if (!isTauri()) throw new Error(NATIVE_REQUIRED);
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
