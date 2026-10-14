import { act } from "@testing-library/react";
import { RpcClient } from "./client";
import { decodeFrame, encodeFrame, FRAME, jsonFrame, streamFrame } from "./frame";
import { tauriTransport } from "./transport-tauri";

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: mocks.invoke,
  Channel: class {
    onmessage = (_: ArrayBuffer) => {};
  },
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
function deferred<T>() {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((a, b) => {
    resolve = a;
    reject = b;
  });
  return { promise, resolve, reject };
}
const flush = () => act(async () => {});
beforeEach(() => {
  vi.useFakeTimers();
  mocks.invoke.mockReset();
  mocks.listen.mockReset();
  mocks.listen.mockResolvedValue(vi.fn());
  mocks.invoke.mockImplementation(async (cmd, args) =>
    cmd === "rpc_attach" ? { attachmentId: args.attachmentId, state: "open", revision: "1" } : undefined,
  );
});
afterEach(() => vi.useRealTimers());
test("close while listen is pending never attaches and releases late listener", async () => {
  const wait = deferred<() => void>();
  const off = vi.fn();
  mocks.listen.mockReturnValue(wait.promise);
  const t = tauriTransport();
  t.connect();
  t.close();
  wait.resolve(off);
  await flush();
  expect(off).toHaveBeenCalledOnce();
  expect(mocks.invoke).not.toHaveBeenCalledWith("rpc_attach", expect.anything());
});
test("late attach is detached by its lease and cannot reopen a restarted transport", async () => {
  const wait = deferred<unknown>();
  mocks.invoke.mockImplementation((cmd) => (cmd === "rpc_attach" ? wait.promise : Promise.resolve()));
  const t = tauriTransport();
  const state = vi.fn();
  t.onState(state);
  t.connect();
  await flush();
  const old = mocks.invoke.mock.calls.find((c) => c[0] === "rpc_attach")?.[1];
  t.close();
  t.connect();
  await flush();
  wait.resolve({ attachmentId: old.attachmentId, state: "open", revision: "1" });
  await flush();
  expect(mocks.invoke).toHaveBeenCalledWith("rpc_detach", { attachmentId: old.attachmentId });
  expect(state).not.toHaveBeenCalledWith("open");
  t.close();
});
test("newer notice wins over attach snapshot and bridge reopen retains attachment", async () => {
  const wait = deferred<unknown>();
  mocks.invoke.mockImplementation((cmd) => (cmd === "rpc_attach" ? wait.promise : Promise.resolve()));
  const t = tauriTransport();
  const state = vi.fn();
  t.onState(state);
  t.connect();
  await flush();
  const args = mocks.invoke.mock.calls.find((c) => c[0] === "rpc_attach")?.[1];
  const notice = mocks.listen.mock.calls[0]?.[1];
  notice({ payload: { attachmentId: args.attachmentId, state: "closed", revision: "10" } });
  wait.resolve({ attachmentId: args.attachmentId, state: "open", revision: "9" });
  await flush();
  expect(() => t.send(jsonFrame({}))).toThrow();
  notice({ payload: { attachmentId: args.attachmentId, state: "open", revision: "11" } });
  t.send(jsonFrame({}));
  await flush();
  expect(mocks.invoke).toHaveBeenCalledWith("rpc_send", expect.any(Uint8Array), {
    headers: { "x-devbox-attachment": args.attachmentId },
  });
  t.close();
});
test("serialized writes fail closed, discard queued frames and retry own attachment", async () => {
  const wait = deferred<void>();
  mocks.invoke.mockImplementation((cmd, args) =>
    cmd === "rpc_attach"
      ? Promise.resolve({ attachmentId: args.attachmentId, state: "open", revision: "1" })
      : cmd === "rpc_send"
        ? wait.promise
        : Promise.resolve(),
  );
  const t = tauriTransport();
  const state = vi.fn();
  t.onState(state);
  t.connect();
  await flush();
  t.send(jsonFrame({ a: 1 }));
  t.send(jsonFrame({ a: 2 }));
  await flush();
  expect(mocks.invoke.mock.calls.filter((c) => c[0] === "rpc_send")).toHaveLength(1);
  wait.reject({ kind: "bridgeClosed" });
  await flush();
  expect(state).toHaveBeenLastCalledWith("closed");
  expect(() => t.send(jsonFrame({}))).toThrow();
  await vi.advanceTimersByTimeAsync(200);
  expect(mocks.invoke.mock.calls.filter((c) => c[0] === "rpc_attach")).toHaveLength(2);
  expect(mocks.invoke.mock.calls.filter((c) => c[0] === "rpc_send")).toHaveLength(1);
  t.close();
});
test("listener failure retries, but explicit close cancels scheduled retries", async () => {
  mocks.listen.mockRejectedValueOnce({ kind: "unavailable" });
  const t = tauriTransport();
  const state = vi.fn();
  t.onState(state);
  t.connect();
  await flush();
  expect(state).toHaveBeenLastCalledWith("closed");
  await vi.advanceTimersByTimeAsync(200);
  expect(state).toHaveBeenLastCalledWith("open");
  t.reconnect?.();
  t.close();
  await vi.advanceTimersByTimeAsync(10000);
  expect(mocks.listen).toHaveBeenCalledTimes(2);
});
test("old channel and failed write cannot affect a new attachment", async () => {
  const wait = deferred<void>();
  mocks.invoke.mockImplementation((cmd, args) =>
    cmd === "rpc_attach"
      ? Promise.resolve({ attachmentId: args.attachmentId, state: "open", revision: "1" })
      : cmd === "rpc_send"
        ? wait.promise
        : Promise.resolve(),
  );
  const t = tauriTransport();
  const frame = vi.fn();
  const state = vi.fn();
  t.onFrame(frame);
  t.onState(state);
  t.connect();
  await flush();
  const old = mocks.invoke.mock.calls.find((c) => c[0] === "rpc_attach")?.[1];
  t.send(jsonFrame({}));
  t.close();
  t.connect();
  await flush();
  old.onFrame.onmessage(jsonFrame({ stale: true }).buffer);
  wait.reject({ kind: "closed" });
  await flush();
  expect(frame).not.toHaveBeenCalled();
  expect(state).toHaveBeenLastCalledWith("open");
  t.close();
});
test("malformed incoming frames close the connection and pending writes are bounded", async () => {
  const t = tauriTransport();
  const state = vi.fn();
  t.onState(state);
  t.connect();
  await flush();
  const args = mocks.invoke.mock.calls.find((c) => c[0] === "rpc_attach")?.[1];
  args.onFrame.onmessage(new ArrayBuffer(2));
  expect(state).toHaveBeenLastCalledWith("closed");
  await vi.advanceTimersByTimeAsync(200);
  mocks.invoke.mockImplementation((cmd) => (cmd === "rpc_send" ? new Promise(() => {}) : Promise.resolve()));
  for (let i = 0; i < 256; i++) t.send(jsonFrame({ i }));
  expect(() => t.send(jsonFrame({}))).toThrow("queue is full");
  expect(state).toHaveBeenLastCalledWith("closed");
  t.close();
});
test("reopening the SAME attachment starts a fresh sender and ignores stale failure", async () => {
  const oldSend = deferred<void>();
  mocks.invoke.mockImplementation((cmd, args) =>
    cmd === "rpc_attach"
      ? Promise.resolve({ attachmentId: args.attachmentId, state: "open", revision: "1" })
      : Promise.resolve(),
  );
  const t = tauriTransport();
  const states = vi.fn();
  t.onState(states);
  t.connect();
  await flush();
  const args = mocks.invoke.mock.calls.find((c) => c[0] === "rpc_attach")?.[1];
  const notice = mocks.listen.mock.calls[0]?.[1];
  mocks.invoke.mockImplementation((cmd) => (cmd === "rpc_send" ? oldSend.promise : Promise.resolve()));
  t.send(jsonFrame({ old: 1 }));
  t.send(jsonFrame({ type: "streamCredit", stream: 9, bytes: 1 }, FRAME.control));
  t.send(jsonFrame({ type: "streamCredit", stream: 9, bytes: 2 }, FRAME.control));
  notice({ payload: { attachmentId: args.attachmentId, state: "closed", revision: "2" } });
  notice({ payload: { attachmentId: args.attachmentId, state: "open", revision: "3" } });
  mocks.invoke.mockImplementation(() => Promise.resolve());
  const fresh = jsonFrame({ new: 1 });
  t.send(fresh);
  await flush();
  expect(mocks.invoke.mock.calls.filter((c) => c[0] === "rpc_send")).toHaveLength(2);
  oldSend.reject(new Error("stale epoch"));
  await flush();
  expect(states).toHaveBeenLastCalledWith("open");
  expect(mocks.invoke.mock.calls.filter((c) => c[0] === "rpc_attach")).toHaveLength(1);
  t.send(jsonFrame({ new: 2 }));
  await flush();
  expect(mocks.invoke.mock.calls.filter((c) => c[0] === "rpc_send")).toHaveLength(3);
  t.close();
});
test("frame ownership is copied for both active and waiting invocations", async () => {
  const wait = deferred<void>();
  const t = tauriTransport();
  t.connect();
  await flush();
  mocks.invoke.mockImplementation((cmd) => (cmd === "rpc_send" ? wait.promise : Promise.resolve()));
  const active = jsonFrame({ a: 1 });
  const waiting = jsonFrame({ a: 2 });
  const activeCopy = active.slice();
  const waitingCopy = waiting.slice();
  t.send(active);
  t.send(waiting);
  active.fill(0);
  waiting.fill(0);
  expect(mocks.invoke.mock.calls.find((c) => c[0] === "rpc_send")?.[1]).toEqual(activeCopy);
  wait.resolve();
  await flush();
  expect(mocks.invoke.mock.calls.filter((c) => c[0] === "rpc_send")[1]?.[1]).toEqual(waitingCopy);
  t.close();
});
test("byte limit includes framing and active invocation", async () => {
  const t = tauriTransport();
  const state = vi.fn();
  t.onState(state);
  t.connect();
  await flush();
  mocks.invoke.mockImplementation((cmd) => (cmd === "rpc_send" ? new Promise(() => {}) : Promise.resolve()));
  const frame = encodeFrame({ kind: FRAME.json, channel: 0, body: new Uint8Array(1024 * 1024) });
  for (let i = 0; i < 7; i++) t.send(frame);
  expect(() => t.send(frame)).toThrow("queue is full");
  expect(state).toHaveBeenLastCalledWith("closed");
  t.close();
});

test("native stream ACK burst preserves ready connection, exact credit and input/close ordering", async () => {
  const t = tauriTransport();
  const client = new RpcClient(t, { version: "v", instance: "test", client: "test" });
  client.start();
  await flush();
  const args = mocks.invoke.mock.calls.find((call) => call[0] === "rpc_attach")![1];
  const receive = (frame: Uint8Array) => args.onFrame.onmessage(frame.buffer);
  receive(jsonFrame({ type: "welcome", version: "v", protocol: 1, daemonId: "daemon" }));
  const ended = vi.fn();
  const opening = client.openStream(
    "debug.echo",
    { bytes: 0 },
    {
      onData: (_bytes, ack) => ack(),
      onEnd: ended,
    },
  );
  await flush();
  const request = mocks.invoke.mock.calls
    .filter((call) => call[0] === "rpc_send")
    .map((call) => decodeFrame(call[1]))
    .filter((frame) => frame.kind === FRAME.json)
    .map((frame) => JSON.parse(new TextDecoder().decode(frame.body)))
    .find((message) => message.type === "request");
  receive(jsonFrame({ type: "response", id: request.id, result: { streamId: 9 } }));
  const stream = await opening;
  const first = deferred<void>();
  mocks.invoke.mockClear();
  mocks.invoke.mockImplementation((command) => (command === "rpc_send" ? first.promise : Promise.resolve()));
  for (let i = 0; i < 2000; i++) receive(streamFrame(9, 0, Uint8Array.of(120)));
  expect(client.state).toBe("ready");
  stream.write(Uint8Array.of(113));
  stream.end();
  first.resolve();
  await flush();
  const frames = mocks.invoke.mock.calls.filter((call) => call[0] === "rpc_send").map((call) => decodeFrame(call[1]));
  const messages = frames
    .filter((frame) => frame.kind === FRAME.control)
    .map((frame) => JSON.parse(new TextDecoder().decode(frame.body)));
  expect(messages).toEqual([
    { type: "streamCredit", stream: 9, bytes: 1 },
    { type: "streamCredit", stream: 9, bytes: 1999 },
    { type: "streamClose", stream: 9 },
  ]);
  expect(frames.map((frame) => frame.kind)).toEqual([FRAME.control, FRAME.control, FRAME.stream, FRAME.control]);
  expect(ended).toHaveBeenCalledOnce();
  expect(client.state).toBe("ready");
  client.stop();
});
