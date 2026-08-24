import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import {
  onOpenRequest,
  openInboundNote,
  searchDocs,
  takePendingOpen,
  type OpenRequest,
} from "./api";

const mocks = vi.hoisted(() => ({
  openHandler: null as ((request: OpenRequest) => void) | null,
  order: [] as string[],
}));

vi.mock("./api", () => ({
  listTree: vi.fn(async () => [{ path: "Notes/existing.md", is_dir: false }]),
  listTags: vi.fn(async () => [] as string[]),
  readFile: vi.fn(async () => "# existing"),
  writeFile: vi.fn(async () => undefined),
  createFile: vi.fn(async () => undefined),
  renameFile: vi.fn(async () => undefined),
  deleteFile: vi.fn(async () => undefined),
  dailyNote: vi.fn(async () => ["Journal/today.md", "# Today"] as [string, string]),
  renderMarkdown: vi.fn(async () => ({ title: null, tags: [], html: "<p>rendered</p>", mermaid: [] })),
  onDocsChanged: vi.fn(async () => () => undefined),
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
  openInboundNoteMock.mockReset().mockResolvedValue({ path: "Notes/inbound.md", content: "# inbound" });
  searchDocsMock.mockReset().mockImplementation(async (query) => [
    { path: "Notes/result.md", title: `Result ${query}` },
  ]);
});

afterEach(() => cleanup());

describe("Knowledge Path/Query app-link delivery", () => {
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
});
