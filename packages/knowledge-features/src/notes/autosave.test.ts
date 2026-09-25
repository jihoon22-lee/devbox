import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { NoteAutosave } from "./autosave";
import type { NoteView } from "./noteDocument";

class FakeDocument {
  view: NoteView = {
    sourceVersion: 0,
    path: "a.md",
    content: "",
    revision: "r1",
    dirty: false,
    saving: false,
    conflict: null,
    error: null,
  };
  private listeners = new Set<() => void>();
  save = vi.fn(async () => {
    this.set({ saving: false, dirty: false });
    return true;
  });
  snapshot = () => this.view;
  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };
  set(change: Partial<NoteView>) {
    const bump = "content" in change || "path" in change ? 1 : 0;
    this.view = { ...this.view, ...change, sourceVersion: this.view.sourceVersion + bump };
    this.listeners.forEach((listener) => listener());
  }
}
const timers = {
  set: (cb: () => void, ms: number) => window.setTimeout(cb, ms),
  clear: (h: number) => window.clearTimeout(h),
};

describe("NoteAutosave", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("saves once after edits pause", () => {
    const doc = new FakeDocument();
    const autosave = new NoteAutosave(doc, true, undefined, timers, 1500);
    doc.set({ content: "a", dirty: true });
    vi.advanceTimersByTime(1000);
    doc.set({ content: "ab", dirty: true });
    vi.advanceTimersByTime(1499);
    expect(doc.save).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(doc.save).toHaveBeenCalledTimes(1);
    autosave.dispose();
  });

  it("never saves over a conflict or when disabled", () => {
    const doc = new FakeDocument();
    const autosave = new NoteAutosave(doc, true, undefined, timers, 1500);
    doc.set({ content: "a", dirty: true, conflict: { content: "disk", revision: "r9" } });
    vi.advanceTimersByTime(5000);
    autosave.setEnabled(false);
    doc.set({ content: "b", dirty: true, conflict: null });
    vi.advanceTimersByTime(5000);
    expect(doc.save).not.toHaveBeenCalled();
    autosave.dispose();
  });

  it("stays paused after a restored buffer until the note is saved by hand", async () => {
    const doc = new FakeDocument();
    const autosave = new NoteAutosave(doc, true, undefined, timers, 1500);
    autosave.pause();
    doc.set({ content: "restored", dirty: true });
    vi.advanceTimersByTime(5000);
    expect(doc.save).not.toHaveBeenCalled();
    doc.set({ dirty: false });
    doc.set({ content: "next", dirty: true });
    vi.advanceTimersByTime(1500);
    expect(doc.save).toHaveBeenCalledTimes(1);
    autosave.dispose();
  });

  it("flush saves immediately and calls onSaved", async () => {
    const doc = new FakeDocument();
    const onSaved = vi.fn();
    const autosave = new NoteAutosave(doc, true, onSaved, timers, 1500);
    doc.set({ content: "a", dirty: true });
    expect(await autosave.flush()).toBe(true);
    expect(doc.save).toHaveBeenCalledTimes(1);
    expect(onSaved).toHaveBeenCalled();
    autosave.dispose();
  });
});

it("schedules another save for edits made during an in-flight save", async () => {
  vi.useFakeTimers();
  try {
    const doc = new FakeDocument();
    let finish!: () => void;
    doc.save.mockImplementationOnce(async () => {
      doc.set({ saving: true });
      await new Promise<void>((resolve) => {
        finish = resolve;
      });
      doc.set({ saving: false });
      return false;
    });
    const autosave = new NoteAutosave(doc, true, undefined, timers, 1500);
    doc.set({ content: "first", dirty: true });
    await vi.advanceTimersByTimeAsync(1500);
    doc.set({ content: "second", dirty: true });
    await vi.advanceTimersByTimeAsync(2000);
    expect(doc.save).toHaveBeenCalledTimes(1);
    finish();
    await vi.advanceTimersByTimeAsync(0);
    await vi.advanceTimersByTimeAsync(1500);
    expect(doc.save).toHaveBeenCalledTimes(2);
    autosave.dispose();
  } finally {
    vi.useRealTimers();
  }
});
it("does not retry a failed save forever without a new edit", async () => {
  vi.useFakeTimers();
  try {
    const doc = new FakeDocument();
    doc.save.mockImplementation(async () => {
      doc.set({ saving: true });
      doc.set({ saving: false, error: "failed" });
      return false;
    });
    const autosave = new NoteAutosave(doc, true, undefined, timers, 1500);
    doc.set({ content: "first", dirty: true });
    await vi.advanceTimersByTimeAsync(10000);
    expect(doc.save).toHaveBeenCalledTimes(1);
    autosave.dispose();
  } finally {
    vi.useRealTimers();
  }
});
it("keeps restored content paused across a view remount", async () => {
  vi.useFakeTimers();
  try {
    const doc = new FakeDocument();
    const first = new NoteAutosave(doc, true, undefined, timers, 1500);
    first.pause();
    doc.set({ content: "restored", dirty: true });
    first.dispose();
    const second = new NoteAutosave(doc, true, undefined, timers, 1500);
    doc.set({ content: "restored edit", dirty: true });
    await vi.advanceTimersByTimeAsync(5000);
    expect(doc.save).not.toHaveBeenCalled();
    second.dispose();
  } finally {
    vi.useRealTimers();
  }
});
