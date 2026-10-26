import { undoDepth } from "@codemirror/commands";
import { EditorSelection, EditorState, StateEffect } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, expect, test, vi } from "vitest";
import { useToasts } from "../../../ui";
import { memoryJournal } from "../../documents/offlineJournal";
import { useDocuments } from "../../documents/store";
import { FakeDocumentsDaemon } from "../../documents/test-daemon";
import { MAX_HANDOFF_BYTES, registerTransformsNavigation, useTransformHandoff } from "../../tools/transforms/handoff";
import { CodeEditor } from "./CodeEditor";

vi.mock("../../../rpc/RpcProvider", () => ({ useRpcSession: () => ({ state: "ready" }) }));
const key = { root: "project:p", path: "fixture.txt" };
let daemon: FakeDocumentsDaemon, stop: () => void, navigation: () => void;
beforeAll(() =>
  Object.defineProperties(Range.prototype, {
    getClientRects: { configurable: true, value: () => [] },
    getBoundingClientRect: { configurable: true, value: () => new DOMRect() },
  }),
);
beforeEach(() => {
  daemon = new FakeDocumentsDaemon();
  daemon.write(key, 'before {"n":1} after');
  stop = useDocuments.getState().connect(daemon, { journal: memoryJournal() });
  navigation = registerTransformsNavigation(vi.fn());
  useToasts.setState({ items: [] });
});
afterEach(() => {
  cleanup();
  stop();
  navigation();
  vi.restoreAllMocks();
});
async function editor(text?: string) {
  if (text !== undefined) daemon.write(key, text);
  render(<CodeEditor docKey={key} />);
  const input = await screen.findByRole("textbox", { name: "코드 편집기" });
  await waitFor(() => expect(input).toHaveAttribute("contenteditable", "true"));
  return EditorView.findFromDOM(input)!;
}
const button = () => screen.getByRole("button", { name: "선택을 변환 도구로 보내기" });
test("explicit JSON selection reaches the existing handoff without location or source mutation", async () => {
  const view = await editor();
  act(() => view.dispatch({ changes: { from: 12, to: 13, insert: "2" } }));
  act(() => view.dispatch({ selection: { anchor: 7, head: 14 } }));
  const before = useDocuments.getState().find(key);
  const undo = undoDepth(view.state);
  const writes = vi.spyOn(Storage.prototype, "setItem");
  fireEvent.click(button());
  await waitFor(() =>
    expect(useTransformHandoff.getState().pending).toMatchObject({
      text: '{"n":2}',
      source: "editor",
      toolId: "json-format",
    }),
  );
  expect(view.state.doc.toString()).toBe('before {"n":2} after');
  expect(before?.state).toBe("Dirty");
  expect(undo).toBe(1);
  expect(useDocuments.getState().find(key)?.editSeq).toBe(before?.editSeq);
  expect(useDocuments.getState().find(key)?.state).toBe(before?.state);
  expect(undoDepth(view.state)).toBe(undo);
  expect(daemon.count("documents.save")).toBe(0);
  expect(writes).not.toHaveBeenCalled();
});

test("empty selection disables the action and selection changes update it reactively", async () => {
  const view = await editor();
  expect(button()).toBeDisabled();
  fireEvent.click(button());
  expect(useTransformHandoff.getState().pending).toBeNull();
  act(() => view.dispatch({ selection: { anchor: 0, head: 6 } }));
  expect(button()).toBeEnabled();
  act(() => view.dispatch({ selection: { anchor: 3 } }));
  expect(button()).toBeDisabled();
});
test("multiple selections are joined in document order with newlines and empty cursors ignored", async () => {
  const view = await editor("first middle last");
  act(() => view.dispatch({ effects: StateEffect.appendConfig.of(EditorState.allowMultipleSelections.of(true)) }));
  act(() =>
    view.dispatch({
      selection: EditorSelection.create([
        EditorSelection.range(13, 17),
        EditorSelection.cursor(8),
        EditorSelection.range(0, 5),
      ]),
    }),
  );
  fireEvent.click(button());
  await waitFor(() => expect(useTransformHandoff.getState().pending?.text).toBe("first\nlast"));
  expect(view.state.doc.toString()).toBe("first middle last");
  expect(undoDepth(view.state)).toBe(0);
});
test("nonASCII selections reuse the exact512KiB UTF8 bound and scalar-safe truncation", async () => {
  const exact = "aa" + "한".repeat((MAX_HANDOFF_BYTES - 2) / 3);
  const view = await editor(exact + "😀");
  act(() => view.dispatch({ selection: { anchor: 0, head: exact.length } }));
  fireEvent.click(button());
  await waitFor(() => expect(useTransformHandoff.getState().pending?.text).toBe(exact));
  expect(useToasts.getState().items).toHaveLength(0);
  act(() => view.dispatch({ selection: { anchor: 0, head: view.state.doc.length } }));
  fireEvent.click(button());
  await waitFor(() => expect(useToasts.getState().items.at(-1)?.title).toContain("512KiB"));
  expect(useTransformHandoff.getState().pending?.text).toBe(exact);
  expect(view.state.doc.toString()).toBe(exact + "😀");
});
test("a failed older navigation cannot clear a rapidly newer editor selection", async () => {
  const view = await editor("older newer");
  navigation();
  let reject!: (error: Error) => void;
  const navigate = vi
    .fn()
    .mockImplementationOnce(
      () =>
        new Promise((_yes, no) => {
          reject = no;
        }),
    )
    .mockResolvedValue(undefined);
  navigation = registerTransformsNavigation(navigate);
  act(() => view.dispatch({ selection: { anchor: 0, head: 5 } }));
  fireEvent.click(button());
  act(() => view.dispatch({ selection: { anchor: 6, head: 11 } }));
  fireEvent.click(button());
  await act(async () => reject(new Error("private old selection")));
  expect(useTransformHandoff.getState().pending).toMatchObject({ text: "newer", source: "editor" });
  expect(navigate).toHaveBeenCalledTimes(2);
  expect(navigate.mock.calls.every(([search]) => Object.keys(search).sort().join(",") === "tool,view")).toBe(true);
  expect(useToasts.getState().items).toHaveLength(0);
});
