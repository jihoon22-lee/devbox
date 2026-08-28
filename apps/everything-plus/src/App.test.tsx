import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import {
  cancelIndex,
  copyPath,
  deleteSavedQuery,
  indexStatus,
  listSavedQueries,
  onOpenRequest,
  openFile,
  openIn,
  openTargets,
  revealFile,
  saveSavedQuery,
  searchContent,
  searchFiles,
  takePendingOpen,
  type OpenRequest,
} from "./api";

const mocks = vi.hoisted(() => ({
  openHandler: null as ((request: OpenRequest) => void) | null,
  order: [] as string[],
}));

vi.mock("./api", () => ({
  indexStatus: vi.fn(async () => ({
    indexing: false,
    cancel_requested: false,
    total_files: 1,
    indexed_files: 1,
    content_indexed_files: 1,
    content_truncated_files: 0,
    content_failed_files: 0,
    roots: 1,
    last_indexed_at: null,
    last_error: null,
  })),
  listRoots: vi.fn(async () => []),
  listSavedQueries: vi.fn(async () => []),
  watcherStatuses: vi.fn(async () => []),
  searchFiles: vi.fn(async (query: string) => [{ id: 1, path: `C:\\files\\${query}`, name: query, ext: "", size: 1, modified_ts: 0 }]),
  searchContent: vi.fn(async () => []),
  addRoot: vi.fn(async () => undefined),
  removeRoot: vi.fn(async () => undefined),
  indexNow: vi.fn(async () => undefined),
  cancelIndex: vi.fn(async () => undefined),
  openFile: vi.fn(async () => undefined),
  revealFile: vi.fn(async () => undefined),
  copyPath: vi.fn(async () => undefined),
  openTargets: vi.fn(async () => [
    { id: "code-pad", displayName: "Code Pad" },
    { id: "workbench", displayName: "Workbench" },
  ]),
  openIn: vi.fn(async () => undefined),
  saveSavedQuery: vi.fn(async () => ({
    id: 1,
    name: "saved",
    query: "query",
    filter: {},
    createdAt: 0,
    updatedAt: 0,
  })),
  deleteSavedQuery: vi.fn(async () => undefined),
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
const searchFilesMock = vi.mocked(searchFiles);
const searchContentMock = vi.mocked(searchContent);
const indexStatusMock = vi.mocked(indexStatus);
const listSavedQueriesMock = vi.mocked(listSavedQueries);
const saveSavedQueryMock = vi.mocked(saveSavedQuery);
const deleteSavedQueryMock = vi.mocked(deleteSavedQuery);
const cancelIndexMock = vi.mocked(cancelIndex);
const openFileMock = vi.mocked(openFile);
const revealFileMock = vi.mocked(revealFile);
const copyPathMock = vi.mocked(copyPath);
const openTargetsMock = vi.mocked(openTargets);
const openInMock = vi.mocked(openIn);
const writeTextMock = vi.fn<(text: string) => Promise<void>>();

beforeEach(() => {
  mocks.openHandler = null;
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
  searchFilesMock.mockReset().mockImplementation(async (query: string) => [
    { id: 1, path: `C:\\files\\${query}`, name: query, ext: "", size: 1, modified_ts: 0 },
  ]);
  searchContentMock.mockReset().mockResolvedValue([]);
  listSavedQueriesMock.mockReset().mockResolvedValue([]);
  saveSavedQueryMock.mockReset().mockResolvedValue({
    id: 1,
    name: "saved",
    query: "fixture",
    filter: {},
    createdAt: 0,
    updatedAt: 0,
  });
  deleteSavedQueryMock.mockReset().mockResolvedValue(undefined);
  indexStatusMock.mockReset().mockResolvedValue({
    indexing: false,
    cancel_requested: false,
    total_files: 1,
    indexed_files: 1,
    content_indexed_files: 1,
    content_truncated_files: 0,
    content_failed_files: 0,
    roots: 1,
    last_indexed_at: null,
    last_error: null,
  });
  cancelIndexMock.mockReset().mockResolvedValue(undefined);
  openFileMock.mockReset().mockResolvedValue(undefined);
  revealFileMock.mockReset().mockResolvedValue(undefined);
  copyPathMock.mockReset().mockResolvedValue(undefined);
  openTargetsMock.mockReset().mockResolvedValue([
    { id: "code-pad", displayName: "Code Pad" },
    { id: "workbench", displayName: "Workbench" },
  ]);
  openInMock.mockReset().mockResolvedValue(undefined);
  writeTextMock.mockReset().mockResolvedValue(undefined);
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText: writeTextMock },
  });
});

afterEach(() => cleanup());

describe("Everything+ Query app-link delivery", () => {
  it("listens before cold take and immediately searches the trimmed Query", async () => {
    takePendingOpenMock.mockImplementationOnce(async () => {
      mocks.order.push("take");
      return { target: { kind: "query", text: "  Cargo.toml  " }, from: "devbox-launcher" };
    });

    render(<App />);

    await waitFor(() => expect(searchFilesMock).toHaveBeenCalledWith("Cargo.toml"));
    expect(mocks.order.slice(0, 2)).toEqual(["listen", "take"]);
    expect((screen.getByPlaceholderText("Search file names...") as HTMLInputElement).value).toBe("Cargo.toml");
    expect(await screen.findByText("Cargo.toml")).toBeTruthy();
  });

  it("takes the authoritative hot request instead of applying a stale event payload", async () => {
    render(<App />);
    await waitFor(() => expect(mocks.openHandler).not.toBeNull());
    await waitFor(() => expect(takePendingOpenMock).toHaveBeenCalledTimes(1));
    takePendingOpenMock.mockResolvedValueOnce({ target: { kind: "query", text: "fresh" }, from: null });

    await act(async () => {
      mocks.openHandler?.({ target: { kind: "query", text: "stale-secret" }, from: null });
    });

    await waitFor(() => expect(searchFilesMock).toHaveBeenCalledWith("fresh"));
    expect(searchFilesMock).not.toHaveBeenCalledWith("stale-secret");
    expect(document.body.textContent).not.toContain("stale-secret");
  });

  it("applies a Launcher query filter at the native search boundary", async () => {
    takePendingOpenMock.mockResolvedValueOnce({
      target: {
        kind: "query",
        text: "  cargo  ",
        filter: { extensions: [".RS"], sourceRootId: 3, contentStatus: "TRUNCATED" },
      },
      from: "devbox-launcher",
    });

    render(<App />);

    await waitFor(() => expect(searchFilesMock).toHaveBeenCalledWith(
      "cargo",
      undefined,
      { extensions: ["rs"], sourceRootId: 3, contentStatus: "truncated" },
    ));
    expect(screen.getByText("Filters (3)")).toBeTruthy();
  });

  it("does not let an older search response replace the inbound Query results", async () => {
    let resolveOldSearch: ((value: Awaited<ReturnType<typeof searchFiles>>) => void) | undefined;
    searchFilesMock.mockImplementationOnce(
      () => new Promise((resolve) => {
        resolveOldSearch = resolve;
      }),
    );

    render(<App />);
    const input = screen.getByPlaceholderText("Search file names...");
    fireEvent.change(input, { target: { value: "old" } });
    await waitFor(() => expect(searchFilesMock).toHaveBeenCalledWith("old"));

    takePendingOpenMock.mockResolvedValueOnce({ target: { kind: "query", text: "fresh" }, from: null });
    await act(async () => {
      mocks.openHandler?.({ target: { kind: "query", text: "ignored" }, from: null });
    });
    await waitFor(() => expect(screen.getByText("fresh")).toBeTruthy());

    await act(async () => {
      resolveOldSearch?.([{ id: 2, path: "C:\\files\\old", name: "old", ext: "", size: 1, modified_ts: 0 }]);
    });

    expect(screen.queryByText("old")).toBeNull();
    expect(screen.getByText("fresh")).toBeTruthy();
  });

  it("falls back to cold pull when listener setup fails", async () => {
    onOpenRequestMock.mockRejectedValueOnce(new Error("listener unavailable"));
    takePendingOpenMock.mockResolvedValueOnce({ target: { kind: "query", text: "fallback" }, from: null });

    render(<App />);

    await waitFor(() => expect(searchFilesMock).toHaveBeenCalledWith("fallback"));
    expect(screen.getByText("Everything+")).toBeTruthy();
  });

  it("shows invalid Query errors without searching or closing", async () => {
    takePendingOpenMock.mockResolvedValueOnce({ target: { kind: "query", text: "   " }, from: null });

    render(<App />);

    expect(await screen.findByText("요청한 검색어를 사용할 수 없습니다")).toBeTruthy();
    expect(screen.getByText("Everything+")).toBeTruthy();
    expect(searchFilesMock).not.toHaveBeenCalled();
  });

  it("enforces the UTF-8 search bound before invoking the backend", async () => {
    render(<App />);
    const input = screen.getByPlaceholderText("Search file names...");
    fireEvent.change(input, {
      target: { value: "가".repeat(2_000) },
    });

    expect(await screen.findByText("검색어가 너무 길거나 사용할 수 없는 문자를 포함합니다.")).toBeTruthy();
    expect(searchFilesMock).not.toHaveBeenCalled();

    fireEvent.change(input, { target: { value: "recovered" } });
    await waitFor(() => expect(searchFilesMock).toHaveBeenCalledWith("recovered"));
    expect(screen.queryByText("검색어가 너무 길거나 사용할 수 없는 문자를 포함합니다.")).toBeNull();
  });

  it("offers cancellation while indexing with an accessible busy action", async () => {
    indexStatusMock.mockResolvedValue({
      indexing: true,
      cancel_requested: false,
      total_files: 20,
      indexed_files: 4,
      content_indexed_files: 2,
      content_truncated_files: 1,
      content_failed_files: 0,
      roots: 1,
      last_indexed_at: null,
      last_error: null,
    });

    render(<App />);

    const cancel = await screen.findByRole("button", { name: "Cancel" });
    fireEvent.click(cancel);
    await waitFor(() => expect(cancelIndexMock).toHaveBeenCalledTimes(1));
    expect(screen.getByText("Indexing... 4 files")).toBeTruthy();
  });
});

async function renderNamedResults() {
  searchFilesMock.mockResolvedValueOnce([
    { id: 1, path: "C:\\files\\alpha.txt", name: "alpha.txt", ext: "txt", size: 10, modified_ts: 0 },
    { id: 2, path: "C:\\files\\beta.md", name: "beta.md", ext: "md", size: 20, modified_ts: 0 },
  ]);
  render(<App />);
  fireEvent.change(screen.getByPlaceholderText("Search file names..."), {
    target: { value: "fixture" },
  });
  await screen.findByText("beta.md");
}

function resultRow(name: string): HTMLTableRowElement {
  const element = screen.getByText(name).closest("tr");
  if (!(element instanceof HTMLTableRowElement)) throw new Error("result row was not rendered");
  return element;
}

describe("Everything+ result context menu", () => {
  it("selects the right-clicked row and exposes every app-owned action", async () => {
    await renderNamedResults();
    const beta = resultRow("beta.md");

    fireEvent.contextMenu(beta, { clientX: 20, clientY: 30 });

    expect(beta.getAttribute("aria-selected")).toBe("true");
    expect(screen.getByRole("menu", { name: "Search result actions" })).toBeTruthy();
    for (const label of [
      "Open",
      "Show in folder",
      "Copy path",
      "Copy file name",
      "Open in another app",
    ]) {
      expect(screen.getByRole("menuitem", { name: label })).toBeTruthy();
    }
    await waitFor(() => expect(
      screen.getByRole("menuitem", { name: "Open in another app" }).getAttribute("aria-disabled"),
    ).toBeNull());
  });

  it("runs row actions with the exact selected path and file name", async () => {
    await renderNamedResults();
    const beta = resultRow("beta.md");

    fireEvent.contextMenu(beta);
    fireEvent.click(screen.getByRole("menuitem", { name: "Open" }));
    await waitFor(() => expect(openFileMock).toHaveBeenCalledWith("C:\\files\\beta.md"));

    fireEvent.contextMenu(beta);
    fireEvent.click(screen.getByRole("menuitem", { name: "Show in folder" }));
    await waitFor(() => expect(revealFileMock).toHaveBeenCalledWith("C:\\files\\beta.md"));

    fireEvent.contextMenu(beta);
    fireEvent.click(screen.getByRole("menuitem", { name: "Copy path" }));
    await waitFor(() => expect(copyPathMock).toHaveBeenCalledWith("C:\\files\\beta.md"));

    fireEvent.contextMenu(beta);
    fireEvent.click(screen.getByRole("menuitem", { name: "Copy file name" }));
    await waitFor(() => expect(writeTextMock).toHaveBeenCalledWith("beta.md"));
  });

  it("opens only a catalog-derived submenu target", async () => {
    await renderNamedResults();
    const beta = resultRow("beta.md");
    fireEvent.contextMenu(beta);
    const submenu = screen.getByRole("menuitem", { name: "Open in another app" });
    await waitFor(() => expect(submenu.getAttribute("aria-disabled")).toBeNull());
    fireEvent.mouseEnter(submenu);
    fireEvent.click(await screen.findByRole("menuitem", { name: "Code Pad" }));

    await waitFor(() => expect(openInMock).toHaveBeenCalledWith(
      "code-pad",
      "C:\\files\\beta.md",
    ));
  });

  it("opens with Shift+F10 and restores focus after selection", async () => {
    await renderNamedResults();
    const alpha = resultRow("alpha.txt");
    alpha.focus();

    fireEvent.keyDown(alpha, { key: "F10", code: "F10", shiftKey: true });
    const copyName = screen.getByRole("menuitem", { name: "Copy file name" });
    fireEvent.click(copyName);

    await waitFor(() => expect(writeTextMock).toHaveBeenCalledWith("alpha.txt"));
    await waitFor(() => expect(document.activeElement).toBe(alpha));
  });

  it("keeps the catalog submenu disabled when no installed target exists", async () => {
    openTargetsMock.mockResolvedValueOnce([]);
    await renderNamedResults();
    const beta = resultRow("beta.md");
    fireEvent.contextMenu(beta);

    const submenu = screen.getByRole("menuitem", { name: "Open in another app" });
    await waitFor(() => expect(submenu.getAttribute("aria-disabled")).toBe("true"));
    fireEvent.click(submenu);
    expect(screen.queryByRole("menuitem", { name: "Code Pad" })).toBeNull();
    expect(openInMock).not.toHaveBeenCalled();
  });

  it("uses the same menu for content results and reports launch failure", async () => {
    searchContentMock.mockResolvedValueOnce([
      { path: "C:\\notes\\meeting.md", name: "meeting.md", snippet: "fixture text" },
    ]);
    openInMock.mockRejectedValueOnce(new Error("대상 앱을 실행하지 못했습니다"));
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Content" }));
    fireEvent.change(screen.getByPlaceholderText("Search file contents..."), {
      target: { value: "fixture" },
    });
    const contentRow = (await screen.findByText("meeting.md")).closest("tr");
    if (!(contentRow instanceof HTMLTableRowElement)) throw new Error("content row was not rendered");

    fireEvent.contextMenu(contentRow);
    const submenu = screen.getByRole("menuitem", { name: "Open in another app" });
    await waitFor(() => expect(submenu.getAttribute("aria-disabled")).toBeNull());
    fireEvent.mouseEnter(submenu);
    fireEvent.click(await screen.findByRole("menuitem", { name: "Workbench" }));

    expect(await screen.findByText("대상 앱을 실행하지 못했습니다")).toBeTruthy();
  });
});

describe("Everything+ filters and saved queries", () => {
  it("passes bounded native filters instead of filtering only in the renderer", async () => {
    searchFilesMock.mockResolvedValueOnce([
      { id: 1, path: "C:\\files\\main.rs", name: "main.rs", ext: "rs", size: 20, modified_ts: 100 },
    ]);
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Filters" }));
    fireEvent.change(screen.getByLabelText("File extensions"), { target: { value: " .RS, md " } });
    fireEvent.change(screen.getByPlaceholderText("Search file names..."), { target: { value: "main" } });

    await waitFor(() => expect(searchFilesMock).toHaveBeenCalledWith(
      "main",
      undefined,
      { extensions: ["md", "rs"] },
    ));
    expect(screen.getByText("main.rs")).toBeTruthy();
    expect(screen.getByText("Filters (1)")).toBeTruthy();
  });

  it("saves a query definition and loads it without persisting result rows", async () => {
    saveSavedQueryMock.mockResolvedValueOnce({
      id: 7,
      name: "Rust sources",
      query: "cargo",
      filter: { extensions: ["rs"] },
      createdAt: 1,
      updatedAt: 2,
    });
    render(<App />);
    fireEvent.change(screen.getByPlaceholderText("Search file names..."), { target: { value: "cargo" } });
    fireEvent.change(screen.getByLabelText("Saved query name"), { target: { value: "Rust sources" } });
    fireEvent.click(screen.getByRole("button", { name: "Save query" }));

    await waitFor(() => expect(saveSavedQueryMock).toHaveBeenCalledWith({
      name: "Rust sources",
      query: "cargo",
      filter: {},
    }));
    expect(await screen.findByRole("button", { name: "Rust sources" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Rust sources" }));
    expect((screen.getByPlaceholderText("Search file names...") as HTMLInputElement).value).toBe("cargo");
    expect(screen.getByLabelText("File extensions")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Delete saved query Rust sources" }));
    await waitFor(() => expect(deleteSavedQueryMock).toHaveBeenCalledWith(7));
    expect(screen.queryByRole("button", { name: "Rust sources" })).toBeNull();
  });

  it("does not let the initial saved-query response overwrite a completed save", async () => {
    let resolveInitial: ((value: Awaited<ReturnType<typeof listSavedQueries>>) => void) | undefined;
    listSavedQueriesMock.mockImplementationOnce(
      () => new Promise((resolve) => {
        resolveInitial = resolve;
      }),
    );
    saveSavedQueryMock.mockResolvedValueOnce({
      id: 9,
      name: "Saved during load",
      query: "cargo",
      filter: {},
      createdAt: 1,
      updatedAt: 2,
    });

    render(<App />);
    fireEvent.change(screen.getByPlaceholderText("Search file names..."), { target: { value: "cargo" } });
    fireEvent.change(screen.getByLabelText("Saved query name"), { target: { value: "Saved during load" } });
    fireEvent.click(screen.getByRole("button", { name: "Save query" }));

    await waitFor(() => expect(screen.getByRole("button", { name: "Saved during load" })).toBeTruthy());
    await act(async () => {
      resolveInitial?.([]);
    });
    expect(screen.getByRole("button", { name: "Saved during load" })).toBeTruthy();
  });
});
