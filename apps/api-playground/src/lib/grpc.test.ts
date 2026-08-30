import { describe, expect, it } from "vitest";
import type { GrpcExchangeSummary } from "../grpcApi";
import {
  GRPC_HISTORY_KEY,
  GRPC_HISTORY_SCHEMA,
  MAX_GRPC_HISTORY,
  appendGrpcHistory,
  emptyGrpcHistory,
  parseGrpcHistory,
  saveGrpcHistory,
  splitGrpcRequestMessages,
  type GrpcHistoryStore,
} from "./grpc";

const ENTRY_KEYS = [
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
];

function summary(overrides: Partial<GrpcExchangeSummary> = {}): GrpcExchangeSummary {
  return {
    sourceKind: "local-proto",
    service: "Greeter",
    method: "SayHello",
    rpcKind: "unary",
    requestMessageCount: 1,
    responseMessageCount: 1,
    startedAtMs: 1_700_000_000_000,
    elapsedMs: 5,
    status: "OK",
    tlsMode: "plaintext",
    credentialUsed: false,
    ...overrides,
  };
}

function historyJson(entries: unknown[], extra: Record<string, unknown> = {}): string {
  return JSON.stringify({
    schema: GRPC_HISTORY_SCHEMA,
    entries,
    ...extra,
  });
}

function storage(): Storage {
  const values = new Map<string, string>();
  return {
    getItem: (key) => values.get(key) ?? null,
    setItem: (key, value) => { values.set(key, value); },
    removeItem: (key) => { values.delete(key); },
    clear: () => { values.clear(); },
    key: (index) => [...values.keys()][index] ?? null,
    get length() { return values.size; },
  };
}

describe("gRPC summary-only history", () => {
  it("round-trips the exact schema and stores no body, endpoint, credential, or PEM fields", () => {
    const saved = saveGrpcHistory({
      schema: GRPC_HISTORY_SCHEMA,
      entries: [summary()],
    }, storage());

    expect(saved).toEqual({
      schema: GRPC_HISTORY_SCHEMA,
      entries: [summary()],
    });
    expect(Object.keys(saved.entries[0])).toEqual(ENTRY_KEYS);
    const serialized = JSON.stringify(saved);
    for (const forbidden of [
      "requestBody",
      "responseBody",
      "responses",
      "endpoint",
      "credentialId",
      "privateKeyPem",
      "-----BEGIN",
    ]) {
      expect(serialized).not.toContain(forbidden);
    }
  });

  it("fails closed for unknown root or entry keys and an invalid schema", () => {
    const valid = summary();
    expect(parseGrpcHistory(historyJson([valid], { extra: true }))).toBeNull();
    expect(parseGrpcHistory(historyJson([{
      ...valid,
      responseBody: { secret: "must-not-persist" },
    }]))).toBeNull();
    expect(parseGrpcHistory(JSON.stringify({
      schema: "devbox.api-playground.grpc-history/v0",
      entries: [],
    }))).toBeNull();
    expect(parseGrpcHistory(JSON.stringify({
      schema: GRPC_HISTORY_SCHEMA,
      entries: [],
      endpoint: "https://secret.example.test",
    }))).toBeNull();
  });

  it("enforces RPC count, time, name, status, TLS, and history-count bounds", () => {
    const invalidEntries: unknown[] = [
      summary({ requestMessageCount: 0 }),
      summary({ requestMessageCount: 2 }),
      summary({ responseMessageCount: 0 }),
      summary({ responseMessageCount: 2 }),
      summary({ rpcKind: "client-streaming", requestMessageCount: 101, responseMessageCount: 0, status: "INTERNAL" }),
      summary({ rpcKind: "server-streaming", responseMessageCount: 101 }),
      summary({ startedAtMs: 0 }),
      summary({ startedAtMs: 8_640_000_000_000_001 }),
      summary({ elapsedMs: -1 }),
      summary({ startedAtMs: Number.MAX_SAFE_INTEGER + 1 }),
      summary({ service: "contains whitespace" }),
      summary({ method: "" }),
      summary({ status: "NOT_A_GRPC_STATUS" as GrpcExchangeSummary["status"] }),
      summary({ tlsMode: "unknown" as GrpcExchangeSummary["tlsMode"] }),
      summary({ tlsMode: "plaintext", credentialUsed: true }),
    ];
    for (const invalid of invalidEntries) {
      expect(parseGrpcHistory(historyJson([invalid]))).toBeNull();
    }

    const tooManyEntries = Array.from({ length: MAX_GRPC_HISTORY + 1 }, (_, index) => (
      summary({ method: `Method${index}` })
    ));
    expect(parseGrpcHistory(historyJson(tooManyEntries))).toBeNull();
  });

  it("prepends new summaries and retains only the newest bounded entries", () => {
    const entries = Array.from({ length: MAX_GRPC_HISTORY }, (_, index) => (
      summary({ method: `Method${index}` })
    ));
    const original: GrpcHistoryStore = { schema: GRPC_HISTORY_SCHEMA, entries };
    const appended = appendGrpcHistory(original, summary({ method: "Newest" }));

    expect(appended.entries).toHaveLength(MAX_GRPC_HISTORY);
    expect(appended.entries[0].method).toBe("Newest");
    expect(appended.entries[MAX_GRPC_HISTORY - 1].method).toBe(`Method${MAX_GRPC_HISTORY - 2}`);
    expect(original.entries).toEqual(entries);
  });

  it("preserves each streaming message's raw slice, including duplicate object keys", () => {
    const raw = '[{"id":1,"id":2},{"name":"first","name":"second"}]';

    expect(splitGrpcRequestMessages(raw, "client-streaming")).toEqual([
      '{"id":1,"id":2}',
      '{"name":"first","name":"second"}',
    ]);
    expect(splitGrpcRequestMessages(raw, "bidirectional-streaming")).toEqual([
      '{"id":1,"id":2}',
      '{"name":"first","name":"second"}',
    ]);
  });

  it("requires an array for streaming requests and rejects empty, commented, or trailing-comma input", () => {
    expect(() => splitGrpcRequestMessages("{}", "client-streaming"))
      .toThrow("grpc_request_invalid");
    expect(() => splitGrpcRequestMessages("[]", "client-streaming"))
      .toThrow("grpc_request_invalid");
    expect(() => splitGrpcRequestMessages("[{\"id\":1},]", "client-streaming"))
      .toThrow("grpc_request_invalid");
    expect(() => splitGrpcRequestMessages("[{\"id\":1 /* no comments */}]", "client-streaming"))
      .toThrow("grpc_request_invalid");
  });

  it("uses the documented storage key when the caller saves a history", () => {
    const backing = storage();
    saveGrpcHistory(emptyGrpcHistory(), backing);
    expect(backing.getItem(GRPC_HISTORY_KEY)).toBe(JSON.stringify(emptyGrpcHistory()));
  });
});
