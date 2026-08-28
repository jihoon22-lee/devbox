import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  GIT_CLEANUP_BUSY,
  GIT_CLEANUP_CANCELLED,
  GIT_CLEANUP_ERROR,
  GIT_CLEANUP_STATE_CHANGED,
  repoCleanup,
  repoCleanupCancel,
  repoCleanupPreview,
  type CleanupPreview,
  type RepoEntry,
} from "../api";
import CleanupPanel, { createCleanupOperationId } from "./CleanupPanel";

vi.mock("../api", () => ({
  GIT_CLEANUP_BUSY: "이미 다른 Git 작업이 진행 중입니다.",
  GIT_CLEANUP_CANCELLED: "Git 정리 작업을 취소했습니다.",
  GIT_CLEANUP_ERROR: "Git 정리 작업을 실행하지 못했습니다.",
  GIT_CLEANUP_STATE_CHANGED: "저장소 상태가 변경되어 Git 정리를 실행하지 않았습니다.",
  repoCleanup: vi.fn(),
  repoCleanupCancel: vi.fn(),
  repoCleanupPreview: vi.fn(),
}));

const repo: RepoEntry = {
  path: "C:\\projects\\sample",
  canonicalKey: "win:c:/projects/sample",
  hasWorktrees: true,
};

const preview: CleanupPreview = {
  revision: "cleanup-0123456789abcdef",
  currentBranch: "main",
  currentHead: "0123456789abcdef0123456789abcdef01234567",
  branches: [
    {
      name: "main",
      head: "0123456789abcdef0123456789abcdef01234567",
      upstream: "origin/main",
      lastCommitUnix: 100,
      current: true,
      checkedOut: true,
      protected: true,
      merged: true,
      stale: false,
      candidate: true,
      eligible: false,
      reasons: ["mergedIntoCurrent"],
      blocked: ["currentBranch", "mainBranch", "checkedOut"],
    },
    {
      name: "merged-candidate",
      head: "fedcba9876543210fedcba9876543210fedcba98",
      upstream: null,
      lastCommitUnix: 100,
      current: false,
      checkedOut: false,
      protected: false,
      merged: true,
      stale: false,
      candidate: true,
      eligible: true,
      reasons: ["mergedIntoCurrent"],
      blocked: [],
    },
  ],
  worktrees: [
    {
      path: "C:\\projects\\sample",
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
    {
      path: "C:\\projects\\sample-linked",
      head: "fedcba9876543210fedcba9876543210fedcba98",
      branch: "linked-candidate",
      isMain: false,
      bare: false,
      locked: false,
      prunable: false,
      dirty: true,
      untracked: true,
      ignored: false,
      candidate: true,
      eligible: false,
      reasons: ["linkedWorktree"],
      blocked: ["dirty", "untracked"],
    },
  ],
};

const repoCleanupPreviewMock = vi.mocked(repoCleanupPreview);
const repoCleanupMock = vi.mocked(repoCleanup);
const repoCleanupCancelMock = vi.mocked(repoCleanupCancel);

beforeEach(() => {
  repoCleanupPreviewMock.mockReset().mockResolvedValue(preview);
  repoCleanupMock.mockReset().mockResolvedValue({
    previewRevision: preview.revision,
    attempted: 1,
    removed: 1,
    items: [{ kind: "branch", target: "merged-candidate", outcome: "removed", reason: null }],
  });
  repoCleanupCancelMock.mockReset().mockResolvedValue(true);
});

afterEach(() => cleanup());

describe("CleanupPanel", () => {
  it("ignores duplicate preview requests while the native read is busy", async () => {
    let resolvePreview: ((value: CleanupPreview) => void) | undefined;
    repoCleanupPreviewMock.mockReturnValueOnce(new Promise((resolve) => { resolvePreview = resolve; }));
    render(<CleanupPanel repo={repo} />);
    const inspect = screen.getByRole("button", { name: "정리 후보 검사" });
    fireEvent.click(inspect);
    fireEvent.click(inspect);
    expect(repoCleanupPreviewMock).toHaveBeenCalledTimes(1);
    resolvePreview?.(preview);
    await screen.findByText("현재 branch에 이미 병합됨");
  });

  it("cancels an in-flight preview and does not publish its late result", async () => {
    let resolvePreview: ((value: CleanupPreview) => void) | undefined;
    repoCleanupPreviewMock.mockReturnValueOnce(new Promise((resolve) => {
      resolvePreview = resolve;
    }));
    render(<CleanupPanel repo={repo} />);
    fireEvent.click(screen.getByRole("button", { name: "정리 후보 검사" }));
    expect(repoCleanupPreviewMock).toHaveBeenCalledWith(repo.path, expect.stringMatching(/^[A-Za-z0-9._-]+$/u));
    fireEvent.click(screen.getByRole("button", { name: "검사 취소" }));
    await waitFor(() => expect(repoCleanupCancelMock).toHaveBeenCalledWith(expect.any(String)));
    resolvePreview?.(preview);
    await waitFor(() => expect(screen.queryByRole("checkbox", { name: "branch merged-candidate" })).toBeNull());
    expect(screen.getByText("정리 후보 검사를 취소했습니다. 최신 후보를 다시 검사하세요.")).toBeTruthy();
    expect(screen.getByRole("alert").textContent).toBe(GIT_CLEANUP_CANCELLED);
  });

  it("shows merged rationale and blocks main, dirty, and untracked targets", async () => {
    render(<CleanupPanel repo={repo} />);
    fireEvent.click(screen.getByRole("button", { name: "정리 후보 검사" }));
    await screen.findByText("현재 branch에 이미 병합됨");

    expect(screen.getByRole("checkbox", { name: "branch main" })).toHaveProperty("disabled", true);
    expect(screen.getByRole("checkbox", { name: "worktree C:\\projects\\sample-linked" }))
      .toHaveProperty("disabled", true);
    expect(screen.getByText(/dirty 파일이 있어 차단됨|커밋되지 않은 변경이 있어 차단됨/)).toBeTruthy();
    expect(screen.getByText("worktree 후보 0개")).toBeTruthy();
  });

  it("requires preview selection confirmation and sends only selected safe targets", async () => {
    render(<CleanupPanel repo={repo} />);
    fireEvent.click(screen.getByRole("button", { name: "정리 후보 검사" }));
    await screen.findByRole("checkbox", { name: "branch merged-candidate" });
    fireEvent.click(screen.getByRole("checkbox", { name: "branch merged-candidate" }));
    fireEvent.click(screen.getByRole("button", { name: "선택 항목 정리 (1)" }));
    expect(repoCleanupMock).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog", { name: "선택한 정리를 실행할까요?" })).toBeTruthy();
    expect(screen.getByText("정리 대상:")).toBeTruthy();
    expect(screen.getByText("branch merged-candidate")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "정리 실행" }));
    await waitFor(() => expect(repoCleanupMock).toHaveBeenCalledWith(
      repo.path,
      ["merged-candidate"],
      [],
      preview.revision,
      expect.stringMatching(/^[A-Za-z0-9._-]+$/u),
    ));
    expect(await screen.findByText("정리 결과")).toBeTruthy();
    expect(screen.getByText("branch merged-candidate")).toBeTruthy();
    expect(document.body.textContent).not.toContain("credential");
  });

  it("keeps the preview safe when native errors contain a raw path", async () => {
    repoCleanupPreviewMock.mockRejectedValueOnce(new Error("C:\\secret\\credential-root"));
    render(<CleanupPanel repo={repo} />);
    fireEvent.click(screen.getByRole("button", { name: "정리 후보 검사" }));
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toBe(GIT_CLEANUP_ERROR);
    expect(document.body.textContent).not.toContain("credential-root");
  });

  it("cancels an in-flight cleanup and ignores a stale result after unmount", async () => {
    let resolveCleanup: ((value: Awaited<ReturnType<typeof repoCleanup>>) => void) | undefined;
    repoCleanupMock.mockReturnValueOnce(new Promise((resolve) => { resolveCleanup = resolve; }));
    const view = render(<CleanupPanel repo={repo} />);
    fireEvent.click(screen.getByRole("button", { name: "정리 후보 검사" }));
    await screen.findByRole("checkbox", { name: "branch merged-candidate" });
    fireEvent.click(screen.getByRole("checkbox", { name: "branch merged-candidate" }));
    fireEvent.click(screen.getByRole("button", { name: "선택 항목 정리 (1)" }));
    fireEvent.click(screen.getByRole("button", { name: "정리 실행" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "취소" })).toBeTruthy());
    fireEvent.click(screen.getByRole("button", { name: "취소" }));
    await waitFor(() => expect(repoCleanupCancelMock).toHaveBeenCalledWith(expect.any(String)));
    view.unmount();
    resolveCleanup?.({
      previewRevision: preview.revision,
      attempted: 1,
      removed: 1,
      items: [{ kind: "branch", target: "merged-candidate", outcome: "removed", reason: null }],
    });
  });

  it("uses bounded opaque IDs and fixed state-change errors", async () => {
    const first = createCleanupOperationId();
    const second = createCleanupOperationId();
    expect(first).not.toBe(second);
    expect(first).toMatch(/^[A-Za-z0-9._-]+$/u);

    repoCleanupMock.mockRejectedValueOnce(new Error(GIT_CLEANUP_STATE_CHANGED));
    render(<CleanupPanel repo={repo} />);
    fireEvent.click(screen.getByRole("button", { name: "정리 후보 검사" }));
    await screen.findByRole("checkbox", { name: "branch merged-candidate" });
    fireEvent.click(screen.getByRole("checkbox", { name: "branch merged-candidate" }));
    fireEvent.click(screen.getByRole("button", { name: "선택 항목 정리 (1)" }));
    fireEvent.click(screen.getByRole("button", { name: "정리 실행" }));
    expect((await screen.findByRole("alert")).textContent).toBe(GIT_CLEANUP_STATE_CHANGED);
    expect(document.body.textContent).not.toContain(GIT_CLEANUP_BUSY);
    expect(document.body.textContent).not.toContain(GIT_CLEANUP_CANCELLED);
    expect(screen.queryByRole("checkbox", { name: "branch merged-candidate" })).toBeNull();
    expect(screen.getByText(/최신 후보를 다시 검사하세요\./u)).toBeTruthy();
  });

  it("drops a result whose revision no longer matches the approved preview", async () => {
    repoCleanupMock.mockResolvedValueOnce({
      previewRevision: "cleanup-different-revision",
      attempted: 1,
      removed: 0,
      items: [],
    });
    render(<CleanupPanel repo={repo} />);
    fireEvent.click(screen.getByRole("button", { name: "정리 후보 검사" }));
    await screen.findByRole("checkbox", { name: "branch merged-candidate" });
    fireEvent.click(screen.getByRole("checkbox", { name: "branch merged-candidate" }));
    fireEvent.click(screen.getByRole("button", { name: "선택 항목 정리 (1)" }));
    fireEvent.click(screen.getByRole("button", { name: "정리 실행" }));

    expect((await screen.findByRole("alert")).textContent).toBe(GIT_CLEANUP_STATE_CHANGED);
    expect(screen.queryByRole("checkbox", { name: "branch merged-candidate" })).toBeNull();
    expect(screen.getByText("정리 결과가 승인한 preview와 달라졌습니다. 최신 후보를 다시 검사하세요.")).toBeTruthy();
  });
});
