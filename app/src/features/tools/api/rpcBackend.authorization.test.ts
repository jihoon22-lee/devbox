import { expect, it, vi } from "vitest";
import type { RpcClient } from "../../../rpc/client";
import type { StreamHandlers } from "../../../rpc/client-streams";
import type { SendEvent, SendRequest } from "../../../rpc/gen/rpc";
import { emptyDraft, toSnapshot } from "./model/draft";
import { rpcBackend } from "./rpcBackend";

const request: SendRequest = {
  snapshot: toSnapshot({ ...emptyDraft(), url: "https://owned.invalid/" }),
  env: { variables: [] },
  origin: null,
  runId: null,
  cookieJar: null,
  sourceRoot: { kind: "personal" },
};
function setup() {
  let handlers!: StreamHandlers;
  const end = vi.fn();
  const openStream = vi.fn((_method, _params, next: StreamHandlers) => {
    handlers = next;
    return Promise.resolve({ end });
  });
  const backend = rpcBackend({ openStream } as unknown as RpcClient);
  const controller = new AbortController();
  const authorize = vi.fn();
  const ack = vi.fn();
  const result = backend.send(request, controller.signal, authorize).catch((error) => error);
  const emit = (event: SendEvent) => handlers.onData(new TextEncoder().encode(`${JSON.stringify(event)}\n`), ack);
  return { controller, authorize, ack, result, emit, end, openStream, handlers: () => handlers };
}
it("split same-stream frames carry Started identity and clear authorization at response head", async () => {
  const s = setup();
  await Promise.resolve();
  const bytes = new TextEncoder().encode(
    '{"type":"started","historyId":"h1","sentAtMs":1}\n{"type":"authorize","url":"https://auth.invalid/?state=1"}\n',
  );
  for (const byte of bytes) s.handlers().onData(new Uint8Array([byte]), s.ack);
  expect(s.authorize).toHaveBeenLastCalledWith({ historyId: "h1", url: "https://auth.invalid/?state=1" });
  s.emit({
    type: "head",
    status: 200,
    reason: "OK",
    httpVersion: "HTTP/1.1",
    headers: [],
    elapsedMs: 1,
    redirects: [],
  });
  expect(s.authorize).toHaveBeenLastCalledWith(null);
  s.emit({ type: "end", sizeBytes: 0, elapsedMs: 1, binary: false, historyTruncated: false });
  s.handlers().onEnd();
  expect(await s.result).toMatchObject({ historyId: "h1", status: 200 });
});
it("a second Started cannot rebind authorization to a different request identity", async () => {
  const s = setup();
  await Promise.resolve();
  s.emit({ type: "started", historyId: "h1", sentAtMs: 1 });
  s.emit({ type: "started", historyId: "h2", sentAtMs: 2 });
  s.emit({ type: "authorize", url: "https://auth.invalid/" });
  expect((await s.result).code).toBe("internal");
  expect(s.authorize).not.toHaveBeenCalled();
  expect(s.end).toHaveBeenCalled();
  expect(s.ack).toHaveBeenCalledTimes(3);
});
it("abort settles once and late data is acknowledged without forwarding authorization", async () => {
  const s = setup();
  await Promise.resolve();
  s.emit({ type: "started", historyId: "h1", sentAtMs: 1 });
  s.controller.abort();
  s.emit({ type: "authorize", url: "https://auth.invalid/" });
  s.handlers().onEnd();
  expect((await s.result).code).toBe("cancelled");
  expect(s.authorize).not.toHaveBeenCalled();
  expect(s.ack).toHaveBeenCalledTimes(2);
});
it("an already cancelled request never opens a transport", async () => {
  const openStream = vi.fn();
  const backend = rpcBackend({ openStream } as unknown as RpcClient);
  const controller = new AbortController();
  controller.abort();
  await expect(backend.send(request, controller.signal, vi.fn())).rejects.toMatchObject({ code: "cancelled" });
  expect(openStream).not.toHaveBeenCalled();
});
