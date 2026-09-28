import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { assertNoA11yViolations } from "@devbox/a11y/testing";
import { WorkspaceOperationError } from "../../transport";
import * as api from "../api";
import BranchPanel from "./BranchPanel";
vi.mock("../api", () => ({
  repoBranches: vi.fn(),
  repoBranchCreate: vi.fn(),
  repoBranchRename: vi.fn(),
  repoBranchDelete: vi.fn(),
  repoSwitch: vi.fn(),
  repoStashPush: vi.fn(),
  repoLocalCancel: vi.fn(),
}));
const repo = { path: "/repo", canonicalKey: "posix:/repo", hasWorktrees: true };
const branch = (name: string, extra = {}) => ({
  name,
  remote: false,
  commit: "a".repeat(40),
  upstream: null,
  ahead: 0,
  behind: 0,
  worktree: null,
  committedAt: 1,
  subject: "fixture",
  ...extra,
});
beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(api.repoLocalCancel).mockResolvedValue(false);
  vi.mocked(api.repoBranches).mockResolvedValue({
    current: "main",
    detached: false,
    truncated: false,
    branches: [
      branch("main", { upstream: "origin/main", ahead: 2, behind: 1, worktree: "/repo" }),
      branch("topic"),
      branch("busy", { worktree: "/other" }),
      branch("origin/next", { remote: true }),
    ],
  });
  vi.mocked(api.repoBranchDelete).mockResolvedValue({ name: "topic", commit: "b".repeat(40) });
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});
it("lists current, tracking and occupied branches accessibly", async () => {
  const { container } = render(<BranchPanel repo={repo} />);
  await screen.findByText("앞섬 2 · 뒤처짐 1");
  expect(screen.getByText("현재")).toBeTruthy();
  expect(screen.getByText("다른 작업 폴더에서 사용 중")).toBeTruthy();
  expect(screen.queryByRole("button", { name: "busy 삭제" })).toBeNull();
  expect(screen.queryByRole("button", { name: "busy 전환" })).toBeNull();
  fireEvent.click(screen.getByText("원격 branch"));
  expect(screen.getByRole("button", { name: "origin/next 추적 branch로 전환" })).toBeTruthy();
  await assertNoA11yViolations(container);
});
it("creates and renames branches and reloads repository state", async () => {
  const changed = vi.fn();
  render(<BranchPanel repo={repo} onChanged={changed} />);
  await screen.findByText("topic");
  fireEvent.change(screen.getByRole("textbox", { name: "branch 이름" }), { target: { value: "feature/a" } });
  fireEvent.click(screen.getByRole("button", { name: "새 branch 만들기" }));
  await waitFor(() =>
    expect(api.repoBranchCreate).toHaveBeenCalledWith(
      repo.path,
      { name: "feature/a", startPoint: null, checkout: true },
      expect.any(String),
    ),
  );
  await waitFor(() => expect(changed).toHaveBeenCalled());
  fireEvent.click(screen.getByRole("button", { name: "topic 이름 바꾸기" }));
  fireEvent.change(screen.getByRole("textbox", { name: "새 branch 이름" }), { target: { value: "renamed" } });
  fireEvent.click(screen.getByRole("button", { name: "이름 변경 적용" }));
  await waitFor(() =>
    expect(api.repoBranchRename).toHaveBeenCalledWith(repo.path, "topic", "renamed", expect.any(String)),
  );
});
it("deletes immediately and restores the exact tip without checking it out", async () => {
  render(<BranchPanel repo={repo} />);
  await screen.findByText("topic");
  fireEvent.click(screen.getByRole("button", { name: "topic 삭제" }));
  await screen.findByText("branch 'topic'를 삭제했습니다.");
  fireEvent.click(screen.getByRole("button", { name: "되돌리기" }));
  await waitFor(() =>
    expect(api.repoBranchCreate).toHaveBeenCalledWith(
      repo.path,
      { name: "topic", startPoint: "b".repeat(40), checkout: false },
      expect.any(String),
    ),
  );
});
it("expires undo after eight seconds", async () => {
  render(<BranchPanel repo={repo} />);
  await screen.findByText("topic");
  vi.useFakeTimers();
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "topic 삭제" })));
  expect(screen.getByRole("button", { name: "되돌리기" })).toBeTruthy();
  await act(async () => vi.advanceTimersByTime(8000));
  expect(screen.queryByRole("button", { name: "되돌리기" })).toBeNull();
});
it("offers stash after a blocked switch without discarding changes", async () => {
  vi.mocked(api.repoSwitch).mockRejectedValue(
    new WorkspaceOperationError("커밋하지 않은 변경이 겹칩니다.", "switch_blocked_by_changes"),
  );
  render(<BranchPanel repo={repo} />);
  await screen.findByText("topic");
  fireEvent.click(screen.getByRole("button", { name: "topic 전환" }));
  fireEvent.click(await screen.findByRole("button", { name: "변경을 stash에 저장" }));
  await waitFor(() =>
    expect(api.repoStashPush).toHaveBeenCalledWith(
      repo.path,
      { message: null, includeUntracked: false },
      expect.any(String),
    ),
  );
});
it("displays detached HEAD and rejects undo after repository replacement", async () => {
  const { rerender } = render(<BranchPanel repo={repo} />);
  await screen.findByText("topic");
  fireEvent.click(screen.getByRole("button", { name: "topic 삭제" }));
  await screen.findByRole("button", { name: "되돌리기" });
  vi.mocked(api.repoBranches).mockResolvedValue({ current: null, detached: true, truncated: false, branches: [] });
  rerender(<BranchPanel repo={{ ...repo, path: "/other", canonicalKey: "posix:/other" }} />);
  await screen.findByText("현재 branch 없음(detached)");
  const undo = screen.queryByRole("button", { name: "되돌리기" });
  if (undo) fireEvent.click(undo);
  await act(async () => {});
  expect(api.repoBranchCreate).not.toHaveBeenCalled();
});

it("offers undo only after the deletion refresh releases the action", async () => {
  render(<BranchPanel repo={repo} />);
  await screen.findByText("topic");
  let finish!: (value: Awaited<ReturnType<typeof api.repoBranches>>) => void;
  vi.mocked(api.repoBranches).mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  fireEvent.click(screen.getByRole("button", { name: "topic 삭제" }));
  await waitFor(() => expect(api.repoBranches).toHaveBeenCalledTimes(2));
  expect(screen.queryByRole("button", { name: "되돌리기" })).toBeNull();
  await act(async () => finish({ current: "main", detached: false, truncated: false, branches: [branch("main")] }));
  fireEvent.click(await screen.findByRole("button", { name: "되돌리기" }));
  await waitFor(() => expect(api.repoBranchCreate).toHaveBeenCalled());
});
