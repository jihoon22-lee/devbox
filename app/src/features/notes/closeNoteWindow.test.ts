import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { Methods } from "../../rpc/gen/rpc";
import { keyId, memoryJournal } from "../documents/offlineJournal";
import { useDocuments } from "../documents/store";
import { FakeDocumentsDaemon } from "../documents/test-daemon";
import { noteCloseHandler } from "./closeNoteWindow";

const key = { root: "vault", path: "a.md" };
const docs = () => useDocuments.getState();
let stop: () => void;
let daemon: PausedDaemon;
const destroy = vi.fn(async () => {});
const blocked = vi.fn();
const event = () => ({ preventDefault: vi.fn() });
let gate: Promise<void> | undefined;
let gatedMethod: string;
let entered = () => {};
class PausedDaemon extends FakeDocumentsDaemon {
  override async call<M extends keyof Methods>(method: M, params: Methods[M]["params"]): Promise<Methods[M]["result"]> {
    if (method === gatedMethod) {
      entered();
      await gate;
    }
    return super.call(method, params);
  }
}
const turns = async () => {
  for (let i = 0; i < 10; i++) await Promise.resolve();
};
beforeEach(async () => {
  vi.useFakeTimers();
  vi.clearAllMocks();
  gate = undefined;
  gatedMethod = "documents.save";
  entered = () => {};
  daemon = new PausedDaemon();
  daemon.write(key, "disk");
  stop = docs().connect(daemon, { journal: memoryJournal(), windowId: "popout" });
  docs().open(key, { kind: "note" });
  await docs().settled();
});
afterEach(() => {
  stop();
  vi.useRealTimers();
});

test("native close 100ms after input waits for actual latest save, not debounce or Closed", async () => {
  docs().edit(key, "latest note");
  await vi.advanceTimersByTimeAsync(100);
  let release = () => {};
  gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  const close = noteCloseHandler(destroy, blocked);
  const request = event();
  const closing = close(request);
  expect(request.preventDefault).toHaveBeenCalledOnce();
  await turns();
  expect(destroy).not.toHaveBeenCalled();
  expect(daemon.files.get(keyId(key))?.text).toBe("disk");
  release();
  await closing;
  expect(daemon.files.get(keyId(key))?.text).toBe("latest note");
  expect(destroy).toHaveBeenCalledOnce();
  expect(blocked).not.toHaveBeenCalled();
  expect(docs().find(key)).toBeUndefined();
});

test("close while older save is pending persists the later buffer before destruction", async () => {
  let release = () => {};
  gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  docs().edit(key, "first");
  docs().save(key);
  await turns();
  docs().edit(key, "second latest");
  const close = noteCloseHandler(destroy, blocked);
  const closing = close(event());
  await turns();
  expect(destroy).not.toHaveBeenCalled();
  release();
  await closing;
  expect(daemon.of("documents.save").map((row) => row.text)).toEqual(["first", "second latest"]);
  expect(daemon.files.get(keyId(key))?.text).toBe("second latest");
  expect(destroy).toHaveBeenCalledOnce();
});

test.each(["connection_lost", "documents.conflict", "documents.permission_denied"])(
  "failed save %s keeps the latest buffer visible",
  async (code) => {
    docs().edit(key, "must survive");
    daemon.failing.set("documents.save", code);
    await noteCloseHandler(destroy, blocked)(event());
    expect(destroy).not.toHaveBeenCalled();
    expect(blocked).toHaveBeenCalledOnce();
    expect(docs().find(key)?.text).toBe("must survive");
    expect(docs().windowClosing).toBe(false);
  },
);

test("a lease taken by another window cannot silently discard this unsaved buffer", async () => {
  docs().edit(key, "lease lost draft");
  docs().dispatch(docs().find(key)?.id ?? "missing", { type: "leaseLost", reason: "taken" });
  await noteCloseHandler(destroy, blocked)(event());
  expect(destroy).not.toHaveBeenCalled();
  expect(docs().find(key)?.text).toBe("lease lost draft");
  expect(docs().find(key)?.state).toBe("ReadOnly");
});

test("repeated close requests remain prevented while one save is pending", async () => {
  let release = () => {};
  gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  docs().edit(key, "draft");
  const close = noteCloseHandler(destroy, blocked);
  const first = close(event());
  const second = event();
  await close(second);
  expect(second.preventDefault).toHaveBeenCalledOnce();
  expect(destroy).not.toHaveBeenCalled();
  release();
  await first;
  expect(destroy).toHaveBeenCalledOnce();
});

test("native destruction failure restores editing without claiming closure", async () => {
  destroy.mockRejectedValueOnce(new Error("window unavailable"));
  await noteCloseHandler(destroy, blocked)(event());
  await docs().settled();
  expect(blocked).toHaveBeenCalledOnce();
  expect(docs().windowClosing).toBe(false);
  expect(docs().find(key)).toMatchObject({ state: "Clean", text: "disk" });
});

test("a removed store document is not permission to destroy before release effects settle", async () => {
  gatedMethod = "documents.release";
  const reachedRelease = new Promise<void>((resolve) => {
    entered = resolve;
  });
  let release = () => {};
  gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  docs().edit(key, "durable before release");
  const closing = noteCloseHandler(destroy, blocked)(event());
  await reachedRelease;
  expect(docs().find(key)).toBeUndefined();
  expect(destroy).not.toHaveBeenCalled();
  expect(daemon.files.get(keyId(key))?.text).toBe("durable before release");
  release();
  await closing;
  expect(destroy).toHaveBeenCalledOnce();
});
