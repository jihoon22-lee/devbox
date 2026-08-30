import { invoke } from "@tauri-apps/api/core";
import { isTauri } from "./lib/isTauri";

export type GrpcSelectionKind = "proto" | "import-root" | "ca" | "client-cert" | "client-key";
export type GrpcRootMode = "native" | "custom" | "native+custom";
export type GrpcRpcKind = "unary" | "server-streaming" | "client-streaming" | "bidirectional-streaming";
export type GrpcSourceKind = "local-proto" | "reflection-v1" | "reflection-v1alpha";
export type GrpcTlsMode = "plaintext" | GrpcRootMode;

export interface GrpcNativeSelection {
  selectionId: string;
  kind: GrpcSelectionKind;
  label: string;
  expiresAtMs: number;
}

export interface GrpcCredentialProjection {
  credentialId: string;
  label: string;
  hasCustomCa: boolean;
  hasClientIdentity: boolean;
  createdAtMs: number;
}

export interface GrpcMethodProjection {
  service: string;
  method: string;
  fullName: string;
  inputType: string;
  outputType: string;
  rpcKind: GrpcRpcKind;
  inputTemplate: unknown;
}

export interface GrpcConnectProfile {
  endpoint: string;
  source: {
    kind: "local-proto";
    protoSelectionId: string;
    importRootSelectionId?: string;
  } | {
    kind: "reflection";
  };
  tls: {
    rootMode: GrpcRootMode;
    serverName?: string;
    credentialId?: string;
  };
  connectTimeoutMs: number;
  rpcTimeoutMs: number;
}

export interface GrpcConnectResult {
  connectionId: string;
  authority: string;
  source: {
    kind: GrpcSourceKind;
    label: string | null;
    descriptorFileCount: number;
    serviceCount: number;
  };
  tls: {
    mode: GrpcTlsMode;
    encrypted: boolean;
    credentialUsed: boolean;
    serverNameOverridden: boolean;
  };
  methods: GrpcMethodProjection[];
  rpcTimeoutMs: number;
}

export interface GrpcInvokeResult {
  ok: boolean;
  status: GrpcStatusName;
  responses: unknown[];
  requestMessageCount: number;
  responseMessageCount: number;
  startedAtMs: number;
  elapsedMs: number;
}

export interface GrpcExchangeSummary {
  sourceKind: GrpcSourceKind;
  service: string;
  method: string;
  rpcKind: GrpcRpcKind;
  requestMessageCount: number;
  responseMessageCount: number;
  startedAtMs: number;
  elapsedMs: number;
  status: GrpcStatusName;
  tlsMode: GrpcTlsMode;
  credentialUsed: boolean;
}

export type GrpcStatusName = typeof GRPC_STATUS_NAMES[number];

const GRPC_STATUS_NAMES = [
  "OK", "CANCELLED", "UNKNOWN", "INVALID_ARGUMENT", "DEADLINE_EXCEEDED", "NOT_FOUND",
  "ALREADY_EXISTS", "PERMISSION_DENIED", "RESOURCE_EXHAUSTED", "FAILED_PRECONDITION",
  "ABORTED", "OUT_OF_RANGE", "UNIMPLEMENTED", "INTERNAL", "UNAVAILABLE", "DATA_LOSS",
  "UNAUTHENTICATED",
] as const;
const STATUS_NAMES = new Set<string>(GRPC_STATUS_NAMES);
const RPC_KINDS = new Set<string>([
  "unary", "server-streaming", "client-streaming", "bidirectional-streaming",
]);
const ROOT_MODES = new Set<string>(["native", "custom", "native+custom"]);
const SOURCE_KINDS = new Set<string>(["local-proto", "reflection-v1", "reflection-v1alpha"]);
const TLS_MODES = new Set<string>(["plaintext", "native", "custom", "native+custom"]);
const OPAQUE_ID = /^[a-f0-9]{32}$/u;
const REQUEST_ID = /^[A-Za-z0-9_.-]{1,128}$/u;
const SAFE_ERROR_CODES = new Set([
  "grpc_native_required",
  "grpc_invalid_profile",
  "grpc_source_selection_invalid",
  "grpc_source_invalid",
  "grpc_source_too_large",
  "grpc_descriptor_invalid",
  "grpc_reflection_unavailable",
  "grpc_connection_limit",
  "grpc_connect_timeout",
  "grpc_tls_failed",
  "grpc_credential_storage_unavailable",
  "grpc_credential_storage_failed",
  "grpc_credential_invalid",
  "grpc_connection_stale",
  "grpc_method_unavailable",
  "grpc_request_invalid",
  "grpc_request_too_large",
  "grpc_request_limit",
  "grpc_request_timeout",
  "grpc_request_cancelled",
  "grpc_response_too_large",
  "grpc_protocol_failed",
  "grpc_export_failed",
]);
const MAX_MESSAGE_BYTES = 1024 * 1024;
const MAX_REQUEST_BYTES = 4 * 1024 * 1024;
const MAX_RESPONSE_BYTES = 8 * 1024 * 1024;
const MAX_JSON_DEPTH = 64;
const MAX_JSON_NODES = 20_000;
const MAX_ECMASCRIPT_DATE_MS = 8_640_000_000_000_000;

let requestSequence = 0;

export function nextGrpcRequestId(): string {
  requestSequence = (requestSequence + 1) % Number.MAX_SAFE_INTEGER;
  const random = globalThis.crypto?.randomUUID?.().replace(/-/gu, "");
  return random
    ? `grpc-${random}`
    : `grpc-${Date.now().toString(36)}-${requestSequence.toString(36)}`;
}

export function safeGrpcErrorCode(cause: unknown): string {
  const message = typeof cause === "string"
    ? cause
    : cause instanceof Error ? cause.message : "";
  return SAFE_ERROR_CODES.has(message) ? message : "grpc_protocol_failed";
}

export async function pickGrpcProto(): Promise<GrpcNativeSelection | null> {
  return pickSelection("pick_grpc_proto", "proto");
}

export async function pickGrpcImportRoot(): Promise<GrpcNativeSelection | null> {
  return pickSelection("pick_grpc_import_root", "import-root");
}

export async function pickGrpcCa(): Promise<GrpcNativeSelection | null> {
  return pickSelection("pick_grpc_ca", "ca");
}

export async function pickGrpcClientCertificate(): Promise<GrpcNativeSelection | null> {
  return pickSelection("pick_grpc_client_certificate", "client-cert");
}

export async function pickGrpcClientKey(): Promise<GrpcNativeSelection | null> {
  return pickSelection("pick_grpc_client_key", "client-key");
}

async function pickSelection(
  command: string,
  expected: GrpcSelectionKind,
): Promise<GrpcNativeSelection | null> {
  requireNative();
  const value = await invoke<unknown>(command);
  if (value === null) return null;
  const record = asRecord(value, "grpc_source_selection_invalid");
  if (
    !isOpaqueId(record.selectionId)
    || record.kind !== expected
    || !isSafeLabel(record.label)
    || !boundedInteger(record.expiresAtMs, 1, MAX_ECMASCRIPT_DATE_MS)
  ) {
    throw new Error("grpc_source_selection_invalid");
  }
  return {
    selectionId: record.selectionId,
    kind: expected,
    label: record.label,
    expiresAtMs: record.expiresAtMs,
  };
}

export async function importGrpcTlsCredential(input: {
  label: string;
  caSelectionId?: string;
  clientCertificateSelectionId?: string;
  clientKeySelectionId?: string;
}): Promise<GrpcCredentialProjection> {
  requireNative();
  if (!isSafeText(input.label, 256) || input.label.trim() !== input.label) {
    throw new Error("grpc_credential_invalid");
  }
  const ids = [input.caSelectionId, input.clientCertificateSelectionId, input.clientKeySelectionId]
    .filter((value): value is string => value !== undefined);
  if (
    ids.length === 0
    || ids.some((value) => !OPAQUE_ID.test(value))
    || Boolean(input.clientCertificateSelectionId) !== Boolean(input.clientKeySelectionId)
  ) {
    throw new Error("grpc_credential_invalid");
  }
  const value = await invoke<unknown>("import_grpc_tls_credential", {
    label: input.label,
    ...(input.caSelectionId ? { caSelectionId: input.caSelectionId } : {}),
    ...(input.clientCertificateSelectionId
      ? { clientCertificateSelectionId: input.clientCertificateSelectionId }
      : {}),
    ...(input.clientKeySelectionId ? { clientKeySelectionId: input.clientKeySelectionId } : {}),
  });
  return validateCredential(value);
}

export async function listGrpcTlsCredentials(): Promise<GrpcCredentialProjection[]> {
  requireNative();
  const value = await invoke<unknown>("list_grpc_tls_credentials");
  if (!Array.isArray(value) || value.length > 16) {
    throw new Error("grpc_credential_storage_failed");
  }
  const credentials = value.map(validateCredential);
  if (
    new Set(credentials.map((credential) => credential.credentialId)).size !== credentials.length
    || new Set(credentials.map((credential) => credential.label)).size !== credentials.length
  ) {
    throw new Error("grpc_credential_storage_failed");
  }
  return credentials;
}

export async function deleteGrpcTlsCredential(credentialId: string): Promise<boolean> {
  requireNative();
  if (!OPAQUE_ID.test(credentialId)) throw new Error("grpc_credential_invalid");
  const value = await invoke<unknown>("delete_grpc_tls_credential", { credentialId });
  if (typeof value !== "boolean") throw new Error("grpc_credential_storage_failed");
  return value;
}

export async function connectGrpc(profile: GrpcConnectProfile): Promise<GrpcConnectResult> {
  requireNative();
  validateConnectProfile(profile);
  const value = await invoke<unknown>("connect_grpc", { profile });
  try {
    return validateConnectResult(value);
  } catch (cause) {
    const record = isRecord(value) ? value : null;
    if (record && isOpaqueId(record.connectionId)) {
      try {
        await invoke<void>("disconnect_grpc", { connectionId: record.connectionId });
      } catch {
        // Preserve the projection validation failure after best-effort cleanup.
      }
    }
    throw cause;
  }
}

export async function invokeGrpc(
  connectionId: string,
  requestId: string,
  method: string,
  messages: string[],
): Promise<GrpcInvokeResult> {
  requireNative();
  if (!OPAQUE_ID.test(connectionId) || !REQUEST_ID.test(requestId) || !isSafeName(method)) {
    throw new Error("grpc_request_invalid");
  }
  validateRawMessages(messages);
  const value = await invoke<unknown>("invoke_grpc", {
    connectionId,
    requestId,
    method,
    messages: [...messages],
  });
  return validateInvokeResult(value, messages.length);
}

export async function cancelGrpc(connectionId: string, requestId: string): Promise<boolean> {
  requireNative();
  if (!OPAQUE_ID.test(connectionId) || !REQUEST_ID.test(requestId)) {
    throw new Error("grpc_connection_stale");
  }
  const value = await invoke<unknown>("cancel_grpc", { connectionId, requestId });
  if (typeof value !== "boolean") throw new Error("grpc_protocol_failed");
  return value;
}

export async function disconnectGrpc(connectionId: string): Promise<void> {
  requireNative();
  if (!OPAQUE_ID.test(connectionId)) throw new Error("grpc_connection_stale");
  await invoke<void>("disconnect_grpc", { connectionId });
}

export async function exportGrpcSummary(summary: GrpcExchangeSummary): Promise<boolean> {
  requireNative();
  validateSummary(summary);
  const value = await invoke<unknown>("export_grpc_summary", { summary });
  if (typeof value !== "boolean") throw new Error("grpc_export_failed");
  return value;
}

function validateConnectProfile(profile: GrpcConnectProfile): void {
  if (
    !isSafeText(profile.endpoint, 8 * 1024)
    || !ROOT_MODES.has(profile.tls.rootMode)
    || !Number.isInteger(profile.connectTimeoutMs)
    || profile.connectTimeoutMs < 100
    || profile.connectTimeoutMs > 30_000
    || !Number.isInteger(profile.rpcTimeoutMs)
    || profile.rpcTimeoutMs < 100
    || profile.rpcTimeoutMs > 300_000
    || (profile.tls.serverName !== undefined && !isSafeText(profile.tls.serverName, 253))
    || (profile.tls.credentialId !== undefined && !OPAQUE_ID.test(profile.tls.credentialId))
  ) {
    throw new Error("grpc_invalid_profile");
  }
  if (profile.source.kind === "local-proto") {
    if (
      !OPAQUE_ID.test(profile.source.protoSelectionId)
      || (profile.source.importRootSelectionId !== undefined
        && !OPAQUE_ID.test(profile.source.importRootSelectionId))
    ) {
      throw new Error("grpc_source_selection_invalid");
    }
  } else if (profile.source.kind !== "reflection") {
    throw new Error("grpc_invalid_profile");
  }
}

function validateConnectResult(value: unknown): GrpcConnectResult {
  const record = asRecord(value, "grpc_protocol_failed");
  const source = asRecord(record.source, "grpc_protocol_failed");
  const tls = asRecord(record.tls, "grpc_protocol_failed");
  const sourceKind = String(source.kind);
  const tlsMode = String(tls.mode);
  if (
    !isOpaqueId(record.connectionId)
    || !isSafeName(record.authority)
    || !SOURCE_KINDS.has(sourceKind)
    || (source.label !== null && !isSafeLabel(source.label))
    || ((sourceKind === "local-proto") !== (source.label !== null))
    || !boundedInteger(source.descriptorFileCount, 1, 256)
    || !boundedInteger(source.serviceCount, 1, 256)
    || !TLS_MODES.has(tlsMode)
    || typeof tls.encrypted !== "boolean"
    || typeof tls.credentialUsed !== "boolean"
    || typeof tls.serverNameOverridden !== "boolean"
    || tls.encrypted !== (tlsMode !== "plaintext")
    || (tlsMode === "plaintext" && (tls.credentialUsed || tls.serverNameOverridden))
    || ((tlsMode === "custom" || tlsMode === "native+custom") && !tls.credentialUsed)
    || !boundedInteger(record.rpcTimeoutMs, 100, 300_000)
    || !Array.isArray(record.methods)
    || record.methods.length === 0
    || record.methods.length > 2_000
  ) {
    throw new Error("grpc_protocol_failed");
  }
  const methods = record.methods.map(validateMethod);
  if (new Set(methods.map((method) => method.fullName)).size !== methods.length) {
    throw new Error("grpc_protocol_failed");
  }
  return {
    connectionId: record.connectionId,
    authority: record.authority,
    source: {
      kind: source.kind as GrpcSourceKind,
      label: source.label,
      descriptorFileCount: source.descriptorFileCount,
      serviceCount: source.serviceCount,
    },
    tls: {
      mode: tls.mode as GrpcTlsMode,
      encrypted: tls.encrypted,
      credentialUsed: tls.credentialUsed,
      serverNameOverridden: tls.serverNameOverridden,
    },
    methods,
    rpcTimeoutMs: record.rpcTimeoutMs,
  };
}

function validateMethod(value: unknown): GrpcMethodProjection {
  const record = asRecord(value, "grpc_protocol_failed");
  if (
    !isSafeName(record.service)
    || !isSafeName(record.method)
    || !isSafeName(record.fullName)
    || record.fullName !== `${record.service}.${record.method}`
    || !isSafeName(record.inputType)
    || !isSafeName(record.outputType)
    || !RPC_KINDS.has(String(record.rpcKind))
  ) {
    throw new Error("grpc_protocol_failed");
  }
  validateJson(record.inputTemplate, 256 * 1024);
  return {
    service: record.service,
    method: record.method,
    fullName: record.fullName,
    inputType: record.inputType,
    outputType: record.outputType,
    rpcKind: record.rpcKind as GrpcRpcKind,
    inputTemplate: structuredClone(record.inputTemplate),
  };
}

function validateInvokeResult(value: unknown, expectedRequestCount: number): GrpcInvokeResult {
  const record = asRecord(value, "grpc_protocol_failed");
  if (
    typeof record.ok !== "boolean"
    || typeof record.status !== "string"
    || !STATUS_NAMES.has(record.status)
    || record.ok !== (record.status === "OK")
    || !Array.isArray(record.responses)
    || record.responses.length > 100
    || record.requestMessageCount !== expectedRequestCount
    || record.responseMessageCount !== record.responses.length
    || !boundedInteger(record.startedAtMs, 1, MAX_ECMASCRIPT_DATE_MS)
    || !boundedInteger(record.elapsedMs, 0, Number.MAX_SAFE_INTEGER)
  ) {
    throw new Error("grpc_protocol_failed");
  }
  let total = 0;
  const responses = record.responses.map((response) => {
    total += validateJson(response, MAX_MESSAGE_BYTES);
    if (total > MAX_RESPONSE_BYTES) throw new Error("grpc_response_too_large");
    return structuredClone(response);
  });
  return {
    ok: record.ok,
    status: record.status as GrpcStatusName,
    responses,
    requestMessageCount: record.requestMessageCount,
    responseMessageCount: record.responseMessageCount,
    startedAtMs: record.startedAtMs,
    elapsedMs: record.elapsedMs,
  };
}

function validateCredential(value: unknown): GrpcCredentialProjection {
  const record = asRecord(value, "grpc_credential_storage_failed");
  if (
    !isOpaqueId(record.credentialId)
    || !isSafeText(record.label, 256)
    || record.label.trim() !== record.label
    || typeof record.hasCustomCa !== "boolean"
    || typeof record.hasClientIdentity !== "boolean"
    || (!record.hasCustomCa && !record.hasClientIdentity)
    || !boundedInteger(record.createdAtMs, 1, MAX_ECMASCRIPT_DATE_MS)
  ) {
    throw new Error("grpc_credential_storage_failed");
  }
  return {
    credentialId: record.credentialId,
    label: record.label,
    hasCustomCa: record.hasCustomCa,
    hasClientIdentity: record.hasClientIdentity,
    createdAtMs: record.createdAtMs,
  };
}

function validateRawMessages(messages: string[]): void {
  if (!Array.isArray(messages) || messages.length === 0 || messages.length > 100) {
    throw new Error("grpc_request_invalid");
  }
  let total = 0;
  for (const message of messages) {
    if (typeof message !== "string") throw new Error("grpc_request_invalid");
    const bytes = utf8Bytes(message);
    total += bytes;
    if (bytes === 0 || bytes > MAX_MESSAGE_BYTES || total > MAX_REQUEST_BYTES) {
      throw new Error("grpc_request_too_large");
    }
    try {
      validateJson(JSON.parse(message), MAX_MESSAGE_BYTES);
    } catch (cause) {
      if (cause instanceof Error && cause.message === "grpc_response_too_large") {
        throw new Error("grpc_request_too_large");
      }
      throw new Error("grpc_request_invalid");
    }
  }
}

function validateSummary(summary: GrpcExchangeSummary): void {
  const requestMultiple = summary.rpcKind === "client-streaming"
    || summary.rpcKind === "bidirectional-streaming";
  const responseMultiple = summary.rpcKind === "server-streaming"
    || summary.rpcKind === "bidirectional-streaming";
  if (
    !SOURCE_KINDS.has(summary.sourceKind)
    || !isSafeName(summary.service)
    || !isSafeName(summary.method)
    || !RPC_KINDS.has(summary.rpcKind)
    || !boundedInteger(summary.requestMessageCount, 1, requestMultiple ? 100 : 1)
    || !boundedInteger(summary.responseMessageCount, 0, responseMultiple ? 100 : 1)
    || (summary.status === "OK" && !responseMultiple && summary.responseMessageCount !== 1)
    || !boundedInteger(summary.startedAtMs, 1, MAX_ECMASCRIPT_DATE_MS)
    || !boundedInteger(summary.elapsedMs, 0, Number.MAX_SAFE_INTEGER)
    || !STATUS_NAMES.has(summary.status)
    || !TLS_MODES.has(summary.tlsMode)
    || typeof summary.credentialUsed !== "boolean"
    || (summary.tlsMode === "plaintext" && summary.credentialUsed)
  ) {
    throw new Error("grpc_export_failed");
  }
}

function validateJson(value: unknown, byteLimit: number): number {
  const serialized = JSON.stringify(value);
  if (serialized === undefined) throw new Error("grpc_protocol_failed");
  const bytes = utf8Bytes(serialized);
  if (bytes > byteLimit) throw new Error("grpc_response_too_large");
  let nodes = 0;
  const visit = (current: unknown, depth: number): void => {
    nodes += 1;
    if (nodes > MAX_JSON_NODES || depth > MAX_JSON_DEPTH) {
      throw new Error("grpc_response_too_large");
    }
    if (Array.isArray(current)) {
      for (const child of current) visit(child, depth + 1);
    } else if (isRecord(current)) {
      for (const [key, child] of Object.entries(current)) {
        if (utf8Bytes(key) > 4 * 1024 || hasControl(key)) {
          throw new Error("grpc_protocol_failed");
        }
        visit(child, depth + 1);
      }
    } else if (
      current !== null
      && typeof current !== "string"
      && typeof current !== "number"
      && typeof current !== "boolean"
    ) {
      throw new Error("grpc_protocol_failed");
    }
    if (typeof current === "string" && utf8Bytes(current) > 256 * 1024) {
      throw new Error("grpc_response_too_large");
    }
  };
  visit(value, 0);
  return bytes;
}

function requireNative(): void {
  if (!isTauri()) throw new Error("grpc_native_required");
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function asRecord(value: unknown, error: string): Record<string, unknown> {
  if (!isRecord(value)) throw new Error(error);
  return value;
}

function isOpaqueId(value: unknown): value is string {
  return typeof value === "string" && OPAQUE_ID.test(value);
}

function isSafeLabel(value: unknown): value is string {
  return isSafeText(value, 256)
    && value !== "."
    && value !== ".."
    && !value.includes("/")
    && !value.includes("\\");
}

function isSafeName(value: unknown): value is string {
  return isSafeText(value, 1024) && !/\s/u.test(value);
}

function isSafeText(value: unknown, maxBytes: number): value is string {
  return typeof value === "string"
    && value.length > 0
    && utf8Bytes(value) <= maxBytes
    && !hasControl(value);
}

function hasControl(value: string): boolean {
  return /[\u0000-\u001f\u007f]/u.test(value);
}

function isSafeInteger(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

function boundedInteger(value: unknown, minimum: number, maximum: number): value is number {
  return isSafeInteger(value) && value >= minimum && value <= maximum;
}

function utf8Bytes(value: string): number {
  return new TextEncoder().encode(value).byteLength;
}
