import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { assertNoA11yViolations } from "@devbox/a11y/testing";
import { WorkspaceOperationError } from "../../transport";
import * as api from "../api";
import { openUrl } from "../../lib/openUrl";
import PullRequestPanel from "./PullRequestPanel";
vi.mock("../../lib/openUrl", () => ({ openUrl: vi.fn() }));
vi.mock("../api", () => ({
  repoPrStatus: vi.fn(),
  repoPrList: vi.fn(),
  repoPrCreate: vi.fn(),
  repoLastCommit: vi.fn(),
  repoBranches: vi.fn(),
  repoPush: vi.fn(),
  repoLocalCancel: vi.fn(),
  repoRemoteCancel: vi.fn(),
}));
const repo = { path: "/repo", canonicalKey: "posix:/repo", hasWorktrees: false };
beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(openUrl).mockResolvedValue(undefined);
  vi.mocked(api.repoLocalCancel).mockResolvedValue(false);
  vi.mocked(api.repoRemoteCancel).mockResolvedValue(false);
  vi.mocked(api.repoPrStatus).mockResolvedValue({ available: true, reason: null, pr: null });
  vi.mocked(api.repoPrList).mockResolvedValue([]);
  vi.mocked(api.repoBranches).mockResolvedValue({
    current: "feature",
    detached: false,
    branches: [],
    truncated: false,
  });
  vi.mocked(api.repoLastCommit).mockResolvedValue({
    id: "a".repeat(40),
    message: "Fix login\n\nbody",
    pushed: false,
    merge: false,
  });
  vi.mocked(api.repoPrCreate).mockResolvedValue({ url: "https://github.com/example/repo/pull/43" });
});
afterEach(cleanup);
it.each([
  ["gh_missing", "GitHub CLI(gh)를 설치하고 터미널에서 `gh auth login`을 실행해 주세요."],
  ["gh_unauthenticated", "`gh auth login`으로 로그인해 주세요."],
])("guides unavailable gh %s without an error", async (reason, message) => {
  vi.mocked(api.repoPrStatus).mockResolvedValue({ available: false, reason, pr: null });
  const { container } = render(<PullRequestPanel repo={repo} />);
  await screen.findByText(message);
  expect(screen.queryByRole("alert")).toBeNull();
  expect(api.repoPrList).not.toHaveBeenCalled();
  await assertNoA11yViolations(container);
});
it("shows current PR checks and opens its HTTPS URL", async () => {
  vi.mocked(api.repoPrStatus).mockResolvedValue({
    available: true,
    reason: null,
    pr: {
      number: 42,
      title: "Fix login",
      state: "OPEN",
      url: "https://github.com/example/repo/pull/42",
      isDraft: false,
      head: "feature",
      base: "main",
      reviewDecision: "REVIEW_REQUIRED",
      checks: { passed: 2, failed: 1, pending: 1 },
    },
  });
  const { container } = render(<PullRequestPanel repo={repo} />);
  await screen.findByText("#42 Fix login · 열림 · 검토 필요 · 검사 통과 2 · 실패 1 · 진행 중 1");
  fireEvent.click(screen.getByRole("button", { name: "GitHub에서 열기" }));
  expect(openUrl).toHaveBeenCalledWith("https://github.com/example/repo/pull/42");
  await assertNoA11yViolations(container);
});
it("prefills the summary and creates a draft PR with an optional base", async () => {
  const { container } = render(<PullRequestPanel repo={repo} />);
  await waitFor(() => expect(screen.getByRole("textbox", { name: "PR 제목" })).toHaveValue("Fix login"));
  fireEvent.change(screen.getByRole("textbox", { name: "PR 본문" }), { target: { value: "Body" } });
  fireEvent.click(screen.getByRole("checkbox", { name: "초안" }));
  await assertNoA11yViolations(container);
  fireEvent.click(screen.getByRole("button", { name: "PR 만들기" }));
  await waitFor(() =>
    expect(api.repoPrCreate).toHaveBeenCalledWith(repo.path, "Fix login", "Body", null, true, expect.any(String)),
  );
  await screen.findByRole("link", { name: "만든 PR 열기" });
});
it("offers explicit push when the branch has not been pushed", async () => {
  vi.mocked(api.repoPrCreate).mockRejectedValue(
    new WorkspaceOperationError("PR을 만들기 전에 현재 branch를 push해 주세요.", "pr_branch_not_pushed"),
  );
  render(<PullRequestPanel repo={repo} />);
  await waitFor(() => expect(screen.getByRole("textbox", { name: "PR 제목" })).toHaveValue("Fix login"));
  fireEvent.click(screen.getByRole("button", { name: "PR 만들기" }));
  const push = await screen.findByRole("button", { name: "push" });
  fireEvent.click(push);
  await waitFor(() => expect(api.repoPush).toHaveBeenCalledWith(repo.path, expect.any(String)));
});
