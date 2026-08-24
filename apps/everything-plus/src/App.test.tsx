import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import { onOpenRequest, searchFiles, takePendingOpen, type OpenRequest } from "./api";

const mocks = vi.hoisted(() => ({
  openHandler: null as ((request: OpenRequest) => void) | null,
  order: [] as string[],
}));

vi.mock("./api", () => ({
  indexStatus: vi.fn(async () => ({ indexing: false, total_files: 1, indexed_files: 1, roots: 1, last_indexed_at: null })),
  listRoots: vi.fn(async () => []),
  watcherStatuses: vi.fn(async () => []),
  searchFiles: vi.fn(async (query: string) => [{ id: 1, path: `C:\\files\\${query}`, name: query, ext: "", size: 1, modified_ts: 0 }]),
  searchContent: vi.fn(async () => []),
  addRoot: vi.fn(async () => undefined),
  removeRoot: vi.fn(async () => undefined),
  indexNow: vi.fn(async () => undefined),
  openFile: vi.fn(async () => undefined),
  revealFile: vi.fn(async () => undefined),
  copyPath: vi.fn(async () => undefined),
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
  searchFilesMock.mockClear();
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
});
