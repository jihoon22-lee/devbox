import { expect, test, vi } from "vitest";
import type { StreamHandlers } from "../../../rpc/client-streams";
import { LspTransport } from "./lspTransport";

function fixture() {
  let handlers: StreamHandlers;
  const write = vi.fn();
  const end = vi.fn();
  const rpc = {
    openStream: vi.fn(async (_method, _params, h) => {
      handlers = h;
      return { write, end, streamId: 1, result: { streamId: 1 } };
    }),
  };
  const closed = vi.fn();
  const lease = vi.fn(() => "lease-1");
  return {
    rpc,
    write,
    end,
    closed,
    lease,
    receive(bytes: Uint8Array) {
      const ack = vi.fn();
      handlers.onData(bytes, ack);
      expect(ack).toHaveBeenCalledOnce();
    },
    finish() {
      handlers.onEnd();
    },
  };
}
const params = { projectId: "p", language: "typescript" } as const;
const encode = (json: string) => new TextEncoder().encode(`${JSON.stringify({ json })}\n`);

test("transport restores fragmented UTF8 and coalesced JSON lines with handler sets", async () => {
  const f = fixture();
  const transport = await LspTransport.open(f.rpc, params, f.lease, f.closed);
  const a = vi.fn(),
    b = vi.fn();
  transport.subscribe(a);
  transport.subscribe(a);
  transport.subscribe(b);
  const message = JSON.stringify({ jsonrpc: "2.0", method: "x", params: "😀한" });
  const bytes = encode(message);
  for (const byte of bytes) f.receive(Uint8Array.of(byte));
  expect(a).toHaveBeenCalledExactlyOnceWith(message);
  transport.unsubscribe(a);
  const twice = new Uint8Array([...bytes, ...bytes]);
  f.receive(twice);
  expect(a).toHaveBeenCalledTimes(1);
  expect(b).toHaveBeenCalledTimes(3);
  transport.close();
  f.receive(bytes);
  expect(b).toHaveBeenCalledTimes(3);
});

test("send binds current lease and partial write failure closes only its generation", async () => {
  const f = fixture();
  const transport = await LspTransport.open(f.rpc, params, f.lease, f.closed);
  const json = JSON.stringify({
    method: "textDocument/didChange",
    params: { textDocument: { uri: "file:///p/a.ts" } },
  });
  transport.send(json);
  const sent = JSON.parse(new TextDecoder().decode(f.write.mock.calls[0]![0]));
  expect(sent).toEqual({ json, leaseId: "lease-1" });
  expect(f.write.mock.calls[0]![1]).toEqual({ throwOnFailure: true });
  f.write.mockImplementation(() => {
    throw new Error("private payload");
  });
  expect(() => transport.send(json)).toThrow("LSP stream unavailable");
  expect(f.closed).toHaveBeenCalledTimes(1);
  expect(f.end).toHaveBeenCalledTimes(1);
  expect(() => transport.send(json)).toThrow("LSP stream unavailable");
});

test("malformed frames fail closed without logging body and stale EOF is idempotent", async () => {
  const f = fixture();
  const transport = await LspTransport.open(f.rpc, params, f.lease, f.closed);
  const listener = vi.fn();
  transport.subscribe(listener);
  f.receive(new TextEncoder().encode("private-broken-json\n"));
  f.finish();
  expect(f.closed).toHaveBeenCalledTimes(1);
  expect(listener).not.toHaveBeenCalled();
});

test("incremental library messages are converted to the relay's full-text contract", async () => {
  const f = fixture();
  const transport = await LspTransport.open(f.rpc, params, f.lease, f.closed);
  const full = "😀" + "a".repeat(2000);
  transport.fullText = () => full;
  transport.send(
    JSON.stringify({
      method: "textDocument/didChange",
      params: {
        textDocument: { uri: "file:///p/a.ts", version: 2 },
        contentChanges: [{ range: { start: { line: 0, character: 0 }, end: { line: 0, character: 1 } }, text: "x" }],
      },
    }),
  );
  const packet = JSON.parse(new TextDecoder().decode(f.write.mock.calls[0]![0]));
  expect(JSON.parse(packet.json).params.contentChanges).toEqual([{ text: full }]);
});

test("third-party request failures keep error codes but never receive raw server error bodies", async () => {
  const f = fixture();
  const transport = await LspTransport.open(f.rpc, params, f.lease, f.closed);
  const listener = vi.fn();
  transport.subscribe(listener);
  f.receive(
    encode(
      JSON.stringify({
        jsonrpc: "2.0",
        id: 1,
        error: { code: -32801, message: "PRIVATE_SERVER_ERROR", data: "PRIVATE_SERVER_DATA" },
      }),
    ),
  );
  const value = JSON.parse(listener.mock.calls[0]![0]);
  expect(value.error.code).toBe(-32801);
  expect(listener.mock.calls[0]![0]).not.toContain("PRIVATE_");
});
