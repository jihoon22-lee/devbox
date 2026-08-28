import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import {
  onOpenRequest,
  openInboundNote,
  previewKnowledgeDraft,
  saveKnowledgeDraft,
  discardKnowledgeDraft,
  searchDocs,
  takePendingOpen,
  type OpenRequest,
  type KnowledgeDraftPreview,
} from "./api";

const mocks = vi.hoisted(() => ({
  openHandler: null as ((request: OpenRequest) => void) | null,
  quickCaptureHandler: null as (() => void) | null,
  shortcutStatusHandler: null as ((status: { shortcut: string; state: string }) => void) | null,
  order: [] as string[],
}));

vi.mock("./api", () => ({
  listTree: vi.fn(async () => [{ path: "Notes/existing.md", is_dir: false }]),
  listTags: vi.fn(async () => [] as string[]),
  readFile: vi.fn(async () => "# existing"),
  writeFile: vi.fn(async () => undefined),
  previewKnowledgeDraft: vi.fn(),
  saveKnowledgeDraft: vi.fn(),
  discardKnowledgeDraft: vi.fn(async () => undefined),
  renewKnowledgeDraft: vi.fn(async () => ({ leaseUntilMs: Date.now() + 60_000 })),
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
  dailyNote: vi.fn(async () => ["Journal/today.md", "# Today"] as [string, string]),
  renderMarkdown: vi.fn(async () => ({ title: null, tags: [], html: "<p>rendered</p>", mermaid: [] })),
  analyzeWikilinks: vi.fn(async () => []),
  wikilinkCandidates: vi.fn(async () => []),
  backlinks: vi.fn(async () => []),
  onDocsChanged: vi.fn(async () => () => undefined),
  onQuickCaptureRequested: vi.fn().mockImplementation(async (handler: () => void) => {
    mocks.quickCaptureHandler = handler;
    return () => undefined;
  }),
  onQuickCaptureShortcutStatusChanged: vi.fn().mockImplementation(async (handler: (status: { shortcut: string; state: string }) => void) => {
    mocks.shortcutStatusHandler = handler;
    return () => undefined;
  }),
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
  openInboundNote: vi.fn(async () => ({ path: "Notes/inbound.md", content: "# inbound" })),
  searchDocs: vi.fn(async (query: string) => [{ path: "Notes/result.md", title: `Result ${query}` }]),
  takePendingOpen: vi.fn().mockImplementation(async () => {
    mocks.order.push("take");
    return null;
  }),
  onOpenRequest: vi.fn().mockImplementation(async (handler: (request: OpenRequest) => void) => {
    mocks.order.push("listen");
    mocks.openHandler = handler;
    return () => undefined;
  }),
}));

const takePendingOpenMock = vi.mocked(takePendingOpen);
const onOpenRequestMock = vi.mocked(onOpenRequest);
const openInboundNoteMock = vi.mocked(openInboundNote);
const searchDocsMock = vi.mocked(searchDocs);
const previewKnowledgeDraftMock = vi.mocked(previewKnowledgeDraft);
const saveKnowledgeDraftMock = vi.mocked(saveKnowledgeDraft);
const discardKnowledgeDraftMock = vi.mocked(discardKnowledgeDraft);

beforeEach(() => {
  mocks.openHandler = null;
  mocks.quickCaptureHandler = null;
  mocks.shortcutStatusHandler = null;
  mocks.order.length = 0;
  takePendingOpenMock.mockReset().mockImplementation(async () => {
    mocks.order.push("take");
    return null;
  });
  onOpenRequestMock.mockReset().mockImplementation(async (handler) => {
    mocks.order.push("listen");
    mocks.openHandler = handler;
    return () => undefined;
  });
  openInboundNoteMock.mockReset().mockResolvedValue({ path: "Notes/inbound.md", content: "# inbound" });
  searchDocsMock.mockReset().mockImplementation(async (query) => [
    { path: "Notes/result.md", title: `Result ${query}` },
  ]);
  previewKnowledgeDraftMock.mockReset().mockResolvedValue({
    id: "0123456789abcdef0123456789abcdef",
    kind: "knowledge-draft/v1",
    expiresAtMs: Date.now() + 600_000,
    leaseUntilMs: Date.now() + 60_000,
    title: "Life Log digest · 2026-08-27 ~ 2026-08-27",
    body: "# Life Log local digest\n\n## Summary\n\n| Sessions | 0 |",
    tags: ["life-log", "digest", "day"],
    summary: {
      period: "day",
      startDate: "2026-08-27",
      endDate: "2026-08-27",
      timezone: "UTC",
      filter: null,
      pcUsageMs: 0,
      sessionCount: 0,
      activeDays: 0,
      totalDays: 1,
      averageDailyUsageMs: 0,
      gitCommits: 0,
      topApp: null,
    },
    sources: [],
  });
  saveKnowledgeDraftMock.mockReset().mockResolvedValue({
    saved: true,
    path: "Journal/2026-08-27-life-log-day.md",
    handoffDeleted: true,
  });
  discardKnowledgeDraftMock.mockReset().mockResolvedValue(undefined);
});

afterEach(() => cleanup());

describe("Knowledge Path/Query app-link delivery", () => {
  it("opens the same modal from the native quick-capture event and the in-app button", async () => {
    render(<App />);
    await waitFor(() => expect(mocks.quickCaptureHandler).not.toBeNull());

    mocks.quickCaptureHandler?.();
    expect(await screen.findByRole("dialog", { name: "빠른 캡처" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "빠른 캡처 닫기" }));

    fireEvent.click(screen.getByRole("button", { name: /빠른 캡처 Ctrl\+Alt\+K/u }));
    expect(await screen.findByRole("dialog", { name: "빠른 캡처" })).toBeInTheDocument();
  });

  it("keeps the in-app action available when the global shortcut is occupied", async () => {
    render(<App />);
    await waitFor(() => expect(mocks.shortcutStatusHandler).not.toBeNull());
    mocks.shortcutStatusHandler?.({ shortcut: "Ctrl+Alt+K", state: "conflict" });
    expect(await screen.findByText(/전역 단축키 Ctrl\+Alt\+K를 등록하지 못했습니다/u)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /빠른 캡처 Ctrl\+Alt\+K/u }));
    expect(await screen.findByRole("dialog", { name: "빠른 캡처" })).toBeInTheDocument();
  });

  it("registers the listener before taking and opens a cold-start Path once", async () => {
    takePendingOpenMock.mockImplementationOnce(async () => {
      mocks.order.push("take");
      return {
        target: { kind: "path", path: "C:\\Knowledge\\Notes\\inbound.md", line: null, column: null },
        from: "devbox-launcher",
      };
    });

    render(<App />);

    await waitFor(() => expect(openInboundNoteMock).toHaveBeenCalledWith("C:\\Knowledge\\Notes\\inbound.md"));
    expect(mocks.order.slice(0, 2)).toEqual(["listen", "take"]);
    expect(await screen.findByText("Notes/inbound.md")).toBeTruthy();
    expect(takePendingOpenMock).toHaveBeenCalledTimes(1);
  });

  it("uses the pending slot rather than the hot event payload and routes Query to search", async () => {
    render(<App />);
    await waitFor(() => expect(mocks.openHandler).not.toBeNull());
    await waitFor(() => expect(takePendingOpenMock).toHaveBeenCalledTimes(1));

    takePendingOpenMock.mockResolvedValueOnce({
      target: { kind: "query", text: "  ownership  " },
      from: "devbox-launcher",
    });
    await act(async () => {
      mocks.openHandler?.({
        target: { kind: "path", path: "stale-secret-path", line: null, column: null },
        from: "stale",
      });
    });

    await waitFor(() => expect(takePendingOpenMock).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(searchDocsMock).toHaveBeenCalledWith("ownership"));
    expect(screen.getByPlaceholderText("Search docs...")).toHaveValue("ownership");
    expect(await screen.findByText("Result ownership")).toBeTruthy();
    expect(openInboundNoteMock).not.toHaveBeenCalled();
    expect(screen.queryByText(/stale-secret-path/)).toBeNull();
  });

  it("falls back to cold pull when listener setup fails and keeps invalid Path errors generic", async () => {
    onOpenRequestMock.mockRejectedValueOnce(new Error("listener unavailable"));
    takePendingOpenMock.mockResolvedValueOnce({
      target: { kind: "path", path: "C:\\secret\\outside.md", line: null, column: null },
      from: null,
    });
    openInboundNoteMock.mockRejectedValueOnce(new Error("raw path must stay hidden"));

    render(<App />);

    expect(await screen.findByText("요청한 노트를 열 수 없습니다")).toBeTruthy();
    expect(screen.getByText("Knowledge")).toBeTruthy();
    expect(document.body.textContent).not.toContain("C:\\secret\\outside.md");
    expect(document.body.textContent).not.toContain("raw path must stay hidden");
  });

  it("surfaces invalid Query without searching or closing the app", async () => {
    takePendingOpenMock.mockResolvedValueOnce({
      target: { kind: "query", text: "   " },
      from: null,
    });

    render(<App />);

    expect(await screen.findByText("요청한 검색어를 사용할 수 없습니다")).toBeTruthy();
    expect(screen.getByText("Knowledge")).toBeTruthy();
    expect(searchDocsMock).not.toHaveBeenCalled();
  });

  it("previews a knowledge-draft handoff and saves only after explicit confirmation", async () => {
    takePendingOpenMock.mockResolvedValueOnce({
      target: {
        kind: "handoff",
        handoffKind: "knowledge-draft/v1",
        id: "0123456789abcdef0123456789abcdef",
      },
      from: "life-log",
    });

    render(<App />);

    expect(await screen.findByRole("heading", { name: "Life Log draft 미리보기" })).toBeTruthy();
    expect(screen.getByText("Life Log digest · 2026-08-27 ~ 2026-08-27")).toBeTruthy();
    expect(screen.getByText("life-log, digest, day")).toBeTruthy();
    expect(screen.getByLabelText("Knowledge draft body")).toHaveTextContent("## Summary");
    expect(screen.getByLabelText("Knowledge draft size")).toHaveTextContent(/Title .* bytes · Body .* bytes/u);
    expect(screen.getByRole("dialog").getAttribute("aria-describedby")).toBe("knowledge-draft-description");
    expect(saveKnowledgeDraftMock).not.toHaveBeenCalled();

    await act(async () => {
      screen.getByRole("button", { name: "Save draft" }).click();
    });
    await waitFor(() => expect(saveKnowledgeDraftMock).toHaveBeenCalledWith("0123456789abcdef0123456789abcdef"));
    expect(discardKnowledgeDraftMock).not.toHaveBeenCalled();
    expect(await screen.findByText("Knowledge draft를 저장했습니다. handoff는 소비되어 삭제되었습니다.")).toBeTruthy();
  });

  it("cancels a handoff preview by restoring the one-time claim without saving", async () => {
    takePendingOpenMock.mockResolvedValueOnce({
      target: {
        kind: "handoff",
        handoffKind: "knowledge-draft/v1",
        id: "0123456789abcdef0123456789abcdef",
      },
      from: "life-log",
    });

    render(<App />);
    await screen.findByRole("heading", { name: "Life Log draft 미리보기" });
    fireEvent.click(screen.getByRole("button", { name: "취소" }));

    await waitFor(() => expect(discardKnowledgeDraftMock).toHaveBeenCalledWith("0123456789abcdef0123456789abcdef"));
    expect(saveKnowledgeDraftMock).not.toHaveBeenCalled();
    expect(await screen.findByText("Knowledge draft 미리보기를 취소했습니다. 다시 열 수 있습니다.")).toBeTruthy();
  });

  it("maps Escape to cancel and restores focus to the invoking control", async () => {
    render(
      <>
        <button type="button">Open handoff</button>
        <App />
      </>,
    );
    await waitFor(() => expect(mocks.openHandler).not.toBeNull());
    const opener = screen.getByRole("button", { name: "Open handoff" });
    opener.focus();
    takePendingOpenMock.mockResolvedValueOnce({
      target: {
        kind: "handoff",
        handoffKind: "knowledge-draft/v1",
        id: "0123456789abcdef0123456789abcdef",
      },
      from: "life-log",
    });
    await act(async () => {
      mocks.openHandler?.({
        target: {
          kind: "handoff",
          handoffKind: "knowledge-draft/v1",
          id: "0123456789abcdef0123456789abcdef",
        },
        from: "life-log",
      });
    });
    await screen.findByRole("heading", { name: "Life Log draft 미리보기" });

    fireEvent.keyDown(window, { key: "Escape" });
    await waitFor(() => expect(discardKnowledgeDraftMock).toHaveBeenCalledWith("0123456789abcdef0123456789abcdef"));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    await waitFor(() => expect(document.activeElement).toBe(opener));
    expect(saveKnowledgeDraftMock).not.toHaveBeenCalled();
  });

  it("traps modal focus and turns a stale save into a fixed regenerate message", async () => {
    takePendingOpenMock.mockResolvedValueOnce({
      target: {
        kind: "handoff",
        handoffKind: "knowledge-draft/v1",
        id: "0123456789abcdef0123456789abcdef",
      },
      from: "life-log",
    });
    saveKnowledgeDraftMock.mockRejectedValueOnce(
      new Error("Knowledge 저장 위치가 변경되어 다시 확인해야 합니다: /raw/path"),
    );

    render(<App />);
    await screen.findByRole("heading", { name: "Life Log draft 미리보기" });
    const cancel = screen.getByRole("button", { name: "취소" });
    const save = screen.getByRole("button", { name: "Save draft" });
    save.focus();
    fireEvent.keyDown(window, { key: "Tab" });
    expect(document.activeElement).toBe(cancel);

    fireEvent.click(save);
    await waitFor(() => expect(saveKnowledgeDraftMock).toHaveBeenCalled());
    expect(await screen.findByText(/저장 위치가 변경되었거나 draft가 만료되었습니다/u)).toBeTruthy();
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(document.body.textContent).not.toContain("/raw/path");
  });

  it("restores a late native claim after the app unmounts", async () => {
    let resolvePreview: ((preview: KnowledgeDraftPreview) => void) | undefined;
    previewKnowledgeDraftMock.mockImplementationOnce(() => new Promise<KnowledgeDraftPreview>((resolve) => {
      resolvePreview = resolve;
    }));
    takePendingOpenMock.mockResolvedValueOnce({
      target: {
        kind: "handoff",
        handoffKind: "knowledge-draft/v1",
        id: "0123456789abcdef0123456789abcdef",
      },
      from: "life-log",
    });

    const { unmount } = render(<App />);
    await waitFor(() => expect(previewKnowledgeDraftMock).toHaveBeenCalled());
    unmount();
    await act(async () => {
      resolvePreview?.({ id: "0123456789abcdef0123456789abcdef" } as KnowledgeDraftPreview);
      await Promise.resolve();
    });
    expect(discardKnowledgeDraftMock).toHaveBeenCalledWith("0123456789abcdef0123456789abcdef");
    expect(saveKnowledgeDraftMock).not.toHaveBeenCalled();
  });

  it("allows only one in-flight Save action", async () => {
    let resolveSave: ((result: { saved: boolean; path: string; handoffDeleted: boolean }) => void) | undefined;
    saveKnowledgeDraftMock.mockImplementationOnce(() => new Promise<{ saved: boolean; path: string; handoffDeleted: boolean }>((resolve) => {
      resolveSave = resolve;
    }));
    takePendingOpenMock.mockResolvedValueOnce({
      target: {
        kind: "handoff",
        handoffKind: "knowledge-draft/v1",
        id: "0123456789abcdef0123456789abcdef",
      },
      from: "life-log",
    });

    render(<App />);
    await screen.findByRole("heading", { name: "Life Log draft 미리보기" });
    const save = screen.getByRole("button", { name: "Save draft" });
    fireEvent.click(save);
    fireEvent.click(save);
    expect(saveKnowledgeDraftMock).toHaveBeenCalledTimes(1);

    await act(async () => {
      resolveSave?.({
        saved: true,
        path: "Journal/2026-08-27-life-log-day.md",
        handoffDeleted: true,
      });
      await Promise.resolve();
    });
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });
});
