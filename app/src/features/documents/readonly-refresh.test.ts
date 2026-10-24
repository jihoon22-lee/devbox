import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { ReadOnly } from "../../rpc/gen/rpc";
import { keyId, memoryJournal } from "./offlineJournal";
import { useDocuments } from "./store";
import { FakeDocumentsDaemon } from "./test-daemon";

const key = { root: "vault", path: "a.md" };
const note = { kind: "note" } as const;
const reasons = ["binary", "encoding", "too_large"] as const;
let daemon: FakeDocumentsDaemon;
let stop: () => void;
const docs = () => useDocuments.getState();
const doc = () => docs().find(key);
const settle = () => docs().settled();
beforeEach(async () => {
  vi.useFakeTimers();
  daemon = new FakeDocumentsDaemon();
  daemon.write(key, "readable disk");
  stop = docs().connect(daemon, { journal: memoryJournal(), windowId: "main" });
  docs().open(key, note);
  await settle();
});
afterEach(() => {
  stop();
  vi.useRealTimers();
});
async function advance(ms = 1200) {
  await vi.advanceTimersByTimeAsync(ms);
  await settle();
}
async function unreadable(reason: ReadOnly["reason"]) {
  const rev = daemon.write(key, "external protected content " + reason);
  daemon.readOnlyFiles.set(keyId(key), { reason });
  daemon.emit("documents.changed", { key, rev });
  await settle();
  return rev;
}

test.each(reasons)("clean external %s refresh never makes an empty editable autosave buffer", async (reason) => {
  const base = doc()?.diskRev;
  await unreadable(reason);
  expect(doc()).toMatchObject({ state: "ReadOnly", readOnly: { reason }, diskRev: base });
  docs().edit(key, "typed replacement");
  docs().save(key);
  await advance();
  expect(daemon.count("documents.save")).toBe(0);
  expect(daemon.files.get(keyId(key))?.text).toBe("external protected content " + reason);
});

test.each(reasons)(
  "dirty %s refresh preserves the draft and requires explicit conflict replacement",
  async (reason) => {
    const base = doc()?.diskRev;
    docs().edit(key, "my unsaved draft");
    const rev = await unreadable(reason);
    await advance();
    expect(doc()).toMatchObject({
      state: "Conflict",
      text: "my unsaved draft",
      diskRev: base,
      disk: { rev, text: null, readOnly: { reason } },
    });
    expect(daemon.count("documents.save")).toBe(0);
    docs().resolve(key, "disk");
    await settle();
    expect(doc()?.state).toBe("Conflict");
    expect(daemon.files.get(keyId(key))?.text).toBe("external protected content " + reason);
    docs().resolve(key, "mine");
    await settle();
    expect(doc()).toMatchObject({ state: "Clean", text: "my unsaved draft" });
    expect(daemon.of("documents.save")[0]).toMatchObject({ baseRev: rev, text: "my unsaved draft" });
  },
);

test("file refusal survives lease loss; a later readable claim restores the actual text", async () => {
  await unreadable("binary");
  const leaseId = doc()?.leaseId ?? "missing";
  daemon.emit("documents.lease_lost", { key, leaseId });
  await settle();
  expect(doc()).toMatchObject({ state: "ReadOnly", readOnly: { reason: "binary" } });
  const before = daemon.count("documents.take");
  docs().take(key);
  await settle();
  expect(daemon.count("documents.take")).toBe(before);
  const rev = daemon.write(key, "readable again");
  await daemon.call("documents.take", { key, windowId: "other" });
  daemon.emit("documents.changed", { key, rev });
  await settle();
  expect(doc()).toMatchObject({ state: "ReadOnly", readOnly: { reason: "leased_elsewhere" }, text: "readable again" });
  docs().take(key);
  await settle();
  expect(doc()).toMatchObject({ state: "Clean", text: "readable again", diskRev: rev });
  expect(doc()?.diskReadOnly).toBeUndefined();
});

test("take cannot turn an unreadable claim into an empty editable document", async () => {
  const leaseId = doc()?.leaseId ?? "missing";
  await daemon.call("documents.take", { key, windowId: "other" });
  daemon.emit("documents.lease_lost", { key, leaseId });
  await settle();
  daemon.write(key, "unreadable after a missed change event");
  daemon.readOnlyFiles.set(keyId(key), { reason: "encoding" });
  docs().take(key);
  await settle();
  expect(doc()).toMatchObject({
    state: "ReadOnly",
    readOnly: { reason: "encoding" },
    diskReadOnly: { reason: "encoding" },
  });
  docs().edit(key, "replacement");
  await advance();
  expect(daemon.count("documents.save")).toBe(0);
});

test.each(["restore", "discard"] as const)(
  "unreadable refresh preserves recovery until explicit %s",
  async (choice) => {
    docs().close(key);
    await settle();
    const baseRev = daemon.files.get(keyId(key))?.rev ?? null;
    daemon.journals.set(keyId(key), { seq: 5, baseRev, text: "recovery candidate" });
    docs().open(key, note);
    await settle();
    expect(doc()?.state).toBe("Recovering");
    await unreadable("too_large");
    expect(doc()).toMatchObject({ state: "Recovering", recovery: { text: "recovery candidate" }, diskRev: baseRev });
    if (choice === "restore") docs().restore(key);
    else docs().discard(key);
    await advance();
    expect(doc()?.state).toBe(choice === "restore" ? "Conflict" : "ReadOnly");
    expect(daemon.files.get(keyId(key))?.text).toBe("external protected content too_large");
  },
);

test("a dirty draft survives file refusal, lease loss, readable refresh and take", async () => {
  const base = doc()?.diskRev;
  docs().edit(key, "my unsaved draft");
  await unreadable("encoding");
  const leaseId = doc()?.leaseId ?? "missing";
  daemon.emit("documents.lease_lost", { key, leaseId });
  await settle();
  expect(doc()).toMatchObject({
    state: "ReadOnly",
    text: "my unsaved draft",
    diskRev: base,
    readOnly: { reason: "encoding" },
  });
  const rev = daemon.write(key, "new readable external disk");
  await daemon.call("documents.take", { key, windowId: "other" });
  daemon.emit("documents.changed", { key, rev });
  await settle();
  expect(doc()).toMatchObject({
    state: "ReadOnly",
    text: "my unsaved draft",
    diskRev: base,
    readOnly: { reason: "leased_elsewhere" },
  });
  docs().take(key);
  await settle();
  docs().save(key);
  await settle();
  expect(doc()).toMatchObject({ state: "Conflict", text: "my unsaved draft", diskRev: base });
  expect(daemon.files.get(keyId(key))?.text).toBe("new readable external disk");
});

test("restoring the same readable bytes as the old base clears a later file refusal", async () => {
  await unreadable("binary");
  const rev = daemon.write(key, "readable disk");
  daemon.emit("documents.changed", { key, rev });
  await settle();
  expect(doc()).toMatchObject({ state: "Clean", text: "readable disk", diskRev: rev });
  expect(doc()?.diskReadOnly).toBeUndefined();
});

test("a failed readable re-claim remains noneditable and can recover on the next resync", async () => {
  await unreadable("binary");
  daemon.failing.set("documents.claim", "connection_lost");
  const rev = daemon.write(key, "readable after failure");
  daemon.emit("documents.changed", { key, rev });
  await settle();
  expect(doc()?.state).toBe("ReadOnly");
  daemon.failing.clear();
  daemon.resync();
  await settle();
  expect(doc()).toMatchObject({ state: "Clean", text: "readable after failure", diskRev: rev });
});

test("recovery keeps its draft while same-content readable restoration clears file refusal", async () => {
  docs().close(key);
  await settle();
  const baseRev = daemon.files.get(keyId(key))?.rev ?? null;
  daemon.journals.set(keyId(key), { seq: 5, baseRev, text: "recovery candidate" });
  docs().open(key, note);
  await settle();
  await unreadable("binary");
  const rev = daemon.write(key, "readable disk");
  daemon.emit("documents.changed", { key, rev });
  await settle();
  expect(doc()).toMatchObject({ state: "Recovering", recovery: { text: "recovery candidate" }, text: "readable disk" });
  expect(doc()?.diskReadOnly).toBeUndefined();
  expect(doc()?.disk).toBeUndefined();
  expect(daemon.journals.get(keyId(key))?.text).toBe("recovery candidate");
  docs().discard(key);
  await settle();
  expect(doc()).toMatchObject({ state: "Clean", text: "readable disk" });
  expect(daemon.files.get(keyId(key))?.text).toBe("readable disk");
});
