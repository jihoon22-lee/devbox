import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { assertNoA11yViolations } from "@devbox/a11y/testing";
import * as api from "../api";
import StashPanel from "./StashPanel";
vi.mock("../api", () => ({
  repoStashList: vi.fn(),
  repoStashPush: vi.fn(),
  repoStashApply: vi.fn(),
  repoStashDrop: vi.fn(),
  repoStashStore: vi.fn(),
  repoLocalCancel: vi.fn(),
}));
const repo = { path: "/repo", canonicalKey: "posix:/repo", hasWorktrees: false };
beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(api.repoLocalCancel).mockResolvedValue(false);
  vi.mocked(api.repoStashList).mockResolvedValue([
    { index: 0, commit: "a".repeat(40), createdAt: 1, message: "On main: work" },
  ]);
  vi.mocked(api.repoStashApply).mockResolvedValue({ applied: true, conflicts: [] });
  vi.mocked(api.repoStashDrop).mockResolvedValue({ commit: "a".repeat(40), message: "On main: work" });
});
afterEach(cleanup);
it("saves optional untracked files and has no accessibility violations", async () => {
  const { container } = render(<StashPanel repo={repo} />);
  await screen.findByText("On main: work");
  fireEvent.change(screen.getByRole("textbox", { name: "stash 메시지" }), { target: { value: "wip" } });
  fireEvent.click(screen.getByRole("checkbox", { name: "추적하지 않는 파일 포함" }));
  fireEvent.click(screen.getByRole("button", { name: "변경 임시 저장" }));
  await waitFor(() =>
    expect(api.repoStashPush).toHaveBeenCalledWith(
      repo.path,
      { message: "wip", includeUntracked: true },
      expect.any(String),
    ),
  );
  await assertNoA11yViolations(container);
});
it("applies or pops and reports conflicts while retaining the stash", async () => {
  render(<StashPanel repo={repo} />);
  await screen.findByText("On main: work");
  fireEvent.click(screen.getByRole("button", { name: "stash 0 적용" }));
  await waitFor(() => expect(api.repoStashApply).toHaveBeenCalledWith(repo.path, 0, false, expect.any(String)));
  vi.mocked(api.repoStashApply).mockResolvedValue({ applied: false, conflicts: ["README.md"] });
  await waitFor(() => expect(screen.getByRole("button", { name: "stash 0 꺼내기" })).not.toBeDisabled());
  fireEvent.click(screen.getByRole("button", { name: "stash 0 꺼내기" }));
  await screen.findByText("충돌 파일 1개: README.md");
  expect(api.repoStashApply).toHaveBeenLastCalledWith(repo.path, 0, true, expect.any(String));
  expect(screen.getByText("On main: work")).toBeTruthy();
});
it("restores a dropped stash through the saved commit and message", async () => {
  render(<StashPanel repo={repo} />);
  await screen.findByText("On main: work");
  fireEvent.click(screen.getByRole("button", { name: "stash 0 삭제" }));
  fireEvent.click(await screen.findByRole("button", { name: "되돌리기" }));
  await waitFor(() =>
    expect(api.repoStashStore).toHaveBeenCalledWith(repo.path, "a".repeat(40), "On main: work", expect.any(String)),
  );
});
