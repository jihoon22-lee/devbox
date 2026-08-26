import { EditorSelection } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { acceptCompletion, selectedCompletion, startCompletion } from "@codemirror/autocomplete";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { readClipboardText } from "../api";
import MarkdownEditor from "./MarkdownEditor";
import type { WikilinkCandidate, WikilinkOccurrence } from "../types";

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

function setup(options: {
  value?: string;
  wikilinks?: WikilinkOccurrence[];
  loadWikilinkCandidates?: (query: string) => Promise<WikilinkCandidate[]>;
  onNavigateWikilink?: (path: string) => void;
} = {}) {
  const onChange = vi.fn();
  const onError = vi.fn();
  render(
    <MarkdownEditor
      value={options.value ?? "alpha beta"}
      onChange={onChange}
      onSave={() => undefined}
      onError={onError}
      wikilinks={options.wikilinks}
      loadWikilinkCandidates={options.loadWikilinkCandidates}
      onNavigateWikilink={options.onNavigateWikilink}
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

describe("MarkdownEditor wikilinks", () => {
  it("completes after [[ with the canonical indexed path and closing brackets", async () => {
    const loadCandidates = vi.fn(async () => [{
      path: "Notes/Rust.md",
      title: "Rust Study",
      link_target: "Notes/Rust",
    }]);
    const { view } = setup({ value: "[[Ru", loadWikilinkCandidates: loadCandidates });
    view.dispatch({ selection: EditorSelection.cursor(view.state.doc.length) });
    view.focus();

    expect(startCompletion(view)).toBe(true);
    await waitFor(() => expect(loadCandidates).toHaveBeenCalledWith("Ru"));
    await waitFor(() => expect(selectedCompletion(view.state)?.label).toBe("Rust Study"));
    // CodeMirror prevents the same keystroke that opened the menu from accepting it.
    await new Promise((resolve) => setTimeout(resolve, 100));
    expect(acceptCompletion(view)).toBe(true);
    expect(view.state.doc.toString()).toBe("[[Notes/Rust]]");
  });

  it("decorates unresolved links and only navigates a resolved indexed path on Ctrl+click", async () => {
    const onNavigate = vi.fn();
    setup({
      value: "[[Rust]] [[Missing]]",
      wikilinks: [
        {
          target: "Rust",
          label: "Rust",
          line: 1,
          column: 1,
          from: 0,
          to: 8,
          status: "resolved",
          resolved_path: "Notes/Rust.md",
        },
        {
          target: "Missing",
          label: "Missing",
          line: 1,
          column: 10,
          from: 9,
          to: 20,
          status: "missing",
          resolved_path: null,
        },
      ],
      onNavigateWikilink: onNavigate,
    });

    const resolved = await waitFor(() => {
      const element = document.querySelector<HTMLElement>(".cm-wikilink-resolved");
      expect(element).toBeTruthy();
      return element as HTMLElement;
    });
    expect(document.querySelector(".cm-wikilink-missing")).toBeTruthy();
    fireEvent.mouseDown(resolved, { ctrlKey: true });
    expect(onNavigate).toHaveBeenCalledWith("Notes/Rust.md");
  });
});
