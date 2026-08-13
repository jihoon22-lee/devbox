import { cleanup, render, waitFor } from "@testing-library/react";
import { undo } from "@codemirror/commands";
import { EditorView } from "@codemirror/view";
import { afterEach, describe, expect, it, vi } from "vitest";
import CodeEditor from "./CodeEditor";
import { replaceKeymap } from "./extensions";
import { bookmarkField, setBookmarkLines } from "./bookmarks";

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
});
