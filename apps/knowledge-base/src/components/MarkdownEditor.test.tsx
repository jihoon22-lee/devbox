import { EditorSelection } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { readClipboardText } from "../api";
import MarkdownEditor from "./MarkdownEditor";

vi.mock("../api", () => ({
  readClipboardText: vi.fn(async () => "pasted"),
}));

const readClipboardTextMock = vi.mocked(readClipboardText);
const writeTextMock = vi.fn<(text: string) => Promise<void>>();

beforeEach(() => {
  readClipboardTextMock.mockReset().mockResolvedValue("pasted");
  writeTextMock.mockReset().mockResolvedValue(undefined);
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText: writeTextMock },
  });
});

afterEach(() => cleanup());

function setup() {
  const onChange = vi.fn();
  const onError = vi.fn();
  render(
    <MarkdownEditor
      value="alpha beta"
      onChange={onChange}
      onSave={() => undefined}
      onError={onError}
    />,
  );
  const content = document.querySelector(".cm-content");
  if (!(content instanceof HTMLElement)) throw new Error("CodeMirror content was not rendered");
  const view = EditorView.findFromDOM(content);
  if (!view) throw new Error("CodeMirror view was not found");
  return { content, view, onChange, onError };
}

function openByKeyboard(content: HTMLElement) {
  content.focus();
  fireEvent.keyDown(content, { key: "F10", code: "F10", shiftKey: true });
}

describe("MarkdownEditor context menu", () => {
  it("opens from right-click and exposes the exact editor actions", () => {
    const { content } = setup();
    fireEvent.contextMenu(content, { clientX: 12, clientY: 18 });

    expect(screen.getByRole("menu", { name: "Markdown 편집기 작업" })).toBeInTheDocument();
    for (const label of ["잘라내기", "복사", "붙여넣기", "링크 삽입"]) {
      expect(screen.getByRole("menuitem", { name: label })).toBeInTheDocument();
    }
    expect(screen.getByRole("menuitem", { name: "잘라내기" })).toHaveAttribute("aria-disabled", "true");
  });

  it("copies and cuts the current CodeMirror selection and restores editor focus", async () => {
    const { content, view } = setup();
    view.dispatch({ selection: EditorSelection.range(0, 5) });

    openByKeyboard(content);
    fireEvent.click(screen.getByRole("menuitem", { name: "복사" }));
    await waitFor(() => expect(writeTextMock).toHaveBeenCalledWith("alpha"));
    await waitFor(() => expect(document.activeElement).toBe(content));

    view.dispatch({ selection: EditorSelection.range(6, 10) });
    openByKeyboard(content);
    fireEvent.click(screen.getByRole("menuitem", { name: "잘라내기" }));
    await waitFor(() => expect(view.state.doc.toString()).toBe("alpha "));
  });

  it("reads clipboard only after Paste and replaces the selection", async () => {
    const { content, view } = setup();
    view.dispatch({ selection: EditorSelection.range(0, 5) });
    expect(readClipboardTextMock).not.toHaveBeenCalled();

    openByKeyboard(content);
    fireEvent.click(screen.getByRole("menuitem", { name: "붙여넣기" }));

    await waitFor(() => expect(readClipboardTextMock).toHaveBeenCalledTimes(1));
    expect(view.state.doc.toString()).toBe("pasted beta");
  });

  it("inserts a Markdown link around the selected text", async () => {
    vi.spyOn(window, "prompt").mockReturnValue("https://example.com");
    const { content, view } = setup();
    view.dispatch({ selection: EditorSelection.range(0, 5) });

    openByKeyboard(content);
    fireEvent.click(screen.getByRole("menuitem", { name: "링크 삽입" }));

    await waitFor(() => expect(view.state.doc.toString()).toBe(
      "[alpha](https://example.com) beta",
    ));
  });

  it("does not intercept composing Shift+F10 and reports clipboard failures", async () => {
    const { content, onError } = setup();
    fireEvent.keyDown(content, {
      key: "F10",
      code: "F10",
      shiftKey: true,
      isComposing: true,
      keyCode: 229,
    });
    expect(screen.queryByRole("menu", { name: "Markdown 편집기 작업" })).toBeNull();

    readClipboardTextMock.mockRejectedValueOnce(new Error("clipboard unavailable"));
    openByKeyboard(content);
    fireEvent.click(screen.getByRole("menuitem", { name: "붙여넣기" }));
    await waitFor(() => expect(onError).toHaveBeenLastCalledWith("clipboard unavailable"));
  });
});
