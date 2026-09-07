import { describe, expect, it } from "vitest";
import {
  MAX_LINE_BYTES,
  SseEventBuffer,
  SseParseError,
  SseParser,
} from "./sse";

function bytes(value: string): Uint8Array {
  return new TextEncoder().encode(value);
}

function parse(chunks: Uint8Array[]): ReturnType<SseParser["feed"]> {
  const parser = new SseParser();
  const events = chunks.flatMap((chunk) => parser.feed(chunk));
  return [...events, ...parser.finish()];
}

describe("SseParser", () => {
  it("handles BOM, comments, CRLF and multiline data", () => {
    const events = parse([bytes("\ufeff: heartbeat\r\nevent: update\r\ndata: one\r\ndata: two\r\nid: 42\r\nretry: 1500\r\n\r\n")]);
    expect(events).toEqual([{
      event: "update",
      data: "one\ntwo",
      id: "42",
      retryMs: 1500,
    }]);
  });

  it("keeps UTF-8 code points split across byte chunks and flushes EOF", () => {
    const value = bytes("\ufeffdata: café");
    const events = parse([
      new Uint8Array(),
      value.slice(0, 2),
      value.slice(2, 9),
      value.slice(9),
      bytes("\n"),
    ]);
    expect(events).toEqual([{ event: "message", data: "café" }]);
  });

  it("treats a single CR as one line break without dispatching early", () => {
    const events = parse([bytes("data: first\rdata: second\r\r")]);
    expect(events).toEqual([{ event: "message", data: "first\nsecond" }]);
  });

  it("clears an id on an empty id field", () => {
    const events = parse([bytes("id: first\ndata: one\n\nid:\ndata: two\n\n")]);
    expect(events[0]?.id).toBe("first");
    expect(events[1]?.id).toBeUndefined();
  });

  it("retains retry metadata even when a chunk has no dispatched event", () => {
    const parser = new SseParser();
    expect(parser.feed(bytes("retry: 2400\n"))).toEqual([]);
    expect(parser.retryMs).toBe(2400);
    expect(parser.finish()).toEqual([]);
    expect(parser.retryMs).toBe(2400);
  });

  it("rejects invalid UTF-8, retry grammar and bounded data", () => {
    const invalid = new SseParser();
    expect(() => invalid.feed(new Uint8Array([0xff]))).toThrow(SseParseError);
    const retry = new SseParser();
    expect(() => retry.feed(bytes("retry: 1.5\n"))).toThrow(SseParseError);
    const nul = new SseParser();
    expect(() => nul.feed(bytes("id: bad\0id\n"))).toThrow("event id is invalid");
    const line = new SseParser();
    expect(() => line.feed(bytes(`data: ${"x".repeat(MAX_LINE_BYTES)}\n`))).toThrow("line is too long");
    const data = new SseParser();
    const boundedLines = Array.from({ length: 17 }, () => `data: ${"x".repeat(MAX_LINE_BYTES - 16)}\n`).join("");
    expect(() => data.feed(bytes(boundedLines))).toThrow("event data is too large");
  });
});

describe("SseEventBuffer", () => {
  it("evicts oldest events at the retained history bound", () => {
    const buffer = new SseEventBuffer();
    for (let index = 0; index <= 10_000; index += 1) {
      buffer.push({ event: "message", data: String(index) });
    }
    expect(buffer.events).toHaveLength(10_000);
    expect(buffer.evicted).toBe(1);
  });
});
