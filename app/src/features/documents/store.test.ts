import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { DocPolicy } from "../../rpc/gen/rpc";
import { keyId, memoryJournal, type OfflineJournal } from "./offlineJournal";
import { useDocuments } from "./store";
import { FakeDocumentsDaemon } from "./test-daemon";

const key = { root: "vault", path: "a.md" };
const note: DocPolicy = { kind: "note" };
let daemon: FakeDocumentsDaemon;
let journal: OfflineJournal;
let stop: () => void;
const docs = () => useDocuments.getState();
const doc = () => docs().find(key);
const settle = () => docs().settled();
async function advance(ms: number) {
  await vi.advanceTimersByTimeAsync(ms);
  await settle();
}

beforeEach(() => {
  vi.useFakeTimers();
  daemon = new FakeDocumentsDaemon();
  journal = memoryJournal();
  daemon.write(key, "disk");
  stop = docs().connect(daemon, { journal, windowId: "main" });
});
afterEach(() => {
  stop();
  vi.useRealTimers();
});

async function openClean() {
  docs().open(key, note);
  await settle();
  expect(doc()?.state).toBe("Clean");
}
/** One online edit journaled on the daemon (seq 1), then three offline edits that end in an offline draft (seq 4). */
async function editAcrossADisconnect() {
  await openClean();
  docs().edit(key, "disk1");
  await advance(300);
  expect(daemon.journals.size).toBe(1);
  daemon.disconnect();
  for (const text of ["disk12", "disk123", "disk1234"]) docs().edit(key, text);
  await advance(1000);
  expect(await journal.get(key)).toMatchObject({ kind: "draft", seq: 4, text: "disk1234" });
  daemon.write(key, "outside");
  daemon.reconnect();
  await settle();
  expect(doc()).toMatchObject({ state: "Conflict", text: "disk1234", disk: { text: "outside" } });
}
async function reopen() {
  docs().close(key);
  await settle();
  expect(doc()).toBeUndefined();
  docs().open(key, note);
  await settle();
  return doc();
}

test("autosave_journals_after_300ms_and_saves_after_1s", async () => {
  await openClean();
  expect(daemon.count("documents.claim")).toBe(1);
  docs().edit(key, "disk!");
  await advance(299);
  expect(daemon.count("documents.journal")).toBe(0);
  await advance(1);
  expect(daemon.of("documents.journal")).toEqual([
    expect.objectContaining({ seq: 1, text: "disk!", baseRev: expect.objectContaining({ size: 4 }) }),
  ]);
  await advance(700);
  expect(daemon.of("documents.save")).toEqual([expect.objectContaining({ seq: 1, text: "disk!", expect: "present" })]);
  expect(doc()?.state).toBe("Clean");
  expect(daemon.files.get(JSON.stringify(["vault", "a.md"]))?.text).toBe("disk!");
  expect(daemon.journals.size).toBe(0);
});

test("a document opened right after connect is claimed once", async () => {
  stop();
  stop = docs().connect(daemon, { journal, windowId: "main" });
  docs().open(key, note);
  await settle();
  expect(doc()?.state).toBe("Clean");
  expect(daemon.count("documents.claim")).toBe(1);
});

test("take_disk_also_clears_the_offline_journal_and_reconnect_does_not_resave", async () => {
  await editAcrossADisconnect();
  const saves = daemon.count("documents.save");
  docs().resolve(key, "disk");
  await settle();
  expect(doc()).toMatchObject({ state: "Clean", text: "outside" });
  expect(await journal.get(key)).toBeUndefined();
  expect(daemon.of("documents.journal_drop")).toEqual([{ key, uptoSeq: 4, leaseId: doc()?.leaseId }]);
  expect(daemon.journals.size).toBe(0);
  daemon.disconnect();
  daemon.reconnect();
  await advance(2000);
  expect(daemon.count("documents.save")).toBe(saves);
  expect((await reopen())?.state).toBe("Clean");
});

test("server_drop_failure_is_retried_from_a_tombstone", async () => {
  await editAcrossADisconnect();
  daemon.disconnect();
  docs().resolve(key, "disk");
  await settle();
  expect(daemon.of("documents.journal_drop")).toEqual([expect.objectContaining({ key, uptoSeq: 4 })]);
  expect(await journal.get(key)).toEqual({ kind: "tombstone", dropUpto: 4 });
  expect(daemon.journals.size).toBe(1);
  const before = daemon.calls.length;
  const saves = daemon.count("documents.save");
  daemon.reconnect();
  await advance(2000);
  const after = daemon.calls.slice(before).map((c) => c.method);
  expect(after.filter((m) => m === "documents.journal_drop")).toHaveLength(1);
  expect(after.indexOf("documents.journal_drop")).toBeLessThan(after.indexOf("documents.claim"));
  // The retry runs before any reclaim, so it carries no lease (nobody holds the document).
  expect(daemon.of("documents.journal_drop").at(-1)).toEqual({ key, uptoSeq: 4 });
  expect(daemon.count("documents.save")).toBe(saves);
  expect(await journal.get(key)).toBeUndefined();
  expect(daemon.journals.size).toBe(0);
  expect((await reopen())?.state).toBe("Clean");
});

test("recovering_discard_and_close_discard_clear_both_journals", async () => {
  // Recovering → discard, with a daemon journal (seq 2) and a newer offline draft (seq 3).
  daemon.journals.set(JSON.stringify(["vault", "a.md"]), { seq: 2, baseRev: null, text: "server draft" });
  await journal.put(key, { kind: "draft", seq: 3, baseRev: null, text: "offline draft" });
  docs().open(key, note);
  await settle();
  expect(doc()).toMatchObject({ state: "Recovering", recovery: { seq: 3, text: "offline draft" } });
  docs().discard(key);
  await settle();
  expect(daemon.of("documents.journal_drop")).toEqual([{ key, uptoSeq: 3, leaseId: doc()?.leaseId }]);
  expect(daemon.journals.size).toBe(0);
  expect(await journal.get(key)).toBeUndefined();
  expect((await reopen())?.state).toBe("Clean");

  // Dirty → close(discard), with a daemon journal (seq 1) and an offline draft (seq 2).
  docs().edit(key, "disk1");
  await advance(300);
  daemon.failing.set("documents.journal", "internal");
  docs().edit(key, "disk12");
  await advance(300);
  expect(await journal.get(key)).toMatchObject({ kind: "draft", seq: 2 });
  daemon.failing.clear();
  docs().close(key, "discard");
  await settle();
  expect(daemon.of("documents.journal_drop").at(-1)).toEqual(expect.objectContaining({ key, uptoSeq: 2 }));
  expect(daemon.count("documents.journal_drop")).toBe(2);
  expect(daemon.journals.size).toBe(0);
  expect(await journal.get(key)).toBeUndefined();
  expect(doc()).toBeUndefined();
  docs().open(key, note);
  await settle();
  expect(doc()).toMatchObject({ state: "Clean", text: "disk" });
});

test("lease_lost_topic_reclaims_and_goes_read_only_when_taken_elsewhere", async () => {
  await openClean();
  docs().edit(key, "mine");
  const leaseId = doc()?.leaseId ?? "";
  await daemon.call("documents.take", { key, windowId: "other" });
  daemon.emit("documents.lease_lost", { key, leaseId });
  await settle();
  expect(daemon.of("documents.claim").at(-1)).toMatchObject({ key, windowId: "main" });
  expect(daemon.count("documents.take")).toBe(1);
  expect(doc()).toMatchObject({ state: "ReadOnly", text: "mine", readOnly: { reason: "leased_elsewhere" } });
});

test("renew_every_20s_and_reclaims_an_expired_lease", async () => {
  await openClean();
  const first = doc()?.leaseId;
  await advance(20_000);
  expect(daemon.of("documents.renew")).toEqual([{ leaseIds: [first] }]);
  daemon.leases.clear();
  await advance(20_000);
  expect(doc()?.leaseId).toBeDefined();
  expect(doc()?.leaseId).not.toBe(first);
  expect(doc()?.state).toBe("Clean");
});

test("changed_and_moved_topics_reload_and_rekey", async () => {
  await openClean();
  const rev = daemon.write(key, "outside");
  daemon.emit("documents.changed", { key, rev });
  await settle();
  expect(doc()).toMatchObject({ state: "Clean", text: "outside", diskRev: rev });
  const to = { root: "vault", path: "b.md" };
  const id = doc()?.id;
  daemon.emit("documents.moved", { from: key, to });
  await settle();
  expect(doc()).toBeUndefined();
  expect(docs().find(to)).toMatchObject({ id, key: to, state: "Clean" });
});

test("read_only_window_close_keeps_the_holders_journals", async () => {
  await openClean();
  docs().edit(key, "mine");
  await advance(300);
  const other = await daemon.call("documents.take", { key, windowId: "other" });
  await daemon.call("documents.journal", { leaseId: other.leaseId ?? "", seq: 7, baseRev: other.rev, text: "theirs" });
  daemon.emit("documents.lease_lost", { key, leaseId: "L1" });
  await settle();
  expect(doc()?.state).toBe("ReadOnly");
  docs().close(key, "discard");
  await settle();
  expect(doc()).toBeUndefined();
  expect(daemon.count("documents.journal_drop")).toBe(0);
  expect(daemon.journals.get(keyId(key))).toMatchObject({ seq: 7, text: "theirs" });
  expect(await journal.list()).toEqual([]);
  // Even a lease-less drop (an old tombstone) cannot remove the holder's journal.
  await expect(daemon.call("documents.journal_drop", { key, uptoSeq: 9 })).rejects.toMatchObject({
    code: "documents.lease_lost",
  });
});

test("folder_move_rekeys_every_open_document_and_its_offline_entry", async () => {
  const at = (path: string) => ({ root: "vault", path });
  for (const path of ["notes/a.md", "notes/sub/b.md", "notes2/c.md"]) {
    daemon.write(at(path), path);
    docs().open(at(path), note);
  }
  await settle();
  daemon.failing.set("documents.journal", "internal");
  docs().edit(at("notes/a.md"), "notes/a.md!");
  await advance(300);
  daemon.failing.clear();
  expect(await journal.get(at("notes/a.md"))).toMatchObject({ kind: "draft", text: "notes/a.md!" });
  daemon.move(at("notes"), at("archive"));
  await settle();
  expect(docs().find(at("notes/a.md"))).toBeUndefined();
  expect(docs().find(at("archive/a.md"))).toMatchObject({ state: "Dirty", text: "notes/a.md!" });
  expect(docs().find(at("archive/sub/b.md"))).toMatchObject({ state: "Clean" });
  expect(docs().find(at("notes2/c.md"))).toMatchObject({ state: "Clean" });
  expect((await journal.list()).map((e) => e.key)).toEqual([at("archive/a.md")]);
  daemon.resync();
  await advance(2000);
  for (const path of ["archive/a.md", "archive/sub/b.md", "notes2/c.md"])
    expect(docs().find(at(path))?.state).toBe("Clean");
  expect(daemon.files.get(keyId(at("archive/a.md")))?.text).toBe("notes/a.md!");
  expect(await journal.list()).toEqual([]);
});

test("take_here_offers_the_other_windows_draft_before_editing", async () => {
  const other = await daemon.call("documents.claim", { key, policy: note, windowId: "other" });
  await daemon.call("documents.journal", { leaseId: other.leaseId ?? "", seq: 3, baseRev: other.rev, text: "theirs" });
  docs().open(key, note);
  await settle();
  expect(doc()?.state).toBe("ReadOnly");
  docs().take(key);
  await settle();
  expect(doc()).toMatchObject({ state: "Recovering", text: "disk", recovery: { text: "theirs" } });
  await advance(2000);
  expect(daemon.journals.get(keyId(key))).toMatchObject({ seq: 3, text: "theirs" });
  docs().restore(key);
  await advance(300);
  expect(daemon.journals.get(keyId(key))).toMatchObject({ text: "theirs" });
  expect(daemon.journals.get(keyId(key))?.seq).toBeGreaterThan(3);
});
