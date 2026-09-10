import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  GIT_REMOTE_ERROR,
  repoFetch,
  repoPull,
  repoPush,
  repoRemoteCancel,
  repoRemoteStatus,
  type RemoteState,
  type RepoEntry,
} from "../api";
import RemoteSyncPanel from "./RemoteSyncPanel";
import { createRemoteOperationId } from "./RemoteSyncPanel";

vi.mock("../api", () => ({
  GIT_REMOTE_BUSY: "이미 다른 Git 작업이 진행 중입니다.",
  GIT_REMOTE_CANCELLED: "Git 원격 작업을 취소했습니다.",
  GIT_REMOTE_ERROR: "Git 원격 작업을 실행하지 못했습니다.",
  GIT_REMOTE_STATE_CHANGED: "저장소 상태가 변경되어 Git 원격 작업을 실행하지 않았습니다.",
  repoFetch: vi.fn(),
  repoPull: vi.fn(),
  repoPush: vi.fn(),
  repoRemoteCancel: vi.fn(),
  repoRemoteStatus: vi.fn(),
}));

const repo: RepoEntry = {
  path: "C:\\projects\\sample",
  canonicalKey: "win:c:/projects/sample",
  hasWorktrees: false,
};

const otherRepo: RepoEntry = {
  path: "D:\\projects\\other",
  canonicalKey: "win:d:/projects/other",
  hasWorktrees: false,
};

const cleanState: RemoteState = {
  currentBranch: "main",
  upstream: "origin/main",
  ahead: 0,
  behind: 0,
  dirty: false,
  detached: false,
  diverged: false,
  operationInProgress: false,
};

const repoFetchMock = vi.mocked(repoFetch);
const repoPullMock = vi.mocked(repoPull);
const repoPushMock = vi.mocked(repoPush);
const repoRemoteCancelMock = vi.mocked(repoRemoteCancel);
const repoRemoteStatusMock = vi.mocked(repoRemoteStatus);

beforeEach(() => {
  repoFetchMock.mockReset().mockResolvedValue(undefined);
  repoPullMock.mockReset().mockResolvedValue(undefined);
  repoPushMock.mockReset().mockResolvedValue(undefined);
  repoRemoteCancelMock.mockReset().mockResolvedValue(false);
  repoRemoteStatusMock.mockReset().mockResolvedValue({ ...cleanState });
});

afterEach(() => cleanup());

describe("RemoteSyncPanel", () => {
  it("requires explicit pull confirmation and keeps remote details out of the summary", async () => {
    render(<RemoteSyncPanel repo={repo} />);
    fireEvent.click(screen.getByRole("button", { name: "원격 상태 새로고침" }));
    await screen.findByText("원격 작업을 실행할 수 있습니다.");

    fireEvent.click(screen.getByRole("button", { name: "Pull (FF only)" }));
    expect(repoPullMock).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog", { name: "Pull (FF only)을 실행할까요?" })).toBeTruthy();
    expect(document.body.textContent).not.toContain("https://user:credential@secret.example/repo");
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "취소" }));
  });

  it("cancels a confirmation with Escape and returns focus without a native call", async () => {
    render(<RemoteSyncPanel repo={repo} />);
    fireEvent.click(screen.getByRole("button", { name: "원격 상태 새로고침" }));
    await screen.findByText("원격 작업을 실행할 수 있습니다.");

    const trigger = screen.getByRole("button", { name: "Push" });
    trigger.focus();
    fireEvent.click(trigger);
    const dialog = screen.getByRole("dialog", { name: "Push을 실행할까요?" });
    fireEvent.keyDown(dialog, { key: "Escape" });

    expect(screen.queryByRole("dialog")).toBeNull();
    expect(document.activeElement).toBe(trigger);
    expect(repoPushMock).not.toHaveBeenCalled();
  });

  it("invalidates a pull confirmation when the native status changes", async () => {
    repoRemoteStatusMock
      .mockResolvedValueOnce({ ...cleanState })
      .mockResolvedValueOnce({ ...cleanState, dirty: true });
    render(<RemoteSyncPanel repo={repo} />);
    fireEvent.click(screen.getByRole("button", { name: "원격 상태 새로고침" }));
    await screen.findByText("원격 작업을 실행할 수 있습니다.");
    fireEvent.click(screen.getByRole("button", { name: "Pull (FF only)" }));
    expect(screen.getByRole("dialog")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "원격 상태 새로고침" }));
    await screen.findByText("working tree에 변경 사항이 있어 pull/push를 실행할 수 없습니다.");
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(repoPullMock).not.toHaveBeenCalled();
  });

  it("creates unique opaque operation IDs only when a confirmed remote mutation starts", async () => {
    const first = createRemoteOperationId();
    const second = createRemoteOperationId();
    expect(first).not.toBe(second);
    expect(first).toMatch(/^[A-Za-z0-9._-]+$/u);

    render(<RemoteSyncPanel repo={repo} />);
    fireEvent.click(screen.getByRole("button", { name: "원격 상태 새로고침" }));
    await screen.findByText("원격 작업을 실행할 수 있습니다.");
    fireEvent.click(screen.getByRole("button", { name: "Pull (FF only)" }));
    expect(repoPullMock).not.toHaveBeenCalled();
    expect(repoRemoteCancelMock).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Pull (FF only) 실행" }));
    await waitFor(() => expect(repoPullMock).toHaveBeenCalledWith(repo.path, expect.any(String)));
    const operationId = repoPullMock.mock.calls[0][1];
    expect(operationId).toMatch(/^[A-Za-z0-9._-]+$/u);
  });

  it("does not offer cancel during the post-mutation status refresh", async () => {
    let resolveRefresh: ((value: RemoteState) => void) | undefined;
    repoRemoteStatusMock
      .mockResolvedValueOnce({ ...cleanState })
      .mockReturnValueOnce(new Promise((resolve) => { resolveRefresh = resolve; }));
    repoPullMock.mockResolvedValueOnce(undefined);
    render(<RemoteSyncPanel repo={repo} />);
    fireEvent.click(screen.getByRole("button", { name: "원격 상태 새로고침" }));
    await screen.findByText("원격 작업을 실행할 수 있습니다.");
    fireEvent.click(screen.getByRole("button", { name: "Pull (FF only)" }));
    fireEvent.click(screen.getByRole("button", { name: "Pull (FF only) 실행" }));
    await waitFor(() => expect(repoPullMock).toHaveBeenCalled());
    await waitFor(() => expect(screen.getByRole("status").textContent).toContain("완료"));
    expect(screen.queryByRole("button", { name: "취소" })).toBeNull();
    expect(screen.getByRole("region", { name: "Git 원격 동기화" }).getAttribute("aria-busy")).toBe("true");
    resolveRefresh?.({ ...cleanState });
    await waitFor(() => expect(screen.getByRole("region", { name: "Git 원격 동기화" }).getAttribute("aria-busy")).toBe("false"));
  });

  it("loads state and sends exact repository-only actions", async () => {
    render(<RemoteSyncPanel repo={repo} />);
    fireEvent.click(screen.getByRole("button", { name: "원격 상태 새로고침" }));
    await screen.findByText("원격 작업을 실행할 수 있습니다.");

    fireEvent.click(screen.getByRole("button", { name: "Fetch" }));
    await waitFor(() => expect(repoFetchMock).toHaveBeenCalledWith(repo.path, expect.any(String)));
    expect(repoPullMock).not.toHaveBeenCalled();
    expect(repoPushMock).not.toHaveBeenCalled();

    await waitFor(() => expect(screen.getByRole("status").textContent).toContain("완료"));
  });

  it("uses FF-only pull and normal current-branch push without a force action", async () => {
    render(<RemoteSyncPanel repo={repo} />);
    fireEvent.click(screen.getByRole("button", { name: "원격 상태 새로고침" }));
    await screen.findByText("원격 작업을 실행할 수 있습니다.");

    fireEvent.click(screen.getByRole("button", { name: "Pull (FF only)" }));
    expect(repoPullMock).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Pull (FF only) 실행" }));
    await waitFor(() => expect(repoPullMock).toHaveBeenCalledWith(repo.path, expect.any(String)));
    await waitFor(() => expect(screen.getByRole("region", { name: "Git 원격 동기화" }).getAttribute("aria-busy")).toBe("false"));
    fireEvent.click(screen.getByRole("button", { name: "Push" }));
    fireEvent.click(screen.getByRole("button", { name: "Push 실행" }));
    await waitFor(() => expect(repoPushMock).toHaveBeenCalledWith(repo.path, expect.any(String)));
    expect(screen.queryByRole("button", { name: /force|merge|rebase/i })).toBeNull();
  });

  it.each([
    ["dirty", { dirty: true }, "working tree에 변경 사항"],
    ["detached", { detached: true, currentBranch: null }, "detached 상태"],
    ["no upstream", { upstream: null }, "upstream이 없어"],
    ["diverged", { ahead: 1, behind: 1, diverged: true }, "diverged 상태"],
    ["in-progress", { operationInProgress: true }, "다른 Git 작업 또는 merge/rebase가 진행 중"],
  ])("blocks pull/push for %s state without invoking Git", async (_name, overrides, message) => {
    repoRemoteStatusMock.mockResolvedValueOnce({ ...cleanState, ...overrides });
    render(<RemoteSyncPanel repo={repo} />);
    fireEvent.click(screen.getByRole("button", { name: "원격 상태 새로고침" }));
    await screen.findByText(new RegExp(message));

    expect(screen.getByRole("button", { name: "Pull (FF only)" })).toHaveProperty("disabled", true);
    expect(screen.getByRole("button", { name: "Push" })).toHaveProperty("disabled", true);
    fireEvent.click(screen.getByRole("button", { name: "Pull (FF only)" }));
    fireEvent.click(screen.getByRole("button", { name: "Push" }));
    expect(repoPullMock).not.toHaveBeenCalled();
    expect(repoPushMock).not.toHaveBeenCalled();
  });

  it("allows fetch for dirty, detached, no-upstream, and diverged states", async () => {
    for (const overrides of [
      { dirty: true },
      { detached: true, currentBranch: null },
      { upstream: null },
      { ahead: 1, behind: 1, diverged: true },
    ]) {
      cleanup();
      repoRemoteStatusMock.mockReset().mockResolvedValueOnce({ ...cleanState, ...overrides });
      repoFetchMock.mockReset().mockResolvedValue(undefined);
      render(<RemoteSyncPanel repo={repo} />);
      fireEvent.click(screen.getByRole("button", { name: "원격 상태 새로고침" }));
      await screen.findByRole("button", { name: "Fetch" });
      await waitFor(() => expect(screen.getByRole("button", { name: "Fetch" })).toHaveProperty("disabled", false));
      fireEvent.click(screen.getByRole("button", { name: "Fetch" }));
      await waitFor(() => expect(repoFetchMock).toHaveBeenCalledWith(repo.path, expect.any(String)));
    }
  });

  it("blocks duplicate actions, sends cancel, and ignores raw failure details", async () => {
    let resolveFetch: (() => void) | undefined;
    repoFetchMock.mockReturnValueOnce(new Promise<void>((resolve) => { resolveFetch = resolve; }));
    render(<RemoteSyncPanel repo={repo} />);
    fireEvent.click(screen.getByRole("button", { name: "원격 상태 새로고침" }));
    await screen.findByText("원격 작업을 실행할 수 있습니다.");

    const fetchButton = screen.getByRole("button", { name: "Fetch" });
    fireEvent.click(fetchButton);
    fireEvent.click(fetchButton);
    expect(repoFetchMock).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("region", { name: "Git 원격 동기화" }).getAttribute("aria-busy")).toBe("true");

    fireEvent.click(screen.getByRole("button", { name: "취소" }));
    await waitFor(() => expect(repoRemoteCancelMock).toHaveBeenCalledWith(repoFetchMock.mock.calls[0][1]));
    resolveFetch?.();
    await waitFor(() => expect(screen.getByRole("status").textContent).toContain("취소했습니다"));
    await waitFor(() => expect(screen.getByRole("region", { name: "Git 원격 동기화" }).getAttribute("aria-busy")).toBe("false"));
    expect(screen.getByRole("alert").textContent).toBe("Git 원격 작업을 취소했습니다.");

    repoFetchMock.mockRejectedValueOnce(new Error("https://user:credential@secret.example/repo"));
    fireEvent.click(screen.getByRole("button", { name: "원격 상태 새로고침" }));
    await screen.findByText("원격 작업을 실행할 수 있습니다.");
    fireEvent.click(screen.getByRole("button", { name: "Fetch" }));
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toBe(GIT_REMOTE_ERROR);
    expect(alert.textContent).not.toContain("credential");
  });

  it("clears stale state when the post-action refresh fails and blocks more mutations", async () => {
    repoRemoteStatusMock.mockReset()
      .mockResolvedValueOnce({ ...cleanState })
      .mockRejectedValueOnce(new Error("status path must not reach the UI"));
    repoFetchMock.mockResolvedValueOnce(undefined);
    render(<RemoteSyncPanel repo={repo} />);
    fireEvent.click(screen.getByRole("button", { name: "원격 상태 새로고침" }));
    await screen.findByText("원격 작업을 실행할 수 있습니다.");

    fireEvent.click(screen.getByRole("button", { name: "Fetch" }));
    await screen.findByRole("alert");
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Fetch" })).toHaveProperty("disabled", true);
      expect(screen.getByRole("button", { name: "Pull (FF only)" })).toHaveProperty("disabled", true);
      expect(screen.getByRole("button", { name: "Push" })).toHaveProperty("disabled", true);
    });
    expect(screen.getByRole("alert").textContent).toBe(GIT_REMOTE_ERROR);
    expect(repoFetchMock).toHaveBeenCalledTimes(1);
  });

  it("drops stale status and cancels on unmount", async () => {
    let resolveOld: ((value: RemoteState) => void) | undefined;
    repoRemoteStatusMock.mockReset()
      .mockResolvedValue({ ...cleanState })
      .mockReturnValueOnce(new Promise((resolve) => { resolveOld = resolve; }));
    const rendered = render(<RemoteSyncPanel repo={repo} />);
    fireEvent.click(screen.getByRole("button", { name: "원격 상태 새로고침" }));
    rendered.rerender(<RemoteSyncPanel repo={otherRepo} />);
    resolveOld?.({ ...cleanState, currentBranch: "stale" });
    await waitFor(() => expect(screen.getByText(otherRepo.path)).toBeTruthy());
    expect(screen.queryByText("stale")).toBeNull();

    let resolveFetch: (() => void) | undefined;
    repoRemoteStatusMock.mockResolvedValue({ ...cleanState });
    repoFetchMock.mockReturnValueOnce(new Promise<void>((resolve) => { resolveFetch = resolve; }));
    fireEvent.click(screen.getByRole("button", { name: "원격 상태 새로고침" }));
    await screen.findByText("원격 작업을 실행할 수 있습니다.");
    fireEvent.click(screen.getByRole("button", { name: "Fetch" }));
    rendered.unmount();
    await waitFor(() => expect(repoRemoteCancelMock).toHaveBeenCalledWith(repoFetchMock.mock.calls[0][1]));
    resolveFetch?.();
  });
});
