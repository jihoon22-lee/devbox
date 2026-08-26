import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { EditorView } from "@codemirror/view";
import { afterEach, describe, expect, it, vi } from "vitest";
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
  renameFile: vi.fn(async () => undefined),
  deleteFile: vi.fn(async () => undefined),
  entryPath: vi.fn(async (rel: string) => rel),
  revealEntry: vi.fn(async () => undefined),
  openTargets: vi.fn(async () => []),
  openIn: vi.fn(async () => undefined),
  readClipboardText: vi.fn(async () => ""),
  searchDocs: vi.fn(async () => []),
  dailyNote: vi.fn(async () => ["Journal/today.md", "# Today"] as [string, string]),
  renderMarkdown: vi.fn(async () => ({ title: null, tags: [], html: "<p>rendered</p>", mermaid: [] })),
  onDocsChanged: vi.fn(async () => () => undefined),
  takePendingOpen: vi.fn(async () => null),
  onOpenRequest: vi.fn(async () => () => undefined),
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
