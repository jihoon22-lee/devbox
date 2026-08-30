import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  GIT_MUTATION_ERROR,
  repoChanges,
  repoCommit,
  repoLocalCancel,
  repoStage,
  repoUnstage,
  type ChangeEntry,
  type RepoEntry,
} from "../api";
import StageCommitPanel from "./StageCommitPanel";
import { createLocalOperationId } from "./StageCommitPanel";

vi.mock("../api", () => ({
  GIT_MUTATION_ERROR: "Git 변경 사항을 적용하지 못했습니다.",
  repoChanges: vi.fn(),
  repoCommit: vi.fn(),
  repoLocalCancel: vi.fn(),
  repoStage: vi.fn(),
  repoUnstage: vi.fn(),
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

const unstaged: ChangeEntry = {
  path: "src/main.ts",
  oldPath: null,
  indexStatus: " ",
  worktreeStatus: "M",
  kind: "modified",
  staged: false,
  unstaged: true,
};

const staged: ChangeEntry = {
  ...unstaged,
  indexStatus: "M",
  worktreeStatus: " ",
  staged: true,
  unstaged: false,
};

const repoChangesMock = vi.mocked(repoChanges);
const repoStageMock = vi.mocked(repoStage);
const repoUnstageMock = vi.mocked(repoUnstage);
const repoCommitMock = vi.mocked(repoCommit);
const repoLocalCancelMock = vi.mocked(repoLocalCancel);

beforeEach(() => {
  repoChangesMock.mockReset().mockResolvedValue([unstaged]);
  repoStageMock.mockReset().mockResolvedValue(undefined);
  repoUnstageMock.mockReset().mockResolvedValue(undefined);
  repoCommitMock.mockReset().mockResolvedValue(undefined);
  repoLocalCancelMock.mockReset().mockResolvedValue(false);
});

afterEach(() => cleanup());

describe("StageCommitPanel", () => {
  it("does not call native commit before explicit confirmation and redacts the message", async () => {
    repoChangesMock.mockResolvedValueOnce([staged]);
    render(<StageCommitPanel repo={repo} />);
    fireEvent.click(screen.getByRole("button", { name: "변경 파일 불러오기" }));
    await screen.findByRole("checkbox", { name: "unstage src/main.ts" });
    const message = screen.getByRole("textbox", { name: "커밋 메시지" });
    const secret = "credential-message-secret";
    fireEvent.change(message, { target: { value: secret } });
    const trigger = screen.getByRole("button", { name: "Commit (1)" });
    trigger.focus();
    fireEvent.click(trigger);

    expect(repoCommitMock).not.toHaveBeenCalled();
    const dialog = screen.getByRole("dialog", { name: "Commit을 실행할까요?" });
    expect(dialog).toBeTruthy();
    expect(dialog.textContent).not.toContain(secret);
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "취소" }));
  });

  it("does not open commit confirmation for Ctrl+Enter during IME composition", async () => {
    repoChangesMock.mockResolvedValueOnce([staged]);
    render(<StageCommitPanel repo={repo} />);
    fireEvent.click(screen.getByRole("button", { name: "변경 파일 불러오기" }));
    await screen.findByRole("checkbox", { name: "unstage src/main.ts" });
    const message = screen.getByRole("textbox", { name: "커밋 메시지" });
    fireEvent.change(message, { target: { value: "커밋 메시지" } });

    fireEvent.keyDown(message, { key: "Enter", ctrlKey: true, isComposing: true });
    expect(screen.queryByRole("dialog")).toBeNull();
    fireEvent.keyDown(message, { key: "Enter", ctrlKey: true });
    expect(screen.getByRole("dialog", { name: "Commit을 실행할까요?" })).toBeTruthy();
  });

  it("cancels a commit with Escape and returns focus to the trigger", async () => {
    repoChangesMock.mockResolvedValueOnce([staged]);
    render(<StageCommitPanel repo={repo} />);
    fireEvent.click(screen.getByRole("button", { name: "변경 파일 불러오기" }));
    await screen.findByRole("checkbox", { name: "unstage src/main.ts" });
    fireEvent.change(screen.getByRole("textbox", { name: "커밋 메시지" }), {
      target: { value: "cancel this commit" },
    });
    const trigger = screen.getByRole("button", { name: "Commit (1)" });
    trigger.focus();
    fireEvent.click(trigger);
    const dialog = screen.getByRole("dialog", { name: "Commit을 실행할까요?" });
    fireEvent.keyDown(dialog, { key: "Escape" });

    expect(screen.queryByRole("dialog")).toBeNull();
    expect(document.activeElement).toBe(trigger);
    expect(repoCommitMock).not.toHaveBeenCalled();
  });

  it("invalidates an open confirmation when the commit message changes", async () => {
    repoChangesMock.mockResolvedValueOnce([staged]);
    render(<StageCommitPanel repo={repo} />);
    fireEvent.click(screen.getByRole("button", { name: "변경 파일 불러오기" }));
    await screen.findByRole("checkbox", { name: "unstage src/main.ts" });
    const message = screen.getByRole("textbox", { name: "커밋 메시지" });
    fireEvent.change(message, { target: { value: "first message" } });
    fireEvent.click(screen.getByRole("button", { name: "Commit (1)" }));
    expect(screen.getByRole("dialog")).toBeTruthy();

    fireEvent.change(message, { target: { value: "changed after review" } });
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(repoCommitMock).not.toHaveBeenCalled();
  });

  it("uses unique opaque local operation IDs and sends cancellation while native work is active", async () => {
    const first = createLocalOperationId();
    const second = createLocalOperationId();
    expect(first).not.toBe(second);
    expect(first).toMatch(/^[A-Za-z0-9._-]+$/u);

    let resolveStage: (() => void) | undefined;
    repoStageMock.mockReturnValueOnce(new Promise<void>((resolve) => { resolveStage = resolve; }));
    render(<StageCommitPanel repo={repo} />);
    fireEvent.click(screen.getByRole("button", { name: "변경 파일 불러오기" }));
    const checkbox = await screen.findByRole("checkbox", { name: "stage src/main.ts" });
    fireEvent.click(checkbox);
    fireEvent.click(screen.getByRole("button", { name: /선택 항목 stage/ }));

    const operationId = repoStageMock.mock.calls[0][2];
    expect(operationId).toMatch(/^[A-Za-z0-9._-]+$/u);
    expect(screen.getByRole("button", { name: "취소" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "취소" }));
    await waitFor(() => expect(repoLocalCancelMock).toHaveBeenCalledWith(operationId));

    resolveStage?.();
    await waitFor(() => expect(screen.getByRole("button", { name: "변경 파일 불러오기" })).toBeTruthy());
    expect(screen.queryByRole("button", { name: "취소" })).toBeNull();
    expect(screen.queryByRole("checkbox", { name: "stage src/main.ts" })).toBeNull();
    expect(screen.getByRole("alert").textContent).toBe(GIT_MUTATION_ERROR);
  });

  it("loads status and stages only the explicitly selected path", async () => {
    repoChangesMock.mockResolvedValueOnce([unstaged]).mockResolvedValueOnce([staged]);
    render(<StageCommitPanel repo={repo} />);

    fireEvent.click(screen.getByRole("button", { name: "변경 파일 불러오기" }));
    const checkbox = await screen.findByRole("checkbox", { name: "stage src/main.ts" });
    fireEvent.click(checkbox);
    fireEvent.click(screen.getByRole("button", { name: /선택 항목 stage/ }));

    await waitFor(() => expect(repoStageMock).toHaveBeenCalledWith(repo.path, ["src/main.ts"], expect.any(String)));
    await waitFor(() => expect(repoChangesMock).toHaveBeenCalledTimes(2));
    expect(await screen.findByRole("checkbox", { name: "unstage src/main.ts" })).toBeTruthy();
    expect(screen.getByRole("status").textContent).toContain("1개 staged");
  });

  it("unstages the selected index path and commits only existing staged changes", async () => {
    repoChangesMock.mockResolvedValueOnce([staged]).mockResolvedValueOnce([unstaged]);
    render(<StageCommitPanel repo={repo} />);

    fireEvent.click(screen.getByRole("button", { name: "변경 파일 불러오기" }));
    const checkbox = await screen.findByRole("checkbox", { name: "unstage src/main.ts" });
    fireEvent.click(checkbox);
    fireEvent.click(screen.getByRole("button", { name: /선택 항목 unstage/ }));

    await waitFor(() => expect(repoUnstageMock).toHaveBeenCalledWith(repo.path, ["src/main.ts"], expect.any(String)));
    await waitFor(() => expect(screen.queryByRole("checkbox", { name: "unstage src/main.ts" })).toBeNull());

    // A refresh returning no staged entry disables commit; no implicit `git add`
    // or commit call is made from the message field alone.
    fireEvent.change(screen.getByRole("textbox", { name: "커밋 메시지" }), {
      target: { value: "should not commit unstaged changes" },
    });
    expect(screen.getByRole("button", { name: /Commit \(0\)/ })).toHaveProperty("disabled", true);
    expect(repoCommitMock).not.toHaveBeenCalled();
  });

  it("commits explicitly staged files and clears the message only after success", async () => {
    repoChangesMock.mockResolvedValueOnce([staged]).mockResolvedValueOnce([]);
    render(<StageCommitPanel repo={repo} />);
    fireEvent.click(screen.getByRole("button", { name: "변경 파일 불러오기" }));
    await screen.findByRole("checkbox", { name: "unstage src/main.ts" });
    const message = screen.getByRole("textbox", { name: "커밋 메시지" });
    fireEvent.change(message, { target: { value: "Commit selected fixture" } });
    fireEvent.click(screen.getByRole("button", { name: "Commit (1)" }));
    fireEvent.click(screen.getByRole("button", { name: "Commit 실행" }));

    await waitFor(() => expect(repoCommitMock).toHaveBeenCalledWith(repo.path, "Commit selected fixture", expect.any(String)));
    await waitFor(() => expect(repoChangesMock).toHaveBeenCalledTimes(2));
    expect((message as HTMLTextAreaElement).value).toBe("");
    expect(screen.getByRole("status").textContent).toContain("0개 staged");
  });

  it("keeps intent after a failed mutation and exposes only the fixed error", async () => {
    const raw = "C:\\secret\\credential-stage-path";
    repoStageMock.mockRejectedValueOnce(new Error(raw));
    render(<StageCommitPanel repo={repo} />);
    fireEvent.click(screen.getByRole("button", { name: "변경 파일 불러오기" }));
    const checkbox = await screen.findByRole("checkbox", { name: "stage src/main.ts" });
    fireEvent.click(checkbox);
    fireEvent.click(screen.getByRole("button", { name: /선택 항목 stage/ }));

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toBe(GIT_MUTATION_ERROR);
    expect(alert.textContent).not.toContain(raw);
    expect((screen.getByRole("checkbox", { name: "stage src/main.ts" }) as HTMLInputElement).checked).toBe(true);
    expect(repoChangesMock).toHaveBeenCalledTimes(1);
  });

  it("keeps the commit message after a failed commit without reflecting native details", async () => {
    const raw = "remote credential hook path must stay private";
    repoCommitMock.mockRejectedValueOnce(new Error(raw));
    repoChangesMock.mockResolvedValueOnce([staged]);
    render(<StageCommitPanel repo={repo} />);
    fireEvent.click(screen.getByRole("button", { name: "변경 파일 불러오기" }));
    await screen.findByRole("checkbox", { name: "unstage src/main.ts" });
    const message = screen.getByRole("textbox", { name: "커밋 메시지" });
    fireEvent.change(message, { target: { value: "Retry this commit" } });
    fireEvent.click(screen.getByRole("button", { name: "Commit (1)" }));
    fireEvent.click(screen.getByRole("button", { name: "Commit 실행" }));

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toBe(GIT_MUTATION_ERROR);
    expect(alert.textContent).not.toContain(raw);
    expect((message as HTMLTextAreaElement).value).toBe("Retry this commit");
    expect(repoChangesMock).toHaveBeenCalledTimes(1);
  });

  it("drops a status result that belongs to a replaced repository", async () => {
    let resolveOlder: ((value: ChangeEntry[]) => void) | undefined;
    repoChangesMock.mockReturnValueOnce(new Promise((resolve) => { resolveOlder = resolve; }));
    const rendered = render(<StageCommitPanel repo={repo} />);
    fireEvent.click(screen.getByRole("button", { name: "변경 파일 불러오기" }));
    rendered.rerender(<StageCommitPanel repo={otherRepo} />);
    resolveOlder?.([unstaged]);
    await Promise.resolve();

    expect(screen.queryByRole("checkbox", { name: "stage src/main.ts" })).toBeNull();
    expect(screen.getByText(otherRepo.path)).toBeTruthy();
  });

  it("ignores duplicate actions and late results after unmount", async () => {
    let resolveStage: (() => void) | undefined;
    repoStageMock.mockReturnValueOnce(new Promise<void>((resolve) => { resolveStage = resolve; }));
    render(<StageCommitPanel repo={repo} />);
    fireEvent.click(screen.getByRole("button", { name: "변경 파일 불러오기" }));
    const checkbox = await screen.findByRole("checkbox", { name: "stage src/main.ts" });
    fireEvent.click(checkbox);
    const stageButton = screen.getByRole("button", { name: /선택 항목 stage/ });
    fireEvent.click(stageButton);
    fireEvent.click(stageButton);
    expect(repoStageMock).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("region", { name: "Git stage 및 commit" }).getAttribute("aria-busy"))
      .toBe("true");

    cleanup();
    resolveStage?.();
    await Promise.resolve();
    expect(screen.queryByRole("alert")).toBeNull();
  });
});
