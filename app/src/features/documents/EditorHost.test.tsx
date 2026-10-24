import { isolateHistory } from "@codemirror/commands";
import { EditorView } from "@codemirror/view";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { StrictMode } from "react";
import { afterAll, afterEach, beforeAll, beforeEach, expect, test, vi } from "vitest";
import type { DocPolicy } from "../../rpc/gen/rpc";
import { EditorHost } from "./EditorHost";
import { memoryJournal } from "./offlineJournal";
import { useDocuments } from "./store";
import { FakeDocumentsDaemon } from "./test-daemon";

const a = { root: "vault", path: "a.md" };
const b = { root: "vault", path: "b.md" };
const note: DocPolicy = { kind: "note" };
const docs = () => useDocuments.getState();
let daemon: FakeDocumentsDaemon;
let stop: () => void;

// jsdom has no layout. Supply only Range geometry; the editor stays real.
const rangeGeometry = ["getClientRects", "getBoundingClientRect"] as const;
const originalGeometry = rangeGeometry.map((name) => Object.getOwnPropertyDescriptor(Range.prototype, name));
beforeAll(() => {
  Object.defineProperties(Range.prototype, {
    getClientRects: { configurable: true, value: () => [] },
    getBoundingClientRect: { configurable: true, value: () => new DOMRect() },
  });
});
afterAll(() => {
  rangeGeometry.forEach((name, index) => {
    const original = originalGeometry[index];
    if (original) Object.defineProperty(Range.prototype, name, original);
    else Reflect.deleteProperty(Range.prototype, name);
  });
});
beforeEach(async () => {
  daemon = new FakeDocumentsDaemon();
  daemon.write(a, "alpha");
  daemon.write(b, "beta");
  stop = docs().connect(daemon, { journal: memoryJournal(), windowId: "main" });
  docs().open(a, note);
  docs().open(b, note);
  await docs().settled();
});
afterEach(() => {
  cleanup();
  stop();
});

function editor() {
  const content = screen.getByRole("textbox", { name: "문서 편집기" });
  const view = EditorView.findFromDOM(content);
  if (!view) throw new Error("CodeMirror editor missing");
  return { content, view };
}
function type(view: EditorView, text: string) {
  act(() =>
    view.dispatch({
      changes: { from: view.state.doc.length, insert: text },
      selection: { anchor: view.state.doc.length + text.length },
      userEvent: "input.type",
      annotations: isolateHistory.of("full"),
    }),
  );
}
const undo = (content: HTMLElement) =>
  act(() => {
    fireEvent.keyDown(content, { key: "z", code: "KeyZ", keyCode: 90, ctrlKey: true });
  });

test("each document keeps its own undo history across switches (AU-K1)", () => {
  const rendered = render(<EditorHost docKey={a} />);
  const first = editor();
  expect(first.view.state.doc.toString()).toBe("alpha");
  type(first.view, " one");
  type(first.view, " two");
  rendered.rerender(<EditorHost docKey={b} />);
  const second = editor();
  expect(second.view).toBe(first.view);
  expect(second.view.state.doc.toString()).toBe("beta");
  type(second.view, " three");
  rendered.rerender(<EditorHost docKey={a} />);
  const back = editor();
  expect(back.view.state.doc.toString()).toBe("alpha one two");
  undo(back.content);
  expect(back.view.state.doc.toString()).toBe("alpha one");
  expect(docs().find(a)?.text).toBe("alpha one");
  expect(docs().find(b)?.text).toBe("beta three");
  rendered.rerender(<EditorHost docKey={b} />);
  expect(editor().view.state.doc.toString()).toBe("beta three");
});

test("the per-document state survives the StrictMode effect remount", () => {
  const rendered = render(
    <StrictMode>
      <EditorHost docKey={a} />
    </StrictMode>,
  );
  const first = editor();
  expect(first.view.state.doc.toString()).toBe("alpha");
  type(first.view, " one");
  expect(docs().find(a)?.text).toBe("alpha one");
  rendered.rerender(
    <StrictMode>
      <EditorHost docKey={b} />
    </StrictMode>,
  );
  rendered.rerender(
    <StrictMode>
      <EditorHost docKey={a} />
    </StrictMode>,
  );
  const back = editor();
  expect(back.view.state.doc.toString()).toBe("alpha one");
  undo(back.content);
  expect(back.view.state.doc.toString()).toBe("alpha");
  expect(docs().find(a)?.text).toBe("alpha");
});

test("Ctrl+S calls onSave once and stays inside the editor", () => {
  const onSave = vi.fn();
  render(<EditorHost docKey={a} onSave={onSave} />);
  const { content } = editor();
  const event = new KeyboardEvent("keydown", {
    key: "s",
    code: "KeyS",
    keyCode: 83,
    ctrlKey: true,
    bubbles: true,
    cancelable: true,
  });
  act(() => {
    content.dispatchEvent(event);
  });
  expect(onSave).toHaveBeenCalledTimes(1);
  expect(event.defaultPrevented).toBe(true);
  expect(content.closest("[data-keyscope]")?.getAttribute("data-keyscope")).toBe("editor");
});

test("an external reload is one undoable transaction that does not count as an edit", async () => {
  render(<EditorHost docKey={a} />);
  const { content, view } = editor();
  const rev = daemon.write(a, "outside");
  await act(async () => {
    daemon.emit("documents.changed", { key: a, rev });
    await docs().settled();
  });
  expect(view.state.doc.toString()).toBe("outside");
  expect(docs().find(a)).toMatchObject({ state: "Clean", text: "outside", editSeq: 0 });
  undo(content);
  expect(view.state.doc.toString()).toBe("alpha");
  expect(docs().find(a)).toMatchObject({ state: "Dirty", text: "alpha" });
});

test("a read-only document cannot be edited from the keyboard", async () => {
  await daemon.call("documents.take", { key: a, windowId: "other" });
  await act(async () => {
    daemon.emit("documents.lease_lost", { key: a, leaseId: "L1" });
    await docs().settled();
  });
  expect(docs().find(a)?.state).toBe("ReadOnly");
  render(<EditorHost docKey={a} />);
  const { content } = editor();
  expect(content).toHaveAttribute("contenteditable", "false");
  expect(content).toHaveAttribute("aria-readonly", "true");
});

test("section unmount and group move keep the document's undo history", () => {
  const first = render(<EditorHost docKey={a} />);
  type(editor().view, " kept");
  first.unmount();
  const other = render(<EditorHost docKey={b} />);
  type(editor().view, " other");
  other.unmount();
  render(<EditorHost docKey={a} />);
  expect(editor().view.state.doc.toString()).toBe("alpha kept");
  undo(editor().content);
  expect(docs().find(a)?.text).toBe("alpha");
  expect(docs().find(b)?.text).toBe("beta other");
});

test("renaming and recreating the old path keeps independent document undo histories", async () => {
  const renamed = { ...a, path: "renamed.md" };
  const rendered = render(<EditorHost docKey={a} />);
  type(editor().view, " retained");
  await act(async () => {
    daemon.move(a, renamed);
    await docs().settled();
  });
  rendered.rerender(<EditorHost docKey={renamed} />);
  expect(editor().view.state.doc.toString()).toBe("alpha retained");
  await act(async () => {
    daemon.write(a, "recreated");
    docs().open(a, note);
    await docs().settled();
  });
  rendered.rerender(<EditorHost docKey={a} />);
  type(editor().view, " independent");
  rendered.rerender(<EditorHost docKey={renamed} />);
  undo(editor().content);
  expect(docs().find(renamed)?.text).toBe("alpha");
  expect(docs().find(a)?.text).toBe("recreated independent");
});
