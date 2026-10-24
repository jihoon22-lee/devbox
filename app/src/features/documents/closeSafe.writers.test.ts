import { afterEach, beforeEach, expect, test } from "vitest";
import { durableClose } from "./durableClose";
import { memoryJournal, type OfflineJournal } from "./offlineJournal";
import { type DocumentsClient, useDocuments } from "./store";
import { FakeDocumentsDaemon } from "./test-daemon";

const key = { root: "project:p", path: "a.ts" };
let daemon: FakeDocumentsDaemon, journal: OfflineJournal, stop: () => void;
const docs = () => useDocuments.getState();
beforeEach(async () => {
  daemon = new FakeDocumentsDaemon();
  journal = memoryJournal();
  daemon.write(key, "disk");
  stop = docs().connect(daemon, { journal, windowId: "old-owner" });
  docs().open(key, { kind: "code" });
  await docs().settled();
  docs().edit(key, "old buffer 1");
  docs().edit(key, "old buffer 2");
});
afterEach(() => stop());
async function keepOtherDraft(seq = 1) {
  // The other renderer has its own disconnected transport. Its close path runs the
  // real durableClose implementation over the same device journal.
  const offlineClient = { state: "reconnecting" } as DocumentsClient;
  const other = {
    ...docs().find(key)!,
    editSeq: seq,
    savedSeq: 0,
    journalSeq: 0,
    leaseId: "new-owner-lease",
    text: "new owner's only durable draft",
  };
  expect(await durableClose(offlineClient, journal).keep(other)).toBe(true);
  const saved = await journal.get(key);
  expect(saved).toMatchObject({ text: other.text, seq });
  return saved;
}
test("review: discard with stale lease must leave another window's acknowledged local draft intact", async () => {
  // Same daemon take, but deliberately delay lease_lost topic delivery to old owner.
  await daemon.call("documents.take", { key, windowId: "new-owner", policy: { kind: "code" } });
  const saved = await keepOtherDraft();
  expect(await docs().closeSafely(key, "discard")).toBe(false);
  expect(docs().find(key)?.text).toBe("old buffer 2");
  expect(await journal.get(key)).toEqual(saved);
  daemon.disconnect();
  daemon.reconnect();
  await docs().settled();
  expect(await journal.get(key)).toEqual(saved);
  stop();
  daemon.disconnect();
  daemon.reconnect();
  stop = docs().connect(daemon, { journal, windowId: "recovery-window" });
  docs().open(key, { kind: "code" });
  await docs().settled();
  expect(docs().find(key)?.recovery?.text).toBe("new owner's only durable draft");
});
test.each([1, 2])("offline keep cannot replace another owner at seq %s", async (seq) => {
  daemon.disconnect();
  const saved = await keepOtherDraft(seq);
  const closed = await docs().closeSafely(key, "keep");
  expect(await journal.get(key)).toEqual(saved);
  expect(closed).toBe(false);
});

test("one offline writer can update its own acknowledged draft", async () => {
  const writer = durableClose({ state: "reconnecting" } as DocumentsClient, journal);
  const doc = docs().find(key)!;
  expect(await writer.keep(doc)).toBe(true);
  expect(await writer.keep({ ...doc, editSeq: 3, text: "next own draft" })).toBe(true);
  expect(await journal.get(key)).toMatchObject({ seq: 3, text: "next own draft" });
});

test("two concurrent independent offline writers cannot acknowledge different drafts", async () => {
  const client = { state: "reconnecting" } as DocumentsClient;
  const doc = docs().find(key)!;
  const results = await Promise.all([
    durableClose(client, journal).keep({ ...doc, text: "first writer" }),
    durableClose(client, journal).keep({ ...doc, text: "second writer" }),
  ]);
  expect(results.filter(Boolean)).toHaveLength(1);
  expect(await journal.get(key)).toMatchObject({ text: results[0] ? "first writer" : "second writer" });
});

test.each(["keep", "discard", "background"] as const)(
  "reconnect cannot authorize replacing an unoffered foreign draft via %s",
  async (action) => {
    daemon.disconnect();
    const saved = await keepOtherDraft();
    daemon.reconnect();
    await docs().settled();
    expect(docs().find(key)?.text).toBe("old buffer 2");
    if (action === "discard") {
      expect(await docs().closeSafely(key, "discard")).toBe(false);
    } else {
      daemon.disconnect();
      if (action === "keep") expect(await docs().closeSafely(key, "keep")).toBe(false);
      else {
        docs().dispatch(docs().find(key)!.id, { type: "journalTick" });
        await docs().settled();
      }
    }
    expect(await journal.get(key)).toEqual(saved);
    expect(docs().find(key)?.text).toBe("old buffer 2");
  },
);

test("reconnect keeps ownership of its own offline snapshot", async () => {
  daemon.disconnect();
  docs().dispatch(docs().find(key)!.id, { type: "journalTick" });
  await docs().settled();
  expect(await journal.get(key)).toMatchObject({ text: "old buffer 2" });
  daemon.reconnect();
  await docs().settled();
  daemon.disconnect();
  docs().edit(key, "own next edit");
  expect(await docs().closeSafely(key, "keep")).toBe(true);
  expect(await journal.get(key)).toMatchObject({ text: "own next edit" });
});
