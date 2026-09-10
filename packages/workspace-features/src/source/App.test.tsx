import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { assertNoA11yViolations } from "@devbox/a11y/testing";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import {
  createWorktree,
  repoCleanupPreview,
  onOpenRequest,
  openIn,
  openRepositoryFolder,
  openTargets,
  prepareInboundRepository,
  repoStatus,
  repositoryCopyPath,
  scanRoot,
  takePendingOpen,
  worktrees,
  type CleanupPreview,
  type RepoEntry,
  type RepoOpenTarget,
} from "./api";

vi.mock("./api", () => ({
  GIT_CLEANUP_BUSY: "이미 다른 Git 작업이 진행 중입니다.",
  GIT_CLEANUP_CANCELLED: "Git 정리 작업을 취소했습니다.",
  GIT_CLEANUP_ERROR: "Git 정리 작업을 실행하지 못했습니다.",
  GIT_CLEANUP_STATE_CHANGED: "저장소 상태가 변경되어 Git 정리를 실행하지 않았습니다.",
  GIT_REMOTE_BUSY: "이미 다른 Git 작업이 진행 중입니다.",
  GIT_REMOTE_CANCELLED: "Git 원격 작업을 취소했습니다.",
  GIT_REMOTE_ERROR: "Git 원격 작업을 실행하지 못했습니다.",
  GIT_REMOTE_STATE_CHANGED: "저장소 상태가 변경되어 Git 원격 작업을 실행하지 않았습니다.",
  createWorktree: vi.fn(),
  repoCleanup: vi.fn(),
  repoCleanupCancel: vi.fn(),
  repoCleanupPreview: vi.fn(),
  onOpenRequest: vi.fn(),
  openIn: vi.fn(),
  openRepositoryFolder: vi.fn(),
  openTargets: vi.fn(),
  prepareInboundRepository: vi.fn(),
  repoStatus: vi.fn(),
  repositoryCopyPath: vi.fn(),
  scanRoot: vi.fn(),
  takePendingOpen: vi.fn(),
  worktrees: vi.fn(),
}));

const repositories: RepoEntry[] = [
  {
    path: "C:\\projects\\devbox",
    canonicalKey: "win:c:/projects/devbox",
    hasWorktrees: true,
  },
  {
    path: "E:\\projects\\sample",
    canonicalKey: "win:e:/projects/sample",
    hasWorktrees: false,
  },
];

const targets: RepoOpenTarget[] = [
  { id: "code-pad", displayName: "Code Pad", payloadKind: "workspace" },
  { id: "wsl-desktop", displayName: "WSL Desktop", payloadKind: "path" },
];

const scanRootMock = vi.mocked(scanRoot);
const repoStatusMock = vi.mocked(repoStatus);
const worktreesMock = vi.mocked(worktrees);
const createWorktreeMock = vi.mocked(createWorktree);
const repoCleanupPreviewMock = vi.mocked(repoCleanupPreview);
const openTargetsMock = vi.mocked(openTargets);
const openInMock = vi.mocked(openIn);
const repositoryCopyPathMock = vi.mocked(repositoryCopyPath);
const openRepositoryFolderMock = vi.mocked(openRepositoryFolder);
const prepareInboundRepositoryMock = vi.mocked(prepareInboundRepository);
const takePendingOpenMock = vi.mocked(takePendingOpen);
const onOpenRequestMock = vi.mocked(onOpenRequest);
const writeTextMock = vi.fn<(value: string) => Promise<void>>();

beforeEach(() => {
  scanRootMock.mockReset().mockResolvedValue({ repos: repositories, truncated: false });
  repoStatusMock.mockReset().mockImplementation(async (path) => ({
    path,
    branch: { current: "main", ahead: 0, behind: 0, dirty: false, detached: false },
    changes: 0,
  }));
  worktreesMock.mockReset().mockImplementation(async (path) => path === repositories[0].path
    ? [repositories[0].path, "C:\\projects\\devbox-wt"]
    : [path]);
  createWorktreeMock.mockReset().mockImplementation(async (_repoPath, _branch, targetDir) => ({ path: targetDir }));
  repoCleanupPreviewMock.mockReset().mockResolvedValue({
    revision: "cleanup-0123456789abcdef",
    currentBranch: "main",
    currentHead: "0123456789abcdef0123456789abcdef01234567",
    branches: [],
    worktrees: [
      {
        path: repositories[0].path,
        head: "0123456789abcdef0123456789abcdef01234567",
        branch: "main",
        isMain: true,
        bare: false,
        locked: false,
        prunable: false,
        dirty: false,
        untracked: false,
        ignored: false,
        candidate: false,
        eligible: false,
        reasons: ["primaryWorktree"],
        blocked: ["mainWorktree", "currentWorktree"],
      },
    ],
  } satisfies CleanupPreview);
  openTargetsMock.mockReset().mockResolvedValue(targets);
  openInMock.mockReset().mockResolvedValue(undefined);
  repositoryCopyPathMock.mockReset().mockImplementation(async (path) => path);
  openRepositoryFolderMock.mockReset().mockResolvedValue(undefined);
  prepareInboundRepositoryMock.mockReset().mockImplementation(async (path) => ({
    path,
    canonicalKey: path.toLowerCase(),
    hasWorktrees: false,
  }));
  takePendingOpenMock.mockReset().mockResolvedValue(null);
  onOpenRequestMock.mockReset().mockResolvedValue(() => undefined);
  writeTextMock.mockReset().mockResolvedValue(undefined);
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText: writeTextMock },
  });
  Object.defineProperty(window, "requestAnimationFrame", {
    configurable: true,
    value: (callback: FrameRequestCallback) => window.setTimeout(() => callback(performance.now()), 0),
  });
});

afterEach(() => cleanup());

it("초기 셸이 접근성 위반 없이 렌더링된다", async () => {
  const { container } = render(<App />);
  await screen.findByText("C:\\projects\\devbox", { selector: ".repo-path" });
  await assertNoA11yViolations(container);
});

describe("Repo Manager repository context menu", () => {
  it("keeps the newest root scan when an older response resolves later", async () => {
    let resolveOlder: ((value: { repos: RepoEntry[]; truncated: boolean }) => void) | undefined;
    let resolveNewest: ((value: { repos: RepoEntry[]; truncated: boolean }) => void) | undefined;
    scanRootMock
      .mockReturnValueOnce(new Promise((resolve) => { resolveOlder = resolve; }))
      .mockReturnValueOnce(new Promise((resolve) => { resolveNewest = resolve; }));
    render(<App />);
    await waitFor(() => expect(scanRootMock).toHaveBeenCalledWith("C:\\projects"));

    fireEvent.change(screen.getByLabelText("탐색 루트"), {
      target: { value: "D:\\projects" },
    });
    await waitFor(() => expect(scanRootMock).toHaveBeenCalledWith("D:\\projects"));
    resolveNewest?.({ repos: [repositories[1]], truncated: true });
    await screen.findByLabelText("E:\\projects\\sample 저장소");
    expect(screen.getByText(/일부 디렉터리를 건너뛰었습니다/)).toBeTruthy();

    resolveOlder?.({ repos: [repositories[0]], truncated: false });
    await Promise.resolve();
    expect(screen.queryByLabelText("C:\\projects\\devbox 저장소")).toBeNull();
    expect(screen.getByLabelText("E:\\projects\\sample 저장소")).toBeTruthy();
  });

  it("maps legacy native failures to an alert without echoing sensitive details", async () => {
    const secret = "C:\\secret\\credential-root";
    scanRootMock.mockRejectedValueOnce(new Error(secret));
    render(<App />);

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toBe("저장소 목록을 불러오지 못했습니다.");
    expect(alert.textContent).not.toContain(secret);
    expect(document.body.textContent).not.toContain(secret);
  });

  it("selects a repository with Enter and Space, ignores IME, and exposes labelled inputs", async () => {
    render(<App />);
    const first = await screen.findByLabelText("C:\\projects\\devbox 저장소");
    const second = await screen.findByLabelText("E:\\projects\\sample 저장소");
    expect(screen.getByLabelText("탐색 루트")).toBeTruthy();
    expect(screen.getAllByLabelText("새 브랜치")).toHaveLength(2);
    expect(screen.getAllByLabelText("대상 디렉터리")).toHaveLength(2);

    first.focus();
    fireEvent.keyDown(first, { key: "Enter", isComposing: true });
    expect(first.getAttribute("aria-current")).toBeNull();
    fireEvent.keyDown(first, { key: "Enter" });
    expect(first.getAttribute("aria-current")).toBe("true");
    second.focus();
    fireEvent.keyDown(second, { key: " " });
    expect(second.getAttribute("aria-current")).toBe("true");
  });

  it("repository와 정리 preview UI를 유지한다", async () => {
    render(<App />);

    await screen.findByText("C:\\projects\\devbox", { selector: ".repo-path" });
    expect(screen.getByText("C:\\projects\\devbox-wt")).toBeTruthy();
    fireEvent.click(screen.getByLabelText("C:\\projects\\devbox 저장소"));
    fireEvent.click(screen.getByRole("button", { name: "정리 후보 검사" }));
    expect(await screen.findByText(/기본 worktree라서 차단됨/)).toBeTruthy();
    expect(repoCleanupPreviewMock).toHaveBeenCalledWith(
      repositories[0].path,
      expect.stringMatching(/^[A-Za-z0-9._-]+$/u),
    );
  });

  it("우클릭한 exact repository를 선택하고 설계의 네 항목만 표시한다", async () => {
    render(<App />);
    const target = await screen.findByLabelText("E:\\projects\\sample 저장소") as HTMLDivElement;

    fireEvent.contextMenu(target, { clientX: 16, clientY: 24 });

    expect(target.getAttribute("aria-current")).toBe("true");
    for (const label of ["다른 앱으로 열기", "worktree 생성", "경로 복사", "탐색기에서 열기"]) {
      expect(screen.getByRole("menuitem", { name: label })).toBeTruthy();
    }
    expect(screen.queryByRole("menuitem", { name: /remove|제거/u })).toBeNull();
  });

  it("catalog submenu action은 exact repository와 target ID를 backend에 전달한다", async () => {
    render(<App />);
    const target = await screen.findByLabelText("E:\\projects\\sample 저장소") as HTMLDivElement;
    await screen.findAllByRole("button", { name: "Code Pad" });

    fireEvent.contextMenu(target);
    fireEvent.click(screen.getByRole("menuitem", { name: "다른 앱으로 열기" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Code Pad" }));

    await waitFor(() => expect(openInMock).toHaveBeenCalledWith("code-pad", repositories[1].path));
    await waitFor(() => expect(document.activeElement).toBe(target));
  });

  it("Shift+F10 경로 복사는 backend 재검증 결과만 쓰고 focus를 복원한다", async () => {
    render(<App />);
    const target = await screen.findByLabelText("E:\\projects\\sample 저장소") as HTMLDivElement;
    target.focus();

    fireEvent.keyDown(target, { key: "F10", code: "F10", shiftKey: true });
    fireEvent.click(screen.getByRole("menuitem", { name: "경로 복사" }));

    await waitFor(() => expect(repositoryCopyPathMock).toHaveBeenCalledWith(repositories[1].path));
    expect(writeTextMock).toHaveBeenCalledWith(repositories[1].path);
    await waitFor(() => expect(document.activeElement).toBe(target));
  });

  it("Menu key로 exact repository 폴더를 연다", async () => {
    render(<App />);
    const target = await screen.findByLabelText("C:\\projects\\devbox 저장소") as HTMLDivElement;
    target.focus();

    fireEvent.keyDown(target, { key: "ContextMenu", code: "ContextMenu" });
    fireEvent.click(screen.getByRole("menuitem", { name: "탐색기에서 열기" }));

    await waitFor(() => expect(openRepositoryFolderMock).toHaveBeenCalledWith(repositories[0].path));
    await waitFor(() => expect(document.activeElement).toBe(target));
  });

  it("worktree 생성 action은 exact repository의 기존 입력으로 이동하고 자동 생성하지 않는다", async () => {
    render(<App />);
    const target = await screen.findByLabelText("E:\\projects\\sample 저장소") as HTMLDivElement;
    const branchInput = within(target).getByPlaceholderText("새 브랜치");

    fireEvent.contextMenu(target);
    fireEvent.click(screen.getByRole("menuitem", { name: "worktree 생성" }));

    await waitFor(() => expect(document.activeElement).toBe(branchInput));
    expect(createWorktreeMock).not.toHaveBeenCalled();
  });

  it("worktree 텍스트 입력의 기본 context menu와 Shift+F10을 가로채지 않는다", async () => {
    render(<App />);
    const target = await screen.findByLabelText("E:\\projects\\sample 저장소") as HTMLDivElement;
    const branchInput = within(target).getByPlaceholderText("새 브랜치");

    branchInput.focus();
    fireEvent.click(branchInput);
    fireEvent.contextMenu(branchInput);
    fireEvent.keyDown(branchInput, { key: "F10", code: "F10", shiftKey: true });

    expect(screen.queryByRole("menu", { name: "저장소 메뉴" })).toBeNull();
    expect(document.activeElement).toBe(branchInput);
  });

  it("target discovery 실패는 raw 오류를 숨기고 submenu를 fail-closed로 둔다", async () => {
    openTargetsMock.mockRejectedValueOnce(new Error("credential-raw-error"));
    render(<App />);
    const target = await screen.findByLabelText("C:\\projects\\devbox 저장소") as HTMLDivElement;
    await screen.findByText("다른 앱으로 열기 대상을 확인하지 못했습니다");

    fireEvent.contextMenu(target);

    expect(screen.getByRole("menuitem", { name: "다른 앱으로 열기" }).getAttribute("aria-disabled"))
      .toBe("true");
    expect(document.body.textContent?.includes("credential-raw-error")).toBe(false);
  });

  it("copy 실패는 backend 경로나 상세 오류를 화면에 반향하지 않는다", async () => {
    repositoryCopyPathMock.mockRejectedValueOnce(new Error("C:\\secret\\repo"));
    render(<App />);
    const target = await screen.findByLabelText("C:\\projects\\devbox 저장소") as HTMLDivElement;

    fireEvent.contextMenu(target);
    fireEvent.click(screen.getByRole("menuitem", { name: "경로 복사" }));

    expect(await screen.findByText("저장소 경로를 확인하거나 복사하지 못했습니다")).toBeTruthy();
    expect(document.body.textContent?.includes("C:\\secret\\repo")).toBe(false);
    expect(writeTextMock).not.toHaveBeenCalled();
  });
});
