import { EditorView } from "@codemirror/view";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, expect, test, vi } from "vitest";
import { memoryJournal } from "../../documents/offlineJournal";
import { useDocuments } from "../../documents/store";
import { FakeDocumentsDaemon } from "../../documents/test-daemon";
import { CodeEditor } from "./CodeEditor";

vi.mock("../../../rpc/RpcProvider", () => ({ useRpcSession: () => ({ state: "ready" }) }));
const key = { root: "project:p", path: "src/가😀.ts" };
let daemon: FakeDocumentsDaemon, stop: () => void;
beforeAll(() => {
  Object.defineProperties(Range.prototype, {
    getClientRects: { configurable: true, value: () => [] },
    getBoundingClientRect: { configurable: true, value: () => new DOMRect() },
  });
});
beforeEach(() => {
  daemon = new FakeDocumentsDaemon();
  daemon.write(key, "const a = 1;\n");
  stop = useDocuments.getState().connect(daemon, { journal: memoryJournal() });
});
afterEach(() => {
  cleanup();
  stop();
});
test("the source editor uses the code document policy and Ctrl+S writes its buffer", async () => {
  render(<CodeEditor docKey={key} />);
  const input = await screen.findByRole("textbox", { name: "코드 편집기" });
  await waitFor(() => expect(input).toHaveAttribute("contenteditable", "true"));
  const view = EditorView.findFromDOM(input)!;
  act(() => view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: "const a = 2;\n" } }));
  expect(useDocuments.getState().find(key)?.policy).toEqual({ kind: "code" });
  expect(daemon.count("documents.save")).toBe(0);
  await act(async () => {
    fireEvent.keyDown(input, { key: "s", code: "KeyS", ctrlKey: true });
    await useDocuments.getState().settled();
  });
  expect(daemon.of("documents.save")).toEqual([expect.objectContaining({ text: "const a = 2;\n" })]);
});
test("Ctrl+H and Ctrl+G open editor-local replace and line panels", async () => {
  render(<CodeEditor docKey={key} />);
  const input = await screen.findByRole("textbox", { name: "코드 편집기" });
  await waitFor(() => expect(input).toHaveAttribute("contenteditable", "true"));
  fireEvent.keyDown(input, { key: "h", code: "KeyH", ctrlKey: true });
  expect(document.querySelector('.cm-search input[name="replace"]')).not.toBeNull();
  fireEvent.keyDown(input, { key: "g", code: "KeyG", ctrlKey: true });
  expect(screen.getByRole("textbox", { name: /줄로 이동/ })).toBeInTheDocument();
});
