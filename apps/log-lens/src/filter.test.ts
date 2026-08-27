import { describe, expect, it } from "vitest";
import { exportRecords } from "./api";
import { browserSnapshot } from "./browserFixture";
import {
  createSafeRegex,
  filterRecords,
  recordKey,
  truncateUtf8,
  utf8ByteLength,
} from "./filter";
import type { LogRecord } from "./types";

const records: LogRecord[] = [
  {
    sourceId: "a",
    sequence: 1,
    timestampMillis: 10,
    level: "error",
    message: "failed request",
    fields: { code: "500" },
    format: "logfmt",
    truncated: false,
  },
  {
    sourceId: "b",
    sequence: 2,
    timestampMillis: 20,
    level: "info",
    message: "ok",
    fields: { code: "200" },
    format: "jsonl",
    truncated: false,
  },
];

describe("Log Lens filter", () => {
  it("combines source, level, field and literal filters", () => {
    expect(filterRecords(records, {
      text: "fail",
      regex: false,
      sourceId: "a",
      level: "error",
      field: "code",
      fieldValue: "500",
    })).toHaveLength(1);
  });

  it("fails closed for invalid regex and preserves opaque keys", () => {
    expect(filterRecords(records, { text: "[", regex: true })).toEqual([]);
    expect(recordKey(records[0])).toBe("a:1");
  });

  it("matches regex text in fields like the native filter", () => {
    const fieldRecord = { ...records[0], message: "request failed", fields: { requestId: "abc-123" } };
    expect(filterRecords([fieldRecord], { text: "abc-\\d+", regex: true })).toHaveLength(1);
  });

  it("fails closed for browser regexes with backtracking hazards", () => {
    expect(createSafeRegex("(a+)+$")).toBeNull();
    expect(createSafeRegex("(a|aa)+$")).toBeNull();
    expect(createSafeRegex("a{1000}")).toBeNull();
    expect(filterRecords([
      { ...records[0], message: "aaaaaaaaaaaaaaaaaaaaaaaa" },
    ], { text: "(a+)+$", regex: true })).toEqual([]);
  });

  it("keeps browser fixture source IDs distinct for multi-source views", () => {
    const snapshot = browserSnapshot([
      { kind: "localFile", path: "a.log" },
      { kind: "localFile", path: "b.log" },
    ]);
    expect(snapshot.sources.map((source) => source.sourceId)).toEqual([
      "log-source:fixture-0",
      "log-source:fixture-1",
    ]);
    expect(snapshot.records).toHaveLength(4);
    expect(filterRecords(snapshot.records, {
      text: "",
      regex: false,
      sourceId: "log-source:fixture-1",
    })).toHaveLength(2);
  });

  it("bounds filter text in UTF-8 bytes without splitting a scalar", () => {
    const bounded = truncateUtf8("😀".repeat(200), 512);
    expect(utf8ByteLength(bounded)).toBeLessThanOrEqual(512);
    expect(bounded.length % 2).toBe(0);
    expect(filterRecords([], { text: "😀".repeat(200), regex: false })).toEqual([]);
  });

  it("keeps browser export control escaping aligned with the native exporter", async () => {
    const exported = await exportRecords([{
      ...records[0],
      message: "hello\nworld\t",
    }]);
    expect(exported).toEqual({
      text: "10 hello\\nworld\\u{9} code=500\n",
      truncated: false,
    });
  });
});
