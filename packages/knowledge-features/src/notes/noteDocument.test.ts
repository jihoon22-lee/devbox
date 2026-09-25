import { describe, expect, it, vi } from "vitest";
import { NoteDocument } from "./noteDocument";
import type { NoteSnapshot } from "./api";
function deferred<T>() { let resolve!: (value: T) => void; let reject!: (cause: unknown) => void; const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; }); return { promise, resolve, reject }; }
const disk = (content: string | null, revision = "disk-1"): NoteSnapshot => ({ content, revision });
async function fixture() {
  const read = vi.fn(async () => disk("original"));
  const write = vi.fn(async (_path: string, content: string, _revision: string) => disk(content, "disk-2"));
  const note = new NoteDocument(read, write);
  await note.openPath("A.md", () => true);
  return { note, read, write };
}
describe("note editor ordering and native revisions", () => {
  it("keeps edits made during save dirty and serializes duplicate saves", async () => {
    const { note, write } = await fixture(); const pending = deferred<NoteSnapshot>();
    write.mockReturnValueOnce(pending.promise);
    note.edit("A"); const first = note.save(); const duplicate = note.save();
    expect(duplicate).toBe(first);
    note.edit("B"); pending.resolve(disk("A", "saved-A"));
    expect(await first).toBe(false);
    expect(note.snapshot()).toMatchObject({ content: "B", dirty: true, revision: "saved-A" });
    expect(write).toHaveBeenCalledTimes(1);
    await note.save();
    expect(write).toHaveBeenLastCalledWith("A.md", "B", "saved-A");
    expect(note.snapshot().dirty).toBe(false);
  });
  it("does not clear another document when an earlier save finishes", async () => {
    const { note, write } = await fixture(); const pending = deferred<NoteSnapshot>();
    write.mockReturnValueOnce(pending.promise); note.edit("A saved"); const save = note.save();
    await note.openPath("B.md", () => true); note.edit("B unsaved");
    pending.resolve(disk("A saved", "saved-A")); await save;
    expect(note.snapshot()).toMatchObject({ path: "B.md", content: "B unsaved", dirty: true, revision: "disk-1" });
  });
  it("applies only the latest open even when responses arrive in reverse", async () => {
    const { note, read } = await fixture(); const first = deferred<NoteSnapshot>(), second = deferred<NoteSnapshot>();
    read.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
    const a = note.openPath("first.md", () => true), b = note.openPath("latest.md", () => true);
    second.resolve(disk("latest")); await b; first.resolve(disk("old")); await a;
    expect(note.snapshot()).toMatchObject({ path: "latest.md", content: "latest" });
  });
  it("protects edits made while an inbound link is read, even if they were saved", async () => {
    const { note } = await fixture(); const pending = deferred<{ path: string; content: string; revision: string }>();
    const opening = note.open(() => pending.promise, () => true);
    note.edit("typed while waiting"); await note.save();
    pending.resolve({ path: "linked.md", content: "incoming", revision: "link" });
    expect(await opening).toBe(false);
    expect(note.snapshot()).toMatchObject({ path: "A.md", content: "typed while waiting" });
  });
  it("protects a draft without a watcher event and requires the reviewed disk revision to overwrite", async () => {
    const { note, read, write } = await fixture();
    note.edit("my draft"); read.mockResolvedValue(disk("external", "external-1"));
    write.mockRejectedValueOnce(Object.assign(new Error("conflict"), { name: "note_conflict" }));
    expect(await note.save()).toBe(false);
    expect(note.snapshot()).toMatchObject({ content: "my draft", dirty: true, conflict: disk("external", "external-1") });
    expect(await note.saveBeforeQuit()).toBe(false);
    await note.save("external-1");
    expect(write).toHaveBeenLastCalledWith("A.md", "my draft", "external-1");
  });
  it("shows external deletion and never automatically reloads a dirty buffer", async () => {
    const { note, read } = await fixture(); note.edit("keep");
    read.mockResolvedValue(disk(null, "missing")); await note.inspect();
    expect(note.snapshot()).toMatchObject({ content: "keep", dirty: true, conflict: disk(null, "missing") });
    expect(await note.openPath("A.md", () => false)).toBe(false);
    expect(note.snapshot().content).toBe("keep");
  });
  it("quit waits for the old save then saves the newest edit, and fails closed on write errors", async () => {
    const { note, write } = await fixture(); const pending = deferred<NoteSnapshot>();
    write.mockReturnValueOnce(pending.promise); note.edit("old"); void note.save(); note.edit("new");
    const quitting = note.saveBeforeQuit(); pending.resolve(disk("old", "saved-old"));
    expect(await quitting).toBe(true);
    expect(write).toHaveBeenLastCalledWith("A.md", "new", "saved-old");
    note.edit("not saved"); write.mockRejectedValueOnce(new Error("disk full"));
    expect(await note.saveBeforeQuit()).toBe(false); expect(note.snapshot().dirty).toBe(true);
  });
});


it("does not let older inspections clear conflicts or overwrite newer errors", async () => {
  const { note, read } = await fixture();
  const old = deferred<NoteSnapshot>(), latest = deferred<NoteSnapshot>();
  read.mockReturnValueOnce(old.promise).mockReturnValueOnce(latest.promise);
  const a = note.inspect(), b = note.inspect();
  latest.resolve(disk("external", "r2")); await b;
  old.resolve(disk("original")); await a;
  expect(note.snapshot().conflict).toEqual(disk("external", "r2"));
  const staleFailure = deferred<NoteSnapshot>();
  read.mockReturnValueOnce(staleFailure.promise);
  const failing = note.inspect(); await note.inspect();
  staleFailure.reject(new Error("old failure")); await failing;
  expect(note.snapshot().error).toBeNull();
});
it("invalidates inspect across A-B-A, rename and a completed save", async () => {
  for (const action of ["switch", "rename", "save"]) {
    const { note, read } = await fixture(); const old = deferred<NoteSnapshot>();
    read.mockReturnValueOnce(old.promise); const inspection = note.inspect();
    if (action === "switch") { await note.openPath("B.md", () => true); await note.openPath("A.md", () => true); }
    if (action === "rename") await note.renamed("A.md", "renamed.md");
    if (action === "save") { note.edit("saved"); await note.save(); }
    old.resolve(disk("obsolete", "old")); await inspection;
    expect(note.snapshot().conflict).toBeNull();
  }
});


describe("approved deletion ownership", () => {
  it("clears only the approved buffer and consumes completion once", async () => {
    const { note } = await fixture();
    const complete = note.approveRemoval("A.md");
    expect(complete()).toBe(true);
    await note.openPath("A.md", () => true); note.edit("new buffer");
    expect(complete()).toBe(false);
    expect(note.snapshot().content).toBe("new buffer");
  });
  it.each([false, true])("preserves another document or a reopened original (%s)", async reopen => {
    const { note } = await fixture(); const complete = note.approveRemoval("A.md");
    await note.openPath("B.md", () => true);
    if (reopen) await note.openPath("A.md", () => true);
    note.edit("keep unsaved");
    expect(complete()).toBe(false);
    expect(note.snapshot()).toMatchObject({ content: "keep unsaved", dirty: true });
  });
  it("preserves edits after approval including edits saved before completion", async () => {
    const { note } = await fixture(); const complete = note.approveRemoval("A.md");
    note.edit("new saved text"); await note.save();
    expect(complete()).toBe(false);
    expect(note.snapshot()).toMatchObject({ path: "A.md", content: "new saved text", dirty: true });
  });
  it.each([false, true])("preserves a save overlapping deletion (%s)", async finishSaveFirst => {
    const { note, write } = await fixture(); const pending = deferred<NoteSnapshot>();
    write.mockReturnValueOnce(pending.promise); note.edit("submitted");
    const saving = note.save(); const complete = note.approveRemoval("A.md");
    if (finishSaveFirst) { pending.resolve(disk("submitted", "saved")); await saving; }
    expect(complete()).toBe(false);
    expect(note.snapshot().content).toBe("submitted");
    if (!finishSaveFirst) { pending.resolve(disk("submitted", "saved")); await saving; }
    expect(note.snapshot()).toMatchObject({ path: "A.md", dirty: true });
  });
  it("allows an already requested open to finish after deletion", async () => {
    const { note, read } = await fixture(); const pending = deferred<NoteSnapshot>();
    const complete = note.approveRemoval("A.md"); read.mockReturnValueOnce(pending.promise);
    const opening = note.openPath("B.md", () => true);
    expect(complete()).toBe(false);
    pending.resolve(disk("B")); expect(await opening).toBe(true);
    expect(note.snapshot().path).toBe("B.md");
  });
  it("handles directory boundaries without clearing unrelated notes", async () => {
    const { note } = await fixture();
    expect(note.approveRemoval("A")()).toBe(false);
    await note.openPath("Notes/A.md", () => true);
    expect(note.approveRemoval("Notes")()).toBe(true);
  });
});

it.each(["appliedWithConflict", "unknown"] as const)("retains the draft and blocks successful quit for a %s write", async state => {
  const {note, write} = await fixture(); note.edit("my unsaved draft");
  write.mockResolvedValueOnce({content:null,revision:"",saveOutcome:{state,recoveryDirectory:".devbox-save-fixture",warning:"note_commit_unknown"}});
  expect(await note.saveBeforeQuit()).toBe(false);
  expect(note.snapshot()).toMatchObject({content:"my unsaved draft",dirty:true,revision:"disk-1"});
  expect(note.snapshot().error).toContain(".devbox-save-fixture");
});
it("records a committed write with a durability warning as saved", async () => {
  const {note, write} = await fixture(); note.edit("committed");
  write.mockResolvedValueOnce({content:"committed",revision:"saved",saveOutcome:{state:"applied",recoveryDirectory:null,warning:"note_durability_warning"}});
  expect(await note.save()).toBe(true);
  expect(note.snapshot()).toMatchObject({dirty:false,revision:"saved"});
  expect(note.snapshot().error).toContain("파일은 반영");
});

it("versions preview sources across identical reopenings and edits without changing on save status", async () => {
  const { note } = await fixture();
  const original = note.snapshot().sourceVersion;
  await note.openPath("B.md", () => true);
  await note.openPath("A.md", () => true);
  expect(note.snapshot().sourceVersion).toBeGreaterThan(original);
  const reopened = note.snapshot().sourceVersion;
  note.edit("changed"); note.edit("original");
  expect(note.snapshot().sourceVersion).toBeGreaterThan(reopened);
  const edited = note.snapshot().sourceVersion;
  await note.save(); await note.inspect();
  expect(note.snapshot().sourceVersion).toBe(edited);
  await note.openPath("A.md", () => true);
  expect(note.snapshot().sourceVersion).toBeGreaterThan(edited);
});

  it("saves through the switch hook before asking to discard", async () => {
    const { note, write } = await fixture();
    const discard = vi.fn(() => false);
    note.setBeforeSwitch(() => note.save());
    note.edit("changed");
    expect(await note.openPath("B.md", discard)).toBe(true);
    expect(write).toHaveBeenCalledWith("A.md", "changed", "disk-1");
    expect(discard).not.toHaveBeenCalled();
  });
it("preserves the newest open intent while pre-switch saves are waiting", async () => {
  const {note} = await fixture(), older=deferred<void>(), newer=deferred<void>();
  note.edit("dirty"); note.setBeforeSwitch(vi.fn().mockReturnValueOnce(older.promise).mockReturnValueOnce(newer.promise));
  const first=note.open(async()=>({path:"first.md",content:"first",revision:"r"}),()=>true);
  const second=note.open(async()=>({path:"second.md",content:"second",revision:"r"}),()=>true);
  newer.resolve(); expect(await second).toBe(true); older.resolve(); expect(await first).toBe(false);
  expect(note.snapshot().path).toBe("second.md");
});
