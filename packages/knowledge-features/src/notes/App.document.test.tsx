import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import App from "./App";
import { readFile, writeFile, openInboundNote, deleteFile } from "./api";
import type { NoteSnapshot } from "./api";
vi.mock("./api", () => {
  const TREE = [
    { path: "Notes", is_dir: true },
    { path: "Notes/nested.md", is_dir: false },
    { path: "note.md", is_dir: false },
    { path: "image.png", is_dir: false },
  ];
  return {
    loadNoteJournal: vi.fn().mockResolvedValue({ entries: [], otherVaultCount: 0 }),
    saveNoteJournal: vi.fn().mockResolvedValue(undefined),
    clearNoteJournal: vi.fn().mockResolvedValue(undefined),
    discardOtherVaultJournal: vi.fn().mockResolvedValue(undefined),
    listTree: vi.fn(async () => TREE),
    listTags: vi.fn(async () => [] as string[]),
    readFile: vi.fn(async (path: string) => ({
      content: path.endsWith(".md") ? "# Hello" : "binary-content",
      revision: "disk-1",
    })),
    openInboundNote: vi.fn(async () => ({ path: "note.md", content: "# Hello", revision: "disk-1" })),
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
    createTemplate: vi.fn(async () => {
      throw new Error("unused");
    }),
    updateTemplate: vi.fn(async () => {
      throw new Error("unused");
    }),
    deleteTemplate: vi.fn(async () => undefined),
    previewTemplate: vi.fn(async () => {
      throw new Error("unused");
    }),
    saveTemplate: vi.fn(async () => {
      throw new Error("unused");
    }),
    discardTemplatePreview: vi.fn(async () => undefined),
    readClipboardText: vi.fn(async () => ""),
    writeFile: vi.fn(async (_path: string, content: string) => ({ content, revision: "disk-2" })),
    createFile: vi.fn(async () => undefined),
    createDirectory: vi.fn(async () => undefined),
    previewRename: vi.fn(async (from: string, to: string) => ({
      planId: "rename-plan-1",
      from,
      to,
      isDir: false,
      items: [
        { path: `이름 변경 · ${from}`, before: from, after: to, meta: "파일 이동" },
        {
          path: "Projects/source.md",
          before: "L2: [[Notes/nested|별칭]]",
          after: "L2: [[Notes/renamed|별칭]]",
          meta: "위키링크 1개 갱신",
        },
      ],
    })),
    applyRename: vi.fn(async () => ({
      from: "Notes/nested.md",
      to: "Notes/renamed.md",
    })),
    discardRenamePreview: vi.fn(async () => undefined),
    deleteFile: vi.fn(async () => undefined),
    entryPath: vi.fn(async (rel: string) => `C:\\Knowledge\\${rel.replace(/\//g, "\\")}`),
    revealEntry: vi.fn(async () => undefined),
    openTargets: vi.fn(async () => [
      { id: "code-pad", displayName: "Code Pad" },
      { id: "workbench", displayName: "Workbench" },
    ]),
    openIn: vi.fn(async () => undefined),
    searchDocs: vi.fn(async () => []),
    readClipboardImage: vi.fn(async () => null),
    dailyNote: vi.fn(async () => ["daily.md", "# Today"] as [string, string]),
    saveImageAsset: vi.fn(async () => ({
      relativePath: "assets/" + "a".repeat(64) + ".png",
      markdown: "![image](assets/" + "a".repeat(64) + ".png)",
      reused: false,
    })),
    renderMarkdown: vi.fn(async () => ({ title: null, tags: [], html: "<p>rendered</p>", mermaid: [] })),
    analyzeWikilinks: vi.fn(async () => []),
    wikilinkCandidates: vi.fn(async () => []),
    backlinks: vi.fn(async () => []),
    onDocsChanged: vi.fn(async () => () => undefined),
    knowledgeWatcherStatus: vi.fn(async () => ({
      sourceKind: "wsl",
      watchMode: "polling",
      lastSyncedAt: 1_788_105_600_000,
      error: null,
    })),
    onKnowledgeWatcherStatus: vi.fn(async () => () => undefined),
  };
});

vi.mock("./components/MarkdownEditor", () => ({
  default: ({ value, onChange }: { value: string; onChange: (text: string) => void }) => (
    <textarea aria-label="note editor" value={value} onChange={(event) => onChange(event.target.value)} />
  ),
}));
function pending<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  vi.restoreAllMocks();
});
it("preserves edits after the save request and does not submit duplicate writes", async () => {
  const saving = pending<NoteSnapshot>();
  vi.mocked(writeFile).mockReturnValueOnce(saving.promise);
  render(<App />);
  fireEvent.click(await screen.findByText("note.md"));
  const editor = await screen.findByLabelText("note editor");
  fireEvent.change(editor, { target: { value: "submitted" } });
  fireEvent.click(screen.getByRole("button", { name: "저장" }));
  fireEvent.click(screen.getByRole("button", { name: "저장" }));
  expect(writeFile).toHaveBeenCalledTimes(1);
  expect(writeFile).toHaveBeenCalledWith("note.md", "submitted", "disk-1");
  fireEvent.change(editor, { target: { value: "new unsaved" } });
  await act(async () => {
    saving.resolve({ content: "submitted", revision: "saved-1" });
    await saving.promise;
  });
  expect(editor).toHaveValue("new unsaved");
  expect(screen.getByText("● 저장되지 않음")).toBeInTheDocument();
});
it("keeps text typed while another file is loading", async () => {
  const reading = pending<NoteSnapshot>();
  render(<App />);
  fireEvent.click(await screen.findByText("note.md"));
  const editor = await screen.findByLabelText("note editor");
  vi.mocked(readFile).mockReturnValueOnce(reading.promise);
  fireEvent.click(screen.getByText("nested.md"));
  fireEvent.change(editor, { target: { value: "typed while loading" } });
  await act(async () => {
    reading.resolve({ content: "other", revision: "other" });
    await reading.promise;
  });
  expect(editor).toHaveValue("typed while loading");
  expect(screen.getByText("● 저장되지 않음")).toBeInTheDocument();
});
it("ignores an older file response after an external openRequest selected a newer note", async () => {
  const first = pending<NoteSnapshot>(),
    second = pending<NoteSnapshot>();
  vi.mocked(readFile).mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
  const app = render(<App />);
  fireEvent.click(await screen.findByText("note.md"));
  app.rerender(<App openRequest={{ id: 1, path: "Notes/nested.md" }} />);
  await act(async () => {
    second.resolve({ content: "latest", revision: "latest" });
    await second.promise;
  });
  await act(async () => {
    first.resolve({ content: "older", revision: "older" });
    await first.promise;
  });
  expect(screen.getByLabelText("note editor")).toHaveValue("latest");
  expect(openInboundNote).not.toHaveBeenCalled();
});
it("offers disk comparison and reviewed overwrite without losing the draft on conflict", async () => {
  render(<App />);
  fireEvent.click(await screen.findByText("note.md"));
  const editor = await screen.findByLabelText("note editor");
  fireEvent.change(editor, { target: { value: "local draft" } });
  vi.mocked(writeFile).mockRejectedValueOnce(Object.assign(new Error("external change"), { name: "note_conflict" }));
  vi.mocked(readFile).mockResolvedValueOnce({ content: "external version", revision: "external-2" });
  fireEvent.click(screen.getByRole("button", { name: "저장" }));
  await screen.findByRole("region", { name: "노트 저장 충돌" });
  expect(editor).toHaveValue("local draft");
  vi.spyOn(window, "confirm").mockReturnValue(true);
  fireEvent.click(screen.getByRole("button", { name: /비교한 내용에 덮어쓰기/ }));
  await act(async () => {});
  expect(writeFile).toHaveBeenLastCalledWith("note.md", "local draft", "external-2");
});

it("keeps another note's unsaved buffer when a delayed context-menu deletion completes", async () => {
  vi.spyOn(window, "confirm").mockReturnValue(true);
  const deleting = pending<void>();
  vi.mocked(deleteFile).mockReturnValueOnce(deleting.promise);
  render(<App />);
  const first = await screen.findByText("note.md");
  fireEvent.click(first);
  await screen.findByLabelText("note editor");
  fireEvent.contextMenu(first.closest("button")!);
  fireEvent.click(screen.getByRole("menuitem", { name: "삭제" }));
  expect(deleteFile).toHaveBeenCalledWith("note.md");
  fireEvent.click(screen.getByText("nested.md"));
  await act(async () => {});
  fireEvent.change(screen.getByLabelText("note editor"), { target: { value: "B UNSAVED TEXT" } });
  await act(async () => {
    deleting.resolve();
    await deleting.promise;
  });
  expect(screen.getByLabelText("note editor")).toHaveValue("B UNSAVED TEXT");
  expect(screen.getByText("● 저장되지 않음")).toBeInTheDocument();
});
