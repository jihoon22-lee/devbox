import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { NoteJournal } from "./journal";
import type { NoteView } from "./noteDocument";

class FakeDocument {
  view: NoteView = {
    sourceVersion: 0,
    path: "a.md",
    content: "saved",
    revision: "r1",
    dirty: false,
    saving: false,
    conflict: null,
    error: null,
  };
  listeners = new Set<() => void>();
  snapshot = () => this.view;
  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };
  set(change: Partial<NoteView>) {
    this.view = {
      ...this.view,
      ...change,
      sourceVersion: this.view.sourceVersion + ("content" in change || "path" in change ? 1 : 0),
    };
    this.listeners.forEach((listener) => listener());
  }
}
function deferred() {
  let resolve!: () => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<void>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
function fixture() {
  const doc = new FakeDocument();
  const api = { save: vi.fn().mockResolvedValue(undefined), clear: vi.fn().mockResolvedValue(undefined) };
  const error = vi.fn();
  return { doc, api, error, journal: new NoteJournal(doc, api, error) };
}
describe("ordered recovery journal", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());
  it("records the latest input once typing pauses", async () => {
    const { doc, api, journal } = fixture();
    doc.set({ content: "a", dirty: true });
    await vi.advanceTimersByTimeAsync(500);
    doc.set({ content: "ab", dirty: true });
    await vi.advanceTimersByTimeAsync(999);
    expect(api.save).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);
    expect(api.save).toHaveBeenCalledExactlyOnceWith("a.md", "ab", "r1");
    journal.dispose();
  });
  it("finishes the record before clearing a note already saved to disk", async () => {
    const { doc, api, journal } = fixture(),
      writing = deferred();
    api.save.mockReturnValueOnce(writing.promise);
    doc.set({ content: "draft", dirty: true });
    await vi.advanceTimersByTimeAsync(1000);
    doc.set({ dirty: false, revision: "r2" });
    await vi.advanceTimersByTimeAsync(0);
    expect(api.clear).not.toHaveBeenCalled();
    writing.resolve();
    await journal.settled();
    expect(api.clear).toHaveBeenCalledExactlyOnceWith("a.md");
    journal.dispose();
  });
  it("keeps a new draft after an older clear finishes", async () => {
    const { doc, api, journal } = fixture(),
      clearing = deferred();
    doc.set({ content: "old", dirty: true });
    await journal.settled();
    api.clear.mockReturnValueOnce(clearing.promise);
    doc.set({ dirty: false, revision: "r2" });
    await vi.advanceTimersByTimeAsync(0);
    doc.set({ content: "new", dirty: true });
    await vi.advanceTimersByTimeAsync(1000);
    expect(api.save).toHaveBeenCalledTimes(1);
    clearing.resolve();
    await journal.settled();
    expect(api.save).toHaveBeenLastCalledWith("a.md", "new", "r2");
    journal.dispose();
  });
  it("does not clear a previous run's entry when this run's record failed", async () => {
    const { doc, api, journal, error } = fixture();
    api.save.mockRejectedValueOnce("private raw failure");
    doc.set({ content: "draft", dirty: true });
    await journal.settled();
    doc.set({ dirty: false });
    await journal.settled();
    expect(api.clear).not.toHaveBeenCalled();
    expect(error).toHaveBeenCalledWith("journal_unavailable");
    journal.dispose();
  });
  it("keeps dirty content through failed document saves and conflicts", async () => {
    const { doc, api, journal } = fixture();
    doc.set({ content: "draft", dirty: true });
    await journal.settled();
    doc.set({ saving: true });
    doc.set({ saving: false, conflict: { content: "external", revision: "r2" }, error: "conflict" });
    await journal.settled();
    expect(api.clear).not.toHaveBeenCalled();
    expect(doc.view.content).toBe("draft");
    journal.dispose();
  });
  it("only clears owned drafts after an approved switch", async () => {
    const { doc, api, journal } = fixture();
    doc.set({ path: "b.md", content: "other" });
    await journal.settled();
    expect(api.clear).not.toHaveBeenCalled();
    doc.set({ content: "draft", dirty: true });
    await journal.settled();
    doc.set({ path: "c.md", content: "third", dirty: false });
    await journal.settled();
    expect(api.clear).toHaveBeenCalledExactlyOnceWith("b.md");
    journal.dispose();
  });
  it.each(["journal_limit", Object.assign(new Error("limit"), { name: "journal_limit" })])(
    "reports capacity separately (%s)",
    async (failure) => {
      const { doc, api, journal, error } = fixture();
      api.save.mockRejectedValueOnce(failure);
      doc.set({ content: "draft", dirty: true });
      await journal.settled();
      expect(error).toHaveBeenCalledWith("journal_limit");
      expect(doc.view.dirty).toBe(true);
      journal.dispose();
    },
  );
  it("clears an adopted recovery even if manually saved before its first timer", async () => {
    const { doc, api, journal } = fixture();
    doc.set({ content: "recovered", dirty: true });
    journal.adoptRestored("a.md");
    doc.set({ dirty: false, revision: "r2" });
    await journal.settled();
    expect(api.save).not.toHaveBeenCalled();
    expect(api.clear).toHaveBeenCalledExactlyOnceWith("a.md");
    journal.dispose();
  });
  it("drains the pending timer before an explicit journal operation", async () => {
    const { doc, api, journal } = fixture();
    doc.set({ content: "draft", dirty: true });
    await journal.settled();
    expect(api.save).toHaveBeenCalledExactlyOnceWith("a.md", "draft", "r1");
    journal.dispose();
  });
  it("shares outstanding work across remounts and suppresses disposed errors", async () => {
    const { doc, api, journal, error } = fixture(),
      writing = deferred();
    api.save.mockReturnValueOnce(writing.promise);
    doc.set({ content: "old", dirty: true });
    await vi.advanceTimersByTimeAsync(1000);
    journal.dispose();
    const nextError = vi.fn(),
      next = new NoteJournal(doc, api, nextError);
    doc.set({ content: "new", dirty: true });
    await vi.advanceTimersByTimeAsync(1000);
    expect(api.save).toHaveBeenCalledTimes(1);
    writing.reject(new Error("failed"));
    await next.settled();
    expect(error).not.toHaveBeenCalled();
    expect(api.save).toHaveBeenLastCalledWith("a.md", "new", "r1");
    next.dispose();
  });
  it("refreshes the base revision when edits remain after a save", async () => {
    const { doc, api, journal } = fixture();
    doc.set({ content: "during save", dirty: true, saving: true });
    await journal.settled();
    doc.set({ revision: "r2", saving: false });
    await journal.settled();
    expect(api.save).toHaveBeenLastCalledWith("a.md", "during save", "r2");
    journal.dispose();
  });
});
