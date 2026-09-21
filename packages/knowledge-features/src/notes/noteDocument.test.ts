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
