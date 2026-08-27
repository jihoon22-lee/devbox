import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  GIT_MUTATION_ERROR,
  repoChanges,
  repoCommit,
  repoStage,
  repoUnstage,
  type ChangeEntry,
  type RepoEntry,
} from "../api";
import StageCommitPanel from "./StageCommitPanel";

vi.mock("../api", () => ({
  GIT_MUTATION_ERROR: "Git 변경 사항을 적용하지 못했습니다.",
  repoChanges: vi.fn(),
  repoCommit: vi.fn(),
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

beforeEach(() => {
  repoChangesMock.mockReset().mockResolvedValue([unstaged]);
  repoStageMock.mockReset().mockResolvedValue(undefined);
  repoUnstageMock.mockReset().mockResolvedValue(undefined);
  repoCommitMock.mockReset().mockResolvedValue(undefined);
});

afterEach(() => cleanup());

describe("StageCommitPanel", () => {
  it("loads status and stages only the explicitly selected path", async () => {
    repoChangesMock.mockResolvedValueOnce([unstaged]).mockResolvedValueOnce([staged]);
    render(<StageCommitPanel repo={repo} />);

    fireEvent.click(screen.getByRole("button", { name: "변경 파일 불러오기" }));
    const checkbox = await screen.findByRole("checkbox", { name: "stage src/main.ts" });
    fireEvent.click(checkbox);
    fireEvent.click(screen.getByRole("button", { name: /선택 항목 stage/ }));

    await waitFor(() => expect(repoStageMock).toHaveBeenCalledWith(repo.path, ["src/main.ts"]));
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

    await waitFor(() => expect(repoUnstageMock).toHaveBeenCalledWith(repo.path, ["src/main.ts"]));
    await waitFor(() => expect(screen.queryByRole("checkbox", { name: "unstage src/main.ts" })).toBeNull());

    // A refresh returning no staged entry disables commit; no implicit `git add`
    // or commit call is made from the message field alone.
    fireEvent.change(screen.getByRole("textbox", { name: "Commit message" }), {
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
    const message = screen.getByRole("textbox", { name: "Commit message" });
    fireEvent.change(message, { target: { value: "Commit selected fixture" } });
    fireEvent.click(screen.getByRole("button", { name: "Commit (1)" }));

    await waitFor(() => expect(repoCommitMock).toHaveBeenCalledWith(repo.path, "Commit selected fixture"));
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
    const message = screen.getByRole("textbox", { name: "Commit message" });
    fireEvent.change(message, { target: { value: "Retry this commit" } });
    fireEvent.click(screen.getByRole("button", { name: "Commit (1)" }));

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
    expect(screen.getByRole("region", { name: "Git stage and commit" }).getAttribute("aria-busy"))
      .toBe("true");

    cleanup();
    resolveStage?.();
    await Promise.resolve();
    expect(screen.queryByRole("alert")).toBeNull();
  });
});
