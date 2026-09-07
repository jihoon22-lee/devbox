import type {
  GrpcExchangeSummary,
  GrpcRpcKind,
  GrpcSourceKind,
  GrpcStatusName,
  GrpcTlsMode,
} from "../grpcApi";
import { parseTree, type ParseError } from "jsonc-parser";

export const GRPC_HISTORY_KEY = "devbox.api-playground.grpc-history/v1";
export const GRPC_HISTORY_SCHEMA = "devbox.api-playground.grpc-history/v1";
export const MAX_GRPC_HISTORY = 50;
const MAX_STORE_BYTES = 256 * 1024;
const MAX_NAME_BYTES = 1024;
const MAX_MESSAGE_BYTES = 1024 * 1024;
const MAX_REQUEST_BYTES = 4 * 1024 * 1024;
const MAX_ECMASCRIPT_DATE_MS = 8_640_000_000_000_000;
const STATUS_NAMES = new Set([
  "OK", "CANCELLED", "UNKNOWN", "INVALID_ARGUMENT", "DEADLINE_EXCEEDED", "NOT_FOUND",
  "ALREADY_EXISTS", "PERMISSION_DENIED", "RESOURCE_EXHAUSTED", "FAILED_PRECONDITION",
  "ABORTED", "OUT_OF_RANGE", "UNIMPLEMENTED", "INTERNAL", "UNAVAILABLE", "DATA_LOSS",
  "UNAUTHENTICATED",
]);
const RPC_KINDS = new Set([
  "unary", "server-streaming", "client-streaming", "bidirectional-streaming",
]);
const SOURCE_KINDS = new Set(["local-proto", "reflection-v1", "reflection-v1alpha"]);
const TLS_MODES = new Set(["plaintext", "native", "custom", "native+custom"]);
const ROOT_KEYS = new Set(["schema", "entries"]);
const ENTRY_KEYS = new Set([
  "sourceKind",
  "service",
  "method",
  "rpcKind",
  "requestMessageCount",
  "responseMessageCount",
  "startedAtMs",
  "elapsedMs",
  "status",
  "tlsMode",
  "credentialUsed",
]);

export interface GrpcHistoryStore {
  schema: typeof GRPC_HISTORY_SCHEMA;
  entries: GrpcExchangeSummary[];
}

export function emptyGrpcHistory(): GrpcHistoryStore {
  return { schema: GRPC_HISTORY_SCHEMA, entries: [] };
}

export function parseGrpcHistory(raw: string | null): GrpcHistoryStore | null {
  if (raw === null || utf8Bytes(raw) > MAX_STORE_BYTES) return null;
  try {
    const parsed: unknown = JSON.parse(raw);
    if (!isRecord(parsed) || !exactKeys(parsed, ROOT_KEYS) || parsed.schema !== GRPC_HISTORY_SCHEMA) {
      return null;
    }
    if (!Array.isArray(parsed.entries) || parsed.entries.length > MAX_GRPC_HISTORY) return null;
    const entries = parsed.entries.map(projectSummary);
    return { schema: GRPC_HISTORY_SCHEMA, entries };
  } catch {
    return null;
  }
}

export function loadGrpcHistory(storage: Storage = localStorage): GrpcHistoryStore {
  return parseGrpcHistory(storage.getItem(GRPC_HISTORY_KEY)) ?? emptyGrpcHistory();
}

export function appendGrpcHistory(
  store: GrpcHistoryStore,
  summary: GrpcExchangeSummary,
): GrpcHistoryStore {
  const safe = projectSummary(summary);
  return {
    schema: GRPC_HISTORY_SCHEMA,
    entries: [safe, ...store.entries.map(projectSummary)].slice(0, MAX_GRPC_HISTORY),
  };
}

export function saveGrpcHistory(
  store: GrpcHistoryStore,
  storage: Storage = localStorage,
): GrpcHistoryStore {
  const safe: GrpcHistoryStore = {
    schema: GRPC_HISTORY_SCHEMA,
    entries: store.entries.slice(0, MAX_GRPC_HISTORY).map(projectSummary),
  };
  const serialized = JSON.stringify(safe);
  if (utf8Bytes(serialized) > MAX_STORE_BYTES) throw new Error("grpc_history_failed");
  const previous = storage.getItem(GRPC_HISTORY_KEY);
  try {
    storage.setItem(GRPC_HISTORY_KEY, serialized);
    const readBack = parseGrpcHistory(storage.getItem(GRPC_HISTORY_KEY));
    if (!readBack) throw new Error("grpc_history_failed");
    return readBack;
  } catch (cause) {
    try {
      if (previous === null) storage.removeItem(GRPC_HISTORY_KEY);
      else storage.setItem(GRPC_HISTORY_KEY, previous);
    } catch {
      // Preserve the original storage failure.
    }
    throw cause instanceof Error ? cause : new Error("grpc_history_failed");
  }
}

export function clearGrpcHistory(storage: Storage = localStorage): GrpcHistoryStore {
  return saveGrpcHistory(emptyGrpcHistory(), storage);
}

/**
 * Validate the editor document while preserving each message's original JSON
 * slice. The backend therefore still sees duplicate object keys and can reject
 * them instead of receiving a lossy JSON.parse/JSON.stringify round trip.
 */
export function splitGrpcRequestMessages(raw: string, rpcKind: GrpcRpcKind): string[] {
  const totalBytes = utf8Bytes(raw);
  if (totalBytes === 0 || totalBytes > MAX_REQUEST_BYTES) {
    throw new Error("grpc_request_too_large");
  }
  const errors: ParseError[] = [];
  const tree = parseTree(raw, errors, { allowTrailingComma: false, disallowComments: true });
  if (!tree || errors.length > 0) throw new Error("grpc_request_invalid");
  const streaming = rpcKind === "client-streaming" || rpcKind === "bidirectional-streaming";
  const messages = streaming
    ? tree.type === "array" && tree.children
      ? tree.children.map((child) => raw.slice(child.offset, child.offset + child.length))
      : []
    : [raw];
  if (messages.length === 0 || messages.length > (streaming ? 100 : 1)) {
    throw new Error("grpc_request_invalid");
  }
  let retainedBytes = 0;
  for (const message of messages) {
    const bytes = utf8Bytes(message);
    retainedBytes += bytes;
    if (bytes === 0 || bytes > MAX_MESSAGE_BYTES || retainedBytes > MAX_REQUEST_BYTES) {
      throw new Error("grpc_request_too_large");
    }
  }
  return messages;
}

function projectSummary(value: unknown): GrpcExchangeSummary {
  if (!isRecord(value) || !exactKeys(value, ENTRY_KEYS)) throw new Error("grpc_history_invalid");
  const requestMultiple = value.rpcKind === "client-streaming"
    || value.rpcKind === "bidirectional-streaming";
  const responseMultiple = value.rpcKind === "server-streaming"
    || value.rpcKind === "bidirectional-streaming";
  if (
    typeof value.sourceKind !== "string"
    || !SOURCE_KINDS.has(value.sourceKind)
    || !safeName(value.service)
    || !safeName(value.method)
    || typeof value.rpcKind !== "string"
    || !RPC_KINDS.has(value.rpcKind)
    || !boundedInteger(value.requestMessageCount, 1, requestMultiple ? 100 : 1)
    || !boundedInteger(value.responseMessageCount, 0, responseMultiple ? 100 : 1)
    || (value.status === "OK" && !responseMultiple && value.responseMessageCount !== 1)
    || !boundedInteger(value.startedAtMs, 1, MAX_ECMASCRIPT_DATE_MS)
    || !boundedInteger(value.elapsedMs, 0, Number.MAX_SAFE_INTEGER)
    || typeof value.status !== "string"
    || !STATUS_NAMES.has(value.status)
    || typeof value.tlsMode !== "string"
    || !TLS_MODES.has(value.tlsMode)
    || typeof value.credentialUsed !== "boolean"
    || (value.tlsMode === "plaintext" && value.credentialUsed)
  ) {
    throw new Error("grpc_history_invalid");
  }
  return {
    sourceKind: value.sourceKind as GrpcSourceKind,
    service: value.service,
    method: value.method,
    rpcKind: value.rpcKind as GrpcRpcKind,
    requestMessageCount: value.requestMessageCount,
    responseMessageCount: value.responseMessageCount,
    startedAtMs: value.startedAtMs,
    elapsedMs: value.elapsedMs,
    status: value.status as GrpcStatusName,
    tlsMode: value.tlsMode as GrpcTlsMode,
    credentialUsed: value.credentialUsed,
  };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function exactKeys(value: Record<string, unknown>, expected: ReadonlySet<string>): boolean {
  const keys = Object.keys(value);
  return keys.length === expected.size && keys.every((key) => expected.has(key));
}

function safeName(value: unknown): value is string {
  return typeof value === "string"
    && value.length > 0
    && utf8Bytes(value) <= MAX_NAME_BYTES
    && !/[\u0000-\u001f\u007f\s]/u.test(value);
}

function boundedInteger(value: unknown, minimum: number, maximum: number): value is number {
  return typeof value === "number"
    && Number.isSafeInteger(value)
    && value >= minimum
    && value <= maximum;
}

function utf8Bytes(value: string): number {
  return new TextEncoder().encode(value).byteLength;
}
