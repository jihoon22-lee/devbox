import { cleanup, render } from "@testing-library/react";
import { undo } from "@codemirror/commands";
import { EditorView } from "@codemirror/view";
import { afterEach, describe, expect, it, vi } from "vitest";
import CodeEditor from "./CodeEditor";
import { replaceKeymap } from "./extensions";

afterEach(() => cleanup());

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
});
