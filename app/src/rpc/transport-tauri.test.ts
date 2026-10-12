import { act } from "@testing-library/react";
import { jsonFrame } from "./frame";
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
  for (let i = 0; i < 65; i++) t.send(jsonFrame({ i }));
  expect(() => t.send(jsonFrame({}))).toThrow("queue is full");
  expect(state).toHaveBeenLastCalledWith("closed");
  t.close();
});
