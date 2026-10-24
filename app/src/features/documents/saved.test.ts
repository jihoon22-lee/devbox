import { afterEach, expect, test, vi } from "vitest";
import { memoryJournal } from "./offlineJournal";
import { useDocuments } from "./store";
import { FakeDocumentsDaemon } from "./test-daemon";

const key = { root: "project:p", path: "a.ts" };
let cleanup = () => {};
afterEach(() => cleanup());
async function setup() {
  const daemon = new FakeDocumentsDaemon();
  daemon.write(key, "initial");
  cleanup = useDocuments.getState().connect(daemon, { journal: memoryJournal() });
  useDocuments.getState().open(key, { kind: "code" });
  await useDocuments.getState().settled();
  return daemon;
}
test("saved subscriptions receive ACK text A while buffer B stays dirty; exceptions and unsubscribe are isolated", async () => {
  await setup();
  const store = useDocuments.getState();
  const throwing = store.subscribeSaved(() => {
    throw new Error("private-body");
  });
  const listener = vi.fn();
  const off = store.subscribeSaved(listener);
  store.edit(key, "A");
  store.save(key);
  store.edit(key, "B");
  await store.settled();
  expect(listener).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ text: "A", seq: 1, leaseId: "L1" }));
  expect(store.find(key)).toMatchObject({ state: "Dirty", text: "B", savedSeq: 1, error: undefined });
  off();
  throwing();
  store.save(key);
  await store.settled();
  expect(listener).toHaveBeenCalledTimes(1);
});
test("failed save emits no ACK; clean orphan recreation still emits its unchanged sequence", async () => {
  const daemon = await setup();
  const store = useDocuments.getState();
  const listener = vi.fn();
  const off = store.subscribeSaved(listener);
  daemon.failing.set("documents.save", "internal");
  store.edit(key, "A");
  store.save(key);
  await store.settled();
  expect(listener).not.toHaveBeenCalled();
  daemon.failing.clear();
  store.save(key);
  await store.settled();
  listener.mockClear();
  daemon.files.clear();
  daemon.emit("documents.changed", { key, rev: null });
  store.save(key);
  await store.settled();
  expect(listener).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ text: "A", seq: 1 }));
  off();
});
