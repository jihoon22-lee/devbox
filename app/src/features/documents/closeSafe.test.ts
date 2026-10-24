import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { keyId, memoryJournal, type OfflineJournal } from "./offlineJournal";
import { useDocuments } from "./store";
import { FakeDocumentsDaemon } from "./test-daemon";

const key = { root: "project:p", path: "code.ts" };
let daemon: FakeDocumentsDaemon, journal: OfflineJournal, stop: () => void;
const docs = () => useDocuments.getState();
beforeEach(async () => {
  daemon = new FakeDocumentsDaemon();
  journal = memoryJournal();
  daemon.write(key, "disk");
  stop = docs().connect(daemon, { journal, windowId: "main" });
  docs().open(key, { kind: "code" });
  await docs().settled();
});
afterEach(() => {
  stop();
  vi.restoreAllMocks();
});

test("code does not autosave or silently save when a window closes without a choice", async () => {
  docs().edit(key, "draft");
  expect(await docs().prepareWindowClose()).toBe(false);
  expect(daemon.count("documents.save")).toBe(0);
  expect(docs().find(key)?.text).toBe("draft");
  expect(docs().windowClosing).toBe(false);
});
test("keep awaits durable journal acknowledgement before removing the tab", async () => {
  docs().edit(key, "latest draft");
  daemon.disconnect();
  let release!: () => void;
  const original = journal.putIf.bind(journal);
  vi.spyOn(journal, "putIf").mockImplementation(async (...args) => {
    await new Promise<void>((resolve) => {
      release = resolve;
    });
    return original(...args);
  });
  const closing = docs().closeSafely(key, "keep");
  await vi.waitFor(() => expect(release).toBeTypeOf("function"));
  expect(docs().find(key)?.text).toBe("latest draft");
  release();
  expect(await closing).toBe(true);
  expect(docs().find(key)).toBeUndefined();
  expect(await journal.get(key)).toMatchObject({ kind: "draft", text: "latest draft" });
  expect(daemon.files.get(keyId(key))?.text).toBe("disk");
});
test("keep fails safely when both journal destinations reject writes", async () => {
  docs().edit(key, "must stay");
  vi.spyOn(journal, "putIf").mockRejectedValue(new Error("fixture disk full"));
  daemon.failing.set("documents.journal", "internal");
  expect(await docs().closeSafely(key, "keep")).toBe(false);
  expect(docs().find(key)?.text).toBe("must stay");
  expect(daemon.count("documents.save")).toBe(0);
});
test("discard requires daemon drop acknowledgement and keeps the buffer on failure", async () => {
  docs().edit(key, "must stay");
  daemon.failing.set("documents.journal_drop", "internal");
  expect(await docs().closeSafely(key, "discard")).toBe(false);
  expect(docs().find(key)?.text).toBe("must stay");
  daemon.failing.delete("documents.journal_drop");
  expect(await docs().closeSafely(key, "discard")).toBe(true);
  expect(await journal.get(key)).toBeUndefined();
});
test("save close waits for the older write and then saves the latest buffer", async () => {
  const original = daemon.call.bind(daemon);
  let release!: () => void;
  let first = true;
  vi.spyOn(daemon, "call").mockImplementation(async (method, params) => {
    if (method === "documents.save" && first) {
      first = false;
      await new Promise<void>((resolve) => {
        release = resolve;
      });
    }
    return original(method, params);
  });
  docs().edit(key, "first");
  docs().save(key);
  docs().edit(key, "second");
  const closing = docs().closeSafely(key, "save");
  expect(docs().find(key)?.text).toBe("second");
  release();
  expect(await closing).toBe(true);
  expect(daemon.of("documents.save").map((p) => p.text)).toEqual(["first", "second"]);
  expect(daemon.files.get(keyId(key))?.text).toBe("second");
});
test("native cancel while choosing leaves the code buffer and makes no save", async () => {
  docs().edit(key, "unapproved");
  let choosing = false;
  const close = docs().prepareWindowClose(async (_items, signal) => {
    choosing = true;
    return new Promise((resolve) => signal.addEventListener("abort", () => resolve(undefined), { once: true }));
  });
  await vi.waitFor(() => expect(choosing).toBe(true));
  docs().abortWindowClose();
  expect(await close).toBe(false);
  expect(daemon.count("documents.save")).toBe(0);
  expect(docs().find(key)?.text).toBe("unapproved");
});
test("failed save never closes or drops a recoverable draft", async () => {
  docs().edit(key, "draft");
  daemon.failing.set("documents.save", "documents.conflict");
  expect(await docs().closeSafely(key, "save")).toBe(false);
  expect(docs().find(key)?.text).toBe("draft");
  expect(daemon.count("documents.journal_drop")).toBe(0);
});

test("clean code windows also wait for a successful lease release", async () => {
  daemon.failing.set("documents.release", "internal");
  expect(await docs().prepareWindowClose()).toBe(false);
  expect(docs().find(key)?.text).toBe("disk");
});
test("an untouched code document whose claim failed can close safely", async () => {
  const missing = { ...key, path: "missing.ts" };
  daemon.failing.set("documents.claim", "files.not_found");
  docs().open(missing, { kind: "code" });
  await docs().settled();
  expect(docs().find(missing)?.state).toBe("Loading");
  expect(await docs().prepareWindowClose()).toBe(true);
  expect(daemon.count("documents.save")).toBe(0);
});
test("code journaling never becomes a one-second autosave", async () => {
  vi.useFakeTimers();
  try {
    docs().edit(key, "manual only");
    await vi.advanceTimersByTimeAsync(1500);
    await docs().settled();
    expect(daemon.count("documents.journal")).toBe(1);
    expect(daemon.count("documents.save")).toBe(0);
  } finally {
    vi.useRealTimers();
  }
});

test("a lost lease cannot keep over or discard the new owner's shared local journal", async () => {
  docs().edit(key, "old window buffer");
  const leaseId = docs().find(key)!.leaseId!;
  await daemon.call("documents.take", { key, windowId: "other", policy: { kind: "code" } });
  daemon.emit("documents.lease_lost", { key, leaseId });
  await docs().settled();
  expect(docs().find(key)?.state).toBe("ReadOnly");
  const other = {
    kind: "draft" as const,
    seq: docs().find(key)!.editSeq,
    baseRev: docs().find(key)!.diskRev,
    text: "new owner offline draft",
  };
  await journal.put(key, other);
  expect(await docs().closeSafely(key, "keep")).toBe(false);
  expect(await journal.get(key)).toEqual(other);
  expect(await docs().closeSafely(key, "discard")).toBe(true);
  expect(await journal.get(key)).toEqual(other);
});

test("a lease transfer during the keep acknowledgement leaves the old buffer open", async () => {
  docs().edit(key, "old buffer");
  const oldLease = docs().find(key)!.leaseId!;
  const call = daemon.call.bind(daemon);
  vi.spyOn(daemon, "call").mockImplementation(async (method, params) => {
    const reply = await call(method, params);
    if (method === "documents.journal_read") {
      await call("documents.take", { key, windowId: "new owner", policy: { kind: "code" } });
      daemon.emit("documents.lease_lost", { key, leaseId: oldLease });
      await docs().settled();
    }
    return reply;
  });
  expect(await docs().closeSafely(key, "keep")).toBe(false);
  expect(docs().find(key)?.text).toBe("old buffer");
});
