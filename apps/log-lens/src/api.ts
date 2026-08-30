import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { browserSnapshot } from "./browserFixture";
import { filterRecords as applyFilter, utf8ByteLength } from "./filter";
import { isTauri } from "./lib/isTauri";
import type {
  ExportedText,
  FileCursor,
  FilterSpec,
  LogRecord,
  SourceSpec,
  SourceSummary,
  SourcesSnapshot,
  LogSourcePreview,
  SavedView,
  SavedViewsDocument,
  ToolboxDispatch,
  WebhookLogPayload,
} from "./types";

export type { ToolboxDispatch } from "./types";

const MAX_RECORDS = 100_000;
const MAX_EXPORT_BYTES = 8 * 1024 * 1024;
export const TOOLBOX_TEXT_BROWSER_ERROR =
  "Developer Toolbox handoff is desktop-only; clipboard fallback is disabled.";
export const TOOLBOX_TEXT_INVALID_ERROR = "Developer Toolbox handoff response was invalid.";

const HANDOFF_FAILURE_CODES = [
  "handoff-invalid",
  "handoff-response-invalid",
  "handoff-missing",
  "handoff-expired",
  "handoff-lease-expired",
  "handoff-busy",
  "handoff-storage-failed",
  "handoff-claim-storage-failed",
  "handoff-restore-failed",
  "handoff-not-open",
] as const;

export type HandoffFailureCode = typeof HANDOFF_FAILURE_CODES[number];
export type HandoffFailureClass = "terminal" | "retryable";

/**
 * Safe, fixed-code errors crossing the native handoff boundary.  Native
 * storage messages can contain paths or other sensitive details, so callers
 * must use `code` rather than an arbitrary Error string.
 */
export class HandoffApiError extends Error {
  readonly code: HandoffFailureCode;

  constructor(code: HandoffFailureCode) {
    super(code);
    this.name = "HandoffApiError";
    this.code = code;
  }
}

function isHandoffFailureCode(value: unknown): value is HandoffFailureCode {
  return typeof value === "string"
    && (HANDOFF_FAILURE_CODES as readonly string[]).includes(value);
}

/** Extract only an exact allow-listed native code; never return raw details. */
export function handoffErrorCode(error: unknown): HandoffFailureCode | null {
  if (error instanceof HandoffApiError) return error.code;
  if (isRecord(error) && isHandoffFailureCode(error.code)) return error.code;
  const value = typeof error === "string"
    ? error
    : error instanceof Error
      ? error.message
      : null;
  return isHandoffFailureCode(value) ? value : null;
}

/**
 * Claim expiration/mismatch is terminal for this UI instance.  Storage and
 * restore failures are retryable and must keep the exact id/claim in place.
 */
export function classifyHandoffError(error: unknown): HandoffFailureClass {
  const code = handoffErrorCode(error);
  return code === "handoff-storage-failed"
    || code === "handoff-claim-storage-failed"
    || code === "handoff-restore-failed"
    || code === "handoff-response-invalid"
    ? "retryable"
    : "terminal";
}

function sanitizedHandoffError(error: unknown, fallback: HandoffFailureCode): HandoffApiError {
  return new HandoffApiError(handoffErrorCode(error) ?? fallback);
}

export interface OpenRequest {
  target: { kind: "handoff"; handoffKind: string; id: string };
  from: string | null;
}

const HANDOFF_ID_PATTERN = /^[0-9a-f]{32}$/;
const SOURCE_ID_PATTERN = /^log-source:[0-9a-f]{16}$/;
const RUN_SOURCE_PATTERN = /^run-manager:[A-Za-z0-9_-]{1,128}:(stdout|stderr)$/;
const UNIT_PATTERN = /^[A-Za-z0-9_.:@-]{1,128}$/;
const WSL_INJECTION_PATTERN = /[;&|<>`$"'\\(){}*?\[\]!~#%]/;
const HANDOFF_KINDS = ["log-source/v1", "webhook-log/v1"] as const;
const HTTP_TOKEN_PATTERN = /^[A-Za-z0-9!#$%&'*+.^_`|~-]+$/;
const MAX_SAVED_VIEWS = 20;
const REDACTION_MARKERS = ["[REDACTED]", "•••••"] as const;
const SENSITIVE_HEADER_PARTS = [
  "authorization",
  "proxyauthorization",
  "cookie",
  "setcookie",
  "apikey",
  "accesstoken",
  "refreshtoken",
  "token",
  "secret",
  "password",
  "passwd",
  "credential",
  "privatekey",
  "auth",
] as const;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function hasOnlyKeys(value: Record<string, unknown>, keys: readonly string[]): boolean {
  return Object.keys(value).every((key) => keys.includes(key));
}

function isSafeText(value: unknown, maxLength: number, allowEmpty = false): value is string {
  return typeof value === "string"
    && (allowEmpty || value.length > 0)
    && value.length <= maxLength
    && utf8ByteLength(value) <= maxLength
    && !/[\u0000-\u001f\u007f-\u009f]/.test(value);
}

function isSafeWebhookTarget(value: string): boolean {
  if (value.trim() !== value || value.includes("\\")) return false;
  const queryIndex = value.indexOf("?");
  const pathname = queryIndex === -1 ? value : value.slice(0, queryIndex);
  const query = queryIndex === -1 ? null : value.slice(queryIndex + 1);
  if (!pathname.startsWith("/")
    || pathname.startsWith("//")
    || pathname.includes("#")
    || (query !== null && (query.includes("#") || /[\u0000-\u001f\u007f-\u009f]/.test(query)))) return false;
  let decoded: string;
  try {
    decoded = decodeURIComponent(pathname);
  } catch {
    return false;
  }
  return !decoded.startsWith("//")
    && !decoded.includes("\\")
    && !/[\u0000-\u001f\u007f-\u009f]/.test(decoded)
    && !decoded.split("/").some((component) => component === "." || component === "..");
}

function isSensitiveWebhookHeader(name: string): boolean {
  const compact = name.replace(/[^A-Za-z0-9]/g, "").toLowerCase();
  return SENSITIVE_HEADER_PARTS.some((part) => compact.includes(part));
}

function parseOpenRequest(value: unknown): OpenRequest | null {
  if (!isRecord(value) || !hasOnlyKeys(value, ["target", "from"])) return null;
  const target = value.target;
  if (!isRecord(target)
    || !hasOnlyKeys(target, ["kind", "handoffKind", "id"])
    || target.kind !== "handoff"
    || typeof target.handoffKind !== "string"
    || !(HANDOFF_KINDS as readonly string[]).includes(target.handoffKind)
    || typeof target.id !== "string"
    || !HANDOFF_ID_PATTERN.test(target.id)) return null;
  const from = value.from;
  if (from !== null && !isSafeText(from, 64)) return null;
  return {
    target: {
      kind: "handoff",
      handoffKind: target.handoffKind as OpenRequest["target"]["handoffKind"],
      id: target.id,
    },
    from: from as string | null,
  };
}

function parseSourceSummary(value: unknown): SourceSummary | null {
  if (!isRecord(value)
    || !hasOnlyKeys(value, ["sourceId", "kind", "displayName", "readOnly", "handoff"])
    || typeof value.sourceId !== "string"
    || !SOURCE_ID_PATTERN.test(value.sourceId)
    || typeof value.kind !== "string"
    || !["wslFile", "wslJournal", "run", "webhookCapture"].includes(value.kind)
    || typeof value.displayName !== "string"
    || typeof value.readOnly !== "boolean"
    || typeof value.handoff !== "boolean"
    || value.readOnly !== true
    || value.handoff !== true) return null;
  const expectedNames: Record<string, string> = {
    run: "Run Manager handoff",
    wslFile: "WSL file",
    wslJournal: "WSL journal",
    webhookCapture: "Webhook capture",
  };
  if (value.displayName !== expectedNames[value.kind]) return null;
  return {
    sourceId: value.sourceId,
    kind: value.kind as SourceSummary["kind"],
    displayName: value.displayName,
    readOnly: true,
    handoff: true,
  };
}

function isLogSourceApp(value: unknown): value is LogSourcePreview["sourceApp"] {
  return value === "run-manager" || value === "port-manager" || value === "wsl-desktop" || value === "webhook-lab";
}

function parseLogSourcePreview(value: unknown): LogSourcePreview | null {
  if (!isRecord(value)
    || !hasOnlyKeys(value, ["id", "kind", "sourceApp", "expiresAtMs", "leaseUntilMs", "source"])
    || typeof value.id !== "string"
    || !HANDOFF_ID_PATTERN.test(value.id)
    || typeof value.kind !== "string"
    || !(HANDOFF_KINDS as readonly string[]).includes(value.kind)
    || !isLogSourceApp(value.sourceApp)
    || !Number.isSafeInteger(value.expiresAtMs)
    || !Number.isSafeInteger(value.leaseUntilMs)) return null;
  const expiresAtMs = value.expiresAtMs as number;
  const leaseUntilMs = value.leaseUntilMs as number;
  if (expiresAtMs <= 0 || leaseUntilMs <= 0 || leaseUntilMs > expiresAtMs) return null;
  const source = parseSourceSummary(value.source);
  if (!source) return null;
  if ((value.sourceApp === "run-manager" || value.sourceApp === "port-manager") && source.kind !== "run") return null;
  if (value.sourceApp === "wsl-desktop" && !["wslFile", "wslJournal"].includes(source.kind)) return null;
  if (value.sourceApp === "webhook-lab" && (value.kind !== "webhook-log/v1" || source.kind !== "webhookCapture")) return null;
  if (value.sourceApp !== "webhook-lab" && value.kind !== "log-source/v1") return null;
  return {
    id: value.id,
    kind: value.kind as LogSourcePreview["kind"],
    sourceApp: value.sourceApp,
    expiresAtMs,
    leaseUntilMs,
    source,
  };
}

function parseHandoffSource(value: unknown): SourceSpec | null {
  if (!isRecord(value) || typeof value.kind !== "string") return null;
  if (value.kind === "run") {
    if (!hasOnlyKeys(value, ["kind", "sourceId"])
      || typeof value.sourceId !== "string"
      || !RUN_SOURCE_PATTERN.test(value.sourceId)) return null;
    return { kind: "run", sourceId: value.sourceId };
  }
  if (value.kind === "wslFile") {
    if (!hasOnlyKeys(value, ["kind", "distro", "path"])
      || !isSafeText(value.distro, 128)
      || value.distro.trim() !== value.distro
      || value.distro.startsWith("-")
      || WSL_INJECTION_PATTERN.test(value.distro)
      || !isSafeText(value.path, 4_096)
      || value.path.trim() !== value.path
      || !value.path.startsWith("/")
      || value.path === "/"
      || value.path.split("/").slice(1).some((part) => !part || part === "." || part === "..")
      || WSL_INJECTION_PATTERN.test(value.path)) return null;
    return { kind: "wslFile", distro: value.distro, path: value.path };
  }
  if (value.kind === "wslJournal") {
    const unit = value.unit;
    if (!hasOnlyKeys(value, ["kind", "distro", "unit"])
      || !isSafeText(value.distro, 128)
      || value.distro.trim() !== value.distro
      || value.distro.startsWith("-")
      || WSL_INJECTION_PATTERN.test(value.distro)
      || (unit !== undefined
        && unit !== null
        && (typeof unit !== "string" || !UNIT_PATTERN.test(unit)))) return null;
    return {
      kind: "wslJournal",
      distro: value.distro,
      ...(typeof unit === "string" ? { unit } : {}),
    };
  }
  if (value.kind === "webhookCapture") {
    if (!hasOnlyKeys(value, ["kind", "capture"])) return null;
    const capture = parseWebhookLogPayload(value.capture);
    return capture ? { kind: "webhookCapture", capture } : null;
  }
  return null;
}

function parseWebhookLogPayload(value: unknown): WebhookLogPayload | null {
  if (!isRecord(value)
    || !hasOnlyKeys(value, ["schemaVersion", "method", "target", "receivedAtMs", "headerNames", "bodyPreview", "redacted", "truncated"])
    || value.schemaVersion !== 1
    || typeof value.method !== "string"
    || value.method.length > 16
    || value.method !== value.method.toUpperCase()
    || !HTTP_TOKEN_PATTERN.test(value.method)
    || !isSafeText(value.target, 4_096)
    || !isSafeWebhookTarget(value.target)
    || !Number.isSafeInteger(value.receivedAtMs)
    || Math.abs(value.receivedAtMs as number) > 8_640_000_000_000_000
    || !Array.isArray(value.headerNames)
    || value.headerNames.length > 64
    || typeof value.bodyPreview !== "string"
    || utf8ByteLength(value.bodyPreview) > 4 * 1024
    || /[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f-\u009f]/.test(value.bodyPreview)
    || typeof value.redacted !== "boolean"
    || typeof value.truncated !== "boolean") return null;
  const target = value.target as string;
  const bodyPreview = value.bodyPreview as string;
  const names: string[] = [];
  const seen = new Set<string>();
  let totalBytes = 0;
  let requiresRedacted = REDACTION_MARKERS.some((marker) => target.includes(marker) || bodyPreview.includes(marker));
  for (const name of value.headerNames) {
    if (typeof name !== "string" || utf8ByteLength(name) > 256 || !HTTP_TOKEN_PATTERN.test(name)) return null;
    const normalized = name.toLowerCase();
    if (seen.has(normalized)) return null;
    seen.add(normalized);
    totalBytes += utf8ByteLength(name);
    if (totalBytes > 4 * 1024) return null;
    requiresRedacted ||= isSensitiveWebhookHeader(name);
    names.push(name);
  }
  if (requiresRedacted && !value.redacted) return null;
  return {
    schemaVersion: 1,
    method: value.method,
    target,
    receivedAtMs: value.receivedAtMs as number,
    headerNames: names,
    bodyPreview,
    redacted: value.redacted,
    truncated: value.truncated,
  };
}

export async function takePendingOpen(): Promise<OpenRequest | null> {
  if (!isTauri()) return null;
  const request = parseOpenRequest(await invoke<unknown>("take_pending_open"));
  return request;
}

export async function onOpenRequest(handler: () => void): Promise<() => void> {
  if (!isTauri()) return () => undefined;
  return listen<OpenRequest>("devbox://open", () => handler());
}

export async function previewLogSource(handoffKind: OpenRequest["target"]["handoffKind"], id: string): Promise<LogSourcePreview> {
  if (!isTauri()) throw new Error("Log Lens source handoff is desktop-only");
  if (!(HANDOFF_KINDS as readonly string[]).includes(handoffKind) || !HANDOFF_ID_PATTERN.test(id)) throw new HandoffApiError("handoff-invalid");
  let response: unknown;
  try {
    response = await invoke<unknown>("preview_log_source", { handoffKind, id });
  } catch (error) {
    throw sanitizedHandoffError(error, "handoff-claim-storage-failed");
  }
  const preview = parseLogSourcePreview(response);
  if (!preview || preview.id !== id || preview.kind !== handoffKind) throw new HandoffApiError("handoff-response-invalid");
  return preview;
}

function parsePersistedSource(value: unknown): SourceSpec | null {
  if (!isRecord(value) || typeof value.kind !== "string") return null;
  if (value.kind === "localFile") {
    return hasOnlyKeys(value, ["kind", "path"]) && isSafeText(value.path, 4_096)
      ? { kind: "localFile", path: value.path }
      : null;
  }
  if (value.kind === "directory") {
    return hasOnlyKeys(value, ["kind", "path", "pattern"])
      && isSafeText(value.path, 4_096)
      && isSafeText(value.pattern, 128)
      ? { kind: "directory", path: value.path, pattern: value.pattern }
      : null;
  }
  if (value.kind === "wslJournal") {
    const unit = value.unit;
    if (!hasOnlyKeys(value, ["kind", "distro", "unit"])
      || !isSafeText(value.distro, 128)
      || (unit !== undefined && unit !== null && (typeof unit !== "string" || !UNIT_PATTERN.test(unit)))) return null;
    return { kind: "wslJournal", distro: value.distro, ...(typeof unit === "string" ? { unit } : {}) };
  }
  if (value.kind === "run") {
    return hasOnlyKeys(value, ["kind", "sourceId"])
      && typeof value.sourceId === "string"
      && RUN_SOURCE_PATTERN.test(value.sourceId)
      ? { kind: "run", sourceId: value.sourceId }
      : null;
  }
  if (value.kind === "container") {
    return hasOnlyKeys(value, ["kind", "engine", "containerId"])
      && (value.engine === "docker" || value.engine === "podman")
      && isSafeText(value.containerId, 128)
      ? { kind: "container", engine: value.engine, containerId: value.containerId }
      : null;
  }
  return null;
}

function parseFilterSpec(value: unknown): FilterSpec | null {
  if (!isRecord(value)
    || !hasOnlyKeys(value, ["text", "regex", "sourceId", "level", "startAt", "endAt", "field", "fieldValue"])
    || typeof value.text !== "string"
    || utf8ByteLength(value.text) > 512
    || typeof value.regex !== "boolean") return null;
  const optionalText = [value.sourceId, value.field, value.fieldValue];
  if (optionalText.some((item) => item !== undefined && item !== null
    && (typeof item !== "string" || utf8ByteLength(item) > 4 * 1024))) return null;
  if (value.level !== undefined && value.level !== null
    && !["trace", "debug", "info", "warn", "error", "fatal"].includes(value.level as string)) return null;
  if ([value.startAt, value.endAt].some((item) => item !== undefined && item !== null && !Number.isSafeInteger(item))) return null;
  return {
    text: value.text,
    regex: value.regex,
    ...(typeof value.sourceId === "string" ? { sourceId: value.sourceId } : {}),
    ...(typeof value.level === "string" ? { level: value.level as FilterSpec["level"] } : {}),
    ...(typeof value.startAt === "number" ? { startAt: value.startAt } : {}),
    ...(typeof value.endAt === "number" ? { endAt: value.endAt } : {}),
    ...(typeof value.field === "string" ? { field: value.field } : {}),
    ...(typeof value.fieldValue === "string" ? { fieldValue: value.fieldValue } : {}),
  };
}

function parseSavedView(value: unknown): SavedView | null {
  if (!isRecord(value)
    || !hasOnlyKeys(value, ["name", "sources", "filter"])
    || !isSafeText(value.name, 128)
    || !Array.isArray(value.sources)
    || value.sources.length === 0
    || value.sources.length > 16) return null;
  const sources = value.sources.map(parsePersistedSource);
  const filter = parseFilterSpec(value.filter);
  if (sources.some((source) => source === null) || !filter) return null;
  return { name: value.name, sources: sources as SourceSpec[], filter };
}

function parseSavedViewsDocument(value: unknown): SavedViewsDocument | null {
  if (!isRecord(value)
    || !hasOnlyKeys(value, ["schemaVersion", "revision", "views"])
    || value.schemaVersion !== 1
    || !Number.isSafeInteger(value.revision)
    || (value.revision as number) < 0
    || !Array.isArray(value.views)
    || value.views.length > MAX_SAVED_VIEWS) return null;
  const views = value.views.map(parseSavedView);
  if (views.some((view) => view === null)) return null;
  const names = new Set((views as SavedView[]).map((view) => view.name));
  if (names.size !== views.length) return null;
  return { schemaVersion: 1, revision: value.revision as number, views: views as SavedView[] };
}

let browserSavedViews: SavedViewsDocument = { schemaVersion: 1, revision: 0, views: [] };

export async function listSavedViews(): Promise<SavedViewsDocument> {
  if (!isTauri()) return structuredClone(browserSavedViews);
  const document = parseSavedViewsDocument(await invoke<unknown>("list_saved_views"));
  if (!document) throw new Error("저장된 뷰 응답이 유효하지 않습니다");
  return document;
}

export async function saveSavedView(expectedRevision: number, view: SavedView): Promise<SavedViewsDocument> {
  if (!isTauri()) {
    if (browserSavedViews.revision !== expectedRevision) throw new Error("저장된 뷰가 다른 작업에서 변경되었습니다. 다시 불러온 뒤 시도해 주세요");
    const nextViews = [...browserSavedViews.views.filter((item) => item.name !== view.name), structuredClone(view)];
    if (nextViews.length > MAX_SAVED_VIEWS) throw new Error("저장된 뷰가 최대 개수에 도달했습니다. 기존 뷰를 삭제한 뒤 다시 시도해 주세요");
    browserSavedViews = { schemaVersion: 1, revision: expectedRevision + 1, views: nextViews };
    return structuredClone(browserSavedViews);
  }
  const document = parseSavedViewsDocument(await invoke<unknown>("save_saved_view", { expectedRevision, view }));
  if (!document) throw new Error("저장된 뷰 응답이 유효하지 않습니다");
  return document;
}

export async function removeSavedView(expectedRevision: number, name: string): Promise<SavedViewsDocument> {
  if (!isTauri()) {
    if (browserSavedViews.revision !== expectedRevision) throw new Error("저장된 뷰가 다른 작업에서 변경되었습니다. 다시 불러온 뒤 시도해 주세요");
    const views = browserSavedViews.views.filter((view) => view.name !== name);
    if (views.length === browserSavedViews.views.length) throw new Error("저장된 뷰를 찾을 수 없습니다");
    browserSavedViews = { schemaVersion: 1, revision: expectedRevision + 1, views };
    return structuredClone(browserSavedViews);
  }
  const document = parseSavedViewsDocument(await invoke<unknown>("delete_saved_view", { expectedRevision, name }));
  if (!document) throw new Error("저장된 뷰 응답이 유효하지 않습니다");
  return document;
}

export async function acceptLogSource(id: string): Promise<SourceSpec> {
  if (!isTauri()) throw new Error("Log Lens source handoff is desktop-only");
  if (!HANDOFF_ID_PATTERN.test(id)) throw new HandoffApiError("handoff-invalid");
  let response: unknown;
  try {
    response = await invoke<unknown>("accept_log_source", { id });
  } catch (error) {
    throw sanitizedHandoffError(error, "handoff-storage-failed");
  }
  const source = parseHandoffSource(response);
  if (!source) throw new HandoffApiError("handoff-response-invalid");
  return source;
}

export async function discardLogSource(id: string): Promise<void> {
  if (!isTauri()) throw new Error("Log Lens source handoff is desktop-only");
  if (!HANDOFF_ID_PATTERN.test(id)) throw new HandoffApiError("handoff-invalid");
  try {
    await invoke("discard_log_source", { id });
  } catch (error) {
    throw sanitizedHandoffError(error, "handoff-restore-failed");
  }
}

export async function renewLogSource(id: string): Promise<number> {
  if (!isTauri()) throw new Error("Log Lens source handoff is desktop-only");
  if (!HANDOFF_ID_PATTERN.test(id)) throw new HandoffApiError("handoff-invalid");
  let response: unknown;
  try {
    response = await invoke<unknown>("renew_log_source", { id });
  } catch (error) {
    throw sanitizedHandoffError(error, "handoff-storage-failed");
  }
  const result = response;
  if (!isRecord(result)
    || !hasOnlyKeys(result, ["leaseUntilMs"])
    || !Number.isSafeInteger(result.leaseUntilMs)) {
    throw new HandoffApiError("handoff-response-invalid");
  }
  const leaseUntilMs = result.leaseUntilMs as number;
  if (leaseUntilMs <= 0) throw new HandoffApiError("handoff-response-invalid");
  return leaseUntilMs;
}

export async function summarizeSource(source: SourceSpec): Promise<SourceSummary> {
  if (!isTauri()) return browserSnapshot([source]).sources[0];
  return invoke<SourceSummary>("summarize_source", { source });
}

export async function readSources(
  sources: SourceSpec[],
  cursors: Array<FileCursor | null>,
  sequenceStarts: number[],
  generation: number,
  operationId: string,
): Promise<SourcesSnapshot> {
  if (!isTauri()) return browserSnapshot(sources, operationId, generation);
  return invoke<SourcesSnapshot>("read_sources", {
    sources,
    cursors,
    sequenceStarts,
    generation,
    operationId,
  });
}

export async function cancelRead(operationId: string): Promise<void> {
  if (isTauri()) await invoke("cancel_read", { operationId });
}

function parseToolboxDispatch(value: unknown): ToolboxDispatch | null {
  if (!isRecord(value)
    || !hasOnlyKeys(value, ["handoffId", "redacted"])
    || typeof value.handoffId !== "string"
    || value.handoffId.length === 0
    || value.handoffId.length > 128
    || /[\u0000-\u001f\u007f-\u009f]/.test(value.handoffId)
    || typeof value.redacted !== "boolean") return null;
  return {
    handoffId: value.handoffId,
    redacted: value.redacted,
  };
}

/** Publish only the current explicit log selection to Developer Toolbox. */
export async function sendSelectionToToolbox(text: string): Promise<ToolboxDispatch> {
  if (!isTauri()) throw new Error(TOOLBOX_TEXT_BROWSER_ERROR);
  const response = await invoke<unknown>("send_selection_to_toolbox", { text });
  const dispatch = parseToolboxDispatch(response);
  if (!dispatch) throw new Error(TOOLBOX_TEXT_INVALID_ERROR);
  return dispatch;
}

export async function filterRecords(records: LogRecord[], filter: FilterSpec): Promise<LogRecord[]> {
  if (!isTauri()) {
    return applyFilter(records, filter);
  }
  return invoke<LogRecord[]>("filter_log_records", { records, filter });
}

function escapeLogfmtValue(value: string): string {
  const quoted = /[\s"\\\u0000-\u001f\u007f-\u009f]/.test(value);
  if (!quoted) return value;
  const escaped = value
    .replace(/\\/g, "\\\\")
    .replace(/"/g, '\\"')
    .replace(/\n/g, "\\n")
    .replace(/\r/g, "\\r")
    .replace(/\t/g, "\\t");
  return `"${escaped}"`;
}

function escapeExportMessage(value: string): string {
  return value
    .replace(/\\/g, "\\\\")
    .replace(/\n/g, "\\n")
    .replace(/\r/g, "\\r")
    .replace(/[\u0000-\u001f\u007f-\u009f]/g, (character) => `\\u{${character.codePointAt(0)?.toString(16) ?? "0"}}`);
}

export async function exportRecords(records: LogRecord[]): Promise<ExportedText> {
  if (!isTauri()) {
    const encoder = new TextEncoder();
    let text = "";
    let bytes = 0;
    let truncated = records.length > MAX_RECORDS;
    for (const record of records.slice(0, MAX_RECORDS)) {
      let line = record.timestampMillis === null ? "" : `${record.timestampMillis} `;
      line += escapeExportMessage(record.message);
      for (const [key, value] of Object.entries(record.fields).sort(([left], [right]) => {
        if (left === right) return 0;
        return left < right ? -1 : 1;
      })) {
        if (["timestamp", "time", "ts", "level", "severity", "msg", "message"].includes(key)) continue;
        line += ` ${key}=${escapeLogfmtValue(value)}`;
      }
      line += "\n";
      const lineBytes = encoder.encode(line).byteLength;
      if (bytes + lineBytes > MAX_EXPORT_BYTES) {
        truncated = true;
        break;
      }
      text += line;
      bytes += lineBytes;
    }
    return { text, truncated };
  }
  return invoke<ExportedText>("export_log_records", { records });
}
