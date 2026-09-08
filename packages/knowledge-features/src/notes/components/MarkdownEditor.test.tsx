import { EditorSelection } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { acceptCompletion, selectedCompletion, startCompletion } from "@codemirror/autocomplete";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { readClipboardImage, readClipboardText } from "../api";
import MarkdownEditor from "./MarkdownEditor";
import {
  IMAGE_BUSY_ERROR,
  IMAGE_MULTIPLE_ERROR,
  IMAGE_STALE_ERROR,
  IMAGE_TOO_LARGE_ERROR,
} from "../lib/imageAssets";
import type { ImageAsset, WikilinkCandidate, WikilinkOccurrence } from "../types";

vi.mock("../api", () => ({
  readClipboardImage: vi.fn(async () => null),
  readClipboardText: vi.fn(async () => "pasted"),
}));

const readClipboardTextMock = vi.mocked(readClipboardText);
const readClipboardImageMock = vi.mocked(readClipboardImage);
const writeTextMock = vi.fn<(text: string) => Promise<void>>();

beforeEach(() => {
  readClipboardTextMock.mockReset().mockResolvedValue("pasted");
  readClipboardImageMock.mockReset().mockResolvedValue(null);
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
  documentKey?: string | null;
  onImageImport?: (file: File) => Promise<ImageAsset>;
} = {}) {
  const onChange = vi.fn();
  const onError = vi.fn();
  const rendered = render(
    <MarkdownEditor
      value={options.value ?? "alpha beta"}
      onChange={onChange}
      onSave={() => undefined}
      onError={onError}
      wikilinks={options.wikilinks}
      loadWikilinkCandidates={options.loadWikilinkCandidates}
      onNavigateWikilink={options.onNavigateWikilink}
      documentKey={options.documentKey}
      onImageImport={options.onImageImport}
    />,
  );
  const content = document.querySelector(".cm-content");
  if (!(content instanceof HTMLElement)) throw new Error("CodeMirror content was not rendered");
  const view = EditorView.findFromDOM(content);
  if (!view) throw new Error("CodeMirror view was not found");
  return { content, view, onChange, onError, rerender: rendered.rerender };
}

function openByKeyboard(content: HTMLElement) {
  content.focus();
  fireEvent.keyDown(content, { key: "F10", code: "F10", shiftKey: true });
}

function imageTransfer(files: File[]): DataTransfer {
  return {
    files,
    items: files.map((file) => ({
      kind: "file",
      type: file.type,
      getAsFile: () => file,
    })),
    getData: () => "",
  } as unknown as DataTransfer;
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
    expect(readClipboardImageMock).not.toHaveBeenCalled();
    expect(view.state.doc.toString()).toBe("pasted beta");
  });

  it("reads an image from the clipboard only after explicit Paste", async () => {
    const image = new File([new Uint8Array([1, 2, 3])], "clipboard.png", { type: "image/png" });
    readClipboardImageMock.mockResolvedValueOnce(image);
    const onImageImport = vi.fn(async () => ({
      relativePath: "assets/" + "f".repeat(64) + ".png",
      markdown: "![image](assets/" + "f".repeat(64) + ".png)",
      reused: false,
    }));
    const { content, view } = setup({ value: "before", onImageImport });

    expect(readClipboardImageMock).not.toHaveBeenCalled();
    openByKeyboard(content);
    fireEvent.click(screen.getByRole("menuitem", { name: "붙여넣기" }));

    await waitFor(() => expect(onImageImport).toHaveBeenCalledWith(image));
    expect(readClipboardTextMock).not.toHaveBeenCalled();
    await waitFor(() => expect(view.state.doc.toString()).toContain(
      "![image](assets/" + "f".repeat(64) + ".png)",
    ));
  });

  it("reports an oversized explicit clipboard image instead of falling back to text", async () => {
    readClipboardImageMock.mockRejectedValueOnce(new Error(IMAGE_TOO_LARGE_ERROR));
    const onImageImport = vi.fn(async () => ({
      relativePath: "assets/" + "f".repeat(64) + ".png",
      markdown: "![image](assets/" + "f".repeat(64) + ".png)",
      reused: false,
    }));
    const { content, onError } = setup({ value: "before", onImageImport });

    openByKeyboard(content);
    fireEvent.click(screen.getByRole("menuitem", { name: "붙여넣기" }));

    await waitFor(() => expect(onError).toHaveBeenLastCalledWith(IMAGE_TOO_LARGE_ERROR));
    expect(readClipboardTextMock).not.toHaveBeenCalled();
  });

  it("intercepts an image paste and inserts the native-generated Markdown node", async () => {
    const image = new File([new Uint8Array([1, 2, 3])], "screenshot.png", { type: "image/png" });
    const onImageImport = vi.fn(async (file: File) => {
      expect(file).toBe(image);
      return {
        relativePath: "assets/" + "a".repeat(64) + ".png",
        markdown: "![image](assets/" + "a".repeat(64) + ".png)",
        reused: false,
      };
    });
    const { content, view } = setup({ value: "before", onImageImport });

    fireEvent.paste(content, { clipboardData: imageTransfer([image]) });

    await waitFor(() => expect(view.state.doc.toString()).toContain(
      "![image](assets/" + "a".repeat(64) + ".png)",
    ));
    expect(onImageImport).toHaveBeenCalledTimes(1);
  });

  it("drops one image at the editor cursor and rejects multi-image partial actions", async () => {
    const first = new File([new Uint8Array([1])], "first.png", { type: "image/png" });
    const second = new File([new Uint8Array([2])], "second.png", { type: "image/png" });
    const onImageImport = vi.fn(async () => ({
      relativePath: "assets/" + "b".repeat(64) + ".png",
      markdown: "![image](assets/" + "b".repeat(64) + ".png)",
      reused: false,
    }));
    const { content, view, onError } = setup({ value: "before", onImageImport });
    view.dispatch({ selection: EditorSelection.cursor(3) });
    vi.spyOn(view, "posAtCoords").mockReturnValue(3);

    fireEvent.drop(content, {
      dataTransfer: imageTransfer([first]),
      clientX: 0,
      clientY: 0,
    });
    await waitFor(() => expect(view.state.doc.toString()).toBe(
      "bef![image](assets/" + "b".repeat(64) + ".png)ore",
    ));

    fireEvent.paste(content, { clipboardData: imageTransfer([first, second]) });
    expect(onImageImport).toHaveBeenCalledTimes(1);
    expect(onError).toHaveBeenLastCalledWith(IMAGE_MULTIPLE_ERROR);
  });

  it("preserves IME paste and suppresses a second image action while busy", async () => {
    const image = new File([new Uint8Array([1])], "screenshot.png", { type: "image/png" });
    let resolveImport: ((asset: ImageAsset) => void) | undefined;
    const pending = new Promise<ImageAsset>((resolve) => { resolveImport = resolve; });
    const onImageImport = vi.fn(() => pending);
    const { content, view, onError } = setup({ onImageImport });

    const composingPaste = new Event("paste", { bubbles: true, cancelable: true });
    Object.defineProperty(composingPaste, "clipboardData", {
      configurable: true,
      value: imageTransfer([image]),
    });
    Object.defineProperty(composingPaste, "isComposing", {
      configurable: true,
      value: true,
    });
    content.dispatchEvent(composingPaste);
    expect(onImageImport).not.toHaveBeenCalled();

    fireEvent.paste(content, { clipboardData: imageTransfer([image]) });
    await waitFor(() => expect(onImageImport).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(document.querySelector(".codemirror-editor"))
      .toHaveAttribute("aria-busy", "true"));
    expect(screen.getByRole("status")).toHaveTextContent("이미지 저장 중");
    fireEvent.paste(content, { clipboardData: imageTransfer([image]) });
    expect(onError).toHaveBeenLastCalledWith(IMAGE_BUSY_ERROR);

    resolveImport?.({
      relativePath: "assets/" + "c".repeat(64) + ".png",
      markdown: "![image](assets/" + "c".repeat(64) + ".png)",
      reused: false,
    });
    await waitFor(() => expect(view.state.doc.toString()).toContain("![image]"));
  });

  it("does not overwrite text changed while the image import is in flight", async () => {
    const image = new File([new Uint8Array([1])], "screenshot.png", { type: "image/png" });
    let resolveImport: ((asset: ImageAsset) => void) | undefined;
    const pending = new Promise<ImageAsset>((resolve) => { resolveImport = resolve; });
    const onImageImport = vi.fn(() => pending);
    const { content, view, onError } = setup({ value: "before", onImageImport });

    fireEvent.paste(content, { clipboardData: imageTransfer([image]) });
    await waitFor(() => expect(onImageImport).toHaveBeenCalledTimes(1));
    view.dispatch({ changes: { from: 0, to: 0, insert: "changed " } });
    resolveImport?.({
      relativePath: "assets/" + "d".repeat(64) + ".png",
      markdown: "![image](assets/" + "d".repeat(64) + ".png)",
      reused: false,
    });

    await waitFor(() => expect(onError).toHaveBeenLastCalledWith(IMAGE_STALE_ERROR));
    expect(view.state.doc.toString()).toBe("changed before");
  });

  it("does not insert a completed import into a different note with identical text", async () => {
    const image = new File([new Uint8Array([1])], "screenshot.png", { type: "image/png" });
    let resolveImport: ((asset: ImageAsset) => void) | undefined;
    const pending = new Promise<ImageAsset>((resolve) => { resolveImport = resolve; });
    const onImageImport = vi.fn(() => pending);
    const { content, view, onError, rerender } = setup({
      value: "same note text",
      documentKey: "first.md",
      onImageImport,
    });

    fireEvent.paste(content, { clipboardData: imageTransfer([image]) });
    await waitFor(() => expect(onImageImport).toHaveBeenCalledTimes(1));
    rerender(
      <MarkdownEditor
        value="same note text"
        onChange={() => undefined}
        onSave={() => undefined}
        onError={onError}
        documentKey="second.md"
        onImageImport={onImageImport}
      />,
    );
    resolveImport?.({
      relativePath: "assets/" + "e".repeat(64) + ".png",
      markdown: "![image](assets/" + "e".repeat(64) + ".png)",
      reused: false,
    });

    await waitFor(() => expect(onError).toHaveBeenLastCalledWith(IMAGE_STALE_ERROR));
    expect(view.state.doc.toString()).toBe("same note text");
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
