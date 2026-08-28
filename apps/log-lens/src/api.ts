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
} from "./types";

const MAX_RECORDS = 100_000;
const MAX_EXPORT_BYTES = 8 * 1024 * 1024;

export interface OpenRequest {
  target: { kind: "handoff"; handoffKind: string; id: string };
  from: string | null;
}

const HANDOFF_ID_PATTERN = /^[0-9a-f]{32}$/;
const SOURCE_ID_PATTERN = /^log-source:[0-9a-f]{16}$/;
const RUN_SOURCE_PATTERN = /^run-manager:[A-Za-z0-9_-]{1,128}:(stdout|stderr)$/;
const UNIT_PATTERN = /^[A-Za-z0-9_.:@-]{1,128}$/;
const WSL_INJECTION_PATTERN = /[;&|<>`$"'\\(){}*?\[\]!~#%]/;

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

function parseOpenRequest(value: unknown): OpenRequest | null {
  if (!isRecord(value) || !hasOnlyKeys(value, ["target", "from"])) return null;
  const target = value.target;
  if (!isRecord(target)
    || !hasOnlyKeys(target, ["kind", "handoffKind", "id"])
    || target.kind !== "handoff"
    || target.handoffKind !== "log-source/v1"
    || typeof target.id !== "string"
    || !HANDOFF_ID_PATTERN.test(target.id)) return null;
  const from = value.from;
  if (from !== null && !isSafeText(from, 64)) return null;
  return {
    target: { kind: "handoff", handoffKind: "log-source/v1", id: target.id },
    from: from as string | null,
  };
}

function parseSourceSummary(value: unknown): SourceSummary | null {
  if (!isRecord(value)
    || !hasOnlyKeys(value, ["sourceId", "kind", "displayName", "readOnly", "handoff"])
    || typeof value.sourceId !== "string"
    || !SOURCE_ID_PATTERN.test(value.sourceId)
    || typeof value.kind !== "string"
    || !["wslFile", "wslJournal", "run"].includes(value.kind)
    || typeof value.displayName !== "string"
    || typeof value.readOnly !== "boolean"
    || typeof value.handoff !== "boolean"
    || value.readOnly !== true
    || value.handoff !== true) return null;
  const expectedNames: Record<string, string> = {
    run: "Run Manager handoff",
    wslFile: "WSL file",
    wslJournal: "WSL journal",
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

function parseLogSourcePreview(value: unknown): LogSourcePreview | null {
  if (!isRecord(value)
    || !hasOnlyKeys(value, ["id", "kind", "sourceApp", "expiresAtMs", "leaseUntilMs", "source"])
    || typeof value.id !== "string"
    || !HANDOFF_ID_PATTERN.test(value.id)
    || value.kind !== "log-source/v1"
    || (value.sourceApp !== "run-manager" && value.sourceApp !== "wsl-desktop")
    || !Number.isSafeInteger(value.expiresAtMs)
    || !Number.isSafeInteger(value.leaseUntilMs)) return null;
  const expiresAtMs = value.expiresAtMs as number;
  const leaseUntilMs = value.leaseUntilMs as number;
  if (expiresAtMs <= 0 || leaseUntilMs <= 0 || leaseUntilMs > expiresAtMs) return null;
  const source = parseSourceSummary(value.source);
  if (!source) return null;
  if (value.sourceApp === "run-manager" && source.kind !== "run") return null;
  if (value.sourceApp === "wsl-desktop" && !["wslFile", "wslJournal"].includes(source.kind)) return null;
  return {
    id: value.id,
    kind: "log-source/v1",
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
  return null;
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

export async function previewLogSource(id: string): Promise<LogSourcePreview> {
  if (!isTauri()) throw new Error("Log Lens source handoff is desktop-only");
  if (!HANDOFF_ID_PATTERN.test(id)) throw new Error("Log Lens source handoff request is invalid");
  const preview = parseLogSourcePreview(await invoke<unknown>("preview_log_source", { id }));
  if (!preview || preview.id !== id) throw new Error("Log Lens source handoff response is invalid");
  return preview;
}

export async function acceptLogSource(id: string): Promise<SourceSpec> {
  if (!isTauri()) throw new Error("Log Lens source handoff is desktop-only");
  if (!HANDOFF_ID_PATTERN.test(id)) throw new Error("Log Lens source handoff request is invalid");
  const source = parseHandoffSource(await invoke<unknown>("accept_log_source", { id }));
  if (!source) throw new Error("Log Lens source handoff response is invalid");
  return source;
}

export async function discardLogSource(id: string): Promise<void> {
  if (!isTauri()) throw new Error("Log Lens source handoff is desktop-only");
  if (!HANDOFF_ID_PATTERN.test(id)) throw new Error("Log Lens source handoff request is invalid");
  await invoke("discard_log_source", { id });
}

export async function renewLogSource(id: string): Promise<number> {
  if (!isTauri()) throw new Error("Log Lens source handoff is desktop-only");
  if (!HANDOFF_ID_PATTERN.test(id)) throw new Error("Log Lens source handoff request is invalid");
  const result = await invoke<unknown>("renew_log_source", { id });
  if (!isRecord(result)
    || !hasOnlyKeys(result, ["leaseUntilMs"])
    || !Number.isSafeInteger(result.leaseUntilMs)) {
    throw new Error("Log Lens source handoff response is invalid");
  }
  const leaseUntilMs = result.leaseUntilMs as number;
  if (leaseUntilMs <= 0) throw new Error("Log Lens source handoff response is invalid");
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
