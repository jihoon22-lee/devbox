import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import {
  applyRename,
  createDirectory,
  createFile,
  deleteFile,
  discardRenamePreview,
  entryPath,
  openIn,
  openTargets,
  previewRename,
  readFile,
  revealEntry,
} from "./api";

const writeTextMock = vi.fn<(text: string) => Promise<void>>();

/** 폴더, .md 파일, .png 파일을 포함한 tree fixture. */
vi.mock("./api", () => {
  const TREE = [
    { path: "Notes", is_dir: true },
    { path: "Notes/nested.md", is_dir: false },
    { path: "note.md", is_dir: false },
    { path: "image.png", is_dir: false },
  ];
  return {
    listTree: vi.fn(async () => TREE),
    listTags: vi.fn(async () => [] as string[]),
    readFile: vi.fn(async (path: string) => (path.endsWith(".md") ? "# Hello" : "binary-content")),
    openInboundNote: vi.fn(async () => ({ path: "note.md", content: "# Hello" })),
    takePendingOpen: vi.fn(async () => null),
    onOpenRequest: vi.fn(async () => () => undefined),
    onQuickCaptureRequested: vi.fn(async () => () => undefined),
    onQuickCaptureShortcutStatusChanged: vi.fn(async () => () => undefined),
    quickCaptureShortcutStatus: vi.fn(async () => ({ shortcut: "Ctrl+Alt+K", state: "registered" })),
    previewQuickCapture: vi.fn(async (input: { title: string; body: string; tags: string[] }) => ({
      target: "Inbox",
      ...input,
    })),
    saveQuickCapture: vi.fn(async () => ({ path: "Inbox/quick-capture-test.md" })),
    readClipboardText: vi.fn(async () => ""),
    writeFile: vi.fn(async () => undefined),
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
    dailyNote: vi.fn(async () => ["daily.md", "# Today"] as [string, string]),
    renderMarkdown: vi.fn(async () => ({ title: null, tags: [], html: "<p>rendered</p>", mermaid: [] })),
    analyzeWikilinks: vi.fn(async () => []),
    wikilinkCandidates: vi.fn(async () => []),
    backlinks: vi.fn(async () => []),
    onDocsChanged: vi.fn(async () => () => undefined),
  };
});

const createFileMock = vi.mocked(createFile);
const createDirectoryMock = vi.mocked(createDirectory);
const previewRenameMock = vi.mocked(previewRename);
const applyRenameMock = vi.mocked(applyRename);
const discardRenamePreviewMock = vi.mocked(discardRenamePreview);
const deleteFileMock = vi.mocked(deleteFile);
const entryPathMock = vi.mocked(entryPath);
const revealEntryMock = vi.mocked(revealEntry);
const openTargetsMock = vi.mocked(openTargets);
const openInMock = vi.mocked(openIn);
const readFileMock = vi.mocked(readFile);

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  createFileMock.mockClear();
  createDirectoryMock.mockClear();
  previewRenameMock.mockClear();
  applyRenameMock.mockClear();
  discardRenamePreviewMock.mockClear();
  deleteFileMock.mockClear();
  entryPathMock.mockClear();
  revealEntryMock.mockClear();
  openTargetsMock.mockClear();
  openInMock.mockClear();
  readFileMock.mockClear();
});

describe("knowledge-base App — 모드 토글 & 프리뷰 비활성화", () => {
  it(".md 파일을 열면 분할/프리뷰 버튼이 활성화된다", async () => {
    render(<App />);

    fireEvent.click(await screen.findByText("note.md"));

    const previewBtn = await screen.findByRole("button", { name: "프리뷰" });
    const splitBtn = await screen.findByRole("button", { name: "분할" });
    expect(previewBtn).not.toBeDisabled();
    expect(splitBtn).not.toBeDisabled();
  });

  it(".md가 아닌 파일을 열면 분할/프리뷰 버튼이 비활성화된다", async () => {
    render(<App />);

    fireEvent.click(await screen.findByText("image.png"));

    const previewBtn = await screen.findByRole("button", { name: "프리뷰" });
    const splitBtn = await screen.findByRole("button", { name: "분할" });
    expect(previewBtn).toBeDisabled();
    expect(splitBtn).toBeDisabled();
  });

  it("편집 -> 분할 -> 프리뷰 전환에 따라 editor-body의 mode-* 클래스가 바뀐다", async () => {
    const { container } = render(<App />);
    const body = () => container.querySelector(".editor-body");

    // openFile은 readFile을 await하므로 클릭 직후엔 아직 상태가 안 바뀌어 있을 수 있다 —
    // waitFor로 비동기 상태 반영을 기다린 뒤 단언한다.
    fireEvent.click(await screen.findByText("note.md"));
    await waitFor(() => expect(body()?.className).toContain("mode-edit"));

    fireEvent.click(screen.getByRole("button", { name: "분할" }));
    await waitFor(() => expect(body()?.className).toContain("mode-split"));

    fireEvent.click(screen.getByRole("button", { name: "프리뷰" }));
    await waitFor(() => expect(body()?.className).toContain("mode-preview"));

    fireEvent.click(screen.getByRole("button", { name: "편집" }));
    await waitFor(() => expect(body()?.className).toContain("mode-edit"));
  });

  it("분할/프리뷰 모드로 본 뒤 .md가 아닌 파일로 전환하면 편집 모드로 강제 복귀한다", async () => {
    const { container } = render(<App />);
    const body = () => container.querySelector(".editor-body");

    fireEvent.click(await screen.findByText("note.md"));
    await waitFor(() => expect(body()?.className).toContain("mode-edit"));

    fireEvent.click(screen.getByRole("button", { name: "프리뷰" }));
    await waitFor(() => expect(body()?.className).toContain("mode-preview"));

    fireEvent.click(await screen.findByText("image.png"));
    await waitFor(() => expect(body()?.className).toContain("mode-edit"));
  });
});

function treeButton(nameOrNode: string | HTMLElement): HTMLButtonElement {
  const button = typeof nameOrNode === "string"
    ? Array.from(document.querySelectorAll<HTMLButtonElement>("button[data-tree-path]"))
        .find((candidate) => candidate.dataset.treePath === nameOrNode) ?? null
    : nameOrNode.closest("button");
  if (!(button instanceof HTMLButtonElement)) throw new Error("tree button was not rendered");
  return button;
}

describe("knowledge-base App — tree context menu", () => {
  it("우클릭한 폴더를 먼저 선택하고 정확한 앱 소유 메뉴를 표시한다", async () => {
    render(<App />);
    await screen.findByText("nested.md");
    const notes = treeButton("Notes");

    fireEvent.contextMenu(notes, { clientX: 20, clientY: 30 });

    expect(notes).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("menu", { name: "Knowledge 트리 작업" })).toBeInTheDocument();
    for (const label of [
      "새 파일",
      "새 폴더",
      "이름 변경",
      "삭제",
      "경로 복사",
      "탐색기에서 열기",
      "다른 앱으로 열기",
    ]) {
      expect(screen.getByRole("menuitem", { name: label })).toBeInTheDocument();
    }
    expect(screen.getByRole("menuitem", { name: "삭제" })).toHaveClass("danger");
    await waitFor(() => expect(
      screen.getByRole("menuitem", { name: "다른 앱으로 열기" }),
    ).not.toHaveAttribute("aria-disabled"));
  });

  it("대상 폴더를 기준으로 새 파일과 새 폴더를 만든다", async () => {
    const promptMock = vi.spyOn(window, "prompt");
    render(<App />);
    await screen.findByText("nested.md");
    const notes = treeButton("Notes");

    promptMock.mockReturnValueOnce("Notes/idea.md");
    fireEvent.contextMenu(notes);
    fireEvent.click(screen.getByRole("menuitem", { name: "새 파일" }));
    await waitFor(() => expect(createFileMock).toHaveBeenCalledWith(
      "Notes/idea.md",
      "---\ntitle: \n---\n\n",
    ));

    promptMock.mockReturnValueOnce("Notes/Archive");
    fireEvent.contextMenu(notes);
    fireEvent.click(screen.getByRole("menuitem", { name: "새 폴더" }));
    await waitFor(() => expect(createDirectoryMock).toHaveBeenCalledWith("Notes/Archive"));
  });

  it("이름변경 diff를 먼저 표시하고 전체 승인 뒤에만 transaction을 적용한다", async () => {
    vi.spyOn(window, "prompt").mockReturnValueOnce("Notes/renamed.md");
    const confirmMock = vi.spyOn(window, "confirm").mockReturnValue(true);
    render(<App />);
    const nested = treeButton(await screen.findByText("nested.md"));

    fireEvent.contextMenu(nested);
    fireEvent.click(screen.getByRole("menuitem", { name: "이름 변경" }));
    await waitFor(() => expect(previewRenameMock).toHaveBeenCalledWith(
      "Notes/nested.md",
      "Notes/renamed.md",
    ));
    expect(applyRenameMock).not.toHaveBeenCalled();
    const dialog = screen.getByRole("dialog", { name: "이름 변경 미리보기" });
    expect(dialog).toHaveTextContent("Projects/source.md");
    expect(dialog).toHaveTextContent("[[Notes/nested|별칭]]");
    expect(dialog).toHaveTextContent("[[Notes/renamed|별칭]]");
    expect(within(dialog).queryByRole("checkbox")).toBeNull();

    fireEvent.click(within(dialog).getByRole("button", { name: "전체 적용 (2)" }));
    await waitFor(() => expect(applyRenameMock).toHaveBeenCalledWith("rename-plan-1"));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());

    fireEvent.contextMenu(nested);
    fireEvent.click(screen.getByRole("menuitem", { name: "삭제" }));
    await waitFor(() => expect(deleteFileMock).toHaveBeenCalledWith("Notes/nested.md"));
    expect(confirmMock).toHaveBeenCalledWith(expect.stringContaining("되돌릴 수 없습니다"));
  });

  it("이름변경 미리보기를 취소하면 backend의 보관 plan도 폐기한다", async () => {
    vi.spyOn(window, "prompt").mockReturnValueOnce("Notes/renamed.md");
    render(<App />);
    const nested = treeButton(await screen.findByText("nested.md"));

    fireEvent.contextMenu(nested);
    fireEvent.click(screen.getByRole("menuitem", { name: "이름 변경" }));
    const dialog = await screen.findByRole("dialog", { name: "이름 변경 미리보기" });
    expect(within(dialog).getByRole("button", { name: "취소" })).toBeInTheDocument();
    fireEvent.keyDown(window, { key: "Escape" });

    await waitFor(() => expect(discardRenamePreviewMock).toHaveBeenCalledWith("rename-plan-1"));
    expect(applyRenameMock).not.toHaveBeenCalled();
  });

  it("transaction 성공 뒤 재읽기만 실패하면 stale 본문을 지우고 metadata 갱신을 유지한다", async () => {
    vi.spyOn(window, "prompt").mockReturnValueOnce("Notes/renamed.md");
    render(<App />);
    const nested = treeButton(await screen.findByText("nested.md"));
    fireEvent.click(nested);
    await waitFor(() => expect(document.querySelector(".path")?.textContent).toBe("Notes/nested.md"));
    readFileMock.mockRejectedValueOnce(new Error("raw filesystem error"));

    fireEvent.contextMenu(nested);
    fireEvent.click(screen.getByRole("menuitem", { name: "이름 변경" }));
    const dialog = await screen.findByRole("dialog", { name: "이름 변경 미리보기" });
    fireEvent.click(within(dialog).getByRole("button", { name: "전체 적용 (2)" }));

    expect(await screen.findByText("이름은 변경했지만 현재 노트를 다시 읽지 못했습니다"))
      .toBeInTheDocument();
    expect(document.body.textContent).not.toContain("raw filesystem error");
    expect(document.querySelector(".path")?.textContent).toBe("Notes/renamed.md");
    expect(document.querySelector(".cm-content")?.textContent).toBe("");
  });

  it("검증된 absolute path 복사·탐색기 표시·catalog 대상 열기를 실행한다", async () => {
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText: writeTextMock },
    });
    writeTextMock.mockResolvedValue(undefined);
    render(<App />);
    const nested = treeButton(await screen.findByText("nested.md"));

    fireEvent.contextMenu(nested);
    fireEvent.click(screen.getByRole("menuitem", { name: "경로 복사" }));
    await waitFor(() => expect(entryPathMock).toHaveBeenCalledWith("Notes/nested.md"));
    expect(writeTextMock).toHaveBeenCalledWith("C:\\Knowledge\\Notes\\nested.md");

    fireEvent.contextMenu(nested);
    fireEvent.click(screen.getByRole("menuitem", { name: "탐색기에서 열기" }));
    await waitFor(() => expect(revealEntryMock).toHaveBeenCalledWith("Notes/nested.md"));

    fireEvent.contextMenu(nested);
    const submenu = screen.getByRole("menuitem", { name: "다른 앱으로 열기" });
    fireEvent.mouseEnter(submenu);
    fireEvent.click(await screen.findByRole("menuitem", { name: "Code Pad" }));
    await waitFor(() => expect(openInMock).toHaveBeenCalledWith("code-pad", "Notes/nested.md"));
  });

  it("Shift+F10/Menu key를 지원하고 닫힌 뒤 원래 tree row로 focus를 복구한다", async () => {
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText: writeTextMock },
    });
    writeTextMock.mockResolvedValue(undefined);
    render(<App />);
    const nested = treeButton(await screen.findByText("nested.md"));
    nested.focus();

    fireEvent.keyDown(nested, { key: "F10", code: "F10", shiftKey: true });
    fireEvent.click(screen.getByRole("menuitem", { name: "경로 복사" }));

    await waitFor(() => expect(document.activeElement).toBe(nested));
  });

  it("설치 대상이 없으면 submenu를 비활성화하고 action 실패를 복구 가능한 오류로 표시한다", async () => {
    openTargetsMock.mockResolvedValueOnce([]);
    revealEntryMock.mockRejectedValueOnce(new Error("표시 실패"));
    render(<App />);
    const nested = treeButton(await screen.findByText("nested.md"));

    fireEvent.contextMenu(nested);
    expect(screen.getByRole("menuitem", { name: "다른 앱으로 열기" })).toHaveAttribute(
      "aria-disabled",
      "true",
    );
    fireEvent.click(screen.getByRole("menuitem", { name: "탐색기에서 열기" }));
    expect(await screen.findByText("표시 실패")).toBeInTheDocument();
  });
});
