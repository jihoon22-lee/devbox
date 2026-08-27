import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import {
  createWorktree,
  onOpenRequest,
  prepareInboundRepository,
  scanRoot,
  takePendingOpen,
  type OpenRequest,
} from "./api";

const fixtures = vi.hoisted(() => ({
  known: {
    path: "C:\\projects\\known",
    canonicalKey: "win:c:/projects/known",
    hasWorktrees: false,
  },
  draft: {
    path: "D:\\outside\\new-repo",
    canonicalKey: "win:d:/outside/new-repo",
    hasWorktrees: false,
  },
  openHandler: null as ((request: OpenRequest) => void) | null,
  order: [] as string[],
}));

vi.mock("./api", () => ({
  GIT_REMOTE_BUSY: "이미 다른 Git 작업이 진행 중입니다.",
  GIT_REMOTE_CANCELLED: "Git 원격 작업을 취소했습니다.",
  GIT_REMOTE_ERROR: "Git 원격 작업을 실행하지 못했습니다.",
  GIT_REMOTE_STATE_CHANGED: "저장소 상태가 변경되어 Git 원격 작업을 실행하지 않았습니다.",
  scanRoot: vi.fn(async () => ({ repos: [fixtures.known], truncated: false })),
  repoStatus: vi.fn(async (path: string) => ({
    path,
    branch: { current: "main", ahead: 0, behind: 0, dirty: false, detached: false },
    changes: 0,
  })),
  worktrees: vi.fn(async () => []),
  createWorktree: vi.fn(async () => undefined),
  worktreeClean: vi.fn(async () => true),
  openTargets: vi.fn(async () => []),
  openIn: vi.fn(async () => undefined),
  prepareInboundRepository: vi.fn(async (path: string) => {
    if (path.toLowerCase() === fixtures.known.path.toLowerCase()) return fixtures.known;
    if (path === fixtures.draft.path) return fixtures.draft;
    throw new Error(path);
  }),
  takePendingOpen: vi.fn().mockImplementation(async () => {
    fixtures.order.push("take");
    return null;
  }),
  onOpenRequest: vi.fn().mockImplementation(async (handler: (request: OpenRequest) => void) => {
    fixtures.order.push("listen");
    fixtures.openHandler = handler;
    return () => undefined;
  }),
}));

const scanRootMock = vi.mocked(scanRoot);
const prepareInboundRepositoryMock = vi.mocked(prepareInboundRepository);
const takePendingOpenMock = vi.mocked(takePendingOpen);
const onOpenRequestMock = vi.mocked(onOpenRequest);
const createWorktreeMock = vi.mocked(createWorktree);

function pathRequest(path: string): OpenRequest {
  return {
    target: { kind: "path", path, line: null, column: null },
    from: "workbench",
  };
}

beforeEach(() => {
  fixtures.openHandler = null;
  fixtures.order.length = 0;
  scanRootMock.mockReset().mockResolvedValue({ repos: [fixtures.known], truncated: false });
  prepareInboundRepositoryMock.mockReset().mockImplementation(async (path) => {
    if (path.toLowerCase() === fixtures.known.path.toLowerCase()) return fixtures.known;
    if (path === fixtures.draft.path) return fixtures.draft;
    throw new Error(path);
  });
  takePendingOpenMock.mockReset().mockImplementation(async () => {
    fixtures.order.push("take");
    return null;
  });
  onOpenRequestMock.mockReset().mockImplementation(async (handler) => {
    fixtures.order.push("listen");
    fixtures.openHandler = handler;
    return () => undefined;
  });
  createWorktreeMock.mockClear();
});

afterEach(() => cleanup());

describe("Repo Manager Path app-link delivery", () => {
  it("listens before cold take, selects the matching repository, and focuses its card", async () => {
    takePendingOpenMock.mockImplementationOnce(async () => {
      fixtures.order.push("take");
      return pathRequest("C:\\Projects\\Known");
    });

    render(<App />);

    const path = await screen.findByText(fixtures.known.path, { selector: ".repo-path" });
    const card = path.closest(".repo-card") as HTMLDivElement;
    await waitFor(() => expect(card.classList.contains("selected")).toBe(true));
    await waitFor(() => expect(document.activeElement).toBe(card));
    expect(fixtures.order.slice(0, 2)).toEqual(["listen", "take"]);
  });

  it("pulls the authoritative hot Path instead of applying the event payload", async () => {
    render(<App />);
    await waitFor(() => expect(fixtures.openHandler).not.toBeNull());
    await waitFor(() => expect(takePendingOpenMock).toHaveBeenCalledTimes(1));
    takePendingOpenMock.mockResolvedValueOnce(pathRequest(fixtures.known.path));

    await act(async () => {
      fixtures.openHandler?.(pathRequest("C:\\stale-secret"));
    });

    await waitFor(() =>
      expect(prepareInboundRepositoryMock).toHaveBeenCalledWith(fixtures.known.path),
    );
    expect(prepareInboundRepositoryMock).not.toHaveBeenCalledWith("C:\\stale-secret");
    expect(document.body.textContent).not.toContain("stale-secret");
  });

  it("does not let an older Path validation replace the newest hot request", async () => {
    let resolveOlder: ((value: typeof fixtures.draft) => void) | undefined;
    prepareInboundRepositoryMock.mockImplementationOnce(
      () => new Promise((resolve) => {
        resolveOlder = resolve;
      }),
    );
    render(<App />);
    await waitFor(() => expect(fixtures.openHandler).not.toBeNull());
    await waitFor(() => expect(takePendingOpenMock).toHaveBeenCalledTimes(1));

    takePendingOpenMock.mockResolvedValueOnce(pathRequest(fixtures.draft.path));
    await act(async () => fixtures.openHandler?.(pathRequest("ignored-one")));
    await waitFor(() =>
      expect(prepareInboundRepositoryMock).toHaveBeenCalledWith(fixtures.draft.path),
    );

    takePendingOpenMock.mockResolvedValueOnce(pathRequest(fixtures.known.path));
    await act(async () => fixtures.openHandler?.(pathRequest("ignored-two")));
    const knownPath = await screen.findByText(fixtures.known.path, { selector: ".repo-path" });
    await waitFor(() =>
      expect(knownPath.closest(".repo-card")?.classList.contains("selected")).toBe(true),
    );

    await act(async () => resolveOlder?.(fixtures.draft));

    expect(screen.queryByRole("region", { name: "Repository 등록 초안" })).toBeNull();
    expect(knownPath.closest(".repo-card")?.classList.contains("selected")).toBe(true);
  });

  it("creates a non-persistent draft for a valid repository outside the current list", async () => {
    takePendingOpenMock.mockResolvedValueOnce(pathRequest(fixtures.draft.path));

    render(<App />);

    expect(await screen.findByRole("region", { name: "Repository 등록 초안" })).toBeTruthy();
    expect(screen.getByText(fixtures.draft.path)).toBeTruthy();
    expect(screen.getByText(/아직 저장하거나 Git 명령을 실행하지 않았습니다/)).toBeTruthy();
    expect(scanRootMock).toHaveBeenCalledTimes(1);
    expect(scanRootMock).toHaveBeenCalledWith("C:\\projects");
    expect(createWorktreeMock).not.toHaveBeenCalled();
  });

  it("scans a draft only after explicit confirmation and then selects it", async () => {
    scanRootMock
      .mockResolvedValueOnce({ repos: [fixtures.known], truncated: false })
      .mockResolvedValueOnce({ repos: [fixtures.draft], truncated: false });
    takePendingOpenMock.mockResolvedValueOnce(pathRequest(fixtures.draft.path));
    render(<App />);
    const explore = await screen.findByRole("button", { name: "이 경로 탐색" });

    fireEvent.click(explore);

    await waitFor(() => expect(scanRootMock).toHaveBeenCalledWith(fixtures.draft.path));
    const path = await screen.findByText(fixtures.draft.path, { selector: ".repo-path" });
    await waitFor(() => expect(path.closest(".repo-card")?.classList.contains("selected")).toBe(true));
  });

  it("shows a generic recoverable error for an invalid Path without echoing it", async () => {
    const secretPath = "C:\\invalid\\repo-path-secret";
    takePendingOpenMock.mockResolvedValueOnce(pathRequest(secretPath));

    render(<App />);

    expect(await screen.findByText("repository 경로를 확인할 수 없습니다")).toBeTruthy();
    expect(screen.getByText("Repo Manager")).toBeTruthy();
    expect(document.body.textContent).not.toContain(secretPath);
    expect(createWorktreeMock).not.toHaveBeenCalled();
  });

  it("falls back to the cold pull when listener registration fails", async () => {
    onOpenRequestMock.mockRejectedValueOnce(new Error("listener unavailable"));
    takePendingOpenMock.mockResolvedValueOnce(pathRequest(fixtures.known.path));

    render(<App />);

    await waitFor(() => expect(prepareInboundRepositoryMock).toHaveBeenCalledWith(fixtures.known.path));
    expect(screen.getByText("Repo Manager")).toBeTruthy();
  });
});
