import { cleanup, fireEvent, render, waitFor, within } from "@testing-library/react";
import { undo } from "@codemirror/commands";
import { EditorView } from "@codemirror/view";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import CodeEditor from "./CodeEditor";
import { replaceKeymap } from "./extensions";
import { bookmarkField, setBookmarkLines } from "./bookmarks";

const { readClipboardTextMock, writeClipboardTextMock } = vi.hoisted(() => ({
  readClipboardTextMock: vi.fn<() => Promise<string>>(),
  writeClipboardTextMock: vi.fn<(text: string) => Promise<void>>(),
}));

vi.mock("../api", () => ({
  readClipboardText: readClipboardTextMock,
}));

beforeEach(() => {
  readClipboardTextMock.mockReset().mockResolvedValue("붙여넣기");
  writeClipboardTextMock.mockReset().mockResolvedValue(undefined);
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText: writeClipboardTextMock },
  });
});

afterEach(() => cleanup());

// jsdom's Range lacks layout APIs used by CM6's rectangle-selection layer.
// The production WebView supplies these methods; returning an empty rect list
// keeps this behavior-focused suite deterministic without changing the editor.
if (!Range.prototype.getClientRects) {
  Range.prototype.getClientRects = () => [] as unknown as DOMRectList;
}

const baseProps = {
  docId: "doc:one",
  path: "/workspace/one.ts",
  readOnly: false,
  syntaxHighlightingEnabled: true,
  fontSize: 13,
  onFocus: vi.fn(),
};

describe("CodeEditor lifecycle", () => {
  it("syncs external values without reporting them as local edits", () => {
    const onChange = vi.fn();
    const { container, rerender } = render(<CodeEditor {...baseProps} value="before" onChange={onChange} />);
    const editor = container.querySelector(".cm-editor") as HTMLElement | null;
    expect(editor).not.toBeNull();
    if (!editor) throw new Error("CodeMirror did not mount");

    rerender(<CodeEditor {...baseProps} value="after" onChange={onChange} />);

    expect(container.querySelector(".cm-content")?.textContent).toBe("after");
    expect(onChange).not.toHaveBeenCalled();
    expect(container.querySelector(".cm-editor")).toBe(editor);
    expect(undo(EditorView.findFromDOM(editor)!)).toBe(false);
  });

  it("reconfigures read-only state and exposes replace on Mod-H without remounting", () => {
    const { container, rerender } = render(
      <CodeEditor {...baseProps} value="const answer = 42;" onChange={vi.fn()} />,
    );
    const editor = container.querySelector(".cm-editor") as HTMLElement;
    const view = EditorView.findFromDOM(editor);
    expect(view).not.toBeNull();
    expect(replaceKeymap[0].run?.(view!)).toBe(true);
    expect(container.querySelector('input[aria-label="Replace"]')).not.toBeNull();

    rerender(<CodeEditor {...baseProps} value="const answer = 42;" readOnly onChange={vi.fn()} />);
    expect(container.querySelector(".cm-content")?.getAttribute("contenteditable")).toBe("false");
    expect(container.querySelector(".cm-editor")).toBe(editor);
  });

  it("restores cursor/bookmarks and reports mapped bookmark lines after edits", async () => {
    const onCursorChange = vi.fn();
    const onBookmarksChange = vi.fn();
    const { container } = render(
      <CodeEditor
        {...baseProps}
        value={"one\ntwo\nthree"}
        cursor={4}
        bookmarks={[1]}
        onChange={vi.fn()}
        onCursorChange={onCursorChange}
        onBookmarksChange={onBookmarksChange}
      />,
    );
    const editor = container.querySelector(".cm-editor") as HTMLElement;
    const view = EditorView.findFromDOM(editor);
    expect(view?.state.selection.main.head).toBe(4);
    await waitFor(() => expect(view?.state.field(bookmarkField)).toEqual([1]));

    view?.dispatch({ changes: { from: 0, insert: "zero\n" } });
    expect(view?.state.field(bookmarkField)).toEqual([2]);
    expect(onBookmarksChange).toHaveBeenLastCalledWith([2]);

    view?.dispatch({ effects: setBookmarkLines.of([2]) });
    view?.dispatch({ selection: { anchor: 0 } });
    expect(onCursorChange).toHaveBeenLastCalledWith(0);
  });

  it("reports the main cursor after a local edit, including multi-selection edits", () => {
    const onChange = vi.fn();
    const onCursorChange = vi.fn();
    const { container } = render(
      <CodeEditor
        {...baseProps}
        value={"one\ntwo\nthree"}
        onChange={onChange}
        onCursorChange={onCursorChange}
      />,
    );
    const editor = container.querySelector(".cm-editor") as HTMLElement;
    const view = EditorView.findFromDOM(editor);
    if (!view) throw new Error("CodeMirror did not mount");

    view.dispatch({
      changes: { from: 0, insert: "zero\n" },
      selection: { anchor: 5 },
    });

    expect(onChange).toHaveBeenCalledWith("zero\none\ntwo\nthree");
    expect(onCursorChange).toHaveBeenLastCalledWith(5);
  });

  it("runs cut, copy, and native clipboard paste against the CodeMirror selection", async () => {
    const onChange = vi.fn();
    const rendered = render(
      <CodeEditor {...baseProps} value="one two" onChange={onChange} />,
    );
    const editor = rendered.container.querySelector(".cm-editor") as HTMLElement;
    const content = rendered.container.querySelector(".cm-content") as HTMLElement;
    const view = EditorView.findFromDOM(editor)!;
    view.dispatch({ selection: { anchor: 0, head: 3 } });

    fireEvent.contextMenu(content, { clientX: 1, clientY: 1 });
    fireEvent.click(within(rendered.getByRole("menu", { name: "코드 편집기 작업" })).getByRole("menuitem", { name: "복사" }));
    await waitFor(() => expect(writeClipboardTextMock).toHaveBeenCalledWith("one"));

    fireEvent.contextMenu(content, { clientX: 1, clientY: 1 });
    fireEvent.click(within(rendered.getByRole("menu", { name: "코드 편집기 작업" })).getByRole("menuitem", { name: "잘라내기" }));
    await waitFor(() => expect(view.state.doc.toString()).toBe(" two"));
    expect(onChange).toHaveBeenLastCalledWith(" two");

    fireEvent.contextMenu(content, { clientX: 1, clientY: 1 });
    fireEvent.click(within(rendered.getByRole("menu", { name: "코드 편집기 작업" })).getByRole("menuitem", { name: "붙여넣기" }));
    await waitFor(() => expect(readClipboardTextMock).toHaveBeenCalledTimes(1));
    expect(view.state.doc.toString()).toBe("붙여넣기 two");
  });

  it("keeps mutation actions disabled for a read-only editor", () => {
    const rendered = render(
      <CodeEditor {...baseProps} readOnly value="one" onChange={vi.fn()} />,
    );
    const editor = rendered.container.querySelector(".cm-editor") as HTMLElement;
    const content = rendered.container.querySelector(".cm-content") as HTMLElement;
    const view = EditorView.findFromDOM(editor)!;
    view.dispatch({ selection: { anchor: 0, head: 3 } });
    fireEvent.contextMenu(content, { clientX: 1, clientY: 1 });

    const menu = rendered.getByRole("menu", { name: "코드 편집기 작업" });
    expect(within(menu).getByRole("menuitem", { name: "잘라내기" }).getAttribute("aria-disabled")).toBe("true");
    expect(within(menu).getByRole("menuitem", { name: "붙여넣기" }).getAttribute("aria-disabled")).toBe("true");
    expect(within(menu).getByRole("menuitem", { name: "복사" }).getAttribute("aria-disabled")).toBeNull();
  });

  it("does not delete a newer selection when clipboard writing resolves late", async () => {
    let resolveWrite!: () => void;
    writeClipboardTextMock.mockImplementationOnce(() => new Promise<void>((resolve) => {
      resolveWrite = resolve;
    }));
    const onError = vi.fn();
    const rendered = render(
      <CodeEditor {...baseProps} value="one two" onChange={vi.fn()} onError={onError} />,
    );
    const editor = rendered.container.querySelector(".cm-editor") as HTMLElement;
    const content = rendered.container.querySelector(".cm-content") as HTMLElement;
    const view = EditorView.findFromDOM(editor)!;
    view.dispatch({ selection: { anchor: 0, head: 3 } });
    fireEvent.contextMenu(content, { clientX: 1, clientY: 1 });
    fireEvent.click(within(rendered.getByRole("menu", { name: "코드 편집기 작업" })).getByRole("menuitem", { name: "잘라내기" }));
    view.dispatch({ selection: { anchor: 4, head: 7 } });
    resolveWrite();

    await waitFor(() => expect(onError).toHaveBeenCalledWith(expect.stringContaining("잘라내기를 취소")));
    expect(view.state.doc.toString()).toBe("one two");
    expect(view.state.selection.main.from).toBe(4);
  });

  it("uses the clicked editor cursor for negotiated LSP navigation actions", () => {
    const onNavigate = vi.fn();
    const rendered = render(
      <CodeEditor
        {...baseProps}
        value="one two"
        cursor={4}
        onChange={vi.fn()}
        canGoToDefinition
        canFindReferences
        onNavigate={onNavigate}
      />,
    );
    const content = rendered.container.querySelector(".cm-content") as HTMLElement;
    fireEvent.contextMenu(content, { clientX: 1, clientY: 1 });
    fireEvent.click(within(rendered.getByRole("menu", { name: "코드 편집기 작업" })).getByRole("menuitem", { name: "정의로 이동" }));
    expect(onNavigate).toHaveBeenCalledWith("doc:one", "definition", 0);

    fireEvent.contextMenu(content, { clientX: 1, clientY: 1 });
    fireEvent.click(within(rendered.getByRole("menu", { name: "코드 편집기 작업" })).getByRole("menuitem", { name: "참조 찾기" }));
    expect(onNavigate).toHaveBeenCalledWith("doc:one", "references", 0);
  });

  it("opens from the menu key, restores editor focus, and ignores IME composition", async () => {
    const rendered = render(
      <CodeEditor {...baseProps} value="one" onChange={vi.fn()} />,
    );
    const content = rendered.container.querySelector(".cm-content") as HTMLElement;
    content.focus();
    fireEvent.keyDown(content, { key: "F10", shiftKey: true, isComposing: true });
    expect(rendered.queryByRole("menu")).toBeNull();

    fireEvent.keyDown(content, { key: "ContextMenu", code: "ContextMenu" });
    const menu = rendered.getByRole("menu", { name: "코드 편집기 작업" });
    fireEvent.keyDown(menu, { key: "Escape" });
    await waitFor(() => expect(document.activeElement).toBe(content));

    fireEvent.keyDown(content, { key: "ContextMenu", code: "ContextMenu" });
    rendered.rerender(
      <CodeEditor {...baseProps} value="one" visible={false} onChange={vi.fn()} />,
    );
    await waitFor(() => expect(rendered.queryByRole("menu")).toBeNull());
  });
});
