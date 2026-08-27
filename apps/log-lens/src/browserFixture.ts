import type { LogRecord, SourceSpec, SourcesSnapshot } from "./types";

const fixtureRecords: LogRecord[] = [
  {
    sourceId: "log-source:fixture",
    sequence: 0,
    timestampMillis: Date.parse("2026-08-27T09:00:00Z"),
    level: "info",
    message: "Log Lens browser fixture",
    fields: { mode: "offline" },
    format: "plain",
    truncated: false,
  },
  {
    sourceId: "log-source:fixture",
    sequence: 1,
    timestampMillis: Date.parse("2026-08-27T09:00:01Z"),
    level: "warn",
    message: "Add a local or adapter source to inspect logs",
    fields: {},
    format: "plain",
    truncated: false,
  },
];

function fixtureSourceId(index: number): string {
  // Keep the browser fixture's source identity stable and opaque while still
  // exercising the same per-source filtering/selection behavior as native.
  return `log-source:fixture-${index}`;
}

export function browserSnapshot(
  sources: SourceSpec[] = [{ kind: "localFile", path: "fixture.log" }],
  operationId = "browser-fixture",
  generation = 0,
): SourcesSnapshot {
  const summaries = sources.map((source, index) => ({
    sourceId: fixtureSourceId(index),
    kind: source.kind,
    displayName: "Browser fixture",
    readOnly: true,
    handoff: source.kind === "run",
  }));
  return {
    operationId,
    generation,
    sources: summaries,
    records: summaries.flatMap((summary) => fixtureRecords.map((record) => ({
      ...record,
      sourceId: summary.sourceId,
    }))),
    cursors: sources.map(() => null),
    statuses: sources.map(() => "initial"),
    truncated: false,
    droppedRecords: 0,
    droppedBytes: 0,
  };
}
