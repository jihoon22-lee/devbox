import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { type UpdateSnapshot, updateSnapshot, useNativeUpdate } from "./nativeUpdate";

const native = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn(), off: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: native.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: native.listen }));
const listeners: ((event: { payload: unknown }) => void)[] = [];
const snapshot = (changes: Partial<UpdateSnapshot> = {}): UpdateSnapshot => ({
  enabled: true,
  disabledReason: null,
  phase: "idle",
  generation: 0,
  revision: 0,
  currentVersion: "1.0.0",
  availableVersion: null,
  receivedBytes: 0,
  totalBytes: null,
  error: null,
  nextAction: "check",
  ...changes,
});
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
function emit(value: unknown, index = listeners.length - 1) {
  act(() => listeners[index]?.({ payload: value }));
}
beforeEach(() => {
  Object.assign(window, { __TAURI_INTERNALS__: {} });
  listeners.length = 0;
  native.invoke.mockReset().mockResolvedValue(snapshot());
  native.off.mockReset();
  native.listen.mockReset().mockImplementation(async (_name, listener) => {
    listeners.push(listener);
    return native.off;
  });
});
afterEach(() => {
  cleanup();
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
});

test("browser copies no native state and never invokes or subscribes", async () => {
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  const { result } = renderHook(useNativeUpdate);
  await act(async () => {
    await result.current.run("check");
  });
  expect(result.current.native).toBe(false);
  expect(result.current.snapshot).toBeNull();
  expect(native.invoke).not.toHaveBeenCalled();
  expect(native.listen).not.toHaveBeenCalled();
});

test("registration precedes status and a newer event fences late initial query and equal revision", async () => {
  const registration = deferred<() => void>();
  const query = deferred<unknown>();
  native.listen.mockImplementationOnce((_name, listener) => {
    listeners.push(listener);
    return registration.promise;
  });
  native.invoke.mockReturnValueOnce(query.promise);
  const { result } = renderHook(useNativeUpdate);
  expect(native.invoke).not.toHaveBeenCalled();
  emit(snapshot({ revision: 4, generation: 2, phase: "downloading", nextAction: "cancel" }));
  await act(async () => registration.resolve(native.off));
  expect(native.listen).toHaveBeenCalledWith("devbox://update", expect.any(Function));
  expect(native.invoke).toHaveBeenCalledWith("update_status");
  await act(async () => query.resolve(snapshot({ revision: 1 })));
  emit(snapshot({ revision: 4, phase: "failed", error: "network" }));
  expect(result.current.snapshot?.phase).toBe("downloading");
  expect(result.current.snapshot?.revision).toBe(4);
});

test("unmount during registration releases the late listener without querying or cancelling", async () => {
  const registration = deferred<() => void>();
  native.listen.mockReturnValueOnce(registration.promise);
  const view = renderHook(useNativeUpdate);
  view.unmount();
  await act(async () => registration.resolve(native.off));
  expect(native.off).toHaveBeenCalledOnce();
  expect(native.invoke).not.toHaveBeenCalled();
});

test("a new window receives the ongoing generation and can cancel; closing a window never cancels", async () => {
  const ongoing = snapshot({
    generation: 12,
    revision: 25,
    phase: "downloading",
    nextAction: "cancel",
    receivedBytes: 10,
    totalBytes: 20,
  });
  native.invoke.mockResolvedValue(ongoing);
  const first = renderHook(useNativeUpdate);
  await waitFor(() => expect(first.result.current.snapshot).toEqual(ongoing));
  first.unmount();
  expect(native.off).toHaveBeenCalledOnce();
  expect(native.invoke.mock.calls.every(([command]) => command === "update_status")).toBe(true);
  const next = renderHook(useNativeUpdate);
  await waitFor(() => expect(next.result.current.snapshot).toEqual(ongoing));
  emit(snapshot({ revision: 90, phase: "failed", error: "digest" }), 0);
  expect(next.result.current.snapshot).toEqual(ongoing);
  native.invoke.mockResolvedValueOnce(
    snapshot({ generation: 13, revision: 26, phase: "available", nextAction: "download" }),
  );
  await act(async () => next.result.current.run("cancel"));
  expect(native.invoke).toHaveBeenLastCalledWith("update_cancel", { generation: 12 });
  expect(next.result.current.snapshot?.generation).toBe(13);
});

test("duplicate clicks share the short command latch and a newer event fences its late response", async () => {
  const available = snapshot({ generation: 3, revision: 5, phase: "available", nextAction: "download" });
  native.invoke.mockResolvedValueOnce(available);
  const { result } = renderHook(useNativeUpdate);
  await waitFor(() => expect(result.current.snapshot).toEqual(available));
  const command = deferred<unknown>();
  native.invoke.mockReturnValueOnce(command.promise);
  let pending!: Promise<void>;
  act(() => {
    pending = result.current.run("download");
    void result.current.run("download");
  });
  expect(result.current.busy).toBe(true);
  expect(native.invoke.mock.calls.filter(([name]) => name === "update_download")).toHaveLength(1);
  expect(native.invoke).toHaveBeenLastCalledWith("update_download", { generation: 3 });
  emit(snapshot({ generation: 4, revision: 8, phase: "ready", nextAction: "none" }));
  await act(async () => {
    command.resolve(snapshot({ generation: 4, revision: 6, phase: "downloading", nextAction: "cancel" }));
    await pending;
  });
  expect(result.current.snapshot?.phase).toBe("ready");
  expect(result.current.busy).toBe(false);
});

test("late command rejection cannot overwrite a newer native event", async () => {
  const { result } = renderHook(useNativeUpdate);
  await waitFor(() => expect(result.current.snapshot).not.toBeNull());
  const command = deferred<unknown>();
  native.invoke.mockReturnValueOnce(command.promise);
  let pending!: Promise<void>;
  act(() => {
    pending = result.current.run("check");
  });
  emit(snapshot({ revision: 2, generation: 1, phase: "available", nextAction: "download" }));
  await act(async () => {
    command.reject("network");
    await pending;
  });
  expect(result.current.issue).toBeNull();
  expect(result.current.snapshot?.nextAction).toBe("download");
});

test("late initial query rejection does not hide an event snapshot", async () => {
  const query = deferred<unknown>();
  native.invoke.mockReturnValueOnce(query.promise);
  const { result } = renderHook(useNativeUpdate);
  await waitFor(() => expect(native.invoke).toHaveBeenCalledOnce());
  emit(snapshot({ revision: 2 }));
  await act(async () => query.reject(new Error("https://private.invalid/body")));
  expect(result.current.issue).toBeNull();
  expect(result.current.snapshot?.revision).toBe(2);
});

test.each(["query", "command"])("late malformed %s response cannot hide a newer event", async (source) => {
  const reply = deferred<unknown>();
  if (source === "query") native.invoke.mockReturnValueOnce(reply.promise);
  const { result } = renderHook(useNativeUpdate);
  await waitFor(() => expect(native.invoke).toHaveBeenCalledOnce());
  let pending: Promise<void> | undefined;
  if (source === "command") {
    await waitFor(() => expect(result.current.snapshot).not.toBeNull());
    native.invoke.mockReturnValueOnce(reply.promise);
    act(() => {
      pending = result.current.run("check");
    });
  }
  emit(snapshot({ revision: 4, generation: 2, phase: "available", nextAction: "download" }));
  await act(async () => {
    reply.resolve({ revision: 0, error: "private" });
    await pending;
  });
  expect(result.current.issue).toBeNull();
  expect(result.current.snapshot?.revision).toBe(4);
});

test.each(["network", "digest", "staleOperation", { body: "private" }])(
  "command rejection %j is mapped without raw details",
  async (error) => {
    const { result } = renderHook(useNativeUpdate);
    await waitFor(() => expect(result.current.snapshot).not.toBeNull());
    native.invoke.mockRejectedValueOnce(error);
    await act(async () => result.current.run("check"));
    expect(result.current.issue).toBe(typeof error === "string" ? error : "unavailable");
    expect(result.current.busy).toBe(false);
  },
);

test.each([
  null,
  {},
  snapshot({ revision: -1 }),
  snapshot({ generation: Number.MAX_SAFE_INTEGER + 1 }),
  snapshot({ receivedBytes: Number.NaN }),
  snapshot({ totalBytes: -1 }),
  { ...snapshot(), phase: "unknown" },
  { ...snapshot(), disabledReason: "other" },
  { ...snapshot(), disabledReason: undefined },
  { ...snapshot(), error: "https://private.invalid" },
  { ...snapshot(), nextAction: "install" },
  { ...snapshot(), currentVersion: "https://private.invalid/body" },
])("malformed native payload %j never becomes an actionable snapshot", async (value) => {
  native.invoke.mockResolvedValueOnce(value);
  const { result } = renderHook(useNativeUpdate);
  await waitFor(() => expect(result.current.issue).toBe("invalidSnapshot"));
  expect(result.current.snapshot).toBeNull();
  await act(async () => result.current.run("download"));
  expect(native.invoke).toHaveBeenCalledTimes(1);
});

test("listener failure can be retried and payload copies omit unexpected fields", async () => {
  native.listen.mockRejectedValueOnce(new Error("raw secret"));
  const { result } = renderHook(useNativeUpdate);
  await waitFor(() => expect(result.current.issue).toBe("unavailable"));
  expect(native.invoke).not.toHaveBeenCalled();
  act(() => result.current.refresh());
  await waitFor(() => expect(result.current.snapshot).toEqual(snapshot()));
  expect(updateSnapshot({ ...snapshot(), url: "private", path: "secret" })).toEqual(snapshot());
});

test("disabled native snapshots never dispatch user actions", async () => {
  native.invoke.mockResolvedValueOnce(snapshot({ enabled: false, disabledReason: "developmentVersion" }));
  const { result } = renderHook(useNativeUpdate);
  await waitFor(() => expect(result.current.snapshot).not.toBeNull());
  await act(async () => result.current.run("check"));
  expect(native.invoke).toHaveBeenCalledTimes(1);
});

test("installation requires a current captured generation and ticket rather than the generic action dispatcher", async () => {
  native.invoke.mockResolvedValueOnce(
    snapshot({ phase: "ready", generation: 7, revision: 10, availableVersion: "1.4.0", nextAction: "install" }),
  );
  const { result } = renderHook(useNativeUpdate);
  await waitFor(() => expect(result.current.snapshot?.phase).toBe("ready"));
  await act(async () => result.current.run("install" as never));
  expect(native.invoke).toHaveBeenCalledTimes(1);
  await act(async () => result.current.confirmInstall(6, 81));
  expect(native.invoke).toHaveBeenCalledTimes(1);
  expect(result.current.issue).toBe("staleOperation");
  emit(snapshot({ phase: "ready", generation: 8, revision: 11, availableVersion: "1.4.0", nextAction: "install" }));
  await act(async () => result.current.confirmInstall(8, 0));
  expect(native.invoke).toHaveBeenCalledTimes(1);
});
