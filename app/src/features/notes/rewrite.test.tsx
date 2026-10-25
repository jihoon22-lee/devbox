import { isolateHistory } from "@codemirror/commands";
import { EditorView } from "@codemirror/view";
import { act, cleanup, render, screen } from "@testing-library/react";
import { afterAll, afterEach, beforeAll, beforeEach, expect, test, vi } from "vitest";
import type { RewriteRequest } from "../../rpc/gen/rpc";
import { EditorHost } from "../documents/EditorHost";
import { keyId, memoryJournal } from "../documents/offlineJournal";
import { useDocuments } from "../documents/store";
import { FakeDocumentsDaemon } from "../documents/test-daemon";
import { applyEdits, listenRewrites } from "./rewrite";

const key = { root: "vault", path: "b.md" };
const docs = () => useDocuments.getState();
const doc = () => docs().find(key);
let daemon: FakeDocumentsDaemon;
let stops: (() => void)[] = [];
const disk = () => daemon.files.get(keyId(key))?.text;
const replies = () => daemon.of("notes.rewrite_reply");
async function advance(ms: number) {
  await vi.advanceTimersByTimeAsync(ms);
  await docs().settled();
}

const geometry = ["getClientRects", "getBoundingClientRect"] as const;
const saved = geometry.map((name) => Object.getOwnPropertyDescriptor(Range.prototype, name));
beforeAll(() => {
  Object.defineProperties(Range.prototype, {
    getClientRects: { configurable: true, value: () => [] },
    getBoundingClientRect: { configurable: true, value: () => new DOMRect() },
  });
});
afterAll(() => {
  geometry.forEach((name, i) => {
    const original = saved[i];
    if (original) Object.defineProperty(Range.prototype, name, original);
    else Reflect.deleteProperty(Range.prototype, name);
  });
});
beforeEach(async () => {
  vi.useFakeTimers();
  daemon = new FakeDocumentsDaemon();
  daemon.write(key, "see [[a]]");
  stops = [docs().connect(daemon, { journal: memoryJournal(), windowId: "main" }), listenRewrites(daemon)];
  docs().open(key, { kind: "note" });
  await docs().settled();
});
afterEach(() => {
  cleanup();
  for (const stop of stops) stop();
  vi.useRealTimers();
});

/** An unsaved edit journaled as seq 1 (the 1 s autosave has not run yet). */
async function dirtyJournaled(text: string) {
  docs().edit(key, text);
  await advance(300);
  expect(doc()).toMatchObject({ state: "Dirty", editSeq: 1, journalSeq: 1 });
}
const request = (patch: Partial<RewriteRequest> = {}): RewriteRequest => ({
  opId: "op1",
  key,
  leaseId: doc()?.leaseId as string,
  journalSeq: 1,
  edits: [{ fromUtf16: 6, toUtf16: 7, text: "z/a2" }],
  ...patch,
});

test("edits apply from the end so earlier offsets stay valid, and bad edits are refused", () => {
  const text = "한글 😀 [[a]] [[a]]";
  const edits = [
    { fromUtf16: 8, toUtf16: 9, text: "b" },
    { fromUtf16: 14, toUtf16: 15, text: "c" },
  ];
  expect(applyEdits(text, edits)).toBe("한글 😀 [[b]] [[c]]");
  expect(applyEdits(text, [{ fromUtf16: 3, toUtf16: 99, text: "" }])).toBeNull();
  expect(
    applyEdits(text, [
      { fromUtf16: 2, toUtf16: 6, text: "" },
      { fromUtf16: 4, toUtf16: 8, text: "" },
    ]),
  ).toBeNull();
});

test("the lease holder applies the edits to its buffer and editor, saves, then replies applied", async () => {
  render(<EditorHost docKey={key} />);
  const view = EditorView.findFromDOM(screen.getByRole("textbox", { name: "문서 편집기" })) as EditorView;
  await dirtyJournaled("see [[a]] draft");
  act(() => daemon.emit("notes.rewrite_request", request()));
  await advance(0);
  expect(disk()).toBe("see [[z/a2]] draft");
  expect(replies()).toEqual([{ opId: "op1", key, applied: true }]);
  expect(view.state.doc.toString()).toBe("see [[z/a2]] draft");
  // The next keystroke continues from the rewritten text instead of bringing the old link back.
  act(() =>
    view.dispatch({
      changes: { from: view.state.doc.length, insert: "!" },
      userEvent: "input.type",
      annotations: isolateHistory.of("full"),
    }),
  );
  expect(doc()?.text).toBe("see [[z/a2]] draft!");
});

test("a buffer that moved past the journaled seq answers not applied and keeps the buffer", async () => {
  await dirtyJournaled("see [[a]] draft");
  docs().edit(key, "see [[a]] draft 2");
  act(() => daemon.emit("notes.rewrite_request", request()));
  await advance(0);
  expect(replies()).toEqual([{ opId: "op1", key, applied: false }]);
  expect(doc()?.text).toBe("see [[a]] draft 2");
  expect(daemon.count("documents.save")).toBe(0);
});

test("another window's lease gets no reply from this window", async () => {
  await dirtyJournaled("see [[a]] draft");
  act(() => daemon.emit("notes.rewrite_request", request({ leaseId: "someone-else" })));
  await advance(0);
  expect(replies()).toEqual([]);
  expect(doc()?.text).toBe("see [[a]] draft");
});

test("a save that ends in a conflict answers not applied and never overwrites the disk", async () => {
  await dirtyJournaled("see [[a]] draft");
  daemon.write(key, "changed outside");
  act(() => daemon.emit("notes.rewrite_request", request()));
  await advance(0);
  expect(doc()?.state).toBe("Conflict");
  expect(disk()).toBe("changed outside");
  expect(replies()).toEqual([{ opId: "op1", key, applied: false }]);
});

test("an append edit (not a rename) is handled the same way", async () => {
  await dirtyJournaled("see [[a]] draft");
  act(() =>
    daemon.emit(
      "notes.rewrite_request",
      request({ opId: "append1", edits: [{ fromUtf16: 15, toUtf16: 15, text: "\n- 09:05 hi" }] }),
    ),
  );
  await advance(0);
  expect(disk()).toBe("see [[a]] draft\n- 09:05 hi");
  expect(replies()).toEqual([{ opId: "append1", key, applied: true }]);
});

test("response rewrite refuses serialized CRLF and BOM overflow before changing the dirty buffer", async () => {
  await dirtyJournaled("see [[a]] draft");
  const current = doc();
  if (!current) throw new Error("open fixture");
  useDocuments.setState((state) => ({ docs: { ...state.docs, [current.id]: { ...current, eol: "crlf", bom: true } } }));
  const append = "x\n".repeat(700_000);
  expect(new TextEncoder().encode(append).length).toBeLessThan(2 * 1024 * 1024);
  act(() =>
    daemon.emit(
      "notes.rewrite_request",
      request({ edits: [{ fromUtf16: current.text.length, toUtf16: current.text.length, text: append }] }),
    ),
  );
  await advance(0);
  expect(doc()?.text).toBe("see [[a]] draft");
  expect(disk()).toBe("see [[a]]");
  expect(replies()).toEqual([{ opId: "op1", key, applied: false }]);
});

test("a duplicate response rewrite cannot apply the same append twice", async () => {
  await dirtyJournaled("see [[a]] draft");
  const end = doc()?.text.length ?? 0;
  const append = request({ edits: [{ fromUtf16: end, toUtf16: end, text: "\nresponse-once" }] });
  act(() => {
    daemon.emit("notes.rewrite_request", append);
    daemon.emit("notes.rewrite_request", append);
  });
  await advance(0);
  expect(disk()?.match(/response-once/g)).toHaveLength(1);
  expect(doc()?.text.match(/response-once/g)).toHaveLength(1);
});
