import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { EditorView } from "@codemirror/view";
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import App from "./App";
import { analyzeWikilinks, backlinks, openInboundNote } from "./api";

vi.mock("./api", () => ({
  listTree: vi.fn(async () => [
    { path: "Current.md", is_dir: false },
    { path: "Source.md", is_dir: false },
  ]),
  listTags: vi.fn(async () => []),
  readFile: vi.fn(async (path: string) => path === "Current.md" ? "[[Missing]]" : "first\n  [[Current]]"),
  writeFile: vi.fn(async () => undefined),
  createFile: vi.fn(async () => undefined),
  createDirectory: vi.fn(async () => undefined),
  previewRename: vi.fn(async () => { throw new Error("unused"); }),
  applyRename: vi.fn(async () => { throw new Error("unused"); }),
  discardRenamePreview: vi.fn(async () => undefined),
  deleteFile: vi.fn(async () => undefined),
  entryPath: vi.fn(async (rel: string) => rel),
  revealEntry: vi.fn(async () => undefined),
  openTargets: vi.fn(async () => []),
  openIn: vi.fn(async () => undefined),
  readClipboardText: vi.fn(async () => ""),
  readClipboardImage: vi.fn(async () => null),
  saveImageAsset: vi.fn(async () => ({
    relativePath: "assets/" + "a".repeat(64) + ".png",
    markdown: "![image](assets/" + "a".repeat(64) + ".png)",
    reused: false,
  })),
  searchDocs: vi.fn(async () => []),
  dailyNote: vi.fn(async () => ["Journal/today.md", "# Today"] as [string, string]),
  renderMarkdown: vi.fn(async () => ({ title: null, tags: [], html: "<p>rendered</p>", mermaid: [] })),
  onDocsChanged: vi.fn(async () => () => undefined),
  takePendingOpen: vi.fn(async () => null),
  onOpenRequest: vi.fn(async () => () => undefined),
  onQuickCaptureRequested: vi.fn(async () => () => undefined),
  onQuickCaptureShortcutStatusChanged: vi.fn(async () => () => undefined),
  quickCaptureShortcutStatus: vi.fn(async () => ({ shortcut: "Ctrl+Alt+K", state: "registered" })),
  previewQuickCapture: vi.fn(async (input: { title: string; body: string; tags: string[] }) => ({
    previewId: "qc-1",
    target: "Inbox",
    ...input,
  })),
  saveQuickCapture: vi.fn(async () => ({ path: "Inbox/quick-capture-test.md" })),
  discardQuickCapturePreview: vi.fn(async () => undefined),
  listTemplates: vi.fn(async () => []),
  createTemplate: vi.fn(async () => { throw new Error("unused"); }),
  updateTemplate: vi.fn(async () => { throw new Error("unused"); }),
  deleteTemplate: vi.fn(async () => undefined),
  previewTemplate: vi.fn(async () => { throw new Error("unused"); }),
  saveTemplate: vi.fn(async () => { throw new Error("unused"); }),
  discardTemplatePreview: vi.fn(async () => undefined),
  analyzeWikilinks: vi.fn(async () => [{
    target: "Missing",
    label: "Missing",
    line: 1,
    column: 1,
    from: 0,
    to: 11,
    status: "missing",
    resolved_path: null,
  }]),
  wikilinkCandidates: vi.fn(async () => []),
  backlinks: vi.fn(async (rel: string) => rel === "Current.md" ? [{
    source_path: "Source.md",
    target: "Current",
    line: 2,
    column: 3,
  }] : []),
  openInboundNote: vi.fn(async (path: string) => ({
    path,
    content: path === "Source.md" ? "first\n  [[Current]]" : "[[Missing]]",
  })),
}));

const analyzeMock = vi.mocked(analyzeWikilinks);
const backlinksMock = vi.mocked(backlinks);
const openInboundNoteMock = vi.mocked(openInboundNote);
const originalRangeClientRects = Object.getOwnPropertyDescriptor(
  Range.prototype,
  "getClientRects",
);

beforeAll(() => {
  // CodeMirror measures a requested scroll position on the next animation frame.
  // jsdom has no Range geometry API, so give that test-only measurement an empty result.
  Object.defineProperty(Range.prototype, "getClientRects", {
    configurable: true,
    value: () => [],
  });
});

afterAll(() => {
  if (originalRangeClientRects) {
    Object.defineProperty(Range.prototype, "getClientRects", originalRangeClientRects);
  } else {
    Reflect.deleteProperty(Range.prototype, "getClientRects");
  }
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("Knowledge wikilink and backlink integration", () => {
  it("shows unresolved health and opens a backlink at the indexed source position", async () => {
    render(<App />);
    fireEvent.click(await screen.findByText("Current.md"));

    await waitFor(
      () => expect(analyzeMock).toHaveBeenCalledWith("[[Missing]]"),
      { timeout: 5_000 },
    );
    await waitFor(() => expect(backlinksMock).toHaveBeenCalledWith("Current.md"));
    expect(
      await screen.findByText("1 unresolved", undefined, { timeout: 5_000 }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Backlinks (1)" })).toBeInTheDocument();

    fireEvent.click(within(screen.getByLabelText("Backlinks")).getByRole(
      "button",
      { name: /Source\.md/u },
    ));
    await waitFor(() => expect(openInboundNoteMock).toHaveBeenCalledWith("Source.md"));
    await waitFor(() => expect(document.querySelector(".path")?.textContent).toBe("Source.md"));

    const content = document.querySelector(".cm-content");
    if (!(content instanceof HTMLElement)) throw new Error("CodeMirror content missing");
    const view = EditorView.findFromDOM(content);
    if (!view) throw new Error("CodeMirror view missing");
    await waitFor(() => expect(view.state.selection.main.head).toBe(8));
    expect(screen.queryByText("● unsaved")).toBeNull();
  }, 15_000);
});
