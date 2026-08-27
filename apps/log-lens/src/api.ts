import { invoke } from "@tauri-apps/api/core";
import { browserSnapshot } from "./browserFixture";
import { filterRecords as applyFilter } from "./filter";
import { isTauri } from "./lib/isTauri";
import type {
  ExportedText,
  FileCursor,
  FilterSpec,
  LogRecord,
  SourceSpec,
  SourceSummary,
  SourcesSnapshot,
} from "./types";

const MAX_RECORDS = 100_000;
const MAX_EXPORT_BYTES = 8 * 1024 * 1024;

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
